use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tempfile::TempDir;
use zip::ZipArchive;

use crate::manifest::{
    canonical_manifest_bytes, canonical_pack_path, ContentPackManifest, MANIFEST_PATH,
};

#[derive(Debug, Clone, Copy)]
pub struct ImportLimits {
    pub max_archive_bytes: u64,
    pub max_uncompressed_bytes: u64,
    pub max_entry_bytes: u64,
    pub max_manifest_bytes: u64,
    pub max_entries: usize,
    pub max_compression_ratio: f64,
}

impl Default for ImportLimits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 2 * 1024 * 1024 * 1024,
            max_uncompressed_bytes: 4 * 1024 * 1024 * 1024,
            max_entry_bytes: 1024 * 1024 * 1024,
            max_manifest_bytes: 4 * 1024 * 1024,
            max_entries: 4_096,
            max_compression_ratio: 200.0,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PackImportError {
    #[error("pack file must use the .adhdpack extension")]
    WrongExtension,
    #[error("pack archive is too large")]
    ArchiveTooLarge,
    #[error("pack contains too many entries")]
    TooManyEntries,
    #[error("pack expands beyond the configured size limit")]
    ExpandedSizeLimit,
    #[error("archive entry is too large: {0}")]
    EntryTooLarge(String),
    #[error("archive entry has a suspicious compression ratio: {0}")]
    CompressionRatio(String),
    #[error("archive path is unsafe or unsupported: {0}")]
    UnsafePath(String),
    #[error("archive contains a duplicate or case-ambiguous path: {0}")]
    DuplicatePath(String),
    #[error("archive contains a symbolic link or unsupported entry: {0}")]
    UnsupportedEntry(String),
    #[error("manifest.json is missing")]
    MissingManifest,
    #[error("manifest exceeds the configured size limit")]
    ManifestTooLarge,
    #[error("manifest actual byte count does not match ZIP metadata")]
    ManifestSizeMismatch,
    #[error("manifest is not canonical JSON")]
    NonCanonicalManifest,
    #[error("manifest JSON is invalid: {0}")]
    ManifestJson(#[from] serde_json::Error),
    #[error(transparent)]
    ManifestValidation(#[from] crate::manifest::ManifestValidationError),
    #[error("archive contains undeclared payload: {0}")]
    UndeclaredPayload(String),
    #[error("manifest declares an asset missing from the archive: {0}")]
    MissingAsset(String),
    #[error("asset byte count does not match manifest: {0}")]
    AssetSizeMismatch(String),
    #[error("asset SHA-256 does not match manifest: {0}")]
    HashMismatch(String),
    #[error("pack installation target already exists")]
    TargetExists,
    #[error("installed pack is corrupt: {0}")]
    InstalledPackCorrupt(String),
    #[error("ZIP operation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("pack filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}

/// Revalidates an installed pack without trusting the registry or following
/// symbolic links. This is intentionally suitable for startup integrity checks.
pub fn verify_installed_pack(
    directory: &Path,
    expected_manifest_sha256: &str,
) -> Result<ContentPackManifest, PackImportError> {
    verify_installed_pack_with_validation(
        directory,
        expected_manifest_sha256,
        ContentPackManifest::validate_published,
    )
}

/// Only the application-bundled private-beta installer may use this variant;
/// archive import and normal registry records always use `verify_installed_pack`.
pub fn verify_bundled_owner_waived_pack(
    directory: &Path,
    expected_manifest_sha256: &str,
) -> Result<ContentPackManifest, PackImportError> {
    verify_installed_pack_with_validation(
        directory,
        expected_manifest_sha256,
        ContentPackManifest::validate_bundled_owner_waived,
    )
}

/// Revalidates an installed app-owned local generation. It is deliberately
/// separate from archive import and accepts no archive input.
pub fn verify_generated_local_pack(
    directory: &Path,
    expected_manifest_sha256: &str,
) -> Result<ContentPackManifest, PackImportError> {
    verify_installed_pack_with_validation(directory, expected_manifest_sha256, |manifest| {
        let job_id = manifest
            .pack
            .id
            .strip_prefix("generated.local.")
            .ok_or_else(|| {
                crate::ManifestValidationError(
                    vec!["generated-local pack id is invalid".to_owned()],
                )
            })?;
        manifest.validate_generated_local(job_id)
    })
}

fn verify_installed_pack_with_validation(
    directory: &Path,
    expected_manifest_sha256: &str,
    validate: fn(&ContentPackManifest) -> Result<(), crate::ManifestValidationError>,
) -> Result<ContentPackManifest, PackImportError> {
    validate_install_root(directory)?;
    let manifest_path = directory.join(MANIFEST_PATH);
    let metadata = fs::symlink_metadata(&manifest_path)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(PackImportError::InstalledPackCorrupt(
            "manifest.json is not a regular file".to_owned(),
        ));
    }
    let declared_manifest_bytes = metadata.len();
    let mut manifest_file = File::open(&manifest_path)?;
    let manifest_bytes = read_manifest_bounded(
        &mut manifest_file,
        declared_manifest_bytes,
        ImportLimits::default().max_manifest_bytes,
    )?;
    if hash_bytes(&manifest_bytes) != expected_manifest_sha256 {
        return Err(PackImportError::InstalledPackCorrupt(
            "manifest hash differs from the registry".to_owned(),
        ));
    }
    let manifest: ContentPackManifest = serde_json::from_slice(&manifest_bytes)?;
    validate(&manifest)?;
    if canonical_manifest_bytes(&manifest)? != manifest_bytes {
        return Err(PackImportError::NonCanonicalManifest);
    }

    let declared = manifest.all_declared_assets();
    let assets_root = directory.join("assets");
    let assets_metadata = fs::symlink_metadata(&assets_root).map_err(|_| {
        PackImportError::InstalledPackCorrupt("assets directory is missing".to_owned())
    })?;
    if is_link_or_reparse(&assets_metadata) || !assets_metadata.is_dir() {
        return Err(PackImportError::InstalledPackCorrupt(
            "assets root is a link, reparse point, or not a directory".to_owned(),
        ));
    }
    let mut seen_files = HashSet::new();
    let mut seen_paths = HashSet::new();
    seen_paths.insert("assets".to_owned());
    verify_installed_tree(
        directory,
        &assets_root,
        &declared,
        &mut seen_files,
        &mut seen_paths,
    )?;
    for path in declared.keys() {
        if !seen_files.contains(&path.to_ascii_lowercase()) {
            return Err(PackImportError::MissingAsset((*path).to_owned()));
        }
    }
    Ok(manifest)
}

fn validate_install_root(directory: &Path) -> Result<(), PackImportError> {
    let root_metadata = fs::symlink_metadata(directory).map_err(|error| {
        PackImportError::InstalledPackCorrupt(format!("install root is unavailable: {error}"))
    })?;
    if is_link_or_reparse(&root_metadata) || !root_metadata.is_dir() {
        return Err(PackImportError::InstalledPackCorrupt(
            "install root is a link, reparse point, or not a directory".to_owned(),
        ));
    }

    let mut manifest_seen = false;
    let mut assets_seen = false;
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_str().ok_or_else(|| {
            PackImportError::InstalledPackCorrupt(
                "install root contains a non-UTF-8 entry".to_owned(),
            )
        })?;
        let metadata = fs::symlink_metadata(entry.path())?;
        match name {
            MANIFEST_PATH
                if !manifest_seen && !is_link_or_reparse(&metadata) && metadata.is_file() =>
            {
                manifest_seen = true;
            }
            "assets" if !assets_seen && !is_link_or_reparse(&metadata) && metadata.is_dir() => {
                assets_seen = true;
            }
            _ => {
                return Err(PackImportError::InstalledPackCorrupt(format!(
                    "install root contains unexpected, aliased, linked, or mistyped entry: {name}"
                )));
            }
        }
    }
    if !manifest_seen || !assets_seen {
        return Err(PackImportError::InstalledPackCorrupt(
            "install root must contain exactly plain manifest.json and assets entries".to_owned(),
        ));
    }
    Ok(())
}

fn verify_installed_tree(
    root: &Path,
    directory: &Path,
    declared: &std::collections::HashMap<&str, crate::manifest::DeclaredAsset>,
    seen_files: &mut HashSet<String>,
    seen_paths: &mut HashSet<String>,
) -> Result<(), PackImportError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let entry_path = entry.path();
        let relative = entry_path
            .strip_prefix(root)
            .map_err(|_| PackImportError::UnsafePath(entry_path.display().to_string()))?
            .to_str()
            .ok_or_else(|| PackImportError::UnsafePath("non-UTF-8 installed path".to_owned()))?
            .replace('\\', "/");
        let normalized = canonical_pack_path(&relative)
            .ok_or_else(|| PackImportError::UnsafePath(relative.clone()))?;
        let folded = normalized.to_ascii_lowercase();
        if !seen_paths.insert(folded.clone()) {
            return Err(PackImportError::DuplicatePath(relative));
        }
        if is_link_or_reparse(&metadata) {
            return Err(PackImportError::UnsupportedEntry(relative));
        }
        if metadata.is_dir() {
            verify_installed_tree(root, &entry_path, declared, seen_files, seen_paths)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(PackImportError::UnsupportedEntry(relative));
        }
        let asset = declared
            .iter()
            .find(|(path, _)| path.to_ascii_lowercase() == folded)
            .map(|(_, asset)| *asset)
            .ok_or_else(|| PackImportError::UndeclaredPayload(relative.clone()))?;
        if metadata.len() != asset.bytes() {
            return Err(PackImportError::AssetSizeMismatch(relative));
        }
        if !hash_file_sha256(&entry.path())?.eq_ignore_ascii_case(asset.sha256()) {
            return Err(PackImportError::HashMismatch(relative));
        }
        seen_files.insert(folded);
    }
    Ok(())
}

