use crate::voice_runtime;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::SampleFormat;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

const MODEL_ID: &str = "chatterbox-nano-minimal";
const MODEL_DISPLAY_NAME: &str = "Chatterbox-Nano voice cloning model";
const MODEL_REPO: &str = "ResembleAI/chatterbox-nano";
const MODEL_REVISION: &str = "71ccd1d0081b430592cea481f4307e764e07bc64";
const GIBIBYTE: u64 = 1024 * 1024 * 1024;
const MIN_REQUIRED_RAM_BYTES: u64 = 8 * GIBIBYTE;
// ponytail: 5 GB is 3.3 GB measured worker peak plus headroom, rounded up. Provisional until generation_log.jsonl has more runs.
#[cfg(not(target_os = "macos"))]
const DEFAULT_MIN_AVAILABLE_RAM_BYTES: u64 = 5 * GIBIBYTE;
// macOS "available" includes inactive cache pages, so 5 GB is not the same as Windows ullAvailPhys.
#[cfg(target_os = "macos")]
const DEFAULT_MIN_AVAILABLE_RAM_BYTES: u64 = 6 * GIBIBYTE;
#[cfg(not(target_os = "macos"))]
const MIN_AVAILABLE_RAM_AFTER_MEASURE_BYTES: u64 = 4 * GIBIBYTE;
#[cfg(target_os = "macos")]
const MIN_AVAILABLE_RAM_AFTER_MEASURE_BYTES: u64 = 5 * GIBIBYTE;
const GENERATION_PEAK_HEADROOM_BYTES: u64 = GIBIBYTE;
// ponytail: 1 GB extra while the model is already resident. Replace after generation_log warm runs.
const LOADED_WORKER_HEADROOM_BYTES: u64 = GIBIBYTE;
const WORKER_IDLE_TIMEOUT: Duration = Duration::from_secs(120);
const EXTRA_DOWNLOAD_HEADROOM_BYTES: u64 = 512 * 1024 * 1024;
const MAX_REFERENCE_AUDIO_BYTES: usize = 60 * 1024 * 1024;
const DOWNLOAD_CANCELLED: &str = "download cancelled";
const MAX_VOICE_GENERATION: Duration = Duration::from_secs(30 * 60);
const DOWNLOAD_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const DOWNLOAD_READ_TIMEOUT: Duration = Duration::from_secs(10);
const DOWNLOAD_TOTAL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const STARTER_PROFILES: &[(&str, &str, &str)] = &[
    ("derek", "Starter Male", "derek.wav"),
    ("elise", "Starter Female", "elise.wav"),
];

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceDownloadPhase {
    Idle,
    Downloading,
    Cancelling,
    Complete,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceDownloadSnapshot {
    pub phase: VoiceDownloadPhase,
    pub downloaded_bytes: u64,
    pub total_bytes: u64,
    pub current_file: Option<String>,
    pub elapsed_seconds: u64,
    pub eta_seconds: Option<u64>,
    pub message: String,
    pub model: VoiceModelStatus,
    pub runtime: voice_runtime::VoiceRuntimeStatus,
}

#[derive(Debug)]
pub struct VoiceDownloadState {
    phase: VoiceDownloadPhase,
    downloaded_bytes: u64,
    started_downloaded_bytes: u64,
    total_bytes: u64,
    current_file: Option<String>,
    started_at: Option<Instant>,
    error: Option<String>,
    cancel: Option<Arc<AtomicBool>>,
}

impl Default for VoiceDownloadState {
    fn default() -> Self {
        Self {
            phase: VoiceDownloadPhase::Idle,
            downloaded_bytes: 0,
            started_downloaded_bytes: 0,
            total_bytes: required_model_bytes(),
            current_file: None,
            started_at: None,
            error: None,
            cancel: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum VoiceRecordingPhase {
    Idle,
    Recording,
    Recorded,
    Error,
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceRecordingSnapshot {
    pub phase: VoiceRecordingPhase,
    pub path: Option<String>,
    pub bytes: u64,
    pub duration_seconds: f64,
    pub elapsed_seconds: u64,
    pub sample_rate: Option<u32>,
    pub message: String,
}

pub struct VoiceRecordingState {
    phase: VoiceRecordingPhase,
    started_at: Option<Instant>,
    sample_rate: Option<u32>,
    command: Option<mpsc::Sender<RecordingCommand>>,
    last: Option<VoiceRecordingClip>,
    error: Option<String>,
}

impl Default for VoiceRecordingState {
    fn default() -> Self {
        Self {
            phase: VoiceRecordingPhase::Idle,
            started_at: None,
            sample_rate: None,
            command: None,
            last: None,
            error: None,
        }
    }
}

enum RecordingCommand {
    Stop(PathBuf, mpsc::Sender<Result<VoiceRecordingClip, String>>),
    Cancel(mpsc::Sender<()>),
}

#[derive(Clone, Debug)]
struct VoiceRecordingClip {
    path: PathBuf,
    bytes: u64,
    duration_seconds: f64,
    sample_rate: u32,
}

#[derive(Clone)]
pub struct VoiceWorker {
    inner: Arc<WorkerInner>,
}

struct WorkerInner {
    live: Mutex<Option<LiveWorker>>,
    killer: Mutex<Option<WorkerKiller>>,
    busy: AtomicBool,
    loaded: AtomicBool,
    stop_after: AtomicBool,
    last_used: Mutex<Instant>,
    generation: AtomicU64,
}

struct LiveWorker {
    child: Child,
    stdin: ChildStdin,
    stdout: Option<BufReader<std::process::ChildStdout>>,
    _lifetime: WorkerLifetime,
}

#[derive(Clone)]
struct WorkerKiller {
    pid: u32,
    #[cfg(windows)]
    job: Option<isize>,
}

struct JobOutcome {
    reused_model: bool,
    reused_voice: bool,
    spawn_ms: u64,
    imports_ms: Option<u64>,
    model_load_ms: Option<u64>,
    condition_ms: Option<u64>,
    synthesis_ms: Option<u64>,
    wav_write_ms: Option<u64>,
    peak_ram: u64,
    peak_private: u64,
}

#[derive(Serialize)]
struct ServeRequest<'a> {
    id: String,
    op: &'a str,
    reference: String,
    out_dir: String,
    phrases: &'a [VoicePhraseInput],
    overwrite: bool,
}

#[derive(Deserialize)]
struct ServeReply {
    ok: bool,
    error: Option<String>,
    loaded: Option<bool>,
    reused_model: Option<bool>,
    reused_voice: Option<bool>,
    phases_ms: Option<ServePhases>,
}

#[derive(Deserialize, Default)]
struct ServePhases {
    imports: Option<u64>,
    model_load: Option<u64>,
    condition: Option<u64>,
    synthesis: Option<u64>,
    wav_write: Option<u64>,
}

impl Drop for WorkerInner {
    fn drop(&mut self) {
        if let Some(killer) = self.killer.lock().ok().and_then(|mut slot| slot.take()) {
            killer.terminate();
        }
        if let Some(mut worker) = self.live.lock().ok().and_then(|mut slot| slot.take()) {
            let _ = worker.child.kill();
            let _ = worker.child.wait();
        }
    }
}

impl VoiceWorker {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WorkerInner {
                live: Mutex::new(None),
                killer: Mutex::new(None),
                busy: AtomicBool::new(false),
                loaded: AtomicBool::new(false),
                stop_after: AtomicBool::new(false),
                last_used: Mutex::new(Instant::now()),
                generation: AtomicU64::new(0),
            }),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.loaded.load(Ordering::SeqCst)
    }

    pub fn release(&self) {
        self.inner.stop_after.store(true, Ordering::SeqCst);
        if !self.inner.busy.load(Ordering::SeqCst) {
            self.kill();
        }
    }

    pub fn cancel(&self) {
        self.inner.stop_after.store(true, Ordering::SeqCst);
        self.kill();
    }

    pub fn tick(&self, root: &Path) {
        if self.inner.busy.load(Ordering::SeqCst) {
            return;
        }
        let idle = self
            .inner
            .last_used
            .lock()
            .ok()
            .map(|used| used.elapsed() >= WORKER_IDLE_TIMEOUT)
            .unwrap_or(false);
        let scarce = worker_memory_is_scarce();
        if idle || scarce {
            self.kill();
            let _ = root;
        }
    }

    fn kill(&self) {
        if let Ok(killer) = self.inner.killer.lock() {
            if let Some(killer) = killer.as_ref() {
                killer.terminate();
            }
        }
        if let Ok(mut live) = self.inner.live.lock() {
            if let Some(mut worker) = live.take() {
                let _ = worker.child.kill();
                let _ = worker.child.wait();
            }
        }
        if let Ok(mut killer) = self.inner.killer.lock() {
            *killer = None;
        }
        self.inner.loaded.store(false, Ordering::SeqCst);
        self.inner.busy.store(false, Ordering::SeqCst);
    }

    fn run_job(
        &self,
        root: &Path,
        reference: &Path,
        out_dir: &Path,
        phrases: &[VoicePhraseInput],
    ) -> Result<JobOutcome, String> {
        if self.inner.busy.swap(true, Ordering::SeqCst) {
            return Err("a voice clip is already generating".to_string());
        }
        self.inner.stop_after.store(false, Ordering::SeqCst);
        let generation = self.inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let started = Instant::now();
        let result = self.run_job_inner(root, reference, out_dir, phrases, generation, started);
        self.inner.busy.store(false, Ordering::SeqCst);
        if let Ok(mut used) = self.inner.last_used.lock() {
            *used = Instant::now();
        }
        if self.inner.stop_after.swap(false, Ordering::SeqCst) {
            self.kill();
        }
        result
    }

    fn run_job_inner(
        &self,
        root: &Path,
        reference: &Path,
        out_dir: &Path,
        phrases: &[VoicePhraseInput],
        generation: u64,
        started: Instant,
    ) -> Result<JobOutcome, String> {
        let mut live = self
            .inner
            .live
            .lock()
            .map_err(|_| "voice worker lock".to_string())?;
        let mut spawn_ms = 0;
        let dead = match live.as_mut() {
            Some(worker) => !matches!(worker.child.try_wait(), Ok(None)),
            None => true,
        };
        if dead {
            *live = None;
            let spawn_started = Instant::now();
            let (worker, killer) = spawn_serve_worker(root)?;
            if let Ok(mut slot) = self.inner.killer.lock() {
                *slot = Some(killer);
            }
            *live = Some(worker);
            spawn_ms = spawn_started.elapsed().as_millis() as u64;
        }
        let worker = live.as_mut().ok_or_else(|| "voice worker missing".to_string())?;
        let request = ServeRequest {
            id: generation.to_string(),
            op: "generate",
            reference: reference.to_string_lossy().into_owned(),
            out_dir: out_dir.to_string_lossy().into_owned(),
            phrases,
            overwrite: true,
        };
        let mut encoded = serde_json::to_vec(&request).map_err(|error| error.to_string())?;
        encoded.push(b'\n');
        worker
            .stdin
            .write_all(&encoded)
            .map_err(|error| error.to_string())?;
        worker.stdin.flush().map_err(|error| error.to_string())?;

        let mut stdout = worker
            .stdout
            .take()
            .ok_or_else(|| "voice worker stdout missing".to_string())?;
        let pid = worker.child.id();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut line = String::new();
            let result = stdout.read_line(&mut line);
            let _ = tx.send((result, line, stdout));
        });

        let killer = self.inner.killer.lock().ok().and_then(|slot| slot.clone());
        let mut peak = process_tree_ram(pid).unwrap_or_default();
        persist_generation_peak(root, peak.working_set);
        let reply = loop {
            if started.elapsed() >= MAX_VOICE_GENERATION
                || self.inner.generation.load(Ordering::SeqCst) != generation
            {
                if let Some(killer) = &killer {
                    killer.terminate();
                }
                drop(live);
                self.kill();
                return Err("voice generation timed out".to_string());
            }
            if let Some(used) = process_tree_ram(pid) {
                if used.working_set > peak.working_set {
                    peak = used;
                    persist_generation_peak(root, peak.working_set);
                }
            }
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok((Ok(_), line, stdout)) => {
                    if let Some(worker) = live.as_mut() {
                        worker.stdout = Some(stdout);
                    }
                    break line;
                }
                Ok((Err(error), _, _)) => {
                    let cancelled = self.inner.stop_after.load(Ordering::SeqCst);
                    drop(live);
                    self.kill();
                    return Err(if cancelled {
                        "voice generation cancelled".to_string()
                    } else {
                        error.to_string()
                    });
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    let cancelled = self.inner.stop_after.load(Ordering::SeqCst);
                    drop(live);
                    self.kill();
                    return Err(if cancelled {
                        "voice generation cancelled".to_string()
                    } else {
                        "voice worker stopped".to_string()
                    });
                }
            }
        };

        finish_generation_peak(root, peak.working_set);
        let parsed: ServeReply =
            serde_json::from_str(reply.trim()).map_err(|error| error.to_string())?;
        self.inner
            .loaded
            .store(parsed.loaded.unwrap_or(parsed.ok), Ordering::SeqCst);
        if !parsed.ok {
            return Err(parsed
                .error
                .unwrap_or_else(|| "voice worker failed".to_string()));
        }
        let phases = parsed.phases_ms.unwrap_or_default();
        Ok(JobOutcome {
            reused_model: parsed.reused_model.unwrap_or(false),
            reused_voice: parsed.reused_voice.unwrap_or(false),
            spawn_ms,
            imports_ms: phases.imports,
            model_load_ms: phases.model_load,
            condition_ms: phases.condition,
            synthesis_ms: phases.synthesis,
            wav_write_ms: phases.wav_write,
            peak_ram: peak.working_set,
            peak_private: peak.private,
        })
    }
}

