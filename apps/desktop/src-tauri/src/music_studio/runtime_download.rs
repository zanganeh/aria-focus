use super::{
    reject_duplicate_json_keys, safe_relative, verify_package, PackageManifest, RuntimeInstallDto,
    StudioRuntimePaths,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::blocking::{Client, Response};
use reqwest::header::{CONTENT_RANGE, RANGE};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::Duration;

const DISTRIBUTION_MANIFEST: &str = "runtime-distribution.json";
const DISTRIBUTION_SIGNATURE: &str = "runtime-distribution.sig";
const MAX_DISTRIBUTION_DOCUMENT_BYTES: u64 = 1024 * 1024;
const MAX_CHUNK_BYTES: u64 = 1_900_000_000;
const DISK_SAFETY_BYTES: u64 = 1_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DistributionManifest {
    format: u8,
    runtime_version: String,
    package_manifest_sha256: String,
    required_bytes: u64,
    download_bytes: u64,
    chunks: Vec<DistributionChunk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DistributionChunk {
    index: u16,
    file_name: String,
    bytes: u64,
    sha256: String,
}

fn canonical_bytes(manifest: &DistributionManifest) -> Result<Vec<u8>, String> {
    serde_json::to_vec(manifest)
        .map_err(|_| "Music Studio download information is invalid.".to_owned())
}

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn validate_manifest(manifest: &DistributionManifest) -> Result<(), String> {
    if manifest.format != 1
        || manifest.runtime_version.is_empty()
        || manifest.runtime_version.len() > 64
        || !manifest
            .runtime_version
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        || !valid_sha256(&manifest.package_manifest_sha256)
        || manifest.required_bytes == 0
        || manifest.chunks.is_empty()
        || manifest.chunks.len() > 1_000
    {
        return Err("Music Studio download information is invalid.".into());
    }
    let mut total = 0_u64;
    for (position, chunk) in manifest.chunks.iter().enumerate() {
        if usize::from(chunk.index) != position
            || !safe_relative(&chunk.file_name)
            || chunk.file_name.contains('/')
            || !chunk.file_name.ends_with(".part")
            || chunk.bytes == 0
            || chunk.bytes > MAX_CHUNK_BYTES
            || !valid_sha256(&chunk.sha256)
        {
            return Err("Music Studio download information is invalid.".into());
        }
        total = total
            .checked_add(chunk.bytes)
            .ok_or_else(|| "Music Studio download information is invalid.".to_owned())?;
    }
    if total != manifest.download_bytes {
        return Err("Music Studio download information is invalid.".into());
    }
    Ok(())
}

fn verify_distribution(
    manifest_bytes: &[u8],
    signature_bytes: &[u8],
    public_key: &[u8; 32],
) -> Result<DistributionManifest, String> {
    reject_duplicate_json_keys(manifest_bytes)?;
    let manifest: DistributionManifest = serde_json::from_slice(manifest_bytes)
        .map_err(|_| "Music Studio download information is invalid.".to_owned())?;
    validate_manifest(&manifest)?;
    if canonical_bytes(&manifest)? != manifest_bytes {
        return Err("Music Studio download information is not canonical.".into());
    }
    let encoded = std::str::from_utf8(signature_bytes)
        .map_err(|_| "Music Studio download signature is invalid.".to_owned())?
        .trim();
    let signature: [u8; 64] = STANDARD
        .decode(encoded)
        .map_err(|_| "Music Studio download signature is invalid.".to_owned())?
        .try_into()
        .map_err(|_| "Music Studio download signature is invalid.".to_owned())?;
    VerifyingKey::from_bytes(public_key)
        .and_then(|key| key.verify(manifest_bytes, &Signature::from_bytes(&signature)))
        .map_err(|_| "Music Studio download signature is invalid.".to_owned())?;
    Ok(manifest)
}

fn validate_base_url(base: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(base)
        .map_err(|_| "Music Studio download location is invalid.".to_owned())?;
    let permitted = parsed.scheme() == "https" && parsed.host_str() == Some("github.com");
    #[cfg(test)]
    let permitted = permitted
        || (parsed.scheme() == "http"
            && matches!(parsed.host_str(), Some("127.0.0.1" | "localhost")));
    if !permitted || parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("Music Studio download location is invalid.".into());
    }
    Ok(base.trim_end_matches('/').to_owned())
}

fn fetch_bounded(client: &Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .and_then(Response::error_for_status)
        .map_err(|_| {
            "Music Studio download could not connect. Check your connection and retry.".to_owned()
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DISTRIBUTION_DOCUMENT_BYTES)
    {
        return Err("Music Studio download information is too large.".into());
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_DISTRIBUTION_DOCUMENT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Music Studio download information could not be read.".to_owned())?;
    if bytes.len() as u64 > MAX_DISTRIBUTION_DOCUMENT_BYTES {
        return Err("Music Studio download information is too large.".into());
    }
    Ok(bytes)
}

fn hash_file(path: &Path) -> Result<(String, u64), String> {
    let mut file = File::open(path)
        .map_err(|_| "Music Studio downloaded file could not be checked.".to_owned())?;
    let mut hash = Sha256::new();
    let mut bytes = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| "Music Studio downloaded file could not be checked.".to_owned())?;
        if read == 0 {
            break;
        }
        hash.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((format!("{:x}", hash.finalize()), bytes))
}

fn update_state(
    state: &Arc<Mutex<RuntimeInstallDto>>,
    stage: &str,
    detail: String,
    downloaded_bytes: u64,
    total_bytes: u64,
    required_disk_bytes: u64,
) -> Result<(), String> {
    *state
        .lock()
        .map_err(|_| "Music Studio is busy.".to_owned())? = RuntimeInstallDto {
        status: "installing".into(),
        stage: stage.into(),
        detail,
        downloaded_bytes,
        total_bytes: Some(total_bytes),
        required_disk_bytes: Some(required_disk_bytes),
        resumable: true,
    };
    Ok(())
}

fn existing_downloaded(directory: &Path, chunks: &[DistributionChunk]) -> u64 {
    chunks
        .iter()
        .map(|chunk| {
            let complete = directory.join(&chunk.file_name);
            let partial = directory.join(format!("{}.partial", chunk.file_name));
            if complete.is_file() {
                complete.metadata().map(|value| value.len()).unwrap_or(0)
            } else {
                partial
                    .metadata()
                    .map(|value| value.len().min(chunk.bytes))
                    .unwrap_or(0)
            }
        })
        .sum()
}

struct DownloadContext<'a> {
    client: &'a Client,
    base: &'a str,
    directory: &'a Path,
    manifest: &'a DistributionManifest,
    state: &'a Arc<Mutex<RuntimeInstallDto>>,
    cancel: &'a AtomicBool,
    required_disk_bytes: u64,
}

fn download_chunk(
    context: &DownloadContext<'_>,
    chunk: &DistributionChunk,
    completed_before: u64,
) -> Result<(), String> {
    let complete = context.directory.join(&chunk.file_name);
    if complete.is_file() {
        let (hash, bytes) = hash_file(&complete)?;
        if hash == chunk.sha256 && bytes == chunk.bytes {
            return Ok(());
        }
        fs::remove_file(&complete)
            .map_err(|_| "A damaged Music Studio download could not be replaced.".to_owned())?;
    }
    let partial = context
        .directory
        .join(format!("{}.partial", chunk.file_name));
    let mut offset = partial.metadata().map(|value| value.len()).unwrap_or(0);
    if offset > chunk.bytes {
        fs::remove_file(&partial)
            .map_err(|_| "A damaged Music Studio download could not be replaced.".to_owned())?;
        offset = 0;
    }
    if offset == chunk.bytes {
        let (hash, bytes) = hash_file(&partial)?;
        if hash == chunk.sha256 && bytes == chunk.bytes {
            fs::rename(&partial, &complete)
                .map_err(|_| "Music Studio download could not be finalized.".to_owned())?;
            return Ok(());
        }
        fs::remove_file(&partial)
            .map_err(|_| "A damaged Music Studio download could not be replaced.".to_owned())?;
        offset = 0;
    }

    let url = format!("{}/{}", context.base, chunk.file_name);
    let mut request = context.client.get(url);
    if offset > 0 {
        request = request.header(RANGE, format!("bytes={offset}-"));
    }
    let mut response = request
        .send()
        .map_err(|_| "Music Studio download was interrupted. Retry to resume.".to_owned())?;
    if offset > 0 && response.status() == reqwest::StatusCode::PARTIAL_CONTENT {
        let prefix = format!("bytes {offset}-");
        if !response
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with(&prefix))
        {
            return Err("Music Studio download resume response was invalid.".into());
        }
    } else if response.status().is_success() {
        offset = 0;
    } else {
        return Err("Music Studio download was interrupted. Retry to resume.".into());
    }
    let mut output = OpenOptions::new()
        .create(true)
        .write(true)
        .append(offset > 0)
        .truncate(offset == 0)
        .open(&partial)
        .map_err(|_| "Music Studio download could not be saved.".to_owned())?;
    let mut written = offset;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        if context.cancel.load(Ordering::SeqCst) {
            output
                .sync_all()
                .map_err(|_| "Music Studio download could not be paused safely.".to_owned())?;
            return Err("Music Studio setup was cancelled. Retry to resume the download.".into());
        }
        let read = response
            .read(&mut buffer)
            .map_err(|_| "Music Studio download was interrupted. Retry to resume.".to_owned())?;
        if read == 0 {
            break;
        }
        written = written
            .checked_add(read as u64)
            .ok_or_else(|| "Music Studio download is too large.".to_owned())?;
        if written > chunk.bytes {
            return Err("Music Studio download exceeded its signed size.".into());
        }
        output
            .write_all(&buffer[..read])
            .map_err(|_| "Music Studio download could not be saved.".to_owned())?;
        update_state(
            context.state,
            "downloading",
            format!(
                "Downloading Music Studio file {} of {}.",
                usize::from(chunk.index) + 1,
                context.manifest.chunks.len()
            ),
            completed_before + written,
            context.manifest.download_bytes,
            context.required_disk_bytes,
        )?;
    }
    output
        .sync_all()
        .map_err(|_| "Music Studio download could not be secured.".to_owned())?;
    drop(output);
    let (hash, bytes) = hash_file(&partial)?;
    if bytes != chunk.bytes || hash != chunk.sha256 {
        let _ = fs::remove_file(&partial);
        return Err("Music Studio download failed its integrity check. Retry the download.".into());
    }
    fs::rename(partial, complete)
        .map_err(|_| "Music Studio download could not be finalized.".to_owned())?;
    Ok(())
}