pub struct StagedPack {
    pub manifest: ContentPackManifest,
    pub canonical_manifest: Vec<u8>,
    pub manifest_sha256: String,
    pub archive_sha256: String,
    staging: TempDir,
}

pub struct InstalledPackFiles {
    pub directory: PathBuf,
    pub manifest_sha256: String,
    pub archive_sha256: String,
}

impl StagedPack {
    pub fn install_to(self, target: &Path) -> Result<InstalledPackFiles, PackImportError> {
        if target.exists() {
            return Err(PackImportError::TargetExists);
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        let manifest_sha256 = self.manifest_sha256.clone();
        let archive_sha256 = self.archive_sha256.clone();
        #[allow(deprecated)]
        let source = self.staging.into_path();
        if let Err(error) = fs::rename(&source, target) {
            let _ = fs::remove_dir_all(&source);
            return Err(PackImportError::Io(error));
        }
        sync_directory_after_rename(target.parent().unwrap_or_else(|| Path::new(".")))?;
        Ok(InstalledPackFiles {
            directory: target.to_path_buf(),
            manifest_sha256,
            archive_sha256,
        })
    }
}

#[cfg(unix)]
fn sync_directory_after_rename(directory: &Path) -> Result<(), PackImportError> {
    File::open(directory)?.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn sync_directory_after_rename(_directory: &Path) -> Result<(), PackImportError> {
    // Windows directory handles require platform-specific flags. Startup
    // receipt reconciliation remains authoritative on Windows.
    Ok(())
}

pub fn stage_pack(
    archive_path: &Path,
    staging_root: &Path,
    limits: ImportLimits,
) -> Result<StagedPack, PackImportError> {
    if !archive_path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("adhdpack"))
    {
        return Err(PackImportError::WrongExtension);
    }
    let archive_size = fs::metadata(archive_path)?.len();
    if archive_size > limits.max_archive_bytes {
        return Err(PackImportError::ArchiveTooLarge);
    }
    let archive_sha256 = hash_file_sha256(archive_path)?;
    let file = File::open(archive_path)?;
    let mut archive = ZipArchive::new(file)?;
    if archive.len() > limits.max_entries {
        return Err(PackImportError::TooManyEntries);
    }

    let mut names = HashSet::new();
    let mut folded_names = HashSet::new();
    let mut file_names = HashSet::new();
    let mut manifest_bytes = None;
    let mut expanded = 0u64;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let raw_name = entry.name().to_owned();
        validate_entry_type(&entry, &raw_name)?;
        if entry.is_dir() {
            return Err(PackImportError::UnsupportedEntry(raw_name));
        }
        let normalized = validate_archive_path(&raw_name)?;
        let folded = normalized.to_ascii_lowercase();
        if !names.insert(normalized.clone()) || !folded_names.insert(folded) {
            return Err(PackImportError::DuplicatePath(raw_name));
        }
        file_names.insert(normalized.clone());
        if entry.size() > limits.max_entry_bytes {
            return Err(PackImportError::EntryTooLarge(raw_name));
        }
        expanded = expanded
            .checked_add(entry.size())
            .ok_or(PackImportError::ExpandedSizeLimit)?;
        if expanded > limits.max_uncompressed_bytes {
            return Err(PackImportError::ExpandedSizeLimit);
        }
        let compressed = entry.compressed_size();
        if entry.size() > 0
            && (compressed == 0
                || entry.size() as f64 / compressed as f64 > limits.max_compression_ratio)
        {
            return Err(PackImportError::CompressionRatio(raw_name));
        }
        if raw_name == MANIFEST_PATH {
            let declared_size = entry.size();
            manifest_bytes = Some(read_manifest_bounded(
                &mut entry,
                declared_size,
                limits.max_manifest_bytes,
            )?);
        }
    }