impl WorkerKiller {
    fn terminate(&self) {
        #[cfg(windows)]
        {
            if let Some(job) = self.job {
                unsafe {
                    let _ = windows::Win32::System::JobObjects::TerminateJobObject(
                        windows::Win32::Foundation::HANDLE(job as *mut core::ffi::c_void),
                        1,
                    );
                }
                return;
            }
        }
        #[cfg(unix)]
        {
            let _ = Command::new("kill")
                .arg("-9")
                .arg(self.pid.to_string())
                .status();
        }
        #[cfg(windows)]
        {
            use windows::Win32::Foundation::CloseHandle;
            use windows::Win32::System::Threading::{
                OpenProcess, TerminateProcess, PROCESS_TERMINATE,
            };
            if let Ok(handle) = unsafe { OpenProcess(PROCESS_TERMINATE, false, self.pid) } {
                let _ = unsafe { TerminateProcess(handle, 1) };
                let _ = unsafe { CloseHandle(handle) };
            }
        }
    }
}

fn spawn_serve_worker(root: &Path) -> Result<(LiveWorker, WorkerKiller), String> {
    let mut command = voice_runtime::run_worker_command(root)?;
    command
        .arg("--serve")
        .arg("--model-dir")
        .arg(model_dir(root))
        .arg("--device")
        .arg("cpu")
        .arg("--cache-dir")
        .arg(root.join("worker_cache"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let Some(mut stderr) = child.stderr.take() {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = stderr.read_to_end(&mut bytes);
        });
    }
    let lifetime = bind_worker_lifetime(&child);
    let killer = WorkerKiller {
        pid: child.id(),
        #[cfg(windows)]
        job: lifetime.raw_job(),
    };
    let stdin = child
        .stdin
        .take()
        .ok_or_else(|| "voice worker stdin missing".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "voice worker stdout missing".to_string())?;
    Ok((
        LiveWorker {
            child,
            stdin,
            stdout: Some(BufReader::new(stdout)),
            _lifetime: lifetime,
        },
        killer,
    ))
}