struct ChunkReader {
    paths: Vec<PathBuf>,
    index: usize,
    current: Option<File>,
}

impl ChunkReader {
    fn new(paths: Vec<PathBuf>) -> Self {
        Self {
            paths,
            index: 0,
            current: None,
        }
    }
}

impl Read for ChunkReader {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.current.is_none() {
                let Some(path) = self.paths.get(self.index) else {
                    return Ok(0);
                };
                self.current = Some(File::open(path)?);
                self.index += 1;
            }
            let read = self.current.as_mut().expect("current set").read(buffer)?;
            if read > 0 {
                return Ok(read);
            }
            self.current = None;
        }
    }
}

fn extract_chunks(
    chunks: &[PathBuf],
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|_| "Music Studio download could not be unpacked.".to_owned())?;
    let reader = ChunkReader::new(chunks.to_vec());
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|_| "Music Studio download archive is invalid.".to_owned())?;
    for entry in entries {
        if cancel.load(Ordering::SeqCst) {
            return Err("Music Studio setup was cancelled. Retry to resume the download.".into());
        }
        let mut entry =
            entry.map_err(|_| "Music Studio download archive is invalid.".to_owned())?;
        let path = entry
            .path()
            .map_err(|_| "Music Studio download archive is invalid.".to_owned())?;
        let relative = path
            .to_str()
            .filter(|value| safe_relative(value))
            .ok_or_else(|| "Music Studio download archive contains an unsafe path.".to_owned())?
            .to_owned();
        let target = destination.join(&relative);
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|_| "Music Studio download could not be unpacked.".to_owned())?;
        } else if kind.is_file() {
            if target.exists() {
                return Err("Music Studio download archive contains duplicate files.".into());
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .map_err(|_| "Music Studio download could not be unpacked.".to_owned())?;
            }
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(target)
                .map_err(|_| "Music Studio download could not be unpacked.".to_owned())?;
            std::io::copy(&mut entry, &mut output)
                .and_then(|_| output.sync_all())
                .map_err(|_| "Music Studio download could not be unpacked.".to_owned())?;
        } else {
            return Err("Music Studio download archive contains an unsupported entry.".into());
        }
    }
    Ok(())
}

