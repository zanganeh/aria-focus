//! Verification and local installation of the bundled Music Studio runtime.
//! The public key below is the release trust anchor.  Development builds may
//! inject a key through `StudioRuntimePaths::with_public_key`; release key
//! rotation is an intentional source-asset change reviewed with the package.
use base64::{engine::general_purpose::STANDARD, Engine as _};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use music_studio_domain::{
    CapabilityState, StudioCapability, StudioHardwareInfo, StudioRuntimeInfo,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};

mod runtime_download;

const MANIFEST: &str = "package-manifest.json";
const SIGNATURE: &str = "package-manifest.sig";
const RUNTIME: &str = "runtime";
const VERIFIED_INSTALL: &str = "runtime.verified.json";
const PUBLIC_KEY: &str = include_str!("../assets/music-studio-runtime-public-key.txt");
const DISTRIBUTION_BASE: &str =
    "https://github.com/zanganeh/aria-focus/releases/download/studio-runtime-v1.0.0";
const PUBLIC_RUNTIME_REQUIRED_BYTES: u64 = 14_212_159_917;

#[derive(Debug, Clone)]
pub struct StudioRuntimePaths {
    pub package_source: PathBuf,
    pub installed: PathBuf,
    pub staging: PathBuf,
    pub resources_dir: PathBuf,
    pub database_path: PathBuf,
    distribution_base: String,
    public_key: [u8; 32],
}
impl StudioRuntimePaths {
    pub fn for_app_data(app_data: &Path, package_source: PathBuf) -> Self {
        let root = app_data.join("music-studio");
        Self {
            package_source,
            installed: root.join("runtime"),
            staging: root.join("runtime.staging"),
            resources_dir: root,
            database_path: app_data.join("preferences.sqlite3"),
            distribution_base: DISTRIBUTION_BASE.into(),
            public_key: pinned_key().unwrap_or([0; 32]),
        }
    }
    #[allow(dead_code)] // injection seam for isolated verifier/installer tests
    pub fn with_public_key(mut self, key: [u8; 32]) -> Self {
        self.public_key = key;
        self
    }
    #[allow(dead_code)]
    pub fn with_distribution_base(mut self, base: impl Into<String>) -> Self {
        self.distribution_base = base.into();
        self
    }
    pub fn draft_output_path(&self, job_id: &music_studio_domain::StudioJobId) -> PathBuf {
        self.resources_dir
            .join("drafts")
            .join(format!("{}.flac", job_id.as_str()))
    }
    pub fn job_output_dir(&self, job_id: &music_studio_domain::StudioJobId) -> PathBuf {
        self.resources_dir
            .join("studio-staging")
            .join(job_id.as_str())
    }
    pub fn job_request_path(&self, job_id: &music_studio_domain::StudioJobId) -> PathBuf {
        self.resources_dir
            .join("studio-staging")
            .join(format!("{}.request.json", job_id.as_str()))
    }
    pub fn verified_installed_runtime(&self) -> Result<(PathBuf, String), String> {
        let manifest = verify_installed_layout(self)?;
        Ok((self.installed.join(RUNTIME), manifest.runtime_version))
    }
    fn verified_marker(&self) -> PathBuf {
        self.resources_dir.join(VERIFIED_INSTALL)
    }
}
fn pinned_key() -> Result<[u8; 32], String> {
    STANDARD
        .decode(PUBLIC_KEY.trim())
        .map_err(|_| "Music Studio setup needs attention.".to_owned())
        .and_then(|v| {
            v.try_into()
                .map_err(|_| "Music Studio setup needs attention.".to_owned())
        })
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct PackageManifest {
    format: u8,
    runtime_version: String,
    required_bytes: u64,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VerifiedInstallMarker {
    format: u8,
    manifest_sha256: String,
    runtime_version: String,
}
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    path: String,
    sha256: String,
    bytes: u64,
}

fn safe_relative(value: &str) -> bool {
    let p = Path::new(value);
    !value.is_empty()
        && !p.is_absolute()
        && p.components().all(|c| matches!(c, Component::Normal(_)))
        && value.replace('\\', "/") == value
}
fn canonical_manifest_bytes(manifest: &PackageManifest) -> Result<Vec<u8>, String> {
    let mut files = Vec::with_capacity(manifest.files.len());
    for file in &manifest.files {
        files.push(format!(
            "{{\"bytes\":{},\"path\":{},\"sha256\":{}}}",
            file.bytes,
            serde_json::to_string(&file.path)
                .map_err(|_| "Music Studio setup needs attention.".to_owned())?,
            serde_json::to_string(&file.sha256)
                .map_err(|_| "Music Studio setup needs attention.".to_owned())?,
        ));
    }
    Ok(format!(
        "{{\"files\":[{}],\"format\":{},\"required_bytes\":{},\"runtime_version\":{}}}",
        files.join(","),
        manifest.format,
        manifest.required_bytes,
        serde_json::to_string(&manifest.runtime_version)
            .map_err(|_| "Music Studio setup needs attention.".to_owned())?,
    )
    .into_bytes())
}

/// `serde_json` intentionally accepts duplicate object members. Packages are a
/// signed security boundary, so scan their JSON first and reject ambiguity.
fn reject_duplicate_json_keys(input: &[u8]) -> Result<(), String> {
    struct Scanner<'a> {
        bytes: &'a [u8],
        at: usize,
    }
    impl<'a> Scanner<'a> {
        fn ws(&mut self) {
            while self.at < self.bytes.len() && self.bytes[self.at].is_ascii_whitespace() {
                self.at += 1;
            }
        }
        fn string(&mut self) -> Result<String, String> {
            if self.bytes.get(self.at) != Some(&b'\"') {
                return Err("Music Studio setup needs attention.".into());
            }
            let start = self.at;
            self.at += 1;
            let mut escaped = false;
            while let Some(&byte) = self.bytes.get(self.at) {
                self.at += 1;
                if escaped {
                    escaped = false;
                    continue;
                }
                if byte == b'\\' {
                    escaped = true;
                } else if byte == b'\"' {
                    return serde_json::from_slice(&self.bytes[start..self.at])
                        .map_err(|_| "Music Studio setup needs attention.".to_owned());
                }
            }
            Err("Music Studio setup needs attention.".into())
        }
        fn value(&mut self) -> Result<(), String> {
            self.ws();
            match self.bytes.get(self.at) {
                Some(b'{') => self.object(),
                Some(b'[') => self.array(),
                Some(b'\"') => self.string().map(|_| ()),
                Some(_) => {
                    let start = self.at;
                    while let Some(&b) = self.bytes.get(self.at) {
                        if b == b',' || b == b']' || b == b'}' || b.is_ascii_whitespace() {
                            break;
                        }
                        self.at += 1;
                    }
                    if start == self.at {
                        Err("Music Studio setup needs attention.".into())
                    } else {
                        Ok(())
                    }
                }
                None => Err("Music Studio setup needs attention.".into()),
            }
        }
        fn object(&mut self) -> Result<(), String> {
            self.at += 1;
            self.ws();
            let mut seen = BTreeSet::new();
            if self.bytes.get(self.at) == Some(&b'}') {
                self.at += 1;
                return Ok(());
            }
            loop {
                self.ws();
                let key = self.string()?;
                if !seen.insert(key) {
                    return Err("Music Studio setup needs attention.".into());
                }
                self.ws();
                if self.bytes.get(self.at) != Some(&b':') {
                    return Err("Music Studio setup needs attention.".into());
                }
                self.at += 1;
                self.value()?;
                self.ws();
                match self.bytes.get(self.at) {
                    Some(b',') => self.at += 1,
                    Some(b'}') => {
                        self.at += 1;
                        return Ok(());
                    }
                    _ => return Err("Music Studio setup needs attention.".into()),
                }
            }
        }
        fn array(&mut self) -> Result<(), String> {
            self.at += 1;
            self.ws();
            if self.bytes.get(self.at) == Some(&b']') {
                self.at += 1;
                return Ok(());
            }
            loop {
                self.value()?;
                self.ws();
                match self.bytes.get(self.at) {
                    Some(b',') => self.at += 1,
                    Some(b']') => {
                        self.at += 1;
                        return Ok(());
                    }
                    _ => return Err("Music Studio setup needs attention.".into()),
                }
            }
        }
    }
    let mut scanner = Scanner {
        bytes: input,
        at: 0,
    };
    scanner.value()?;
    scanner.ws();
    if scanner.at == input.len() {
        Ok(())
    } else {
        Err("Music Studio setup needs attention.".into())
    }
}
fn unsafe_node(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return true;
    };
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    false
}
fn hash(path: &Path) -> Result<(String, u64), String> {
    let mut f =
        fs::File::open(path).map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    let mut h = Sha256::new();
    let mut n = 0;
    let mut b = [0; 65536];
    loop {
        let count = f
            .read(&mut b)
            .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
        if count == 0 {
            break;
        };
        h.update(&b[..count]);
        n += count as u64;
    }
    Ok((format!("{:x}", h.finalize()), n))
}

