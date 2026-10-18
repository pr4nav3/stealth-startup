use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
#[cfg(debug_assertions)]
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

const WORKER_EVENT: &str = "webcam-frame";
/// The plan's floor. With the worker's 3-of-5 smoothing a phone shows up in under a second.
const WORKER_FPS: &str = "4";
/// After the worker fails (no camera, camera denied), sessions wait this long to try again.
const RETRY_AFTER: Duration = Duration::from_secs(30);
#[cfg(not(debug_assertions))]
const WORKER_NAME: &str = "supervisor-webcam-worker";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebcamFrame {
    #[serde(default)]
    pub face_in_frame: bool,
    #[serde(default)]
    pub gaze: Option<String>,
    #[serde(default)]
    pub person_in_frame: bool,
    #[serde(default)]
    pub present: bool,
    #[serde(default)]
    pub phone_in_frame: bool,
    #[serde(default)]
    pub looking_at_phone: bool,
    #[serde(default)]
    pub phone_count: u32,
    #[serde(default)]
    pub phone_confidence: f32,
    #[serde(default)]
    pub jpeg: String,
    /// "cpu" or "gpu", as requested.
    #[serde(default)]
    pub device: String,
    /// The ONNX Runtime provider that actually runs the model.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub error: Option<String>,
}

impl WebcamFrame {
    fn error_frame(message: impl Into<String>) -> Self {
        Self {
            face_in_frame: false,
            gaze: None,
            person_in_frame: false,
            present: false,
            phone_in_frame: false,
            looking_at_phone: false,
            phone_count: 0,
            phone_confidence: 0.0,
            jpeg: String::new(),
            device: String::new(),
            provider: String::new(),
            error: Some(message.into()),
        }
    }
}

/// Called for every reading, preview or not. Frames themselves never leave this module except
/// as the preview JPEG, which only the Settings page asks for and nothing stores.
pub type FrameObserver = fn(&AppHandle, &WebcamFrame);

/// One worker serves both the Settings preview and live sessions.
pub struct WebcamPreview {
    starting: Mutex<()>,
    stop: Mutex<Arc<AtomicBool>>,
    child: Mutex<Option<Child>>,
    /// The running worker sends preview JPEGs.
    preview: AtomicBool,
    failed_at: Arc<Mutex<Option<Instant>>>,
}

impl Default for WebcamPreview {
    fn default() -> Self {
        Self {
            starting: Mutex::new(()),
            stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            child: Mutex::new(None),
            preview: AtomicBool::new(false),
            failed_at: Arc::new(Mutex::new(None)),
        }
    }
}

impl Drop for WebcamPreview {
    fn drop(&mut self) {
        stop_preview(self);
    }
}

impl WebcamPreview {
    pub fn is_preview(&self) -> bool {
        self.preview.load(Ordering::SeqCst)
    }

    pub fn is_starting(&self) -> bool {
        self.starting.try_lock().is_err()
    }

    /// Whether a session should leave the camera alone for now after a failure.
    pub fn recently_failed(&self) -> bool {
        self.failed_at
            .lock()
            .ok()
            .and_then(|at| *at)
            .is_some_and(|at| at.elapsed() < RETRY_AFTER)
    }

    fn mark_failed(&self) {
        if let Ok(mut at) = self.failed_at.lock() {
            *at = Some(Instant::now());
        }
    }
    fn take_child(&self) -> Option<Child> {
        self.child.lock().ok().and_then(|mut slot| slot.take())
    }

    fn store_child(&self, child: Child) -> Result<(), String> {
        let mut slot = self
            .child
            .lock()
            .map_err(|_| "webcam lock unavailable".to_string())?;
        *slot = Some(child);
        Ok(())
    }

    fn replace_stop_flag(&self) -> Result<Arc<AtomicBool>, String> {
        let mut slot = self
            .stop
            .lock()
            .map_err(|_| "webcam lock unavailable".to_string())?;
        slot.store(true, Ordering::SeqCst);
        let next = Arc::new(AtomicBool::new(false));
        *slot = Arc::clone(&next);
        Ok(next)
    }

    pub fn is_running(&self) -> bool {
        self.child
            .lock()
            .ok()
            .and_then(|mut slot| {
                let child = slot.as_mut()?;
                match child.try_wait() {
                    Ok(None) => Some(true),
                    Ok(Some(_)) => {
                        *slot = None;
                        Some(false)
                    }
                    Err(_) => Some(false),
                }
            })
            .unwrap_or(false)
    }
}

/// Starts the worker unless one already serves: a preview worker also serves a session, and a
/// session worker is replaced by a preview one when the Settings page asks for pictures.
pub fn start_worker(
    app: AppHandle,
    state: &WebcamPreview,
    gpu: bool,
    preview: bool,
    on_frame: FrameObserver,
) -> Result<(), String> {
    let _one_start_at_a_time = state
        .starting
        .lock()
        .map_err(|_| "webcam lock unavailable".to_string())?;
    if state.is_running() && (state.is_preview() || !preview) {
        return Ok(());
    }
    stop_preview(state);
    let started = spawn_worker(app, state, gpu, preview, on_frame);
    if started.is_err() {
        state.mark_failed();
    }
    started
}