pub(super) fn download_and_extract(
    paths: &StudioRuntimePaths,
    state: &Arc<Mutex<RuntimeInstallDto>>,
    cancel: &AtomicBool,
) -> Result<PackageManifest, String> {
    let base = validate_base_url(&paths.distribution_base)?;
    let client = Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .timeout(Duration::from_secs(120))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|_| "Music Studio download could not start.".to_owned())?;
    update_state(
        state,
        "checking_download",
        "Checking Music Studio download information.".into(),
        0,
        0,
        0,
    )?;
    let manifest_bytes = fetch_bounded(&client, &format!("{base}/{DISTRIBUTION_MANIFEST}"))?;
    let signature_bytes = fetch_bounded(&client, &format!("{base}/{DISTRIBUTION_SIGNATURE}"))?;
    let manifest = verify_distribution(&manifest_bytes, &signature_bytes, &paths.public_key)?;
    fs::create_dir_all(&paths.resources_dir)
        .map_err(|_| "Music Studio download folder could not be created.".to_owned())?;
    let download_dir = paths
        .resources_dir
        .join("downloads")
        .join(&manifest.runtime_version);
    fs::create_dir_all(&download_dir)
        .map_err(|_| "Music Studio download folder could not be created.".to_owned())?;
    let already_downloaded = existing_downloaded(&download_dir, &manifest.chunks);
    let remaining_download = manifest.download_bytes.saturating_sub(already_downloaded);
    let required_disk_bytes = remaining_download
        .checked_add(manifest.required_bytes)
        .and_then(|value| value.checked_add(DISK_SAFETY_BYTES))
        .ok_or_else(|| "Music Studio disk requirement is invalid.".to_owned())?;
    let free = fs2::available_space(&paths.resources_dir)
        .map_err(|_| "Music Studio could not check available disk space.".to_owned())?;
    if free < required_disk_bytes {
        return Err(format!(
            "Music Studio needs about {} GB of free space before setup can continue.",
            required_disk_bytes.div_ceil(1_000_000_000)
        ));
    }

    let mut completed = 0_u64;
    let context = DownloadContext {
        client: &client,
        base: &base,
        directory: &download_dir,
        manifest: &manifest,
        state,
        cancel,
        required_disk_bytes,
    };
    for chunk in &manifest.chunks {
        download_chunk(&context, chunk, completed)?;
        completed += chunk.bytes;
    }
    update_state(
        state,
        "extracting",
        "Preparing Music Studio on this device.".into(),
        manifest.download_bytes,
        manifest.download_bytes,
        required_disk_bytes,
    )?;
    let chunk_paths = manifest
        .chunks
        .iter()
        .map(|chunk| download_dir.join(&chunk.file_name))
        .collect::<Vec<_>>();
    extract_chunks(&chunk_paths, &paths.staging, cancel)?;
    let package = verify_package(&paths.staging, &paths.public_key)
        .map_err(|_| "Music Studio download failed its package verification.".to_owned())?;
    if package.runtime_version != manifest.runtime_version
        || package.required_bytes != manifest.required_bytes
        || super::manifest_sha256(&paths.staging)? != manifest.package_manifest_sha256
    {
        return Err("Music Studio download does not match its signed package information.".into());
    }
    fs::remove_dir_all(&download_dir).map_err(|_| {
        "Music Studio is ready, but downloaded setup files could not be removed.".to_owned()
    })?;
    Ok(package)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use std::collections::BTreeMap;
    use std::io::{BufRead, BufReader};
    use std::net::TcpListener;
    use std::sync::atomic::AtomicBool;
    use std::thread;

    fn signed_distribution(signing: &SigningKey) -> (Vec<u8>, Vec<u8>) {
        let manifest = DistributionManifest {
            format: 1,
            runtime_version: "test-v1".into(),
            package_manifest_sha256: "a".repeat(64),
            required_bytes: 12,
            download_bytes: 5,
            chunks: vec![DistributionChunk {
                index: 0,
                file_name: "runtime-000.part".into(),
                bytes: 5,
                sha256: "b".repeat(64),
            }],
        };
        let bytes = canonical_bytes(&manifest).unwrap();
        let signature = format!("{}\n", STANDARD.encode(signing.sign(&bytes).to_bytes()));
        (bytes, signature.into_bytes())
    }

    #[test]
    fn distribution_manifest_is_strict_canonical_and_signed() {
        let signing = SigningKey::from_bytes(&[17; 32]);
        let (bytes, signature) = signed_distribution(&signing);
        let verified =
            verify_distribution(&bytes, &signature, &signing.verifying_key().to_bytes()).unwrap();
        assert_eq!(verified.runtime_version, "test-v1");

        let mut changed = bytes.clone();
        let at = changed.iter().position(|byte| *byte == b'5').unwrap();
        changed[at] = b'6';
        assert!(
            verify_distribution(&changed, &signature, &signing.verifying_key().to_bytes()).is_err()
        );
    }

    #[test]
    fn split_tar_stream_extracts_plain_files_across_chunk_boundaries() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("archive.tar");
        {
            let file = File::create(&archive_path).unwrap();
            let mut builder = tar::Builder::new(file);
            let bytes = b"runtime";
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, "runtime/worker.bin", &bytes[..])
                .unwrap();
            builder.finish().unwrap();
        }
        let archive = fs::read(&archive_path).unwrap();
        let first = temp.path().join("first.part");
        let second = temp.path().join("second.part");
        fs::write(&first, &archive[..archive.len() / 2]).unwrap();
        fs::write(&second, &archive[archive.len() / 2..]).unwrap();
        let destination = temp.path().join("out");
        extract_chunks(&[first, second], &destination, &AtomicBool::new(false)).unwrap();
        assert_eq!(
            fs::read(destination.join("runtime/worker.bin")).unwrap(),
            b"runtime"
        );
    }

    #[test]
    fn extraction_rejects_symbolic_links() {
        let temp = tempfile::tempdir().unwrap();
        let archive_path = temp.path().join("archive.tar");
        {
            let file = File::create(&archive_path).unwrap();
            let mut builder = tar::Builder::new(file);
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(tar::EntryType::Symlink);
            header.set_size(0);
            header.set_mode(0o777);
            header.set_link_name("../outside").unwrap();
            header.set_cksum();
            builder
                .append_data(&mut header, "runtime/link", std::io::empty())
                .unwrap();
            builder.finish().unwrap();
        }

        let destination = temp.path().join("out");
        let error =
            extract_chunks(&[archive_path], &destination, &AtomicBool::new(false)).unwrap_err();
        assert!(error.contains("unsupported entry"));
        assert!(!destination.join("runtime/link").exists());
    }

    #[test]
    fn resumable_http_download_verifies_extracts_and_removes_parts() {
        let temp = tempfile::tempdir().unwrap();
        let signing = SigningKey::from_bytes(&[23; 32]);
        let package = temp.path().join("package");
        fs::create_dir_all(package.join("runtime")).unwrap();
        let payload = b"tiny verified runtime";
        fs::write(package.join("runtime/worker.bin"), payload).unwrap();
        let package_manifest = super::super::PackageManifest {
            format: 1,
            runtime_version: "test-download-v1".into(),
            required_bytes: payload.len() as u64,
            files: vec![super::super::ManifestFile {
                path: "worker.bin".into(),
                sha256: format!("{:x}", Sha256::digest(payload)),
                bytes: payload.len() as u64,
            }],
        };
        let package_bytes = super::super::canonical_manifest_bytes(&package_manifest).unwrap();
        fs::write(package.join(super::super::MANIFEST), &package_bytes).unwrap();
        fs::write(
            package.join(super::super::SIGNATURE),
            format!(
                "{}\n",
                STANDARD.encode(signing.sign(&package_bytes).to_bytes())
            ),
        )
        .unwrap();
        let mut builder = tar::Builder::new(Vec::new());
        for relative in [
            super::super::MANIFEST,
            super::super::SIGNATURE,
            "runtime/worker.bin",
        ] {
            builder
                .append_path_with_name(package.join(relative), relative)
                .unwrap();
        }
        builder.finish().unwrap();
        let archive = builder.into_inner().unwrap();
        let split = archive.len() / 2;
        let data_parts = [&archive[..split], &archive[split..]];
        let chunks = data_parts
            .iter()
            .enumerate()
            .map(|(index, bytes)| DistributionChunk {
                index: index as u16,
                file_name: format!("runtime-{index:03}.part"),
                bytes: bytes.len() as u64,
                sha256: format!("{:x}", Sha256::digest(bytes)),
            })
            .collect::<Vec<_>>();
        let distribution = DistributionManifest {
            format: 1,
            runtime_version: package_manifest.runtime_version.clone(),
            package_manifest_sha256: format!("{:x}", Sha256::digest(&package_bytes)),
            required_bytes: package_manifest.required_bytes,
            download_bytes: chunks.iter().map(|chunk| chunk.bytes).sum(),
            chunks: chunks.clone(),
        };
        let distribution_bytes = canonical_bytes(&distribution).unwrap();
        let signature = format!(
            "{}\n",
            STANDARD.encode(signing.sign(&distribution_bytes).to_bytes())
        )
        .into_bytes();
        let mut responses = BTreeMap::from([
            (
                format!("/{DISTRIBUTION_MANIFEST}"),
                distribution_bytes.clone(),
            ),
            (format!("/{DISTRIBUTION_SIGNATURE}"), signature),
        ]);
        for (chunk, bytes) in chunks.iter().zip(data_parts) {
            responses.insert(format!("/{}", chunk.file_name), bytes.to_vec());
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let saw_range = Arc::new(AtomicBool::new(false));
        let server_range = Arc::clone(&saw_range);
        let request_count = 2 + chunks.len();
        let server = thread::spawn(move || {
            for _ in 0..request_count {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut request_line = String::new();
                reader.read_line(&mut request_line).unwrap();
                let path = request_line.split_whitespace().nth(1).unwrap().to_owned();
                let mut range = None;
                loop {
                    let mut line = String::new();
                    reader.read_line(&mut line).unwrap();
                    if line == "\r\n" || line.is_empty() {
                        break;
                    }
                    if line.to_ascii_lowercase().starts_with("range:") {
                        let start = line
                            .split('=')
                            .nth(1)
                            .and_then(|value| value.split('-').next())
                            .unwrap()
                            .trim()
                            .parse::<usize>()
                            .unwrap();
                        range = Some(start);
                        server_range.store(true, Ordering::SeqCst);
                    }
                }
                let bytes = responses.get(&path).unwrap();
                let start = range.unwrap_or(0);
                let body = &bytes[start..];
                let header = if range.is_some() {
                    format!(
                        "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\nContent-Range: bytes {}-{}/{}\r\nConnection: close\r\n\r\n",
                        body.len(), start, bytes.len() - 1, bytes.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                };
                stream.write_all(header.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });

        let paths = StudioRuntimePaths::for_app_data(temp.path(), temp.path().join("unused"))
            .with_public_key(signing.verifying_key().to_bytes())
            .with_distribution_base(format!("http://{address}"));
        let download_dir = paths
            .resources_dir
            .join("downloads")
            .join(&distribution.runtime_version);
        fs::create_dir_all(&download_dir).unwrap();
        let partial_bytes = data_parts[0].len() / 2;
        fs::write(
            download_dir.join(format!("{}.partial", chunks[0].file_name)),
            &data_parts[0][..partial_bytes],
        )
        .unwrap();
        let state = Arc::new(Mutex::new(RuntimeInstallDto::idle()));
        let installed = download_and_extract(&paths, &state, &AtomicBool::new(false)).unwrap();
        server.join().unwrap();
        assert_eq!(installed, package_manifest);
        assert!(saw_range.load(Ordering::SeqCst));
        assert_eq!(
            fs::read(paths.staging.join("runtime/worker.bin")).unwrap(),
            payload
        );
        assert!(!download_dir.exists());
    }
}