/// Verifies the signed package declaration without reading 14 GB of model
/// payload. Full payload hashing remains mandatory during install and repair.
fn verify_signed_manifest(root: &Path, key: &[u8; 32]) -> Result<PackageManifest, String> {
    if unsafe_node(root) {
        return Err("Music Studio setup needs attention.".into());
    }
    let manifest_path = root.join(MANIFEST);
    let signature_path = root.join(SIGNATURE);
    let runtime = root.join(RUNTIME);
    if unsafe_node(&manifest_path) || unsafe_node(&signature_path) || unsafe_node(&runtime) {
        return Err("Music Studio setup needs attention.".into());
    }
    let mut root_entries = BTreeSet::new();
    for entry in fs::read_dir(root).map_err(|_| "Music Studio setup needs attention.".to_owned())? {
        let entry = entry.map_err(|_| "Music Studio setup needs attention.".to_owned())?;
        if unsafe_node(&entry.path()) {
            return Err("Music Studio setup needs attention.".into());
        }
        root_entries.insert(entry.file_name().to_string_lossy().to_string());
    }
    if root_entries
        != BTreeSet::from([
            MANIFEST.to_owned(),
            SIGNATURE.to_owned(),
            RUNTIME.to_owned(),
        ])
    {
        return Err("Music Studio setup needs attention.".into());
    }
    let bytes =
        fs::read(&manifest_path).map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    reject_duplicate_json_keys(&bytes)?;
    let manifest: PackageManifest = serde_json::from_slice(&bytes)
        .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    let canonical = canonical_manifest_bytes(&manifest)?;
    if bytes != canonical || manifest.format != 1 || manifest.runtime_version.trim().is_empty() {
        return Err("Music Studio setup needs attention.".into());
    }
    let sig = STANDARD
        .decode(
            fs::read_to_string(signature_path)
                .map_err(|_| "Music Studio setup needs attention.".to_owned())?
                .trim(),
        )
        .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    let key = VerifyingKey::from_bytes(key)
        .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    let sig = Signature::from_slice(&sig)
        .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    key.verify(&bytes, &sig)
        .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    let mut expected = BTreeSet::new();
    let mut sum: u64 = 0;
    for entry in &manifest.files {
        if !safe_relative(&entry.path)
            || entry.sha256.len() != 64
            || !entry.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || !expected.insert(entry.path.clone())
        {
            return Err("Music Studio setup needs attention.".into());
        };
        sum = sum
            .checked_add(entry.bytes)
            .ok_or_else(|| "Music Studio setup needs attention.".to_owned())?;
    }
    if sum != manifest.required_bytes {
        return Err("Music Studio setup needs attention.".into());
    }
    Ok(manifest)
}

