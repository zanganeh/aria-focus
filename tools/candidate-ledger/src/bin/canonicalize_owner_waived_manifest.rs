use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    let mut args = env::args().skip(1);
    let input = PathBuf::from(
        args.next()
            .expect("usage: canonicalize_owner_waived_manifest <input> <output>"),
    );
    let output = PathBuf::from(
        args.next()
            .expect("usage: canonicalize_owner_waived_manifest <input> <output>"),
    );
    assert!(
        args.next().is_none(),
        "usage: canonicalize_owner_waived_manifest <input> <output>"
    );
    let source = fs::read(&input).expect("read manifest");
    let manifest: catalogue::ContentPackManifest =
        serde_json::from_slice(&source).expect("valid manifest JSON");
    manifest
        .validate_bundled_owner_waived()
        .expect("owner-waived manifest validation");
    let canonical = catalogue::canonical_manifest_bytes(&manifest).expect("canonical manifest");
    fs::write(output, canonical).expect("write canonical manifest");
}
