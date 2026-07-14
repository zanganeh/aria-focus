use std::{env, fs, path::PathBuf};

fn collect_plain_files(root: &std::path::Path) -> Vec<String> {
    fn visit(root: &std::path::Path, directory: &std::path::Path, files: &mut Vec<String>) {
        let mut entries = fs::read_dir(directory)
            .expect("enumerate staged private-beta resource")
            .collect::<Result<Vec<_>, _>>()
            .expect("read staged private-beta entry");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let file_type = entry.file_type().expect("read private-beta entry type");
            assert!(
                !file_type.is_symlink(),
                "private-beta resource may not contain links"
            );
            if file_type.is_dir() {
                visit(root, &entry.path(), files);
            } else if file_type.is_file() {
                files.push(
                    entry
                        .path()
                        .strip_prefix(root)
                        .expect("private-beta path below resource root")
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            } else {
                panic!("private-beta resource contains a non-file entry");
            }
        }
    }
    let mut files = Vec::new();
    visit(root, root, &mut files);
    files
}

// Lossless -1 dB derivative of the owner-authorized Track E source. Its
// immutable lineage points back to candidate SHA-256 945c74c1...50a727d.
fn main() {
    println!("cargo:rerun-if-env-changed=ARIA_FOCUS_BUNDLED_PACK_DIR");
    let staged = env::var_os("ARIA_FOCUS_BUNDLED_PACK_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("private-beta-pack"));
    println!("cargo:rerun-if-changed={}", staged.display());
    let generated =
        PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR missing")).join("private_beta_trust.rs");
    let published = env::var_os("CARGO_FEATURE_BUNDLED_LIBRARY").is_some();
    let listening_test = env::var_os("CARGO_FEATURE_BUNDLED_LISTENING_TEST").is_some();
    assert!(
        !(published && listening_test),
        "bundled-library and bundled-listening-test are mutually exclusive"
    );
    let source = if (published || listening_test) && staged.join("manifest.json").is_file() {
        let bytes =
            fs::read(staged.join("manifest.json")).expect("read staged private-beta manifest");
        let manifest: catalogue::ContentPackManifest = serde_json::from_slice(&bytes)
            .expect("staged private-beta manifest must be valid JSON");
        if published {
            manifest
                .validate_published()
                .expect("bundled Aria Focus Library manifest failed published validation");
        } else {
            manifest
                .validate_bundled_owner_waived()
                .expect("bundled listening-test manifest failed owner-waived validation");
        }
        let canonical = catalogue::canonical_manifest_bytes(&manifest)
            .expect("canonical private-beta manifest");
        let manifest_sha256 = catalogue::import::hash_bytes(&canonical);
        let mut assets = manifest
            .declared_assets()
            .into_iter()
            .map(|(path, asset)| (path.to_owned(), asset.sha256.clone()))
            .collect::<Vec<_>>();
        assets.sort();
        let mut actual_files = collect_plain_files(&staged);
        actual_files.sort();
        let mut declared_files = assets
            .iter()
            .map(|(path, _)| path.replace('\\', "/"))
            .collect::<Vec<_>>();
        declared_files.push("manifest.json".to_owned());
        declared_files.sort();
        assert_eq!(
            actual_files, declared_files,
            "staged private-beta resource must contain exactly its manifest and declared assets"
        );
        for (path, expected_hash) in &assets {
            let actual = catalogue::hash_file_sha256(&staged.join(path))
                .expect("read staged private-beta asset");
            assert_eq!(
                &actual, expected_hash,
                "staged private-beta asset hash differs: {path}"
            );
        }
        let bundle_bytes = assets
            .iter()
            .fold(canonical.clone(), |mut data, (path, hash)| {
                data.extend_from_slice(path.as_bytes());
                data.push(0);
                data.extend_from_slice(hash.as_bytes());
                data.push(0);
                data
            });
        let bundle_sha256 = catalogue::import::hash_bytes(&bundle_bytes);
        format!("pub const TRUST: Option<PrivateBetaTrust> = Some(PrivateBetaTrust {{ pack_id: {:?}, version: {:?}, manifest_sha256: {:?}, bundle_sha256: {:?}, item_ids: &[{}], published: {:?} }});", manifest.pack.id, manifest.pack.version, manifest_sha256, bundle_sha256, manifest.items.iter().map(|item| format!("{:?}", item.id)).collect::<Vec<_>>().join(", "), published)
    } else {
        "pub const TRUST: Option<PrivateBetaTrust> = None;".to_owned()
    };
    fs::write(generated, source).expect("write generated private-beta trust");
    tauri_build::build()
}