/// Validates a complete package directory. The manifest bytes are deliberately
/// verified exactly as stored: package builders must emit canonical JSON bytes.
fn verify_package(root: &Path, key: &[u8; 32]) -> Result<PackageManifest, String> {
    let manifest = verify_signed_manifest(root, key)?;
    let runtime = root.join(RUNTIME);
    let mut expected = BTreeSet::new();
    for entry in &manifest.files {
        expected.insert(entry.path.clone());
        let file = runtime.join(&entry.path);
        if unsafe_node(&file) || !file.is_file() {
            return Err("Music Studio setup needs attention.".into());
        };
        let (digest, size) = hash(&file)?;
        if digest != entry.sha256 || size != entry.bytes {
            return Err("Music Studio setup needs attention.".into());
        }
    }
    let mut actual = BTreeSet::new();
    collect_files(&runtime, &runtime, &mut actual)?;
    if actual != expected {
        return Err("Music Studio setup needs attention.".into());
    }
    Ok(manifest)
}

fn manifest_sha256(root: &Path) -> Result<String, String> {
    hash(&root.join(MANIFEST)).map(|(digest, _)| digest)
}

fn verify_installed_marker(
    paths: &StudioRuntimePaths,
    manifest: &PackageManifest,
) -> Result<(), String> {
    let marker_path = paths.verified_marker();
    if unsafe_node(&marker_path) || !marker_path.is_file() {
        return Err("Music Studio setup needs attention.".into());
    }
    let bytes =
        fs::read(&marker_path).map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    reject_duplicate_json_keys(&bytes)?;
    let marker: VerifiedInstallMarker = serde_json::from_slice(&bytes)
        .map_err(|_| "Music Studio setup needs attention.".to_owned())?;
    if marker.format != 1
        || marker.runtime_version != manifest.runtime_version
        || marker.manifest_sha256 != manifest_sha256(&paths.installed)?
    {
        return Err("Music Studio setup needs attention.".into());
    }
    Ok(())
}

