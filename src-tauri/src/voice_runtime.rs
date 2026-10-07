use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;
use zip::ZipArchive;

pub const WORKER_API_VERSION: u32 = 1;

const RUNTIME_DIR: &str = "runtime";
const CURRENT_DIR: &str = "current";
const STAGING_DIR: &str = "staging";
const PREVIOUS_DIR: &str = "previous";
const INSTALL_FILE: &str = "install.json";
const DOWNLOAD_FILE: &str = "runtime.download.zip";
const DOWNLOAD_CANCELLED: &str = "download cancelled";
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRuntimeSource {
    Release,
    DevOverride,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceRuntimeStatus {
    pub target: String,
    pub install_dir: String,
    pub installed: bool,
    pub executable_path: Option<String>,
    pub version: Option<String>,
    pub worker_api: Option<u32>,
    pub source: VoiceRuntimeSource,
    pub required_bytes: Option<u64>,
    pub installed_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct VoiceRuntimeDownloadProgress {
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub current_file: Option<String>,
}

pub struct WorkerLaunch {
    pub program: PathBuf,
    pub prefix_args: Vec<PathBuf>,
}

#[derive(Clone, Debug, Deserialize)]
struct VoicePackManifest {
    version: String,
    worker_api: u32,
    runtimes: Vec<RuntimeAsset>,
}

#[derive(Clone, Debug, Deserialize)]
struct RuntimeAsset {
    target: String,
    url: String,
    sha256: String,
    bytes: u64,
    executable: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct RuntimeInstall {
    version: String,
    target: String,
    worker_api: u32,
    executable: String,
    archive_sha256: String,
    archive_bytes: u64,
}

pub fn status(root: &Path) -> VoiceRuntimeStatus {
    if let Some(launch) = dev_worker_launch() {
        return VoiceRuntimeStatus {
            target: runtime_target(),
            install_dir: runtime_root(root).to_string_lossy().into_owned(),
            installed: true,
            executable_path: Some(launch.program.to_string_lossy().into_owned()),
            version: Some("dev-override".to_string()),
            worker_api: Some(WORKER_API_VERSION),
            source: VoiceRuntimeSource::DevOverride,
            required_bytes: Some(0),
            installed_bytes: 0,
        };
    }

    match installed_runtime(root) {
        Ok(Some((install, executable))) => VoiceRuntimeStatus {
            target: runtime_target(),
            install_dir: current_runtime_dir(root).to_string_lossy().into_owned(),
            installed: true,
            executable_path: Some(executable.to_string_lossy().into_owned()),
            version: Some(install.version),
            worker_api: Some(install.worker_api),
            source: VoiceRuntimeSource::Release,
            required_bytes: Some(install.archive_bytes),
            installed_bytes: dir_size(&current_runtime_dir(root)).unwrap_or(0),
        },
        _ => VoiceRuntimeStatus {
            target: runtime_target(),
            install_dir: current_runtime_dir(root).to_string_lossy().into_owned(),
            installed: false,
            executable_path: None,
            version: None,
            worker_api: None,
            source: VoiceRuntimeSource::Missing,
            required_bytes: None,
            installed_bytes: dir_size(&current_runtime_dir(root)).unwrap_or(0),
        },
    }
}

pub fn worker_launch(root: &Path) -> Result<WorkerLaunch, String> {
    if let Some(launch) = dev_worker_launch() {
        return Ok(launch);
    }

    let Some((_install, executable)) = installed_runtime(root)? else {
        return Err("download the voice runtime before generating clips".to_string());
    };

    Ok(WorkerLaunch {
        program: executable,
        prefix_args: Vec::new(),
    })
}

pub fn install_runtime(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(VoiceRuntimeDownloadProgress),
) -> Result<VoiceRuntimeStatus, String> {
    let current = status(root);
    if current.installed {
        progress(VoiceRuntimeDownloadProgress {
            downloaded_bytes: current.required_bytes.unwrap_or(0),
            total_bytes: current.required_bytes.unwrap_or(0),
            current_file: None,
        });
        return Ok(current);
    }

    let manifest = fetch_manifest()?;
    let target = runtime_target();
    let asset = select_runtime_asset(&manifest, &target)?;

    fs::create_dir_all(runtime_root(root)).map_err(|error| error.to_string())?;
    let archive_path = runtime_root(root).join(DOWNLOAD_FILE);
    let download = download_asset(&asset, &archive_path, Arc::clone(&cancel), |downloaded| {
        progress(VoiceRuntimeDownloadProgress {
            downloaded_bytes: downloaded,
            total_bytes: asset.bytes,
            current_file: Some(format!("voice runtime {}", asset.target)),
        });
    });
    if let Err(error) = download {
        let _ = fs::remove_file(&archive_path);
        return Err(error);
    }

    if cancel.load(Ordering::SeqCst) {
        let _ = fs::remove_file(&archive_path);
        return Err(DOWNLOAD_CANCELLED.to_string());
    }

    install_archive(
        root,
        &archive_path,
        &manifest.version,
        &target,
        &asset,
        &cancel,
    )?;

    Ok(status(root))
}

pub fn delete_runtime(root: &Path) -> Result<(), String> {
    let dir = runtime_root(root);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn run_worker_command(root: &Path) -> Result<Command, String> {
    let launch = worker_launch(root)?;
    let mut command = Command::new(launch.program);
    for arg in launch.prefix_args {
        command.arg(arg);
    }
    Ok(command)
}

pub fn runtime_target() -> String {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        other => other,
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" if os == "macos" => "arm64",
        "x86_64" => "x64",
        other => other,
    };
    format!("{os}-{arch}")
}

fn fetch_manifest() -> Result<VoicePackManifest, String> {
    let url = manifest_url()
        .ok_or_else(|| "voice pack manifest URL is not configured for this build".to_string())?;
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| error.to_string())?;
    let body = client
        .get(url)
        .send()
        .map_err(|error| error.to_string())?
        .error_for_status()
        .map_err(|error| error.to_string())?
        .text()
        .map_err(|error| error.to_string())?;
    serde_json::from_str::<VoicePackManifest>(&body).map_err(|error| error.to_string())
}

fn select_runtime_asset(
    manifest: &VoicePackManifest,
    target: &str,
) -> Result<RuntimeAsset, String> {
    if manifest.worker_api != WORKER_API_VERSION {
        return Err(format!(
            "voice runtime manifest API {} is not supported by this app",
            manifest.worker_api
        ));
    }

    let mut matches = manifest
        .runtimes
        .iter()
        .filter(|runtime| runtime.target == target);
    let asset = matches
        .next()
        .ok_or_else(|| format!("no voice runtime is published for {target}"))?;
    if matches.next().is_some() {
        return Err(format!(
            "voice runtime manifest contains duplicate target {target}"
        ));
    }
    if asset.bytes == 0 {
        return Err(format!("voice runtime {target} has an invalid byte count"));
    }
    if asset.sha256.len() != 64 || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("voice runtime {target} has an invalid checksum"));
    }
    if !asset.url.starts_with("https://") {
        return Err(format!("voice runtime {target} must use HTTPS"));
    }
    let executable = asset.executable.as_deref().unwrap_or_else(|| {
        if target.starts_with("windows-") {
            "supervisor-voice-worker.exe"
        } else {
            "supervisor-voice-worker"
        }
    });
    if safe_relative_path(executable).is_none() {
        return Err(format!(
            "voice runtime {target} has an unsafe executable path"
        ));
    }

    Ok(asset.clone())
}