    let manifest_bytes = manifest_bytes.ok_or(PackImportError::MissingManifest)?;
    let manifest: ContentPackManifest = serde_json::from_slice(&manifest_bytes)?;
    manifest.validate_published()?;
    let canonical_manifest = canonical_manifest_bytes(&manifest)?;
    if manifest_bytes != canonical_manifest {
        return Err(PackImportError::NonCanonicalManifest);
    }
    let declared = manifest.all_declared_assets();
    for file_name in file_names
        .iter()
        .filter(|name| name.as_str() != MANIFEST_PATH)
    {
        if !declared.contains_key(file_name.as_str()) {
            return Err(PackImportError::UndeclaredPayload(file_name.clone()));
        }
    }
    for path in declared.keys() {
        if !file_names.contains(*path) {
            return Err(PackImportError::MissingAsset((*path).to_owned()));
        }
    }

    fs::create_dir_all(staging_root)?;
    let staging = tempfile::Builder::new()
        .prefix(".adhdpack-stage-")
        .tempdir_in(staging_root)?;
    for (path, asset) in declared {
        let mut entry = archive.by_name(path)?;
        if entry.size() != asset.bytes() {
            return Err(PackImportError::AssetSizeMismatch(path.to_owned()));
        }
        let destination = staging.path().join(path);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut output = File::create(&destination)?;
        let mut hasher = Sha256::new();
        let mut copied = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = entry.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            copied += count as u64;
            if copied > asset.bytes() || copied > limits.max_entry_bytes {
                return Err(PackImportError::AssetSizeMismatch(path.to_owned()));
            }
            hasher.update(&buffer[..count]);
            output.write_all(&buffer[..count])?;
        }
        if copied != asset.bytes() {
            return Err(PackImportError::AssetSizeMismatch(path.to_owned()));
        }
        let actual = hex_digest(hasher.finalize().as_slice());
        if !actual.eq_ignore_ascii_case(asset.sha256()) {
            return Err(PackImportError::HashMismatch(path.to_owned()));
        }
    }
    fs::write(staging.path().join(MANIFEST_PATH), &canonical_manifest)?;

    Ok(StagedPack {
        manifest,
        manifest_sha256: hash_bytes(&canonical_manifest),
        canonical_manifest,
        archive_sha256,
        staging,
    })
}