fn verify_installed_layout(paths: &StudioRuntimePaths) -> Result<PackageManifest, String> {
    let installed = verify_signed_manifest(&paths.installed, &paths.public_key)?;
    verify_installed_marker(paths, &installed)?;
    let runtime = paths.installed.join(RUNTIME);
    let mut sentinels = BTreeSet::new();
    if let Some(first) = installed.files.first() {
        sentinels.insert(first.path.clone());
    }
    if let Some(last) = installed.files.last() {
        sentinels.insert(last.path.clone());
    }
    for path in [
        "studio_worker.py",
        ".venv/Scripts/python.exe",
        "ace-step-source/cli.py",
        "snapshots/turbo-vae/config.json",
        "snapshots/turbo-vae/acestep-v15-turbo/model.safetensors",
        "snapshots/planner-0.6b/model.safetensors",
    ] {
        if installed.files.iter().any(|entry| entry.path == path) {
            sentinels.insert(path.to_owned());
        }
    }
    for path in sentinels {
        let entry = installed
            .files
            .iter()
            .find(|entry| entry.path == path)
            .ok_or_else(|| "Music Studio setup needs attention.".to_owned())?;
        let file = runtime.join(&entry.path);
        if unsafe_node(&file)
            || !file.is_file()
            || file
                .metadata()
                .map_err(|_| "Music Studio setup needs attention.".to_owned())?
                .len()
                != entry.bytes
        {
            return Err("Music Studio setup needs attention.".into());
        }
    }
    Ok(installed)
}

fn write_verified_marker(
    paths: &StudioRuntimePaths,
    manifest: &PackageManifest,
) -> Result<(), String> {
    fs::create_dir_all(&paths.resources_dir)
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    let marker = VerifiedInstallMarker {
        format: 1,
        manifest_sha256: manifest_sha256(&paths.installed)?,
        runtime_version: manifest.runtime_version.clone(),
    };
    let bytes = serde_json::to_vec(&marker)
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    let target = paths.verified_marker();
    let temporary = paths.resources_dir.join("runtime.verified.tmp");
    let _ = fs::remove_file(&temporary);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    use std::io::Write as _;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    drop(file);
    let _ = fs::remove_file(&target);
    fs::rename(temporary, target)
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())
}
fn collect_files(root: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|_| "Music Studio setup needs attention.".to_owned())? {
        let entry = entry.map_err(|_| "Music Studio setup needs attention.".to_owned())?;
        let path = entry.path();
        if unsafe_node(&path) {
            return Err("Music Studio setup needs attention.".into());
        };
        if path.is_dir() {
            collect_files(root, &path, out)?
        } else if path.is_file() {
            out.insert(
                path.strip_prefix(root)
                    .map_err(|_| "Music Studio setup needs attention.".to_owned())?
                    .to_string_lossy()
                    .replace('\\', "/"),
            );
        } else {
            return Err("Music Studio setup needs attention.".into());
        }
    }
    Ok(())
}