fn manifest_url() -> Option<String> {
    std::env::var("SUPERVISOR_VOICE_PACK_MANIFEST_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
        .or_else(|| {
            option_env!("SUPERVISOR_VOICE_PACK_MANIFEST_URL")
                .map(str::to_string)
                .filter(|url| !url.trim().is_empty())
        })
}

fn install_archive(
    root: &Path,
    archive_path: &Path,
    version: &str,
    target: &str,
    asset: &RuntimeAsset,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let staging = runtime_root(root).join(STAGING_DIR);
    let result = (|| {
        let bytes = fs::metadata(archive_path)
            .map_err(|error| error.to_string())?
            .len();
        if bytes != asset.bytes {
            return Err(format!(
                "downloaded wrong voice runtime byte count: expected {}, got {}",
                asset.bytes, bytes
            ));
        }
        if sha256_file(archive_path)? != asset.sha256 {
            return Err("downloaded voice runtime checksum mismatch".to_string());
        }
        if cancel.load(Ordering::SeqCst) {
            return Err(DOWNLOAD_CANCELLED.to_string());
        }

        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
        }
        fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
        if let Err(error) = extract_zip(archive_path, &staging) {
            let _ = fs::remove_dir_all(&staging);
            return Err(error);
        }
        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_dir_all(&staging);
            return Err(DOWNLOAD_CANCELLED.to_string());
        }

        let executable = asset.executable.clone().unwrap_or_else(default_executable);
        let executable_path = staging.join(&executable);
        if !executable_path.is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(format!("voice runtime is missing executable: {executable}"));
        }
        make_executable(&executable_path)?;

        let had_previous = replace_runtime_dir(root, &staging)?;
        if let Err(error) = write_json(
            &runtime_root(root).join(INSTALL_FILE),
            &RuntimeInstall {
                version: version.to_string(),
                target: target.to_string(),
                worker_api: WORKER_API_VERSION,
                executable,
                archive_sha256: asset.sha256.clone(),
                archive_bytes: asset.bytes,
            },
        ) {
            restore_previous_runtime(root, had_previous);
            return Err(error);
        }
        remove_previous_runtime(root);
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    let _ = fs::remove_file(archive_path);
    result
}