fn validate_entry_type(entry: &zip::read::ZipFile<'_>, name: &str) -> Result<(), PackImportError> {
    if let Some(mode) = entry.unix_mode() {
        let kind = mode & 0o170000;
        if kind != 0 && kind != 0o100000 && kind != 0o040000 {
            return Err(PackImportError::UnsupportedEntry(name.to_owned()));
        }
    }
    Ok(())
}

fn validate_archive_path(path: &str) -> Result<String, PackImportError> {
    let normalized =
        canonical_pack_path(path).ok_or_else(|| PackImportError::UnsafePath(path.to_owned()))?;
    if normalized != MANIFEST_PATH && !normalized.starts_with("assets/") {
        return Err(PackImportError::UnsafePath(path.to_owned()));
    }
    Ok(normalized)
}

pub(crate) fn read_manifest_bounded(
    reader: &mut impl Read,
    declared_size: u64,
    maximum: u64,
) -> Result<Vec<u8>, PackImportError> {
    if declared_size > maximum {
        return Err(PackImportError::ManifestTooLarge);
    }
    let capacity = usize::try_from(declared_size.min(maximum)).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    reader.take(maximum + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        return Err(PackImportError::ManifestTooLarge);
    }
    if bytes.len() as u64 != declared_size {
        return Err(PackImportError::ManifestSizeMismatch);
    }
    Ok(bytes)
}

fn is_link_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn hash_file_sha256(path: &Path) -> Result<String, std::io::Error> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(hex_digest(hasher.finalize().as_slice()))
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex_digest(hasher.finalize().as_slice())
}

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