fn worker_memory_is_scarce() -> bool {
    if !macos_pressure_allows() {
        return true;
    }
    available_ram_bytes()
        .map(|bytes| bytes < LOADED_WORKER_HEADROOM_BYTES)
        .unwrap_or(false)
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceBackendStatus {
    pub model: VoiceModelStatus,
    pub runtime: voice_runtime::VoiceRuntimeStatus,
    pub system: VoiceSystemInfo,
    pub profiles: Vec<VoiceProfileSummary>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceModelStatus {
    pub id: &'static str,
    pub display_name: &'static str,
    pub repo: &'static str,
    pub revision: &'static str,
    pub install_dir: String,
    pub installed: bool,
    pub installed_bytes: u64,
    pub required_bytes: u64,
    pub recommended_free_bytes: u64,
    pub files: Vec<VoiceModelFileStatus>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceModelFileStatus {
    pub path: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub installed: bool,
    pub valid: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct VoiceSystemInfo {
    pub os: String,
    pub arch: String,
    pub cpu_threads: usize,
    pub total_ram_bytes: Option<u64>,
    pub available_ram_bytes: Option<u64>,
    pub min_required_ram_bytes: u64,
    pub min_free_ram_bytes: u64,
    pub last_peak_ram_bytes: Option<u64>,
    pub has_enough_ram: Option<bool>,
    pub available_disk_bytes: Option<u64>,
    pub recommended_free_disk_bytes: u64,
    pub has_enough_disk: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
struct GenerationRamBudget {
    last_peak_ram_bytes: Option<u64>,
    last_recorded_at_unix: Option<u64>,
    generation_count: u64,
}

#[derive(Serialize)]
struct GenerationLogRow {
    at_ms: u64,
    os: &'static str,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    profile: String,
    phrases: usize,
    merge: bool,
    need: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_before: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    available_after: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    total: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_peak: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    worker_private: Option<u64>,
    duration_ms: u64,
    reused_model: bool,
    reused_voice: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    spawn_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    imports_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_load_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    condition_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    synthesis_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    wav_write_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pressure: Option<u32>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GenerateVoiceClipsInput {
    pub profile_id: String,
    pub display_name: Option<String>,
    pub reference_audio_path: String,
    pub phrases: Option<Vec<VoicePhraseInput>>,
    pub delete_reference_after_generation: Option<bool>,
    pub merge_clips: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VoicePhraseInput {
    pub id: String,
    pub tone: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VoiceProfileSummary {
    pub profile_id: String,
    pub display_name: String,
    pub profile_dir: String,
    pub reference_path: Option<String>,
    pub reference_retained: bool,
    pub clip_count: usize,
    pub clips: Vec<VoiceClipSummary>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VoiceClipSummary {
    pub clip_id: String,
    pub phrase_id: String,
    pub file_name: String,
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, Deserialize)]
pub struct DeleteVoiceClipInput {
    pub profile_id: String,
    pub clip_id: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SaveVoiceReferenceInput {
    pub profile_id: String,
    pub display_name: Option<String>,
    pub audio_bytes: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct SaveVoiceReferenceFromPathInput {
    pub profile_id: String,
    pub display_name: Option<String>,
    pub reference_audio_path: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct VoiceProfileMetadata {
    profile_id: String,
    display_name: String,
    reference_retained: bool,
}

#[derive(Clone, Copy)]
struct ModelFile {
    path: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const MODEL_FILES: &[ModelFile] = &[
    ModelFile {
        path: "added_tokens.json",
        bytes: 418,
        sha256: "72e4ab6acb0d9309ac3df4b526ae5fd80a2da5bc5ab7bb02d85096a374f69193",
    },
    ModelFile {
        path: "tokenizer_config.json",
        bytes: 3_878,
        sha256: "bca16a2ac1ddbd78b8d6228f0031884cc74b6ea54b967d6f6d2ebae9ccde23e6",
    },
    ModelFile {
        path: "special_tokens_map.json",
        bytes: 470,
        sha256: "92ba8063bf40aa163eadebbfe0de07c2aebe44cf0d4a9e8726580b0781fd2640",
    },
    ModelFile {
        path: "merges.txt",
        bytes: 456_318,
        sha256: "1ce1664773c50f3e0cc8842619a93edc4624525b728b188a9e0be33b7726adc5",
    },
    ModelFile {
        path: "vocab.json",
        bytes: 999_186,
        sha256: "f6bd25a65e4e63ca31360e9fb11c7e4f9a391a78385d640acd814092dd6eee4f",
    },
    ModelFile {
        path: "conds.pt",
        bytes: 169_454,
        sha256: "b1852099306fd6a7814eb9d0bd10186caba7249596cc23868f78a0eefbfa5033",
    },
    ModelFile {
        path: "ve.safetensors",
        bytes: 5_695_784,
        sha256: "f0921cab452fa278bc25cd23ffd59d36f816d7dc5181dd1bef9751a7fb61f63c",
    },
    ModelFile {
        path: "t3_nano_v1.safetensors",
        bytes: 869_899_204,
        sha256: "72b110185087d945dbdf54dee4e333848e1811bdd5fd6cb16ceb8da50006f0c9",
    },
    ModelFile {
        path: "s3gen_meanflow.safetensors",
        bytes: 1_064_875_036,
        sha256: "d65cb687a2ed581ee6cc297e919ffefa63386944f42364ae13b78a594945514f",
    },
];

pub fn get_status(app: &AppHandle, worker: &VoiceWorker) -> Result<VoiceBackendStatus, String> {
    let root = voice_root(app)?;
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    seed_starter_profiles(&root, Some(app));
    Ok(status_for_root(&root, worker.is_loaded()))
}

pub fn release_worker(worker: &VoiceWorker) {
    worker.release();
}

pub fn cancel_generation(worker: &VoiceWorker) {
    worker.cancel();
}

pub fn tick_worker(app: &AppHandle, worker: &VoiceWorker) {
    let Ok(root) = voice_root(app) else {
        return;
    };
    worker.tick(&root);
}

pub fn download_model(app: AppHandle) -> Result<VoiceModelStatus, String> {
    let root = voice_root(&app)?;
    fs::create_dir_all(model_dir(&root)).map_err(|error| error.to_string())?;
    ensure_download_space(&root)?;
    voice_runtime::install_runtime(&root, Arc::new(AtomicBool::new(false)), |_| {})?;
    download_model_to(&root)?;
    Ok(model_status(&root))
}

pub fn start_model_download(
    app: AppHandle,
    state: Arc<Mutex<VoiceDownloadState>>,
) -> Result<VoiceDownloadSnapshot, String> {
    let root = voice_root(&app)?;
    fs::create_dir_all(model_dir(&root)).map_err(|error| error.to_string())?;

    {
        let download = state
            .lock()
            .map_err(|_| "voice download state unavailable".to_string())?;
        if matches!(
            download.phase,
            VoiceDownloadPhase::Downloading | VoiceDownloadPhase::Cancelling
        ) {
            return Ok(download_snapshot(&download, &root));
        }
    }

    ensure_download_space(&root)?;
    let model = model_status(&root);
    let runtime = voice_runtime::status(&root);
    let required_bytes = required_model_bytes() + runtime.required_bytes.unwrap_or(0);
    if model.installed && runtime.installed {
        let mut download = state
            .lock()
            .map_err(|_| "voice download state unavailable".to_string())?;
        download.phase = VoiceDownloadPhase::Complete;
        download.downloaded_bytes = required_bytes;
        download.started_downloaded_bytes = required_bytes;
        download.total_bytes = required_bytes;
        download.current_file = None;
        download.started_at = None;
        download.error = None;
        download.cancel = None;
        return Ok(download_snapshot(&download, &root));
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut download = state
            .lock()
            .map_err(|_| "voice download state unavailable".to_string())?;
        let installed_bytes = valid_pack_bytes(&root);
        download.phase = VoiceDownloadPhase::Downloading;
        download.downloaded_bytes = installed_bytes;
        download.started_downloaded_bytes = installed_bytes;
        download.total_bytes = required_bytes.max(required_model_bytes());
        download.current_file = None;
        download.started_at = Some(Instant::now());
        download.error = None;
        download.cancel = Some(Arc::clone(&cancel));
    }

    let download_state = Arc::clone(&state);
    let root_for_worker = root.clone();
    std::thread::spawn(move || {
        let result =
            download_pack_to_with_progress(&root_for_worker, Arc::clone(&cancel), |progress| {
                if let Ok(mut download) = download_state.lock() {
                    if matches!(
                        download.phase,
                        VoiceDownloadPhase::Downloading | VoiceDownloadPhase::Cancelling
                    ) {
                        download.downloaded_bytes = progress.downloaded_bytes;
                        if let Some(total_bytes) = progress.total_bytes {
                            download.total_bytes = total_bytes;
                        }
                        download.current_file = progress.current_file;
                    }
                }
            });

        if let Ok(mut download) = download_state.lock() {
            download.current_file = None;
            download.cancel = None;
            match result {
                Ok(()) => {
                    download.phase = VoiceDownloadPhase::Complete;
                    download.downloaded_bytes = pack_required_bytes(&root_for_worker);
                    download.started_downloaded_bytes = download.downloaded_bytes;
                    download.total_bytes = download.downloaded_bytes;
                    download.error = None;
                    download.started_at = None;
                }
                Err(error) if cancel.load(Ordering::SeqCst) || error == DOWNLOAD_CANCELLED => {
                    download.phase = VoiceDownloadPhase::Idle;
                    download.downloaded_bytes = 0;
                    download.started_downloaded_bytes = 0;
                    download.error = None;
                    download.started_at = None;
                }
                Err(error) => {
                    download.phase = VoiceDownloadPhase::Error;
                    download.error = Some(error);
                    download.started_at = None;
                }
            }
        }
    });

    let download = state
        .lock()
        .map_err(|_| "voice download state unavailable".to_string())?;
    Ok(download_snapshot(&download, &root))
}

pub fn model_download_snapshot(
    app: &AppHandle,
    state: &Arc<Mutex<VoiceDownloadState>>,
) -> Result<VoiceDownloadSnapshot, String> {
    let root = voice_root(app)?;
    let download = state
        .lock()
        .map_err(|_| "voice download state unavailable".to_string())?;
    Ok(download_snapshot(&download, &root))
}

pub fn cancel_model_download(
    app: &AppHandle,
    state: &Arc<Mutex<VoiceDownloadState>>,
) -> Result<VoiceDownloadSnapshot, String> {
    let root = voice_root(app)?;
    let mut download = state
        .lock()
        .map_err(|_| "voice download state unavailable".to_string())?;
    if download.phase == VoiceDownloadPhase::Downloading {
        download.phase = VoiceDownloadPhase::Cancelling;
        download.current_file = None;
        if let Some(cancel) = &download.cancel {
            cancel.store(true, Ordering::SeqCst);
        }
    }
    Ok(download_snapshot(&download, &root))
}

pub fn delete_model(app: &AppHandle) -> Result<VoiceModelStatus, String> {
    let root = voice_root(app)?;
    let dir = model_dir(&root);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|error| error.to_string())?;
    }
    voice_runtime::delete_runtime(&root)?;
    Ok(model_status(&root))
}

pub fn list_profiles(app: &AppHandle) -> Result<Vec<VoiceProfileSummary>, String> {
    let root = voice_root(app)?;
    Ok(profile_summaries(&root))
}

pub fn delete_profile(
    app: &AppHandle,
    profile_id: String,
) -> Result<Vec<VoiceProfileSummary>, String> {
    let root = voice_root(app)?;
    let id = clean_id(&profile_id)?;
    if is_starter_profile(&id) {
        return Err("Starter voices stay on this device.".into());
    }
    let dir = profile_dir(&root, &id);
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|error| error.to_string())?;
    }
    Ok(profile_summaries(&root))
}

pub fn save_reference(
    app: &AppHandle,
    input: SaveVoiceReferenceInput,
) -> Result<VoiceProfileSummary, String> {
    let root = voice_root(app)?;
    let profile_id = clean_id(&input.profile_id)?;
    if input.audio_bytes.is_empty() {
        return Err("recorded voice clip is empty".to_string());
    }
    if input.audio_bytes.len() > MAX_REFERENCE_AUDIO_BYTES {
        return Err("recorded voice clip is too large".to_string());
    }

    let display_name = clean_display_name(input.display_name.as_deref(), &profile_id);
    let dir = profile_dir(&root, &profile_id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::write(dir.join("reference.wav"), input.audio_bytes).map_err(|error| error.to_string())?;

    let metadata = VoiceProfileMetadata {
        profile_id: profile_id.clone(),
        display_name,
        reference_retained: true,
    };
    write_json(&dir.join("profile.json"), &metadata)?;
    profile_summary(&dir, &profile_id)
}

pub fn save_reference_from_path(
    app: &AppHandle,
    input: SaveVoiceReferenceFromPathInput,
) -> Result<VoiceProfileSummary, String> {
    let root = voice_root(app)?;
    let root = ensure_voice_root(&root)?;
    let profile_id = clean_id(&input.profile_id)?;
    let reference = supervisor_storage_file(&root, &input.reference_audio_path, "recording")?;
    let bytes = fs::metadata(&reference)
        .map_err(|error| error.to_string())?
        .len();
    if bytes == 0 {
        return Err("recorded voice clip is empty".to_string());
    }
    if bytes > MAX_REFERENCE_AUDIO_BYTES as u64 {
        return Err("recorded voice clip is too large".to_string());
    }

    let display_name = clean_display_name(input.display_name.as_deref(), &profile_id);
    let dir = profile_dir(&root, &profile_id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    fs::copy(&reference, dir.join("reference.wav")).map_err(|error| error.to_string())?;

    let metadata = VoiceProfileMetadata {
        profile_id: profile_id.clone(),
        display_name,
        reference_retained: true,
    };
    write_json(&dir.join("profile.json"), &metadata)?;
    profile_summary(&dir, &profile_id)
}

pub fn start_recording(
    app: &AppHandle,
    state: &Mutex<VoiceRecordingState>,
) -> Result<VoiceRecordingSnapshot, String> {
    let root = voice_root(app)?;
    fs::create_dir_all(recordings_dir(&root)).map_err(|error| error.to_string())?;
    let mut recording = state
        .lock()
        .map_err(|_| "voice recording state unavailable".to_string())?;
    if recording.phase == VoiceRecordingPhase::Recording {
        return Ok(recording_snapshot(&recording));
    }
    if let Some(previous) = recording.last.take() {
        let _ = fs::remove_file(previous.path);
    }

    let (command_tx, command_rx) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::channel();
    std::thread::spawn(move || run_recording_thread(command_rx, ready_tx));
    let sample_rate = ready_rx
        .recv_timeout(Duration::from_secs(3))
        .map_err(|_| "Microphone did not become ready.".to_string())??;

    recording.phase = VoiceRecordingPhase::Recording;
    recording.started_at = Some(Instant::now());
    recording.sample_rate = Some(sample_rate);
    recording.command = Some(command_tx);
    recording.error = None;
    Ok(recording_snapshot(&recording))
}

pub fn stop_recording(
    app: &AppHandle,
    state: &Mutex<VoiceRecordingState>,
) -> Result<VoiceRecordingSnapshot, String> {
    let command = {
        let mut recording = state
            .lock()
            .map_err(|_| "voice recording state unavailable".to_string())?;
        if recording.phase != VoiceRecordingPhase::Recording {
            return Ok(recording_snapshot(&recording));
        }
        recording.command.take()
    };
    let Some(command) = command else {
        let mut recording = state
            .lock()
            .map_err(|_| "voice recording state unavailable".to_string())?;
        recording.phase = VoiceRecordingPhase::Error;
        recording.error = Some("Recording stream was not available.".to_string());
        return Ok(recording_snapshot(&recording));
    };

    let root = voice_root(app)?;
    let dir = recordings_dir(&root);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let path = dir.join(format!("recording-{}.wav", unix_millis()));
    let (result_tx, result_rx) = mpsc::channel();
    command
        .send(RecordingCommand::Stop(path, result_tx))
        .map_err(|_| "Recording stream stopped unexpectedly.".to_string())?;
    let clip = result_rx
        .recv_timeout(Duration::from_secs(10))
        .map_err(|_| "Recording did not stop cleanly.".to_string())?;

    let mut recording = state
        .lock()
        .map_err(|_| "voice recording state unavailable".to_string())?;
    recording.started_at = None;
    recording.sample_rate = None;
    match clip {
        Ok(clip) => {
            if let Some(previous) = recording.last.take() {
                let _ = fs::remove_file(previous.path);
            }
            recording.phase = VoiceRecordingPhase::Recorded;
            recording.last = Some(clip);
            recording.error = None;
        }
        Err(error) => {
            recording.phase = VoiceRecordingPhase::Error;
            recording.error = Some(error);
        }
    }
    Ok(recording_snapshot(&recording))
}

pub fn clear_recording(
    app: &AppHandle,
    state: &Mutex<VoiceRecordingState>,
) -> Result<VoiceRecordingSnapshot, String> {
    let root = voice_root(app)?;
    let command = {
        let mut recording = state
            .lock()
            .map_err(|_| "voice recording state unavailable".to_string())?;
        recording.command.take()
    };
    if let Some(command) = command {
        let (done_tx, done_rx) = mpsc::channel();
        let _ = command.send(RecordingCommand::Cancel(done_tx));
        let _ = done_rx.recv_timeout(Duration::from_secs(2));
    }

    let mut recording = state
        .lock()
        .map_err(|_| "voice recording state unavailable".to_string())?;
    if let Some(last) = recording.last.take() {
        let _ = fs::remove_file(last.path);
    }
    recording.phase = VoiceRecordingPhase::Idle;
    recording.started_at = None;
    recording.sample_rate = None;
    recording.error = None;
    fs::create_dir_all(recordings_dir(&root)).map_err(|error| error.to_string())?;
    Ok(recording_snapshot(&recording))
}

pub fn delete_profile_clips(
    app: &AppHandle,
    profile_id: String,
) -> Result<VoiceProfileSummary, String> {
    let root = voice_root(app)?;
    let profile_id = clean_id(&profile_id)?;
    let dir = profile_dir(&root, &profile_id);
    if !dir.exists() {
        return Err(format!("voice profile not found: {profile_id}"));
    }

    let clips_dir = dir.join("clips");
    if clips_dir.exists() {
        fs::remove_dir_all(&clips_dir).map_err(|error| error.to_string())?;
    }

    profile_summary(&dir, &profile_id)
}

pub fn delete_clip(
    app: &AppHandle,
    input: DeleteVoiceClipInput,
) -> Result<VoiceProfileSummary, String> {
    let root = voice_root(app)?;
    let profile_id = clean_id(&input.profile_id)?;
    let clip_id = clean_id(&input.clip_id)?;
    let clips_dir = profile_dir(&root, &profile_id).join("clips");
    let mut deleted = false;

    if clips_dir.exists() {
        for entry in fs::read_dir(&clips_dir).map_err(|error| error.to_string())? {
            let path = entry.map_err(|error| error.to_string())?.path();
            if path.is_file() && clip_path_matches(&path, &clip_id) {
                fs::remove_file(&path).map_err(|error| error.to_string())?;
                deleted = true;
            }
        }
    }

    if !deleted {
        return Err(format!("voice clip not found: {clip_id}"));
    }

    profile_summary(&profile_dir(&root, &profile_id), &profile_id)
}

pub fn generate_clips(
    app: AppHandle,
    worker: &VoiceWorker,
    input: GenerateVoiceClipsInput,
) -> Result<VoiceProfileSummary, String> {
    let started = Instant::now();
    let root = voice_root(&app)?;
    let root = ensure_voice_root(&root)?;
    seed_starter_profiles(&root, Some(&app));
    let loaded = worker.is_loaded();
    let mut row = GenerationLogRow {
        at_ms: unix_millis() as u64,
        os: std::env::consts::OS,
        ok: false,
        error: None,
        profile: input.profile_id.clone(),
        phrases: input.phrases.as_ref().map(Vec::len).unwrap_or(0),
        merge: input.merge_clips.unwrap_or(false),
        need: required_ram_for_gate(loaded, load_generation_budget(&root).last_peak_ram_bytes),
        available_before: available_ram_bytes(),
        available_after: None,
        total: total_ram_bytes(),
        worker_peak: None,
        worker_private: None,
        duration_ms: 0,
        reused_model: false,
        reused_voice: false,
        spawn_ms: None,
        imports_ms: None,
        model_load_ms: None,
        condition_ms: None,
        synthesis_ms: None,
        wav_write_ms: None,
        pressure: macos_pressure_for_log(),
    };
    let result = generate_clips_inner(&root, worker, input, &mut row);
    row.duration_ms = started.elapsed().as_millis() as u64;
    row.available_after = available_ram_bytes();
    match &result {
        Ok(_) => row.ok = true,
        Err(error) => row.error = Some(truncate_error(error)),
    }
    append_generation_log(&root, &row);
    result
}

fn generate_clips_inner(
    root: &Path,
    worker: &VoiceWorker,
    input: GenerateVoiceClipsInput,
    row: &mut GenerationLogRow,
) -> Result<VoiceProfileSummary, String> {
    let profile_id = clean_id(&input.profile_id)?;
    let display_name = input
        .display_name
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(&profile_id)
        .to_string();
    let reference = supervisor_storage_file(&root, &input.reference_audio_path, "reference audio")?;
    if !model_status(&root).installed || !voice_runtime::status(&root).installed {
        return Err("download the voice cloning model pack before generating clips".to_string());
    }
    ensure_generation_ram(root, worker.is_loaded())?;

    let phrases = input.phrases.unwrap_or_else(default_phrases);
    row.phrases = phrases.len();
    validate_phrases(&phrases)?;

    let dir = profile_dir(root, &profile_id);
    fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let reference_copy = dir.join("reference.wav");
    if !same_path(&reference, &reference_copy) {
        fs::copy(&reference, &reference_copy).map_err(|error| error.to_string())?;
    }

    let staging = generation_staging_dir(&dir);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir_all(&staging).map_err(|error| error.to_string())?;

    match worker.run_job(root, &reference_copy, &staging, &phrases) {
        Ok(outcome) => {
            row.worker_peak = (outcome.peak_ram > 0).then_some(outcome.peak_ram);
            row.worker_private = (outcome.peak_private > 0).then_some(outcome.peak_private);
            row.reused_model = outcome.reused_model;
            row.reused_voice = outcome.reused_voice;
            row.spawn_ms = Some(outcome.spawn_ms);
            row.imports_ms = outcome.imports_ms;
            row.model_load_ms = outcome.model_load_ms;
            row.condition_ms = outcome.condition_ms;
            row.synthesis_ms = outcome.synthesis_ms;
            row.wav_write_ms = outcome.wav_write_ms;
        }
        Err(message) => {
            let _ = fs::remove_dir_all(&staging);
            return Err(message);
        }
    }
    let merge = input.merge_clips.unwrap_or(false);
    let install = if merge {
        merge_generated_clips(&dir, &staging)
    } else {
        replace_generated_clips(&dir, &staging)
    };
    if let Err(error) = install {
        let _ = fs::remove_dir_all(&staging);
        return Err(error);
    }
    if !merge {
        write_phrase_bank(&dir.join("phrases.json"), &phrases)?;
    }
    let _ = fs::remove_dir_all(&staging);

    let reference_retained = !input.delete_reference_after_generation.unwrap_or(false);
    if !reference_retained && reference_copy.exists() {
        fs::remove_file(&reference_copy).map_err(|error| error.to_string())?;
    }

    let metadata = VoiceProfileMetadata {
        profile_id: profile_id.clone(),
        display_name,
        reference_retained,
    };
    write_json(&dir.join("profile.json"), &metadata)?;
    profile_summary(&dir, &profile_id)
}

fn status_for_root(root: &Path, worker_loaded: bool) -> VoiceBackendStatus {
    // Size checks only. The Voice screen polls this, and hashing the model pack keeps the disk busy.
    let model = model_status_for(root, false);
    let runtime = voice_runtime::status(root);
    VoiceBackendStatus {
        system: system_info(root, model.recommended_free_bytes, worker_loaded),
        model,
        runtime,
        profiles: profile_summaries(root),
    }
}

fn model_status(root: &Path) -> VoiceModelStatus {
    model_status_for(root, true)
}

fn model_status_for(root: &Path, validate_hashes: bool) -> VoiceModelStatus {
    let dir = model_dir(root);
    let files = MODEL_FILES
        .iter()
        .map(|file| model_file_status(&dir, file, validate_hashes))
        .collect::<Vec<_>>();
    let installed = files.iter().all(|file| file.valid);
    let installed_bytes = dir_size(&dir).unwrap_or(0);
    let required_bytes = required_model_bytes();

    VoiceModelStatus {
        id: MODEL_ID,
        display_name: MODEL_DISPLAY_NAME,
        repo: MODEL_REPO,
        revision: MODEL_REVISION,
        install_dir: dir.to_string_lossy().into_owned(),
        installed,
        installed_bytes,
        required_bytes,
        recommended_free_bytes: required_bytes + EXTRA_DOWNLOAD_HEADROOM_BYTES,
        files,
    }
}

fn model_file_status(dir: &Path, file: &ModelFile, validate_hash: bool) -> VoiceModelFileStatus {
    let path = dir.join(file.path);
    let installed = path.is_file();
    let valid = installed
        && fs::metadata(&path)
            .map(|metadata| metadata.len() == file.bytes)
            .unwrap_or(false)
        && (!validate_hash
            || sha256_file(&path)
                .map(|hash| hash == file.sha256)
                .unwrap_or(false));

    VoiceModelFileStatus {
        path: file.path,
        bytes: file.bytes,
        sha256: file.sha256,
        installed,
        valid,
    }
}

#[derive(Clone, Debug)]
struct VoiceDownloadProgress {
    downloaded_bytes: u64,
    total_bytes: Option<u64>,
    current_file: Option<String>,
}

fn download_snapshot(state: &VoiceDownloadState, root: &Path) -> VoiceDownloadSnapshot {
    let elapsed_seconds = state
        .started_at
        .map(|started| started.elapsed().as_secs())
        .unwrap_or(0);
    let transferred = state
        .downloaded_bytes
        .saturating_sub(state.started_downloaded_bytes);
    let eta_seconds =
        if state.phase == VoiceDownloadPhase::Downloading && transferred > 0 && elapsed_seconds > 0
        {
            let remaining = state.total_bytes.saturating_sub(state.downloaded_bytes);
            let bytes_per_second = transferred / elapsed_seconds.max(1);
            (bytes_per_second > 0).then_some(remaining.div_ceil(bytes_per_second))
        } else {
            None
        };

    VoiceDownloadSnapshot {
        phase: state.phase,
        downloaded_bytes: state.downloaded_bytes,
        total_bytes: state.total_bytes,
        current_file: state.current_file.clone(),
        elapsed_seconds,
        eta_seconds,
        message: match state.phase {
            VoiceDownloadPhase::Idle => "No model download is running.".to_string(),
            VoiceDownloadPhase::Downloading => state
                .current_file
                .as_ref()
                .map(|file| format!("Downloading {file}"))
                .unwrap_or_else(|| "Downloading model pack".to_string()),
            VoiceDownloadPhase::Cancelling => {
                "Cancelling download. Deleting partially installed files.".to_string()
            }
            VoiceDownloadPhase::Complete => "Model pack ready.".to_string(),
            VoiceDownloadPhase::Error => state
                .error
                .clone()
                .unwrap_or_else(|| "Model download failed.".to_string()),
        },
        model: model_status_for(root, false),
        runtime: voice_runtime::status(root),
    }
}

fn valid_pack_bytes(root: &Path) -> u64 {
    let runtime = voice_runtime::status(root);
    valid_installed_bytes(root) + runtime.required_bytes.unwrap_or(0)
}

fn pack_required_bytes(root: &Path) -> u64 {
    let runtime = voice_runtime::status(root);
    required_model_bytes() + runtime.required_bytes.unwrap_or(0)
}

fn valid_installed_bytes(root: &Path) -> u64 {
    let dir = model_dir(root);
    MODEL_FILES
        .iter()
        .filter(|file| model_file_status(&dir, file, true).valid)
        .map(|file| file.bytes)
        .sum()
}

fn download_model_to(root: &Path) -> Result<(), String> {
    download_model_to_with_progress(root, Arc::new(AtomicBool::new(false)), |_| {})
}

fn download_pack_to_with_progress(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(VoiceDownloadProgress),
) -> Result<(), String> {
    let model_start_bytes = valid_installed_bytes(root);
    let runtime = voice_runtime::install_runtime(root, Arc::clone(&cancel), |runtime_progress| {
        progress(VoiceDownloadProgress {
            downloaded_bytes: model_start_bytes + runtime_progress.downloaded_bytes,
            total_bytes: Some(required_model_bytes() + runtime_progress.total_bytes),
            current_file: runtime_progress.current_file,
        });
    })?;
    let runtime_bytes = runtime.required_bytes.unwrap_or(0);

    download_model_to_with_progress(root, cancel, |model_progress| {
        progress(VoiceDownloadProgress {
            downloaded_bytes: runtime_bytes + model_progress.downloaded_bytes,
            total_bytes: Some(runtime_bytes + required_model_bytes()),
            current_file: model_progress.current_file,
        });
    })
}

fn discard_partial_download(path: &Path) {
    let _ = fs::remove_file(path);
}

fn download_model_to_with_progress(
    root: &Path,
    cancel: Arc<AtomicBool>,
    mut progress: impl FnMut(VoiceDownloadProgress),
) -> Result<(), String> {
    let dir = model_dir(root);
    let client = reqwest::Client::builder()
        .connect_timeout(DOWNLOAD_CONNECT_TIMEOUT)
        .read_timeout(DOWNLOAD_READ_TIMEOUT)
        .timeout(DOWNLOAD_TOTAL_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let mut completed_bytes = valid_installed_bytes(root);
    progress(VoiceDownloadProgress {
        downloaded_bytes: completed_bytes,
        total_bytes: Some(required_model_bytes()),
        current_file: None,
    });

    for file in MODEL_FILES {
        if cancel.load(Ordering::SeqCst) {
            return Err(DOWNLOAD_CANCELLED.to_string());
        }
        let target = dir.join(file.path);
        if model_file_status(&dir, file, true).valid {
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let tmp = target.with_extension("download");
        let mut response = tauri::async_runtime::block_on(async {
            client
                .get(model_file_url(file.path))
                .send()
                .await?
                .error_for_status()
        })
        .map_err(|error| error.to_string())?;
        let mut writer = File::create(&tmp).map_err(|error| error.to_string())?;
        let mut bytes = 0u64;
        progress(VoiceDownloadProgress {
            downloaded_bytes: completed_bytes,
            total_bytes: Some(required_model_bytes()),
            current_file: Some(file.path.to_string()),
        });
        loop {
            if cancel.load(Ordering::SeqCst) {
                discard_partial_download(&tmp);
                return Err(DOWNLOAD_CANCELLED.to_string());
            }
            let Some(chunk) =
                tauri::async_runtime::block_on(response.chunk()).map_err(|error| {
                    if cancel.load(Ordering::SeqCst) {
                        discard_partial_download(&tmp);
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
            bytes += chunk.len() as u64;
            progress(VoiceDownloadProgress {
                downloaded_bytes: completed_bytes + bytes,
                total_bytes: Some(required_model_bytes()),
                current_file: Some(file.path.to_string()),
            });
        }
        writer.flush().map_err(|error| error.to_string())?;
        drop(writer);

        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(&tmp);
            discard_partial_download(&tmp);
            return Err(DOWNLOAD_CANCELLED.to_string());
        }
        if bytes != file.bytes {
            let _ = fs::remove_file(&tmp);
            return Err(format!(
                "downloaded wrong byte count for {}: expected {}, got {}",
                file.path, file.bytes, bytes
            ));
        }
        let hash = sha256_file(&tmp)?;
        if cancel.load(Ordering::SeqCst) {
            let _ = fs::remove_file(&tmp);
            discard_partial_download(&tmp);
            return Err(DOWNLOAD_CANCELLED.to_string());
        }
        if hash != file.sha256 {
            let _ = fs::remove_file(&tmp);
            return Err(format!("downloaded checksum mismatch for {}", file.path));
        }
        if target.exists() {
            fs::remove_file(&target).map_err(|error| error.to_string())?;
        }
        fs::rename(&tmp, &target).map_err(|error| error.to_string())?;
        completed_bytes += file.bytes;
        progress(VoiceDownloadProgress {
            downloaded_bytes: completed_bytes,
            total_bytes: Some(required_model_bytes()),
            current_file: None,
        });
    }

    Ok(())
}

fn merge_generated_clips(profile_dir: &Path, staging_dir: &Path) -> Result<(), String> {
    let staged_clips = staging_dir.join("clips");
    if !staged_clips.is_dir() {
        return Err("voice worker did not produce clip files".to_string());
    }

    let clips_dir = profile_dir.join("clips");
    fs::create_dir_all(&clips_dir).map_err(|error| error.to_string())?;
    let staged = fs::read_dir(&staged_clips).map_err(|error| error.to_string())?;
    for entry in staged {
        let staged_path = entry.map_err(|error| error.to_string())?.path();
        if !staged_path.is_file() {
            continue;
        }
        let Some(file_name) = staged_path.file_name() else {
            continue;
        };
        let phrase_id = staged_path
            .file_stem()
            .and_then(|value| value.to_str())
            .map(clip_phrase_id)
            .unwrap_or_default();
        if !phrase_id.is_empty() {
            let existing = fs::read_dir(&clips_dir).map_err(|error| error.to_string())?;
            for old in existing {
                let old_path = old.map_err(|error| error.to_string())?.path();
                if clip_path_matches(&old_path, phrase_id) {
                    fs::remove_file(&old_path).map_err(|error| error.to_string())?;
                }
            }
        }
        fs::copy(&staged_path, clips_dir.join(file_name)).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn clip_phrase_id(stem: &str) -> &str {
    stem.split_once('_')
        .filter(|(head, _)| !head.is_empty() && head.chars().all(|ch| ch.is_ascii_digit()))
        .map(|(_, rest)| rest)
        .unwrap_or(stem)
}

fn replace_generated_clips(profile_dir: &Path, staging_dir: &Path) -> Result<(), String> {
    let staged_clips = staging_dir.join("clips");
    if !staged_clips.is_dir() {
        return Err("voice worker did not produce clip files".to_string());
    }

    let clips_dir = profile_dir.join("clips");
    let backup_dir = profile_dir.join(format!("clips.backup-{}", unix_millis()));
    if backup_dir.exists() {
        fs::remove_dir_all(&backup_dir).map_err(|error| error.to_string())?;
    }
    if clips_dir.exists() {
        fs::rename(&clips_dir, &backup_dir).map_err(|error| error.to_string())?;
    }

    match fs::rename(&staged_clips, &clips_dir) {
        Ok(()) => {
            if backup_dir.exists() {
                fs::remove_dir_all(&backup_dir).map_err(|error| error.to_string())?;
            }
            Ok(())
        }
        Err(error) => {
            if backup_dir.exists() {
                let _ = fs::rename(&backup_dir, &clips_dir);
            }
            Err(error.to_string())
        }
    }
}

fn ensure_download_space(root: &Path) -> Result<(), String> {
    let recommended = required_model_bytes() + EXTRA_DOWNLOAD_HEADROOM_BYTES;
    let Some(available) = available_disk_bytes(root) else {
        return Ok(());
    };
    if available < recommended {
        return Err(format!(
            "not enough free disk space for voice cloning model: need about {} bytes free, found {} bytes",
            recommended, available
        ));
    }
    Ok(())
}

fn model_file_url(path: &str) -> String {
    format!("https://huggingface.co/{MODEL_REPO}/resolve/{MODEL_REVISION}/{path}")
}

fn profile_summaries(root: &Path) -> Vec<VoiceProfileSummary> {
    let dir = profiles_dir(root);
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut profiles = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let id = path.file_name()?.to_string_lossy().into_owned();
            profile_summary(&path, &id).ok()
        })
        .collect::<Vec<_>>();
    profiles.sort_by(|left, right| left.display_name.cmp(&right.display_name));
    profiles
}

fn profile_summary(dir: &Path, fallback_id: &str) -> Result<VoiceProfileSummary, String> {
    let metadata = read_profile_metadata(dir).unwrap_or_else(|| VoiceProfileMetadata {
        profile_id: fallback_id.to_string(),
        display_name: fallback_id.to_string(),
        reference_retained: dir.join("reference.wav").exists(),
    });
    let clips = clip_summaries(&dir.join("clips"))?;
    let reference_path = dir.join("reference.wav");
    let reference_retained = metadata.reference_retained && reference_path.exists();

    Ok(VoiceProfileSummary {
        profile_id: metadata.profile_id,
        display_name: metadata.display_name,
        profile_dir: dir.to_string_lossy().into_owned(),
        reference_path: reference_retained.then(|| reference_path.to_string_lossy().into_owned()),
        reference_retained,
        clip_count: clips.len(),
        clips,
    })
}

fn read_profile_metadata(dir: &Path) -> Option<VoiceProfileMetadata> {
    let bytes = fs::read(dir.join("profile.json")).ok()?;
    serde_json::from_slice(&bytes).ok()
}

const MIN_CLIP_BYTES: u64 = 44;

fn clip_summaries(dir: &Path) -> Result<Vec<VoiceClipSummary>, String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(Vec::new());
    };
    let mut clips = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.extension().and_then(|value| value.to_str()) != Some("wav") {
            continue;
        }
        let bytes = match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => meta.len(),
            _ => continue,
        };
        if bytes < MIN_CLIP_BYTES || !path.exists() {
            continue;
        }
        let Some(file_name) = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
        else {
            continue;
        };
        let clip_id = path
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| file_name.clone());
        clips.push(VoiceClipSummary {
            phrase_id: clip_phrase_id(&clip_id).to_string(),
            clip_id,
            file_name,
            path: path.to_string_lossy().into_owned(),
            bytes,
        });
    }
    clips.sort_by(|left, right| left.file_name.cmp(&right.file_name));
    Ok(clips)
}

fn clip_path_matches(path: &Path, clip_id: &str) -> bool {
    let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    stem == clip_id || stem.ends_with(&format!("_{clip_id}"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn write_phrase_bank(path: &Path, phrases: &[VoicePhraseInput]) -> Result<(), String> {
    #[derive(Serialize)]
    struct PhraseBank<'a> {
        schema_version: u8,
        description: &'a str,
        phrases: &'a [VoicePhraseInput],
    }

    write_json(
        path,
        &PhraseBank {
            schema_version: 1,
            description: "Supervisor generated voice profile phrase bank.",
            phrases,
        },
    )
}

fn write_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| error.to_string())?;
    fs::write(path, bytes).map_err(|error| error.to_string())
}

fn recording_snapshot(state: &VoiceRecordingState) -> VoiceRecordingSnapshot {
    if state.phase == VoiceRecordingPhase::Recording {
        let elapsed_seconds = state
            .started_at
            .map(|started_at| started_at.elapsed().as_secs())
            .unwrap_or(0);
        let sample_rate = state.sample_rate.unwrap_or(0);
        let estimated_bytes = elapsed_seconds
            .saturating_mul(u64::from(sample_rate))
            .saturating_mul(std::mem::size_of::<i16>() as u64);
        return VoiceRecordingSnapshot {
            phase: VoiceRecordingPhase::Recording,
            path: None,
            bytes: estimated_bytes,
            duration_seconds: elapsed_seconds as f64,
            elapsed_seconds,
            sample_rate: (sample_rate > 0).then_some(sample_rate),
            message: "Recording voice sample.".to_string(),
        };
    }

    if let Some(last) = &state.last {
        return VoiceRecordingSnapshot {
            phase: VoiceRecordingPhase::Recorded,
            path: Some(last.path.to_string_lossy().into_owned()),
            bytes: last.bytes,
            duration_seconds: last.duration_seconds,
            elapsed_seconds: last.duration_seconds.round() as u64,
            sample_rate: Some(last.sample_rate),
            message: "Voice sample ready.".to_string(),
        };
    }

    if let Some(error) = &state.error {
        return VoiceRecordingSnapshot {
            phase: VoiceRecordingPhase::Error,
            path: None,
            bytes: 0,
            duration_seconds: 0.0,
            elapsed_seconds: 0,
            sample_rate: None,
            message: error.clone(),
        };
    }

    VoiceRecordingSnapshot {
        phase: VoiceRecordingPhase::Idle,
        path: None,
        bytes: 0,
        duration_seconds: 0.0,
        elapsed_seconds: 0,
        sample_rate: None,
        message: "Ready to record.".to_string(),
    }
}

fn run_recording_thread(
    commands: mpsc::Receiver<RecordingCommand>,
    ready: mpsc::Sender<Result<u32, String>>,
) {
    let host = cpal::default_host();
    let Some(device) = host.default_input_device() else {
        let _ = ready.send(Err("No microphone input device is available.".to_string()));
        return;
    };
    let supported_config = match device.default_input_config() {
        Ok(config) => config,
        Err(error) => {
            let _ = ready.send(Err(format!("Could not open microphone input: {error}")));
            return;
        }
    };
    let sample_rate = supported_config.sample_rate().0;
    let channels = usize::from(supported_config.channels());
    let stream_config = supported_config.config();
    let samples = Arc::new(Mutex::new(Vec::new()));
    let capture_samples = Arc::clone(&samples);
    let error_handler = |error| eprintln!("Supervisor voice recording stream error: {error}");
    let stream = match supported_config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &stream_config,
            move |data: &[f32], _| capture_f32_samples(data, channels, &capture_samples),
            error_handler,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &stream_config,
            move |data: &[i16], _| capture_i16_samples(data, channels, &capture_samples),
            error_handler,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &stream_config,
            move |data: &[u16], _| capture_u16_samples(data, channels, &capture_samples),
            error_handler,
            None,
        ),
        format => {
            let _ = ready.send(Err(format!(
                "Unsupported microphone sample format: {format:?}"
            )));
            return;
        }
    };
    let Ok(stream) = stream else {
        let _ = ready.send(Err("Could not create microphone input stream.".to_string()));
        return;
    };
    if let Err(error) = stream.play() {
        let _ = ready.send(Err(format!(
            "Could not start microphone recording: {error}"
        )));
        return;
    }
    let _ = ready.send(Ok(sample_rate));

    match commands.recv() {
        Ok(RecordingCommand::Stop(path, result)) => {
            drop(stream);
            let captured = samples
                .lock()
                .map(|samples| samples.clone())
                .unwrap_or_default();
            let _ = result.send(recording_clip_from_samples(path, captured, sample_rate));
        }
        Ok(RecordingCommand::Cancel(done)) => {
            drop(stream);
            let _ = done.send(());
        }
        Err(_) => {
            drop(stream);
        }
    }
}

fn recording_clip_from_samples(
    path: PathBuf,
    samples: Vec<i16>,
    sample_rate: u32,
) -> Result<VoiceRecordingClip, String> {
    if samples.is_empty() {
        return Err("No audio was captured.".to_string());
    }
    write_wav(&path, &samples, sample_rate)?;
    let bytes = fs::metadata(&path)
        .map_err(|error| error.to_string())?
        .len();
    let duration_seconds = samples.len() as f64 / f64::from(sample_rate);
    Ok(VoiceRecordingClip {
        path,
        bytes,
        duration_seconds,
        sample_rate,
    })
}

fn capture_f32_samples(data: &[f32], channels: usize, samples: &Arc<Mutex<Vec<i16>>>) {
    if channels == 0 {
        return;
    }
    if let Ok(mut output) = samples.lock() {
        for frame in data.chunks(channels) {
            let value = frame.first().copied().unwrap_or(0.0).clamp(-1.0, 1.0);
            output.push((value * i16::MAX as f32) as i16);
        }
    }
}

fn capture_i16_samples(data: &[i16], channels: usize, samples: &Arc<Mutex<Vec<i16>>>) {
    if channels == 0 {
        return;
    }
    if let Ok(mut output) = samples.lock() {
        for frame in data.chunks(channels) {
            output.push(frame.first().copied().unwrap_or(0));
        }
    }
}

fn capture_u16_samples(data: &[u16], channels: usize, samples: &Arc<Mutex<Vec<i16>>>) {
    if channels == 0 {
        return;
    }
    if let Ok(mut output) = samples.lock() {
        for frame in data.chunks(channels) {
            let centered = i32::from(frame.first().copied().unwrap_or(u16::MAX / 2)) - 32768;
            output.push(centered.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        }
    }
}

fn write_wav(path: &Path, samples: &[i16], sample_rate: u32) -> Result<(), String> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|error| error.to_string())?;
    for sample in samples {
        writer
            .write_sample(*sample)
            .map_err(|error| error.to_string())?;
    }
    writer.finalize().map_err(|error| error.to_string())
}

fn clean_display_name(value: Option<&str>, fallback: &str) -> String {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .chars()
        .take(80)
        .collect()
}

fn validate_phrases(phrases: &[VoicePhraseInput]) -> Result<(), String> {
    if phrases.is_empty() {
        return Err("at least one voice phrase is required".to_string());
    }
    for phrase in phrases {
        clean_id(&phrase.id)?;
        if phrase.text.trim().is_empty() {
            return Err(format!("voice phrase {} has no text", phrase.id));
        }
    }
    Ok(())
}

fn default_phrases() -> Vec<VoicePhraseInput> {
    [
        (
            "gentle_deep_breath",
            "gentle",
            "Deep breath. Back to the thing.",
        ),
        (
            "gentle_drifted_a_little",
            "gentle",
            "You drifted a little. Come back.",
        ),
        (
            "gentle_one_next_action",
            "gentle",
            "One next action. That is all.",
        ),
        (
            "gentle_soft_reset",
            "gentle",
            "Soft reset. Look at the task again.",
        ),
        (
            "gentle_back_to_plan",
            "gentle",
            "No hurry. Just back to the plan.",
        ),
        (
            "mom_beta_focus",
            "mom",
            "Beta, focus. You opened this for what?",
        ),
        (
            "mom_enough_scrolling",
            "mom",
            "Enough scrolling. Finish the thing first.",
        ),
        ("mom_wander_later", "mom", "You can wander later. Work now."),
        (
            "mom_arre_come_back",
            "mom",
            "Arre, come back. One tab, one task.",
        ),
        (
            "mom_what_was_the_goal",
            "mom",
            "What was the goal? Say it, then do it.",
        ),
        ("stern_not_the_task", "stern", "This is not the task."),
        ("stern_close_it", "stern", "Close it and return."),
        (
            "stern_you_said_it_mattered",
            "stern",
            "You said this mattered. Act like it.",
        ),
        (
            "stern_decide_now",
            "stern",
            "Decide now. Work or stop pretending.",
        ),
        (
            "stern_back_to_work",
            "stern",
            "Back to work. No more bargaining.",
        ),
        (
            "kind_reset_pause_inhale",
            "kind-reset",
            "Pause. Inhale. Now back to the file.",
        ),
        (
            "kind_reset_no_drama",
            "kind-reset",
            "No drama. Just return.",
        ),
        (
            "kind_reset_not_failing",
            "kind-reset",
            "You are not failing. You are drifting. Come back.",
        ),
        (
            "kind_reset_small_next_step",
            "kind-reset",
            "Small next step. That is enough.",
        ),
        (
            "kind_reset_begin_again",
            "kind-reset",
            "Begin again. The work is still here.",
        ),
    ]
    .into_iter()
    .map(|(id, tone, text)| VoicePhraseInput {
        id: id.to_string(),
        tone: tone.to_string(),
        text: text.to_string(),
    })
    .collect()
}

fn clean_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.len() > 80
        || !trimmed
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(format!("invalid id: {value}"));
    }
    Ok(trimmed.to_string())
}

fn system_info(root: &Path, recommended_free_disk_bytes: u64, worker_loaded: bool) -> VoiceSystemInfo {
    let total_ram_bytes = total_ram_bytes();
    let available_ram_bytes = available_ram_bytes();
    let last_peak_ram_bytes = load_generation_budget(root).last_peak_ram_bytes;
    let min_free_ram_bytes = required_ram_for_gate(worker_loaded, last_peak_ram_bytes);
    let available_disk_bytes = available_disk_bytes(root);
    VoiceSystemInfo {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        cpu_threads: std::thread::available_parallelism()
            .map(|value| value.get())
            .unwrap_or(1),
        total_ram_bytes,
        available_ram_bytes,
        min_required_ram_bytes: MIN_REQUIRED_RAM_BYTES,
        min_free_ram_bytes,
        last_peak_ram_bytes,
        has_enough_ram: ram_allows_job(
            available_ram_bytes,
            total_ram_bytes,
            last_peak_ram_bytes,
            worker_loaded,
        )
        .map(|ok| ok && macos_pressure_allows()),
        available_disk_bytes,
        recommended_free_disk_bytes,
        has_enough_disk: available_disk_bytes.map(|bytes| bytes >= recommended_free_disk_bytes),
    }
}

fn required_available_ram_bytes(last_peak_ram_bytes: Option<u64>) -> u64 {
    match last_peak_ram_bytes {
        Some(peak) if peak > 0 => peak
            .saturating_add(GENERATION_PEAK_HEADROOM_BYTES)
            .max(MIN_AVAILABLE_RAM_AFTER_MEASURE_BYTES),
        _ => DEFAULT_MIN_AVAILABLE_RAM_BYTES,
    }
}

fn required_ram_for_gate(worker_loaded: bool, last_peak_ram_bytes: Option<u64>) -> u64 {
    if worker_loaded {
        LOADED_WORKER_HEADROOM_BYTES
    } else {
        required_available_ram_bytes(last_peak_ram_bytes)
    }
}

fn ram_allows_generation(
    available_ram_bytes: Option<u64>,
    total_ram_bytes: Option<u64>,
    last_peak_ram_bytes: Option<u64>,
) -> Option<bool> {
    ram_allows_job(available_ram_bytes, total_ram_bytes, last_peak_ram_bytes, false)
}

fn ram_allows_job(
    available_ram_bytes: Option<u64>,
    total_ram_bytes: Option<u64>,
    last_peak_ram_bytes: Option<u64>,
    worker_loaded: bool,
) -> Option<bool> {
    let need = required_ram_for_gate(worker_loaded, last_peak_ram_bytes);
    let available_ok = available_ram_bytes.map(|bytes| bytes >= need)?;
    let machine_ok = total_ram_bytes
        .map(|bytes| bytes >= MIN_REQUIRED_RAM_BYTES)
        .unwrap_or(true);
    Some(available_ok && machine_ok)
}

fn macos_pressure_allows() -> bool {
    #[cfg(target_os = "macos")]
    {
        // 0 normal, 1 warning, 2 urgent, 3 critical.
        match crate::macos::memory_pressure_level() {
            Some(level) if level >= 2 => false,
            _ => true,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

fn macos_pressure_for_log() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        crate::macos::memory_pressure_level()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

fn ensure_generation_ram(root: &Path, worker_loaded: bool) -> Result<(), String> {
    if !macos_pressure_allows() {
        return Err(
            "macOS is under memory pressure. Close other apps, then try again.".into(),
        );
    }
    let available = available_ram_bytes();
    let total = total_ram_bytes();
    let last_peak = load_generation_budget(root).last_peak_ram_bytes;
    let need = required_ram_for_gate(worker_loaded, last_peak);
    match ram_allows_job(available, total, last_peak, worker_loaded) {
        Some(true) => Ok(()),
        Some(false) => Err(format!(
            "Not enough available RAM to clone a voice. Need about {}, this PC has {} available. Close other apps and try again.",
            format_bytes(need),
            available
                .map(format_bytes)
                .unwrap_or_else(|| "unknown".to_string())
        )),
        None if cfg!(target_os = "windows") => Err(
            "Could not measure available RAM. Refusing to start voice generation on Windows.".to_string(),
        ),
        None => Ok(()),
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes >= GIBIBYTE {
        format!("{:.1} GB", bytes as f64 / GIBIBYTE as f64)
    } else if bytes >= 1024 * 1024 {
        format!("{:.0} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{bytes} B")
    }
}

fn generation_budget_path(root: &Path) -> PathBuf {
    root.join("generation_ram.json")
}

fn generation_log_path(root: &Path) -> PathBuf {
    root.join("generation_log.jsonl")
}

fn append_generation_log(root: &Path, row: &GenerationLogRow) {
    let Ok(mut file) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(generation_log_path(root))
    else {
        return;
    };
    if serde_json::to_writer(&mut file, row).is_ok() {
        let _ = file.write_all(b"\n");
    }
}

fn truncate_error(error: &str) -> String {
    const LIMIT: usize = 180;
    let mut out = String::new();
    for ch in error.chars() {
        if out.len() >= LIMIT {
            out.push('…');
            break;
        }
        out.push(ch);
    }
    out
}

fn load_generation_budget(root: &Path) -> GenerationRamBudget {
    let Ok(bytes) = fs::read(generation_budget_path(root)) else {
        return GenerationRamBudget::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

fn persist_generation_peak(root: &Path, peak_ram_bytes: u64) {
    const MIN_USEFUL_PEAK: u64 = 2 * GIBIBYTE;
    if peak_ram_bytes < MIN_USEFUL_PEAK {
        return;
    }
    let mut budget = load_generation_budget(root);
    if budget
        .last_peak_ram_bytes
        .map(|old| peak_ram_bytes > old)
        .unwrap_or(true)
    {
        budget.last_peak_ram_bytes = Some(peak_ram_bytes);
        budget.last_recorded_at_unix = Some(unix_millis() as u64);
        let _ = write_json(&generation_budget_path(root), &budget);
    }
}

fn finish_generation_peak(root: &Path, peak_ram_bytes: u64) {
    persist_generation_peak(root, peak_ram_bytes);
    let mut budget = load_generation_budget(root);
    budget.generation_count = budget.generation_count.saturating_add(1);
    let _ = write_json(&generation_budget_path(root), &budget);
}

struct WorkerRun {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    peak_ram_bytes: u64,
}

struct WorkerFail {
    message: String,
    peak_ram_bytes: u64,
}

fn run_worker_until(root: &Path, mut command: Command, timeout: Duration) -> Result<WorkerRun, WorkerFail> {
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());
    command.stdin(Stdio::null());
    let mut child = command.spawn().map_err(|error| WorkerFail {
        message: error.to_string(),
        peak_ram_bytes: 0,
    })?;
    let _job = bind_worker_lifetime(&child);
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = stdout.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let stderr_thread = stderr.map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });

    let started = Instant::now();
    let mut peak = process_ram_bytes(pid).unwrap_or(0);
    persist_generation_peak(root, peak);
    let status = loop {
        if started.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            finish_generation_peak(root, peak);
            return Err(WorkerFail {
                message: "voice generation timed out".to_string(),
                peak_ram_bytes: peak,
            });
        }
        if let Some(used) = process_ram_bytes(pid) {
            if used > peak {
                peak = used;
                persist_generation_peak(root, peak);
            }
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(250).min(timeout)),
            Err(error) => {
                let _ = child.kill();
                finish_generation_peak(root, peak);
                return Err(WorkerFail {
                    message: error.to_string(),
                    peak_ram_bytes: peak,
                });
            }
        }
    };
    finish_generation_peak(root, peak);

    let stdout = stdout_thread
        .map(|thread| thread.join().unwrap_or_default())
        .unwrap_or_default();
    let stderr = stderr_thread
        .map(|thread| thread.join().unwrap_or_default())
        .unwrap_or_default();
    Ok(WorkerRun {
        status,
        stdout,
        stderr,
        peak_ram_bytes: peak,
    })
}

struct WorkerLifetime {
    #[cfg(windows)]
    job: Option<isize>,
}

impl WorkerLifetime {
    #[cfg(windows)]
    fn raw_job(&self) -> Option<isize> {
        self.job
    }
}

impl Drop for WorkerLifetime {
    fn drop(&mut self) {
        #[cfg(windows)]
        if let Some(job) = self.job.take() {
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(
                    windows::Win32::Foundation::HANDLE(job as *mut core::ffi::c_void),
                );
            };
        }
    }
}

fn bind_worker_lifetime(child: &std::process::Child) -> WorkerLifetime {
    #[cfg(windows)]
    {
        WorkerLifetime {
            job: bind_windows_job(child),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = child;
        WorkerLifetime {}
    }
}

#[cfg(windows)]
fn bind_windows_job(child: &std::process::Child) -> Option<isize> {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JOBOBJECT_BASIC_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JobObjectBasicLimitInformation,
    };

    let job = unsafe { CreateJobObjectW(None, None).ok()? };
    let limits = JOBOBJECT_BASIC_LIMIT_INFORMATION {
        LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        ..Default::default()
    };
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectBasicLimitInformation,
            &limits as *const JOBOBJECT_BASIC_LIMIT_INFORMATION as *const _,
            std::mem::size_of::<JOBOBJECT_BASIC_LIMIT_INFORMATION>() as u32,
        )
    };
    if configured.is_err() {
        unsafe { let _ = windows::Win32::Foundation::CloseHandle(job); };
        return None;
    }
    let process = HANDLE(child.as_raw_handle());
    if unsafe { AssignProcessToJobObject(job, process) }.is_err() {
        unsafe { let _ = windows::Win32::Foundation::CloseHandle(job); };
        return None;
    }
    Some(job.0 as isize)
}

#[derive(Clone, Copy, Default)]
struct ProcessRam {
    working_set: u64,
    private: u64,
}

fn process_ram_bytes(pid: u32) -> Option<u64> {
    process_tree_ram(pid).map(|ram| ram.working_set)
}

fn process_tree_ram(pid: u32) -> Option<ProcessRam> {
    let mut total = ProcessRam::default();
    for child in process_tree_pids(pid) {
        if let Some(ram) = process_ram_one(child) {
            total.working_set = total.working_set.saturating_add(ram.working_set);
            total.private = total.private.saturating_add(ram.private);
        }
    }
    (total.working_set > 0 || total.private > 0).then_some(total)
}

fn process_tree_pids(root: u32) -> Vec<u32> {
    let mut pids = vec![root];
    let mut index = 0;
    while index < pids.len() {
        let parent = pids[index];
        for child in child_pids(parent) {
            if !pids.contains(&child) {
                pids.push(child);
            }
        }
        index += 1;
    }
    pids
}

#[cfg(target_os = "windows")]
fn child_pids(pid: u32) -> Vec<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    let Ok(snapshot) = snapshot else {
        return Vec::new();
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut children = Vec::new();
    let mut more = unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok();
    while more {
        if entry.th32ParentProcessID == pid {
            children.push(entry.th32ProcessID);
        }
        more = unsafe { Process32NextW(snapshot, &mut entry) }.is_ok();
    }
    let _ = unsafe { CloseHandle(snapshot) };
    children
}

#[cfg(target_os = "linux")]
fn child_pids(pid: u32) -> Vec<u32> {
    fs::read_to_string(format!("/proc/{pid}/task/{pid}/children"))
        .unwrap_or_default()
        .split_whitespace()
        .filter_map(|value| value.parse().ok())
        .collect()
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn child_pids(_pid: u32) -> Vec<u32> {
    Vec::new()
}

#[cfg(target_os = "windows")]
fn process_ram_one(pid: u32) -> Option<ProcessRam> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_QUERY_INFORMATION | PROCESS_VM_READ,
            false,
            pid,
        )
    }
    .ok()?;
    let mut counters = PROCESS_MEMORY_COUNTERS_EX {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
        ..Default::default()
    };
    let ok = unsafe {
        K32GetProcessMemoryInfo(
            handle,
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        )
    }
    .as_bool();
    let _ = unsafe { CloseHandle(handle) };
    ok.then_some(ProcessRam {
        working_set: counters.WorkingSetSize as u64,
        private: counters.PrivateUsage as u64,
    })
}