fn replace_runtime_dir(root: &Path, staging: &Path) -> Result<bool, String> {
    let current = current_runtime_dir(root);
    let previous = runtime_root(root).join(PREVIOUS_DIR);
    if previous.exists() {
        fs::remove_dir_all(&previous).map_err(|error| error.to_string())?;
    }
    let had_previous = current.exists();
    if had_previous {
        fs::rename(&current, &previous).map_err(|error| error.to_string())?;
    }

    if let Err(error) = fs::rename(staging, &current) {
        if previous.exists() {
            let _ = fs::rename(&previous, &current);
        }
        return Err(error.to_string());
    }
    Ok(had_previous)
}

fn restore_previous_runtime(root: &Path, had_previous: bool) {
    let current = current_runtime_dir(root);
    let previous = runtime_root(root).join(PREVIOUS_DIR);
    let _ = fs::remove_dir_all(&current);
    if had_previous {
        let _ = fs::rename(previous, current);
    }
}

fn remove_previous_runtime(root: &Path) {
    let previous = runtime_root(root).join(PREVIOUS_DIR);
    if previous.exists() {
        let _ = fs::remove_dir_all(previous);
    }
}

fn download_asset(
    asset: &RuntimeAsset,
    path: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(u64),
) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .timeout(DOWNLOAD_TOTAL_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    if cancel.load(Ordering::SeqCst) {
        return Err(DOWNLOAD_CANCELLED.to_string());
    }
    let mut response = tauri::async_runtime::block_on(async {
        client.get(&asset.url).send().await?.error_for_status()
    })
    .map_err(|error| error.to_string())?;
    let mut writer = File::create(path).map_err(|error| error.to_string())?;
    let mut downloaded = 0u64;
    progress(downloaded);

    loop {
        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(path);
            return Err(DOWNLOAD_CANCELLED.to_string());
        }
        let Some(chunk) = tauri::async_runtime::block_on(response.chunk()).map_err(|error| {
            if cancel.load(Ordering::SeqCst) {
                DOWNLOAD_CANCELLED.to_string()
            } else {
                error.to_string()
            }
        })?
        else {
            break;
        };
        writer
            .write_all(&chunk)
            .map_err(|error| error.to_string())?;
        downloaded += chunk.len() as u64;
        progress(downloaded);
    }
    writer.flush().map_err(|error| error.to_string())?;

    if downloaded != asset.bytes {
        let _ = fs::remove_file(path);
        return Err(format!(
            "downloaded wrong voice runtime byte count: expected {}, got {}",
            asset.bytes, downloaded
        ));
    }
    Ok(())
}