pub fn detect_capability(paths: &StudioRuntimePaths) -> StudioCapability {
    let base = StudioCapability {
        state: CapabilityState::Checking,
        runtime: StudioRuntimeInfo {
            present: false,
            version: None,
        },
        hardware: StudioHardwareInfo {
            architecture: Some(std::env::consts::ARCH.into()),
            memory_bytes: None,
            accelerator: None,
        },
        detail: None,
        required_bytes: None,
        free_bytes: None,
    };
    if let Ok(m) = verify_installed_layout(paths) {
        return StudioCapability {
            state: CapabilityState::Ready,
            runtime: StudioRuntimeInfo {
                present: true,
                version: Some(m.runtime_version),
            },
            required_bytes: Some(m.required_bytes),
            detail: Some("Music Studio is set up on this device.".into()),
            ..base
        };
    }
    match verify_signed_manifest(&paths.package_source, &paths.public_key) {
        Ok(m) => StudioCapability {
            state: CapabilityState::SetupRequired,
            required_bytes: Some(m.required_bytes),
            detail: Some("Music Studio is ready to install on this device.".into()),
            ..base
        },
        Err(_) if !paths.distribution_base.is_empty() => StudioCapability {
            state: CapabilityState::SetupRequired,
            required_bytes: Some(PUBLIC_RUNTIME_REQUIRED_BYTES),
            detail: Some("Music Studio can be downloaded when you choose to install it.".into()),
            ..base
        },
        Err(_) => StudioCapability {
            state: CapabilityState::NeedsAttention,
            detail: Some("Music Studio isn’t available in this build.".into()),
            ..base
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInstallDto {
    pub status: String,
    pub stage: String,
    pub detail: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub required_disk_bytes: Option<u64>,
    pub resumable: bool,
}
impl RuntimeInstallDto {
    fn idle() -> Self {
        Self {
            status: "idle".into(),
            stage: "waiting".into(),
            detail: "Music Studio is ready when you are.".into(),
            downloaded_bytes: 0,
            total_bytes: None,
            required_disk_bytes: None,
            resumable: false,
        }
    }
}
#[derive(Clone)]
pub struct RuntimeInstaller {
    paths: StudioRuntimePaths,
    state: Arc<Mutex<RuntimeInstallDto>>,
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
    lock: Arc<Mutex<()>>,
}
impl RuntimeInstaller {
    pub fn new(paths: StudioRuntimePaths) -> Self {
        Self {
            paths,
            state: Arc::new(Mutex::new(RuntimeInstallDto::idle())),
            cancel: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            lock: Arc::new(Mutex::new(())),
        }
    }
    pub fn state(&self) -> RuntimeInstallDto {
        self.state
            .lock()
            .map(|x| x.clone())
            .unwrap_or_else(|_| RuntimeInstallDto::idle())
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
        if self.running.load(Ordering::SeqCst) {
            if let Ok(mut state) = self.state.lock() {
                state.detail = "Cancelling Music Studio setup.".into();
            }
        }
    }
    pub fn start(&self, repair: bool) -> Result<RuntimeInstallDto, String> {
        if self
            .running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(self.state());
        }
        self.cancel.store(false, Ordering::SeqCst);
        let initial = RuntimeInstallDto {
            status: "installing".into(),
            stage: "checking".into(),
            detail: "Checking Music Studio setup files.".into(),
            downloaded_bytes: 0,
            total_bytes: None,
            required_disk_bytes: None,
            resumable: false,
        };
        let Ok(mut state) = self.state.lock() else {
            self.running.store(false, Ordering::SeqCst);
            return Err("Music Studio is busy.".to_owned());
        };
        *state = initial.clone();
        drop(state);
        let installer = self.clone();
        if std::thread::Builder::new()
            .name("music-studio-setup".into())
            .spawn(move || {
                let result = installer.install(repair);
                if let Err(message) = result {
                    cleanup(&installer.paths.staging);
                    if let Ok(mut state) = installer.state.lock() {
                        *state = RuntimeInstallDto {
                            status: "idle".into(),
                            stage: "waiting".into(),
                            detail: message,
                            downloaded_bytes: 0,
                            total_bytes: None,
                            required_disk_bytes: None,
                            resumable: true,
                        };
                    }
                }
                installer.running.store(false, Ordering::SeqCst);
            })
            .is_err()
        {
            self.running.store(false, Ordering::SeqCst);
            if let Ok(mut state) = self.state.lock() {
                *state = RuntimeInstallDto::idle();
            }
            return Err("Music Studio setup could not be started.".into());
        }
        Ok(initial)
    }
    fn install(&self, repair: bool) -> Result<RuntimeInstallDto, String> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| "Music Studio is busy.".to_owned())?;
        cleanup(&self.paths.staging);
        let source = if let Ok(source) =
            verify_package(&self.paths.package_source, &self.paths.public_key)
        {
            let mut s = self
                .state
                .lock()
                .map_err(|_| "Music Studio is busy.".to_owned())?;
            *s = RuntimeInstallDto {
                status: "installing".into(),
                stage: "installing".into(),
                detail: "Preparing Music Studio on this device.".into(),
                downloaded_bytes: 0,
                total_bytes: None,
                required_disk_bytes: Some(source.required_bytes),
                resumable: false,
            };
            drop(s);
            copy_package(
                &self.paths.package_source,
                &self.paths.staging,
                &self.cancel,
            )?;
            source
        } else {
            runtime_download::download_and_extract(&self.paths, &self.state, &self.cancel)?
        };
        *self
            .state
            .lock()
            .map_err(|_| "Music Studio is busy.".to_owned())? = RuntimeInstallDto {
            status: "installing".into(),
            stage: "checking".into(),
            detail: "Checking the installed Music Studio files.".into(),
            downloaded_bytes: 0,
            total_bytes: None,
            required_disk_bytes: Some(source.required_bytes),
            resumable: false,
        };
        verify_package(&self.paths.staging, &self.paths.public_key)
            .map_err(|_| "Music Studio setup could not be verified.".to_owned())?;
        if self.cancel.load(Ordering::SeqCst) {
            cleanup(&self.paths.staging);
            return Err("Music Studio setup was cancelled.".into());
        }
        if self.paths.installed.exists() {
            if !repair {
                cleanup(&self.paths.staging);
                return Err("Music Studio is already installed.".into());
            }
            let backup = self.paths.installed.with_extension("previous");
            cleanup(&backup);
            fs::rename(&self.paths.installed, &backup)
                .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
            fs::rename(&self.paths.staging, &self.paths.installed)
                .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
            cleanup(&backup);
        } else {
            fs::rename(&self.paths.staging, &self.paths.installed)
                .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
        }
        write_verified_marker(&self.paths, &source)?;
        let dto = RuntimeInstallDto {
            status: "complete".into(),
            stage: "complete".into(),
            detail: format!(
                "Music Studio {} is ready on this device.",
                source.runtime_version
            ),
            downloaded_bytes: 0,
            total_bytes: None,
            required_disk_bytes: Some(source.required_bytes),
            resumable: false,
        };
        *self
            .state
            .lock()
            .map_err(|_| "Music Studio is busy.".to_owned())? = dto.clone();
        Ok(dto)
    }
}

