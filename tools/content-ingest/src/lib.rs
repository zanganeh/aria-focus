//! Local pack builder. It hashes supplied files and packages them; it never
//! downloads, synthesizes, or modifies audio content.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use catalogue::{canonical_manifest_bytes, hash_file_sha256, ContentPackManifest, MANIFEST_PATH};
use tempfile::NamedTempFile;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

#[derive(Debug, thiserror::Error)]
pub enum BuildError {
    #[error("output must use the .adhdpack extension")]
    WrongExtension,
    #[error("output already exists; refusing to overwrite it")]
    OutputExists,
    #[error("source asset is missing, outside the source directory, a symlink, or not a regular file: {0}")]
    UnsafeSource(String),
    #[error(transparent)]
    ManifestJson(#[from] serde_json::Error),
    #[error(transparent)]
    ManifestValidation(#[from] catalogue::ManifestValidationError),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn build_pack(source: &Path, manifest_path: &Path, output: &Path) -> Result<(), BuildError> {
    if !output
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("adhdpack"))
    {
        return Err(BuildError::WrongExtension);
    }
    if output.exists() {
        return Err(BuildError::OutputExists);
    }
    let source = source.canonicalize()?;
    if !source.is_dir() {
        return Err(BuildError::UnsafeSource(source.display().to_string()));
    }
    let mut manifest: ContentPackManifest = serde_json::from_reader(File::open(manifest_path)?)?;
    for item in &mut manifest.items {
        for variant in &mut item.variants {
            let candidate = checked_source_asset(&source, &variant.asset.path)?;
            variant.asset.bytes = fs::metadata(&candidate)?.len();
            variant.asset.sha256 = hash_file_sha256(&candidate)?;
        }
    }
    manifest.validate_published()?;
    let manifest = manifest.canonicalized();
    let canonical = canonical_manifest_bytes(&manifest)?;

    let output_parent = output.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(output_parent)?;
    let mut temporary = NamedTempFile::new_in(output_parent)?;
    {
        let mut archive = ZipWriter::new(&mut temporary);
        let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        archive.start_file(MANIFEST_PATH, stored)?;
        archive.write_all(&canonical)?;
        let mut paths = manifest
            .declared_assets()
            .keys()
            .map(|path| (*path).to_owned())
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let candidate = checked_source_asset(&source, &path)?;
            archive.start_file(&path, stored)?;
            let mut input = File::open(candidate)?;
            io::copy(&mut input, &mut archive)?;
        }
        archive.finish()?;
    }
    temporary.as_file_mut().sync_all()?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| match error.error.kind() {
            io::ErrorKind::AlreadyExists => BuildError::OutputExists,
            _ => BuildError::Io(error.error),
        })?;
    Ok(())
}

fn checked_source_asset(source: &Path, pack_path: &str) -> Result<PathBuf, BuildError> {
    let normalized = catalogue::canonical_pack_path(pack_path)
        .filter(|path| path.starts_with("assets/"))
        .ok_or_else(|| BuildError::UnsafeSource(pack_path.to_owned()))?;
    let candidate = source.join(&normalized);
    let metadata = fs::symlink_metadata(&candidate)
        .map_err(|_| BuildError::UnsafeSource(pack_path.to_owned()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(BuildError::UnsafeSource(pack_path.to_owned()));
    }
    let canonical = candidate
        .canonicalize()
        .map_err(|_| BuildError::UnsafeSource(pack_path.to_owned()))?;
    if !canonical.starts_with(source) {
        return Err(BuildError::UnsafeSource(pack_path.to_owned()));
    }
    Ok(canonical)
}

pub fn usage(program: &str) -> String {
    format!(
        "Usage: {program} --source <directory> --manifest <manifest.json> --output <pack.adhdpack>"
    )
}

pub fn parse_args(
    args: impl IntoIterator<Item = String>,
) -> Result<(PathBuf, PathBuf, PathBuf), String> {
    let mut source = None;
    let mut manifest = None;
    let mut output = None;
    let mut arguments = args.into_iter();
    let program = arguments
        .next()
        .unwrap_or_else(|| "content-ingest".to_owned());
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(|| usage(&program))?;
        match flag.as_str() {
            "--source" if source.is_none() => source = Some(PathBuf::from(value)),
            "--manifest" if manifest.is_none() => manifest = Some(PathBuf::from(value)),
            "--output" if output.is_none() => output = Some(PathBuf::from(value)),
            _ => return Err(usage(&program)),
        }
    }
    match (source, manifest, output) {
        (Some(source), Some(manifest), Some(output)) => Ok((source, manifest, output)),
        _ => Err(usage(&program)),
    }
}