fn installed_runtime(root: &Path) -> Result<Option<(RuntimeInstall, PathBuf)>, String> {
    let install_path = runtime_root(root).join(INSTALL_FILE);
    let Ok(bytes) = fs::read(&install_path) else {
        return Ok(None);
    };
    let install =
        serde_json::from_slice::<RuntimeInstall>(&bytes).map_err(|error| error.to_string())?;
    if install.target != runtime_target() || install.worker_api != WORKER_API_VERSION {
        return Ok(None);
    }
    let executable = safe_relative_path(&install.executable)
        .ok_or_else(|| "voice runtime install record has an unsafe executable path".to_string())?;
    let executable = current_runtime_dir(root).join(executable);
    if !executable.is_file() {
        return Ok(None);
    }
    Ok(Some((install, executable)))
}

fn extract_zip(archive_path: &Path, out_dir: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|error| error.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        let Some(relative) = safe_relative_path(entry.name()) else {
            return Err(format!(
                "voice runtime archive contains unsafe path: {}",
                entry.name()
            ));
        };
        let out_path = out_dir.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out_path).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = File::create(&out_path).map_err(|error| error.to_string())?;
        io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;

        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out_path, fs::Permissions::from_mode(mode))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute() {
        return None;
    }
    let mut safe = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => safe.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!safe.as_os_str().is_empty()).then_some(safe)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mut permissions = fs::metadata(path)
        .map_err(|error| error.to_string())?
        .permissions();
    permissions.set_mode(permissions.mode() | 0o755);
    fs::set_permissions(path, permissions).map_err(|error| error.to_string())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn dev_worker_launch() -> Option<WorkerLaunch> {
    if let Some(worker) = std::env::var_os("SUPERVISOR_VOICE_WORKER").map(PathBuf::from) {
        let python = std::env::var_os("SUPERVISOR_VOICE_PYTHON").map(PathBuf::from);
        return worker_launch_from_paths(worker, python);
    }

    debug_worker_launch()
}

fn worker_launch_from_paths(worker: PathBuf, python: Option<PathBuf>) -> Option<WorkerLaunch> {
    let worker = resolve_dev_path(worker)?;
    let python = python
        .and_then(resolve_dev_path)
        .or_else(|| inferred_python_for_worker(&worker));
    let is_python_script = worker
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("py"));

    if is_python_script {
        let python = python?;
        return Some(WorkerLaunch {
            program: python,
            prefix_args: vec![worker],
        });
    }

    if let Some(python) = python {
        return Some(WorkerLaunch {
            program: python,
            prefix_args: vec![worker],
        });
    }

    Some(WorkerLaunch {
        program: worker,
        prefix_args: Vec::new(),
    })
}

fn resolve_dev_path(path: PathBuf) -> Option<PathBuf> {
    let candidates = if path.is_absolute() {
        vec![path]
    } else {
        let mut candidates = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd.join(&path));
        }
        if let Some(root) = dev_repo_root() {
            candidates.push(root.join(path));
        }
        candidates
    };

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .map(|candidate| candidate.canonicalize().unwrap_or(candidate))
}

fn inferred_python_for_worker(worker: &Path) -> Option<PathBuf> {
    let dir = worker.parent()?;
    let python = if cfg!(target_os = "windows") {
        dir.join(".venv").join("Scripts").join("python.exe")
    } else {
        dir.join(".venv").join("bin").join("python")
    };
    python
        .is_file()
        .then(|| python.canonicalize().unwrap_or(python))
}

fn debug_worker_launch() -> Option<WorkerLaunch> {
    #[cfg(debug_assertions)]
    {
        let root = dev_repo_root()?;
        let worker = root.join("voice_worker").join("chatterbox_nano_worker.py");
        return worker_launch_from_paths(worker, debug_worker_python(&root));
    }

    #[cfg(not(debug_assertions))]
    {
        None
    }
}