fn spawn_worker(
    app: AppHandle,
    state: &WebcamPreview,
    gpu: bool,
    preview: bool,
    on_frame: FrameObserver,
) -> Result<(), String> {
    camera_access()?;

    let mut command = worker_command(&app)?;
    if preview {
        command.arg("--preview");
    }
    command
        .arg("--fps")
        .arg(WORKER_FPS)
        .arg("--device")
        .arg(if gpu { "gpu" } else { "cpu" })
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("PYTHONUNBUFFERED", "1")
        .env("ORT_LOG_SEVERITY_LEVEL", "3");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start webcam worker: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "webcam worker stdout missing".to_string())?;
    let stop = state.replace_stop_flag()?;
    state.store_child(child)?;
    state.preview.store(preview, Ordering::SeqCst);
    if let Ok(mut at) = state.failed_at.lock() {
        *at = None;
    }

    let failed_at = Arc::clone(&state.failed_at);
    std::thread::spawn(move || read_worker(app, stdout, stop, on_frame, failed_at));
    Ok(())
}

pub fn stop_preview(state: &WebcamPreview) {
    if let Ok(flag) = state.stop.lock() {
        flag.store(true, Ordering::SeqCst);
    }
    if let Some(mut child) = state.take_child() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn read_worker<R: std::io::Read>(
    app: AppHandle,
    stdout: R,
    stop: Arc<AtomicBool>,
    on_frame: FrameObserver,
    failed_at: Arc<Mutex<Option<Instant>>>,
) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let Ok(line) = line else {
            break;
        };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<WebcamFrame>(line) {
            Ok(frame) => {
                on_frame(&app, &frame);
                let _ = app.emit(WORKER_EVENT, &frame);
            }
            Err(_) => {
                let _ = app.emit(
                    WORKER_EVENT,
                    WebcamFrame::error_frame(line.chars().take(180).collect::<String>()),
                );
            }
        }
    }
    if !stop.load(Ordering::SeqCst) {
        if let Ok(mut at) = failed_at.lock() {
            *at = Some(Instant::now());
        }
        let _ = app.emit(
            WORKER_EVENT,
            WebcamFrame::error_frame("Webcam worker stopped"),
        );
    }
}

/// Release builds run the frozen worker that the installer put next to the app
/// (see `scripts/build_webcam_runtime.py` and `tauri.bundle.conf.json`).
#[cfg(not(debug_assertions))]
fn worker_command(app: &AppHandle) -> Result<Command, String> {
    use tauri::Manager;

    let root = app
        .path()
        .resource_dir()
        .map_err(|error| format!("Could not find Supervisor's install folder: {error}"))?
        .join("webcam");
    let worker = root
        .join("worker")
        .join(format!("{WORKER_NAME}{}", std::env::consts::EXE_SUFFIX));
    if !worker.is_file() {
        return Err(format!(
            "The webcam worker is missing from this install ({}). Reinstall Supervisor.",
            worker.display()
        ));
    }
    let mut command = Command::new(&worker);
    command.arg("--models").arg(root.join("models")).current_dir(&root);
    Ok(command)
}

/// Debug builds run the worker script from the source tree with the dev venv.
#[cfg(debug_assertions)]
fn worker_command(_app: &AppHandle) -> Result<Command, String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let bakeoff = root.join("webcam_bakeoff");
    let venv = root.join(".venv-webcam");
    let python = if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    };
    if !python.is_file() {
        return Err(format!(
            "Webcam dev venv missing at {}. See webcam_bakeoff/README.md.",
            venv.display()
        ));
    }
    let mut command = Command::new(python);
    command
        .arg("-u")
        .arg(bakeoff.join("run_tracker.py"))
        .arg("--models")
        .arg(bakeoff.join("models"))
        .current_dir(&bakeoff);
    Ok(command)
}

#[cfg(not(target_os = "macos"))]
fn camera_access() -> Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
use crate::macos::camera_access;

#[cfg(test)]
mod tests {
    use super::WebcamFrame;

    #[test]
    fn parses_worker_json_line() {
        let line = r#"{"face_in_frame":true,"gaze":null,"person_in_frame":true,"present":true,"phone_in_frame":false,"looking_at_phone":false,"phone_count":0,"phone_confidence":0.0,"jpeg":""}"#;
        let frame: WebcamFrame = serde_json::from_str(line).unwrap();
        assert!(frame.face_in_frame);
        assert_eq!(frame.gaze, None);
        assert!(frame.present);
        assert!(!frame.looking_at_phone);
        assert!(frame.jpeg.is_empty());
    }

    #[test]
    fn parses_worker_error_line_with_only_device() {
        let line = r#"{"device":"gpu","error":"GPU mode: DmlExecutionProvider could not start"}"#;
        let frame: WebcamFrame = serde_json::from_str(line).unwrap();
        assert_eq!(frame.device, "gpu");
        assert!(!frame.phone_in_frame);
        assert!(frame.error.unwrap().starts_with("GPU mode"));
    }

    #[test]
    fn parses_null_gaze_and_error() {
        let line = r#"{"face_in_frame":false,"gaze":null,"phone_in_frame":false,"looking_at_phone":false,"error":"could not open camera 0"}"#;
        let frame: WebcamFrame = serde_json::from_str(line).unwrap();
        assert!(!frame.face_in_frame);
        assert_eq!(frame.gaze, None);
        assert_eq!(frame.error.as_deref(), Some("could not open camera 0"));
    }
}