#[cfg(target_os = "linux")]
fn process_ram_one(pid: u32) -> Option<ProcessRam> {
    let text = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let rss = text
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()?
        * 1024;
    Some(ProcessRam {
        working_set: rss,
        private: rss,
    })
}

#[cfg(target_os = "macos")]
fn process_ram_one(pid: u32) -> Option<ProcessRam> {
    crate::macos::process_ram_bytes(pid).map(|bytes| ProcessRam {
        working_set: bytes,
        private: bytes,
    })
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn process_ram_one(_pid: u32) -> Option<ProcessRam> {
    None
}

#[cfg(target_os = "macos")]
fn available_disk_bytes(path: &Path) -> Option<u64> {
    crate::macos::available_disk_bytes(path)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn available_disk_bytes(path: &Path) -> Option<u64> {
    let output = Command::new("df").arg("-k").arg(path).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text.lines().nth(1)?;
    let kb = line.split_whitespace().nth(3)?.parse::<u64>().ok()?;
    Some(kb * 1024)
}

#[cfg(target_os = "windows")]
fn available_disk_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let wide = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut available = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), Some(&mut available), None, None) }
        .ok()?;
    Some(available)
}

#[cfg(target_os = "linux")]
fn total_ram_bytes() -> Option<u64> {
    linux_meminfo_kb("MemTotal:").map(|kb| kb * 1024)
}