#[cfg(debug_assertions)]
fn debug_worker_python(root: &Path) -> Option<PathBuf> {
    let platform_venv = if cfg!(target_os = "windows") {
        ".venv-voice-runtime-windows"
    } else if cfg!(target_os = "macos") {
        ".venv-voice-runtime-macos"
    } else {
        ".venv-voice-runtime-linux"
    };
    let executable = if cfg!(target_os = "windows") {
        PathBuf::from("Scripts").join("python.exe")
    } else {
        PathBuf::from("bin").join("python")
    };

    [root.join(platform_venv), root.join(".venv-voice-runtime")]
        .into_iter()
        .map(|venv| venv.join(&executable))
        .find(|python| python.is_file())
}

fn dev_repo_root() -> Option<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
}

fn default_executable() -> String {
    if cfg!(target_os = "windows") {
        "supervisor-voice-worker.exe".to_string()
    } else {
        "supervisor-voice-worker".to_string()
    }
}

fn runtime_root(root: &Path) -> PathBuf {
    root.join(RUNTIME_DIR)
}

fn current_runtime_dir(root: &Path) -> PathBuf {
    runtime_root(root).join(CURRENT_DIR)
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 1024 * 64];
    loop {
        let read = file.read(&mut buffer).map_err(|error| error.to_string())?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn dir_size(path: &Path) -> io::Result<u64> {
    if !path.exists() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in fs::read_dir(path)? {
        let path = entry?.path();
        let metadata = fs::metadata(&path)?;
        if metadata.is_dir() {
            total += dir_size(&path)?;
        } else {
            total += metadata.len();
        }
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use zip::write::SimpleFileOptions;

    #[test]
    fn target_uses_release_arch_names() {
        let target = runtime_target();
        assert!(target.contains('-'));
        assert!(!target.ends_with("-x86_64"));
        assert!(!target.ends_with("-aarch64"));
    }

    #[test]
    fn safe_relative_path_rejects_zip_slip() {
        assert_eq!(
            safe_relative_path("supervisor-voice-worker").unwrap(),
            PathBuf::from("supervisor-voice-worker")
        );
        assert!(safe_relative_path("../escape").is_none());
        assert!(safe_relative_path("/tmp/escape").is_none());
    }

    #[test]
    fn manifest_rejects_unsafe_or_ambiguous_runtime_assets() {
        let target = runtime_target();
        let mut manifest = manifest_for(&target);
        assert!(select_runtime_asset(&manifest, &target).is_ok());

        manifest.runtimes.push(manifest.runtimes[0].clone());
        assert!(select_runtime_asset(&manifest, &target)
            .unwrap_err()
            .contains("duplicate target"));

        manifest.runtimes.pop();
        manifest.runtimes[0].executable = Some("../worker".to_string());
        assert!(select_runtime_asset(&manifest, &target)
            .unwrap_err()
            .contains("unsafe executable"));
    }

    #[test]
    fn installs_synthetic_runtime_archive_without_launching_worker() {
        let root = test_dir("archive-install");
        let archive = root.join("runtime.zip");
        fs::create_dir_all(&root).unwrap();
        write_test_archive(&archive, &default_executable());
        let asset = asset_for_archive(&archive);

        install_archive(
            &root,
            &archive,
            "test-version",
            &runtime_target(),
            &asset,
            &AtomicBool::new(false),
        )
        .unwrap();

        let (install, executable) = installed_runtime(&root).unwrap().unwrap();
        assert_eq!(install.version, "test-version");
        assert!(executable.is_file());
        assert!(!archive.exists());

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn invalid_archive_does_not_replace_existing_runtime() {
        let root = test_dir("archive-preserve");
        let current = current_runtime_dir(&root);
        let archive = root.join("runtime.zip");
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("keep.txt"), b"keep").unwrap();
        write_test_archive(&archive, "wrong-worker");
        let asset = asset_for_archive(&archive);

        let error = install_archive(
            &root,
            &archive,
            "test-version",
            &runtime_target(),
            &asset,
            &AtomicBool::new(false),
        )
        .unwrap_err();

        assert!(error.contains("missing executable"));
        assert_eq!(fs::read(current.join("keep.txt")).unwrap(), b"keep");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn metadata_write_failure_restores_existing_runtime() {
        let root = test_dir("metadata-rollback");
        let current = current_runtime_dir(&root);
        let archive = root.join("runtime.zip");
        fs::create_dir_all(&current).unwrap();
        fs::write(current.join("keep.txt"), b"keep").unwrap();
        fs::create_dir(runtime_root(&root).join(INSTALL_FILE)).unwrap();
        write_test_archive(&archive, &default_executable());
        let asset = asset_for_archive(&archive);

        assert!(install_archive(
            &root,
            &archive,
            "test-version",
            &runtime_target(),
            &asset,
            &AtomicBool::new(false),
        )
        .is_err());
        assert_eq!(fs::read(current.join("keep.txt")).unwrap(), b"keep");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn python_worker_uses_resolved_sibling_venv() {
        let root = test_dir("worker-launch");
        let worker = root.join("worker.py");
        let python = if cfg!(target_os = "windows") {
            root.join(".venv").join("Scripts").join("python.exe")
        } else {
            root.join(".venv").join("bin").join("python")
        };
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::write(&worker, b"print('ok')").unwrap();
        fs::write(&python, b"").unwrap();

        let launch = worker_launch_from_paths(worker.clone(), None).unwrap();

        assert_eq!(launch.program, python.canonicalize().unwrap());
        assert_eq!(launch.prefix_args, vec![worker.canonicalize().unwrap()]);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_dev_worker_is_not_reported_available() {
        assert!(worker_launch_from_paths(test_dir("missing").join("worker.py"), None).is_none());
    }

    fn manifest_for(target: &str) -> VoicePackManifest {
        VoicePackManifest {
            version: "test-version".to_string(),
            worker_api: WORKER_API_VERSION,
            runtimes: vec![RuntimeAsset {
                target: target.to_string(),
                url: "https://example.invalid/runtime.zip".to_string(),
                sha256: "0".repeat(64),
                bytes: 1,
                executable: Some(default_executable()),
            }],
        }
    }

    fn asset_for_archive(path: &Path) -> RuntimeAsset {
        RuntimeAsset {
            target: runtime_target(),
            url: "https://example.invalid/runtime.zip".to_string(),
            sha256: sha256_file(path).unwrap(),
            bytes: fs::metadata(path).unwrap().len(),
            executable: Some(default_executable()),
        }
    }

    fn write_test_archive(path: &Path, executable: &str) {
        let file = File::create(path).unwrap();
        let mut archive = zip::ZipWriter::new(file);
        archive
            .start_file(executable, SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"synthetic worker").unwrap();
        archive.finish().unwrap();
    }

    #[cfg(debug_assertions)]
    #[test]
    fn debug_worker_prefers_runtime_venv_at_repo_root() {
        let root = test_dir("runtime-venv");
        let venv = if cfg!(target_os = "windows") {
            root.join(".venv-voice-runtime-windows")
        } else if cfg!(target_os = "macos") {
            root.join(".venv-voice-runtime-macos")
        } else {
            root.join(".venv-voice-runtime-linux")
        };
        let python = if cfg!(target_os = "windows") {
            venv.join("Scripts").join("python.exe")
        } else {
            venv.join("bin").join("python")
        };
        fs::create_dir_all(python.parent().unwrap()).unwrap();
        fs::write(&python, b"").unwrap();

        assert_eq!(debug_worker_python(&root).unwrap(), python);

        let _ = fs::remove_dir_all(root);
    }

    fn test_dir(name: &str) -> PathBuf {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or(0);
        std::env::temp_dir().join(format!(
            "supervisor-runtime-{name}-{}-{millis}",
            std::process::id()
        ))
    }
}