fn cleanup(path: &Path) {
    if path.exists() {
        let _ = fs::remove_dir_all(path);
    }
}

fn copy_package(source: &Path, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    fs::create_dir_all(dest)
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    for name in [MANIFEST, SIGNATURE] {
        if cancel.load(Ordering::SeqCst) {
            cleanup(dest);
            return Err("Music Studio setup was cancelled.".into());
        };
        fs::copy(source.join(name), dest.join(name))
            .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    }
    copy_dir(&source.join(RUNTIME), &dest.join(RUNTIME), cancel)
}
fn copy_dir(source: &Path, dest: &Path, cancel: &AtomicBool) -> Result<(), String> {
    fs::create_dir_all(dest)
        .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
    for e in
        fs::read_dir(source).map_err(|_| "Music Studio setup could not be completed.".to_owned())?
    {
        if cancel.load(Ordering::SeqCst) {
            return Err("Music Studio setup was cancelled.".into());
        };
        let e = e.map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
        let p = e.path();
        if unsafe_node(&p) {
            return Err("Music Studio setup could not be completed.".into());
        };
        let target = dest.join(e.file_name());
        if p.is_dir() {
            copy_dir(&p, &target, cancel)?
        } else if p.is_file() {
            let mut i = fs::File::open(p)
                .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
            let mut o = fs::File::create(target)
                .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
            std::io::copy(&mut i, &mut o)
                .map_err(|_| "Music Studio setup could not be completed.".to_owned())?;
        } else {
            return Err("Music Studio setup could not be completed.".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod installer_tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    fn signed_package(root: &Path, signing: &SigningKey) {
        let runtime = root.join(RUNTIME);
        fs::create_dir_all(&runtime).unwrap();
        let payload = b"tiny-runtime";
        fs::write(runtime.join("worker.bin"), payload).unwrap();
        let manifest = PackageManifest {
            format: 1,
            runtime_version: "test-v1".into(),
            required_bytes: payload.len() as u64,
            files: vec![ManifestFile {
                path: "worker.bin".into(),
                sha256: format!("{:x}", Sha256::digest(payload)),
                bytes: payload.len() as u64,
            }],
        };
        let bytes = canonical_manifest_bytes(&manifest).unwrap();
        fs::write(root.join(MANIFEST), &bytes).unwrap();
        fs::write(
            root.join(SIGNATURE),
            format!("{}\n", STANDARD.encode(signing.sign(&bytes).to_bytes())),
        )
        .unwrap();
    }

    #[test]
    fn setup_starts_without_blocking_and_reports_background_failure() {
        let temp = tempfile::tempdir().unwrap();
        let installer = RuntimeInstaller::new(
            StudioRuntimePaths::for_app_data(temp.path(), temp.path().join("missing-package"))
                .with_distribution_base(""),
        );

        let started = installer.start(false).unwrap();
        assert_eq!(started.status, "installing");
        assert_eq!(started.stage, "checking");

        for _ in 0..100 {
            if installer.state().status == "idle" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let finished = installer.state();
        assert_eq!(finished.status, "idle");
        assert_eq!(finished.stage, "waiting");
        assert_eq!(
            finished.detail,
            "Music Studio download location is invalid."
        );
    }

    #[test]
    fn cancellation_has_a_reachable_customer_state() {
        let temp = tempfile::tempdir().unwrap();
        let installer = RuntimeInstaller::new(StudioRuntimePaths::for_app_data(
            temp.path(),
            temp.path().join("package"),
        ));
        installer.running.store(true, Ordering::SeqCst);
        *installer.state.lock().unwrap() = RuntimeInstallDto {
            status: "installing".into(),
            stage: "installing".into(),
            detail: "Preparing Music Studio on this device.".into(),
            downloaded_bytes: 0,
            total_bytes: None,
            required_disk_bytes: None,
            resumable: false,
        };

        installer.cancel();

        assert!(installer.cancel.load(Ordering::SeqCst));
        assert_eq!(installer.state().detail, "Cancelling Music Studio setup.");
    }

    #[test]
    fn verified_marker_makes_capability_fast_and_layout_corruption_visible() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        let app_data = temp.path().join("app-data");
        let signing = SigningKey::from_bytes(&[7; 32]);
        signed_package(&package, &signing);
        let paths = StudioRuntimePaths::for_app_data(&app_data, package)
            .with_public_key(signing.verifying_key().to_bytes());

        assert_eq!(
            detect_capability(&paths).state,
            CapabilityState::SetupRequired
        );
        RuntimeInstaller::new(paths.clone()).install(false).unwrap();
        assert!(paths.verified_marker().is_file());
        assert_eq!(detect_capability(&paths).state, CapabilityState::Ready);
        assert!(paths.verified_installed_runtime().is_ok());

        fs::write(paths.installed.join(RUNTIME).join("worker.bin"), b"short").unwrap();
        assert_eq!(
            detect_capability(&paths).state,
            CapabilityState::SetupRequired
        );
        assert!(paths.verified_installed_runtime().is_err());
    }

    #[test]
    fn full_install_hashing_rejects_payload_changed_after_signing() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        let app_data = temp.path().join("app-data");
        let signing = SigningKey::from_bytes(&[8; 32]);
        signed_package(&package, &signing);
        fs::write(package.join(RUNTIME).join("worker.bin"), b"tampered-run").unwrap();
        let paths = StudioRuntimePaths::for_app_data(&app_data, package)
            .with_public_key(signing.verifying_key().to_bytes());

        assert!(RuntimeInstaller::new(paths.clone()).install(false).is_err());
        assert!(!paths.verified_marker().exists());
        assert!(!paths.installed.exists());
    }
}