#[cfg(test)]
mod tests {
    use catalogue::{stage_pack, ImportLimits};
    use serde_json::json;
    use tempfile::TempDir;

    use super::*;

    fn write_manifest(path: &Path) {
        let manifest = json!({
            "format":"adhdpack","format_version":1,
            "pack":{"id":"ingest.fixture","title":"Ingest Fixture","description":"Generated byte fixture","version":"1.0.0","app_version_requirement":">=0.1.0"},
            "taxonomy":{"genres":[{"id":"ambient","label":"Ambient"}],"moods":[{"id":"steady","label":"Steady"}]},
            "items":[{
                "id":"ingest-item","title":"Ingest Item","genre_ids":["ambient"],"mood_ids":["steady"],"activity_suitability":[{"activity":"deep_work","suitability":0.8},{"activity":"motivation","suitability":0.6},{"activity":"creativity","suitability":0.7},{"activity":"learning","suitability":0.8},{"activity":"light_work","suitability":0.5}],
                "provenance":{"source":"generated test bytes","licence_id":"CC0-1.0","licence_url":null,"composer":"Test","generator":null,"contains_lyrics":false,"contains_speech":false},
                "analysis":{"duration_seconds":60.0,"integrated_lufs":-20.0,"true_peak_dbfs":-3.0,"loudness_range_lu":4.0,"spectral_centroid_hz":1000.0,"high_frequency_energy_ratio":0.1,"onset_density_per_second":1.0,"tempo_bpm":80.0,"tempo_confidence":0.9,"tempo_drift_percent":1.0,"section_change_novelty":0.1,"unexplained_silence_seconds":0.0,"clipped_samples":0,"discontinuity_detected":false,"codec_errors_detected":false,"corruption_detected":false,"vocal_speech_likelihood":0.0},
                "variants":[{"id":"base","asset":{"path":"assets/fixture.wav","sha256":"placeholder","bytes":0,"codec":"wav","sample_rate_hz":48000,"channels":2,"bit_depth":16},"safe_regions":[{"kind":"loop","start_seconds":1.0,"end_seconds":59.0}],"stimulation_available":["off","low"]}],
                "human_qa":{"status":"approved","reviews":[{"reviewer_id":"a","reviewed_at":"2026-01-01","notes":"representative fixture","representative_work_session":true},{"reviewer_id":"b","reviewed_at":"2026-01-02","notes":"representative fixture","representative_work_session":true}]}
            }]
        });
        fs::write(path, serde_json::to_vec_pretty(&manifest).unwrap()).unwrap();
    }

    #[test]
    fn builds_a_canonical_pack_that_the_production_importer_accepts() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("assets")).unwrap();
        fs::write(source.join("assets/fixture.wav"), b"fixture bytes only").unwrap();
        let manifest = temp.path().join("source-manifest.json");
        write_manifest(&manifest);
        let output = temp.path().join("fixture.adhdpack");

        build_pack(&source, &manifest, &output).unwrap();
        let staged = stage_pack(
            &output,
            &temp.path().join("staging"),
            ImportLimits::default(),
        )
        .unwrap();
        assert_eq!(staged.manifest.pack.id, "ingest.fixture");
        assert_eq!(staged.manifest.items[0].variants[0].asset.bytes, 18);
        assert!(matches!(
            build_pack(&source, &manifest, &output),
            Err(BuildError::OutputExists)
        ));
    }

    #[test]
    fn rejects_missing_assets_and_invalid_arguments() {
        let temp = TempDir::new().unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(&source).unwrap();
        let manifest = temp.path().join("manifest.json");
        write_manifest(&manifest);
        assert!(matches!(
            build_pack(&source, &manifest, &temp.path().join("bad.adhdpack")),
            Err(BuildError::UnsafeSource(_))
        ));
        assert!(parse_args(["content-ingest".to_owned(), "--source".to_owned()]).is_err());
    }
}