#[cfg(target_os = "linux")]
fn available_ram_bytes() -> Option<u64> {
    linux_meminfo_kb("MemAvailable:").map(|kb| kb * 1024)
}

#[cfg(target_os = "linux")]
fn linux_meminfo_kb(prefix: &str) -> Option<u64> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    text.lines()
        .find_map(|line| line.strip_prefix(prefix))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
}

#[cfg(target_os = "macos")]
use crate::macos::{available_ram_bytes, total_ram_bytes};

#[cfg(target_os = "windows")]
fn total_ram_bytes() -> Option<u64> {
    windows_memory_status().map(|status| status.ullTotalPhys)
}

#[cfg(target_os = "windows")]
fn available_ram_bytes() -> Option<u64> {
    // ullAvailPhys is available physical memory, not unused-only.
    windows_memory_status().map(|status| status.ullAvailPhys)
}

#[cfg(target_os = "windows")]
fn windows_memory_status() -> Option<windows::Win32::System::SystemInformation::MEMORYSTATUSEX> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};

    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut status) }.ok()?;
    Some(status)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn total_ram_bytes() -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn available_ram_bytes() -> Option<u64> {
    None
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

fn voice_root(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?
        .join("voice"))
}

fn ensure_voice_root(root: &Path) -> Result<PathBuf, String> {
    fs::create_dir_all(root).map_err(|error| error.to_string())?;
    root.canonicalize().map_err(|error| error.to_string())
}

