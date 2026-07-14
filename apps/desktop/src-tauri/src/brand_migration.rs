use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use persistence::PreferencesRepository;

pub const LEGACY_IDENTIFIER: &str = "com.adhdmusic.desktop";
const MARKER: &str = "brand-migration-v1.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BrandMigrationStatus {
    NotNeeded,
    Migrated,
    Conflict,
    Failed,
}

#[derive(Debug, Clone)]
pub struct BrandMigrationState {
    pub status: BrandMigrationStatus,
    pub error: Option<String>,
}

impl BrandMigrationState {
    pub fn ready(status: BrandMigrationStatus) -> Self {
        Self {
            status,
            error: None,
        }
    }

    pub fn failed(error: String) -> Self {
        let status = if error.contains("both contain local data") {
            BrandMigrationStatus::Conflict
        } else {
            BrandMigrationStatus::Failed
        };
        Self {
            status,
            error: Some(error),
        }
    }
}

fn is_link_or_reparse(path: &Path) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("Local data migration could not inspect its folders: {error}"))?;
    if metadata.file_type().is_symlink() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Ok(true);
        }
    }
    Ok(false)
}

fn has_entries(path: &Path) -> Result<bool, String> {
    let mut entries = fs::read_dir(path)
        .map_err(|error| format!("Local data migration could not read its folders: {error}"))?;
    Ok(entries
        .next()
        .transpose()
        .map_err(|error| format!("Local data migration could not read a folder entry: {error}"))?
        .is_some())
}

pub fn legacy_path_for(new_path: &Path) -> Result<PathBuf, String> {
    let parent = new_path.parent().ok_or_else(|| {
        "Local data migration could not locate the application-data parent folder.".to_owned()
    })?;
    Ok(parent.join(LEGACY_IDENTIFIER))
}

pub fn prepare(new_path: &Path) -> Result<BrandMigrationStatus, String> {
    let legacy = legacy_path_for(new_path)?;
    let parent = new_path.parent().expect("legacy_path_for checked parent");
    fs::create_dir_all(parent).map_err(|error| {
        format!("Local data migration could not prepare the application-data folder: {error}")
    })?;

    let new_exists = new_path.exists();
    let legacy_exists = legacy.exists();
    if new_exists && is_link_or_reparse(new_path)? || legacy_exists && is_link_or_reparse(&legacy)?
    {
        return Err("Local data migration refuses linked or redirected application-data folders. Restore ordinary folders and retry.".into());
    }

    if new_exists && legacy_exists && has_entries(new_path)? && has_entries(&legacy)? {
        return Err("Aria Focus and ADHD Music both contain local data. Nothing was merged or deleted. Move one folder aside after making a backup, then retry.".into());
    }

    if legacy_exists && (!new_exists || !has_entries(new_path)?) {
        if new_exists {
            fs::remove_dir(new_path).map_err(|error| {
                format!("Local data migration could not remove an empty destination: {error}")
            })?;
        }
        fs::rename(&legacy, new_path).map_err(|error| {
            format!("Local data migration could not move the existing ADHD Music data. Close the old app and retry: {error}")
        })?;
        repair_installed_pack_paths(new_path, &legacy)?;
        write_marker(new_path)?;
        return Ok(BrandMigrationStatus::Migrated);
    }

    if !new_exists {
        fs::create_dir_all(new_path).map_err(|error| {
            format!("Aria Focus could not create its local data folder: {error}")
        })?;
    }
    let repaired = repair_installed_pack_paths(new_path, &legacy)?;
    if repaired > 0 && !new_path.join(MARKER).exists() {
        write_marker(new_path)?;
    }
    Ok(if repaired > 0 {
        BrandMigrationStatus::Migrated
    } else {
        BrandMigrationStatus::NotNeeded
    })
}

fn repair_installed_pack_paths(new_path: &Path, legacy_path: &Path) -> Result<usize, String> {
    let database = new_path.join("preferences.sqlite3");
    if !database.exists() {
        return Ok(0);
    }
    let mut repository = PreferencesRepository::open(&database).map_err(|error| {
        format!("Local data was moved but its preferences could not be opened safely: {error}")
    })?;
    repository
        .rebase_installed_pack_paths(legacy_path, new_path)
        .map_err(|error| {
            format!("Local data was moved but its installed music paths are unsafe: {error}")
        })
}