fn supervisor_storage_file(root: &Path, value: &str, purpose: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value)
        .canonicalize()
        .map_err(|error| error.to_string())?;
    if !path.starts_with(root) {
        return Err(format!("{purpose} must come from Supervisor voice storage"));
    }
    if !path.is_file() {
        return Err(format!("{} not found: {}", purpose, path.display()));
    }
    Ok(path)
}

fn model_dir(root: &Path) -> PathBuf {
    root.join("model_packs").join(MODEL_ID)
}

fn is_starter_profile(id: &str) -> bool {
    STARTER_PROFILES.iter().any(|(starter, _, _)| *starter == id)
}

fn starter_wav_src(file: &str, app: Option<&AppHandle>) -> Option<PathBuf> {
    let crate_file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("voices")
        .join(file);
    if crate_file.is_file() {
        return Some(crate_file);
    }
    let bundled = app?.path().resource_dir().ok()?.join("voices").join(file);
    bundled.is_file().then_some(bundled)
}

fn seed_starter_profiles(root: &Path, app: Option<&AppHandle>) {
    for (id, name, file) in STARTER_PROFILES {
        let Some(src) = starter_wav_src(file, app) else {
            continue;
        };
        let dir = profile_dir(root, id);
        let dest = dir.join("reference.wav");
        if fs::create_dir_all(&dir).is_err() {
            continue;
        }
        if !dest.exists() && fs::copy(src, &dest).is_err() {
            continue;
        }
        let _ = write_json(
            &dir.join("profile.json"),
            &VoiceProfileMetadata {
                profile_id: (*id).to_string(),
                display_name: (*name).to_string(),
                reference_retained: true,
            },
        );
    }
}