fn write_marker(new_path: &Path) -> Result<(), String> {
    let marker_path = new_path.join(MARKER);
    if marker_path.exists() {
        return Ok(());
    }
    let mut marker = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker_path)
        .map_err(|error| {
            format!("Local data was moved but its migration marker could not be written: {error}")
        })?;
    marker
        .write_all(b"{\"from\":\"com.adhdmusic.desktop\",\"to\":\"com.ariazanganeh.ariafocus\",\"version\":1}\n")
        .map_err(|error| {
            format!("Local data was moved but its migration marker could not be completed: {error}")
        })?;
    marker.sync_all().map_err(|error| {
        format!("Local data was moved but its migration marker could not be secured: {error}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_creates_only_the_new_directory() {
        let temp = tempfile::tempdir().unwrap();
        let new_path = temp.path().join("com.ariazanganeh.ariafocus");
        assert_eq!(prepare(&new_path).unwrap(), BrandMigrationStatus::NotNeeded);
        assert!(new_path.is_dir());
        assert!(!temp.path().join(LEGACY_IDENTIFIER).exists());
    }

    #[test]
    fn legacy_data_is_atomically_moved_with_runtime_and_marker() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join(LEGACY_IDENTIFIER);
        fs::create_dir_all(legacy.join("music-studio/runtime")).unwrap();
        fs::write(legacy.join("preferences-note"), b"preferences").unwrap();
        fs::write(legacy.join("music-studio/runtime/model.bin"), b"model").unwrap();
        let new_path = temp.path().join("com.ariazanganeh.ariafocus");

        assert_eq!(prepare(&new_path).unwrap(), BrandMigrationStatus::Migrated);
        assert!(!legacy.exists());
        assert_eq!(
            fs::read(new_path.join("preferences-note")).unwrap(),
            b"preferences"
        );
        assert_eq!(
            fs::read(new_path.join("music-studio/runtime/model.bin")).unwrap(),
            b"model"
        );
        assert!(new_path.join(MARKER).is_file());
    }

    #[test]
    fn stale_absolute_owner_waived_paths_are_rebased_after_a_prior_folder_move() {
        use persistence::{
            CatalogueRegistry, InstalledPackRecord, PackRegistration, PreferencesRepository,
            RegisteredItem,
        };

        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join(LEGACY_IDENTIFIER);
        let current = temp.path().join("com.ariazanganeh.ariafocus");
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join(MARKER), b"already moved").unwrap();
        let database = current.join("preferences.sqlite3");
        let mut repository = PreferencesRepository::open(&database).unwrap();
        for (suffix, version) in [('1', "1.0.0"), ('2', "2.0.0")] {
            let pack_id = format!("aria.library.v{suffix}");
            let version_key = suffix.to_string().repeat(64);
            let relative = PathBuf::from("content")
                .join("packs")
                .join(&pack_id)
                .join(&version_key);
            fs::create_dir_all(current.join(&relative)).unwrap();
            repository
                .register_pack(&PackRegistration {
                    pack: InstalledPackRecord {
                        pack_id: pack_id.clone(),
                        title: format!("Owner waived v{suffix}"),
                        version: version.into(),
                        manifest_sha256: "a".repeat(64),
                        archive_sha256: "b".repeat(64),
                        install_path: legacy.join(&relative).to_string_lossy().into_owned(),
                        item_count: 1,
                        status: "owner_waived_bundled_private_beta".into(),
                        canonical_manifest: "{}".into(),
                        created_at_unix_seconds: 0,
                    },
                    items: vec![RegisteredItem {
                        item_id: format!("aria-track-v{suffix}"),
                        title: "Track".into(),
                    }],
                    taxonomy: vec![],
                    generated_local_evidence: None,
                })
                .unwrap();
        }
        drop(repository);

        assert_eq!(prepare(&current).unwrap(), BrandMigrationStatus::Migrated);
        let mut repository = PreferencesRepository::open(database).unwrap();
        for record in repository.list_installed_packs().unwrap() {
            assert!(Path::new(&record.install_path).starts_with(&current));
            assert!(!Path::new(&record.install_path).starts_with(&legacy));
        }
        assert_eq!(prepare(&current).unwrap(), BrandMigrationStatus::NotNeeded);
    }

    #[test]
    fn two_nonempty_directories_fail_without_modifying_either() {
        let temp = tempfile::tempdir().unwrap();
        let legacy = temp.path().join(LEGACY_IDENTIFIER);
        let new_path = temp.path().join("com.ariazanganeh.ariafocus");
        fs::create_dir_all(&legacy).unwrap();
        fs::create_dir_all(&new_path).unwrap();
        fs::write(legacy.join("old"), b"old").unwrap();
        fs::write(new_path.join("new"), b"new").unwrap();

        let error = prepare(&new_path).unwrap_err();
        assert!(error.contains("both contain local data"));
        assert_eq!(fs::read(legacy.join("old")).unwrap(), b"old");
        assert_eq!(fs::read(new_path.join("new")).unwrap(), b"new");
    }

    #[cfg(unix)]
    #[test]
    fn linked_legacy_directory_is_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = temp.path().join("outside");
        fs::create_dir(&outside).unwrap();
        symlink(&outside, temp.path().join(LEGACY_IDENTIFIER)).unwrap();
        let new_path = temp.path().join("com.ariazanganeh.ariafocus");
        assert!(prepare(&new_path).unwrap_err().contains("refuses linked"));
    }
}