fn profiles_dir(root: &Path) -> PathBuf {
    root.join("profiles")
}

fn recordings_dir(root: &Path) -> PathBuf {
    root.join("recordings")
}

fn profile_dir(root: &Path, profile_id: &str) -> PathBuf {
    profiles_dir(root).join(profile_id)
}

fn generation_staging_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join(format!(".generation-{}", unix_millis()))
}

fn required_model_bytes() -> u64 {
    MODEL_FILES.iter().map(|file| file.bytes).sum()
}

fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
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

    #[test]
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    fn system_resource_queries_return_real_values() {
        let total = total_ram_bytes().expect("total ram");
        let available = available_ram_bytes().expect("available ram");
        assert!(total > 1024 * 1024 * 1024);
        assert!(available > 0 && available <= total);
        assert!(available > total / 50, "available {available} of {total}");
        assert!(available_disk_bytes(&std::env::temp_dir()).expect("free disk") > 0);
        assert!(process_ram_bytes(std::process::id()).expect("own process ram") > 0);
    }

    #[test]
    fn minimal_manifest_excludes_unused_s3gen_checkpoint() {
        assert!(MODEL_FILES
            .iter()
            .any(|file| file.path == "s3gen_meanflow.safetensors"));
        assert!(!MODEL_FILES
            .iter()
            .any(|file| file.path == "s3gen.safetensors"));
        assert_eq!(required_model_bytes(), 1_942_099_748);
    }

    #[test]
    fn ids_reject_path_traversal() {
        assert_eq!(clean_id("derek_01").unwrap(), "derek_01");
        assert!(clean_id("../derek").is_err());
        assert!(clean_id("derek/profile").is_err());
        assert!(clean_id("").is_err());
    }

    #[test]
    fn generation_ram_gate_uses_available_ram_and_measured_peak() {
        assert_eq!(
            required_available_ram_bytes(None),
            DEFAULT_MIN_AVAILABLE_RAM_BYTES
        );
        assert_eq!(
            required_available_ram_bytes(Some(5 * GIBIBYTE)),
            6 * GIBIBYTE
        );
        assert_eq!(
            required_available_ram_bytes(Some(2 * GIBIBYTE)),
            MIN_AVAILABLE_RAM_AFTER_MEASURE_BYTES
        );

        assert_eq!(
            ram_allows_generation(
                Some(DEFAULT_MIN_AVAILABLE_RAM_BYTES),
                Some(16 * GIBIBYTE),
                None
            ),
            Some(true)
        );
        assert_eq!(
            ram_allows_generation(
                Some(DEFAULT_MIN_AVAILABLE_RAM_BYTES.saturating_sub(GIBIBYTE)),
                Some(16 * GIBIBYTE),
                None
            ),
            Some(false)
        );
        assert_eq!(
            ram_allows_generation(Some(7 * GIBIBYTE), Some(4 * GIBIBYTE), None),
            Some(false)
        );
        assert_eq!(ram_allows_generation(None, Some(16 * GIBIBYTE), None), None);

        assert_eq!(
            required_ram_for_gate(true, None),
            LOADED_WORKER_HEADROOM_BYTES
        );
        assert_eq!(
            ram_allows_job(Some(LOADED_WORKER_HEADROOM_BYTES), Some(16 * GIBIBYTE), None, true),
            Some(true)
        );
        assert_eq!(
            ram_allows_job(
                Some(LOADED_WORKER_HEADROOM_BYTES.saturating_sub(1)),
                Some(16 * GIBIBYTE),
                None,
                true
            ),
            Some(false)
        );
        assert_eq!(
            ram_allows_job(Some(2 * GIBIBYTE), Some(16 * GIBIBYTE), None, true),
            Some(true)
        );
        assert_eq!(
            ram_allows_job(Some(2 * GIBIBYTE), Some(16 * GIBIBYTE), None, false),
            Some(false)
        );
    }

    #[test]
    fn generation_log_writes_one_json_line() {
        let root = test_dir("gen-log");
        fs::create_dir_all(&root).unwrap();
        append_generation_log(
            &root,
            &GenerationLogRow {
                at_ms: 1,
                os: "windows",
                ok: false,
                error: Some("Not enough available RAM".into()),
                profile: "derek".into(),
                phrases: 4,
                merge: true,
                need: 5 * GIBIBYTE,
                available_before: Some(3 * GIBIBYTE),
                available_after: Some(3 * GIBIBYTE),
                total: Some(16 * GIBIBYTE),
                worker_peak: None,
                worker_private: None,
                duration_ms: 12,
                reused_model: false,
                reused_voice: false,
                spawn_ms: None,
                imports_ms: None,
                model_load_ms: None,
                condition_ms: None,
                synthesis_ms: None,
                wav_write_ms: None,
                pressure: None,
            },
        );
        let text = fs::read_to_string(generation_log_path(&root)).unwrap();
        assert!(text.starts_with('{'));
        assert!(text.contains("\"ok\":false"));
        assert!(text.contains("\"profile\":\"derek\""));
        assert!(text.contains("\"phrases\":4"));
        assert!(!text.contains("pressure"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clip_summaries_only_keep_real_wavs_and_phrase_ids() {
        let root = test_dir("clip-summaries");
        let clips = root.join("clips");
        fs::create_dir_all(&clips).unwrap();
        fs::write(clips.join("01_hello.wav"), vec![0u8; 80]).unwrap();
        fs::write(clips.join("empty.wav"), []).unwrap();
        fs::write(clips.join("note.txt"), b"not a clip").unwrap();
        fs::write(clips.join("plain.wav"), vec![1u8; 60]).unwrap();

        let found = clip_summaries(&clips).unwrap();
        let ids: Vec<_> = found.iter().map(|clip| clip.phrase_id.as_str()).collect();
        assert_eq!(ids, ["hello", "plain"]);
        assert!(found.iter().all(|clip| clip.bytes >= 44));
        assert!(found.iter().all(|clip| Path::new(&clip.path).is_file()));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn clip_id_matches_worker_output_names() {
        assert!(clip_path_matches(
            Path::new("01_focus_tab_switching_lock_in.wav"),
            "focus_tab_switching_lock_in"
        ));
        assert!(clip_path_matches(
            Path::new("focus_tab_switching_lock_in.wav"),
            "focus_tab_switching_lock_in"
        ));
        assert!(!clip_path_matches(
            Path::new("01_focus_tab_switching_lock_in.wav"),
            "other"
        ));
    }

    #[test]
    fn status_poll_uses_size_only_model_check() {
        let root = test_dir("status-size-only");
        let dir = model_dir(&root);
        fs::create_dir_all(&dir).unwrap();
        let file = &MODEL_FILES[0];
        let path = dir.join(file.path);
        File::create(&path).unwrap().set_len(file.bytes).unwrap();

        let status = status_for_root(&root, false);
        let listed = status
            .model
            .files
            .iter()
            .find(|item| item.path == file.path)
            .unwrap();
        assert!(listed.valid);
        assert!(!status.model.installed);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn cancelled_download_keeps_verified_model_files() {
        let dir = test_dir("cancel-download");
        fs::create_dir_all(&dir).unwrap();
        let verified = dir.join("verified.bin");
        let partial = dir.join("weights.download");
        fs::write(&verified, b"keep").unwrap();
        fs::write(&partial, b"partial").unwrap();
        discard_partial_download(&partial);
        assert_eq!(fs::read(&verified).unwrap(), b"keep");
        assert!(!partial.exists());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn worker_wait_stops_a_process_at_the_deadline() {
        let root = test_dir("worker-timeout");
        fs::create_dir_all(&root).unwrap();
        let mut command = if cfg!(windows) {
            let mut command = Command::new("ping");
            command.args(["127.0.0.1", "-n", "8"]);
            command
        } else {
            let mut command = Command::new("sleep");
            command.arg("5");
            command
        };
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let error = match run_worker_until(&root, command, Duration::from_millis(200)) {
            Err(fail) => fail.message,
            Ok(_) => panic!("worker should time out"),
        };
        assert!(error.contains("timed out"), "{error}");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn fast_model_status_uses_size_without_hashing() {
        let root = test_dir("fast-model-status");
        let dir = model_dir(&root);
        fs::create_dir_all(&dir).unwrap();
        let file = &MODEL_FILES[0];
        let path = dir.join(file.path);
        File::create(&path).unwrap().set_len(file.bytes).unwrap();

        assert!(model_file_status(&dir, file, false).valid);
        assert!(!model_file_status(&dir, file, true).valid);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn supervisor_storage_file_rejects_external_reference_paths() {
        let root = test_dir("storage-root");
        let outside = test_dir("storage-outside");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let inside_file = root.join("recording.wav");
        let outside_file = outside.join("recording.wav");
        fs::write(&inside_file, b"wav").unwrap();
        fs::write(&outside_file, b"wav").unwrap();
        let root = ensure_voice_root(&root).unwrap();

        assert_eq!(
            supervisor_storage_file(&root, inside_file.to_str().unwrap(), "recording").unwrap(),
            inside_file.canonicalize().unwrap()
        );
        assert!(
            supervisor_storage_file(&root, outside_file.to_str().unwrap(), "recording").is_err()
        );

        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }

    #[test]
    fn merging_generated_clips_replaces_matching_phrase_only() {
        let root = test_dir("merge-clips");
        let profile = root.join("profile");
        let clips = profile.join("clips");
        let staging = profile.join(".generation-test");
        fs::create_dir_all(&clips).unwrap();
        fs::create_dir_all(staging.join("clips")).unwrap();
        fs::write(clips.join("03_hello.wav"), b"old-hello").unwrap();
        fs::write(clips.join("04_other.wav"), b"keep").unwrap();
        fs::write(staging.join("clips").join("01_hello.wav"), b"new-hello").unwrap();

        merge_generated_clips(&profile, &staging).unwrap();

        assert!(!clips.join("03_hello.wav").exists());
        assert_eq!(fs::read(clips.join("01_hello.wav")).unwrap(), b"new-hello");
        assert_eq!(fs::read(clips.join("04_other.wav")).unwrap(), b"keep");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn replacing_generated_clips_keeps_only_staged_clips() {
        let root = test_dir("replace-clips");
        let profile = root.join("profile");
        let clips = profile.join("clips");
        let staging = profile.join(".generation-test");
        fs::create_dir_all(&clips).unwrap();
        fs::create_dir_all(staging.join("clips")).unwrap();
        fs::write(clips.join("old.wav"), b"old").unwrap();
        fs::write(staging.join("clips").join("new.wav"), b"new").unwrap();

        replace_generated_clips(&profile, &staging).unwrap();

        assert!(!clips.join("old.wav").exists());
        assert_eq!(fs::read(clips.join("new.wav")).unwrap(), b"new");

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn seed_starter_profiles_copies_derek_and_elise_references() {
        let root = test_dir("starter-seed");
        seed_starter_profiles(&root, None);
        let derek = profile_dir(&root, "derek").join("reference.wav");
        let elise = profile_dir(&root, "elise").join("reference.wav");
        assert!(derek.is_file(), "missing {derek:?}");
        assert!(elise.is_file(), "missing {elise:?}");
        assert!(derek.metadata().unwrap().len() > MIN_CLIP_BYTES);
        assert!(elise.metadata().unwrap().len() > MIN_CLIP_BYTES);
        seed_starter_profiles(&root, None);
        let first = derek.metadata().unwrap().len();
        seed_starter_profiles(&root, None);
        assert_eq!(derek.metadata().unwrap().len(), first);
        let _ = fs::remove_dir_all(root);
    }

    fn test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "supervisor-voice-{name}-{}-{}",
            std::process::id(),
            unix_millis()
        ))
    }
}
