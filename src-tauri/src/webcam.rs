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
/// Watch rate during a session. The worker jumps to its burst rate (5) from the first frame that
/// looks like a phone, so with 3-of-5 smoothing a phone still shows up in about a second.
const SESSION_FPS: &str = "2";
/// The Settings preview detects at this rate (a phone still clears in about a second)…
const PREVIEW_DETECT_FPS: &str = "4";
/// …and sends pictures at this rate between detections, so the preview looks like video.
const PREVIEW_FPS: &str = "15";
/// A worker that was sending frames and then goes this long without one is stuck (frozen camera
/// driver). Frames come at least twice a second, so this is never just a slow frame.
const STALL_AFTER: Duration = Duration::from_secs(15);
/// A worker that never sends its first line (model load, a wedged camera driver, a slow
/// first launch under antivirus) is given this long before it is replaced.
const FIRST_LINE_AFTER: Duration = Duration::from_secs(60);
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
    /// False on a preview picture sent between detections: the labels are the last detection's.
    #[serde(default = "yes")]
    pub detected: bool,
}

fn yes() -> bool {
    true
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
            detected: true,
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
    /// When the running worker last sent a line; `None` until its first, so a slow start (model
    /// load, camera open, a virus scan of the first launch) is never mistaken for a hang.
    last_line_at: Arc<Mutex<Option<Instant>>>,
    spawned_at: Mutex<Option<Instant>>,
    /// Keeps the Windows job object alive, so the worker dies with the app.
    lifetime: Mutex<Option<crate::voice::WorkerLifetime>>,
    /// GPU mode failed this run: sessions fall back to CPU instead of retrying it every 30 s.
    gpu_failed: Arc<AtomicBool>,
}

impl Default for WebcamPreview {
    fn default() -> Self {
        Self {
            starting: Mutex::new(()),
            stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            child: Mutex::new(None),
            preview: AtomicBool::new(false),
            failed_at: Arc::new(Mutex::new(None)),
            last_line_at: Arc::new(Mutex::new(None)),
            spawned_at: Mutex::new(None),
            lifetime: Mutex::new(None),
            gpu_failed: Arc::new(AtomicBool::new(false)),
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

    /// Running but silent too long: a hung worker that would otherwise never be restarted.
    pub fn is_stalled(&self) -> bool {
        let quiet = self.last_line_at.lock().ok().and_then(|at| *at).map(|at| at.elapsed());
        let stalled = match quiet {
            Some(quiet) => quiet >= STALL_AFTER,
            None => self
                .spawned_at
                .lock()
                .ok()
                .and_then(|at| *at)
                .is_some_and(|at| at.elapsed() >= FIRST_LINE_AFTER),
        };
        stalled && self.is_running()
    }

    pub fn gpu_failed(&self) -> bool {
        self.gpu_failed.load(Ordering::SeqCst)
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
    camera: u32,
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
    let started = spawn_worker(app, state, gpu, camera, preview, on_frame);
    if started.is_err() {
        state.mark_failed();
    }
    started
}

fn spawn_worker(
    app: AppHandle,
    state: &WebcamPreview,
    gpu: bool,
    camera: u32,
    preview: bool,
    on_frame: FrameObserver,
) -> Result<(), String> {
    camera_access()?;

    let mut command = worker_command(&app)?;
    if preview {
        command.arg("--preview").arg("--preview-fps").arg(PREVIEW_FPS);
    }
    command
        .arg("--fps")
        .arg(if preview { PREVIEW_DETECT_FPS } else { SESSION_FPS })
        .arg("--device")
        .arg(if gpu { "gpu" } else { "cpu" })
        .arg("--camera")
        .arg(camera.to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("PYTHONUNBUFFERED", "1")
        .env("ORT_LOG_SEVERITY_LEVEL", "3");

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(windows::Win32::System::Threading::CREATE_NO_WINDOW.0);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }

    let mut child = command
        .spawn()
        .map_err(|error| format!("Could not start webcam worker: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "webcam worker stdout missing".to_string())?;
    let stop = state.replace_stop_flag()?;
    if let Ok(mut lifetime) = state.lifetime.lock() {
        *lifetime = Some(crate::voice::bind_worker_lifetime(&child));
    }
    if let Ok(mut at) = state.spawned_at.lock() {
        *at = Some(Instant::now());
    }
    state.store_child(child)?;
    state.preview.store(preview, Ordering::SeqCst);
    if let Ok(mut at) = state.failed_at.lock() {
        *at = None;
    }
    if let Ok(mut at) = state.last_line_at.lock() {
        *at = None;
    }

    let failed_at = Arc::clone(&state.failed_at);
    let last_line_at = Arc::clone(&state.last_line_at);
    let gpu_failed = Arc::clone(&state.gpu_failed);
    std::thread::spawn(move || read_worker(app, stdout, stop, on_frame, failed_at, last_line_at, gpu_failed));
    Ok(())
}

/// Camera names in the order the worker opens them: OpenCV's Media Foundation backend numbers
/// devices the way MFEnumDeviceSources lists them. Empty where the OS gives no list.
#[cfg(windows)]
pub fn camera_names() -> Vec<String> {
    use windows::core::PWSTR;
    use windows::Win32::Media::MediaFoundation::{
        IMFActivate, MFCreateAttributes, MFEnumDeviceSources, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    };
    use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

    // SAFETY: plain Media Foundation enumeration; every buffer it hands out is freed below.
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let mut attributes = None;
        if MFCreateAttributes(&mut attributes, 1).is_err() {
            return Vec::new();
        }
        let Some(attributes) = attributes else {
            return Vec::new();
        };
        if attributes
            .SetGUID(&MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID)
            .is_err()
        {
            return Vec::new();
        }
        let mut devices: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        if MFEnumDeviceSources(&attributes, &mut devices, &mut count).is_err() || devices.is_null() {
            return Vec::new();
        }
        let names = std::slice::from_raw_parts(devices, count as usize)
            .iter()
            .enumerate()
            .map(|(index, device)| {
                let mut name = PWSTR::null();
                let mut length = 0u32;
                let friendly = device.as_ref().and_then(|device| {
                    device
                        .GetAllocatedString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut name, &mut length)
                        .ok()
                        .and_then(|_| name.to_string().ok())
                });
                if !name.is_null() {
                    CoTaskMemFree(Some(name.0 as *const _));
                }
                friendly.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| format!("Camera {index}"))
            })
            .collect();
        // Each IMFActivate is dropped by the slice iteration's clones going out of scope, but the
        // array itself is ours to free.
        for device in std::slice::from_raw_parts_mut(devices, count as usize) {
            drop(device.take());
        }
        CoTaskMemFree(Some(devices as *const _));
        names
    }
}

/// macOS: the same discovery session OpenCV's AVFoundation backend uses (built-in cameras,
/// then external ones), so index N here is the worker's camera N.
#[cfg(target_os = "macos")]
pub fn camera_names() -> Vec<String> {
    use objc2_av_foundation::{
        AVCaptureDeviceDiscoverySession, AVCaptureDevicePosition, AVCaptureDeviceTypeBuiltInWideAngleCamera,
        AVCaptureDeviceTypeExternal, AVMediaTypeVideo,
    };
    use objc2_foundation::NSArray;

    unsafe {
        let types = NSArray::from_slice(&[
            AVCaptureDeviceTypeBuiltInWideAngleCamera,
            AVCaptureDeviceTypeExternal,
        ]);
        let session = AVCaptureDeviceDiscoverySession::discoverySessionWithDeviceTypes_mediaType_position(
            &types,
            AVMediaTypeVideo,
            AVCaptureDevicePosition::Unspecified,
        );
        session.devices().iter().map(|device| device.localizedName().to_string()).collect()
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn camera_names() -> Vec<String> {
    Vec::new()
}

pub fn stop_preview(state: &WebcamPreview) {
    if let Ok(flag) = state.stop.lock() {
        flag.store(true, Ordering::SeqCst);
    }
    if let Some(mut child) = state.take_child() {
        #[cfg(unix)]
        crate::voice::kill_process_group(child.id());
        let _ = child.kill();
        let _ = child.wait();
    }
    if let Ok(mut lifetime) = state.lifetime.lock() {
        *lifetime = None;
    }
}

fn read_worker<R: std::io::Read>(
    app: AppHandle,
    stdout: R,
    stop: Arc<AtomicBool>,
    on_frame: FrameObserver,
    failed_at: Arc<Mutex<Option<Instant>>>,
    last_line_at: Arc<Mutex<Option<Instant>>>,
    gpu_failed: Arc<AtomicBool>,
) {
    let reader = BufReader::new(stdout);
    for line in reader.lines() {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut at) = last_line_at.lock() {
            *at = Some(Instant::now());
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
                if frame.error.as_deref().is_some_and(|error| error.contains("GPU mode")) {
                    gpu_failed.store(true, Ordering::SeqCst);
                }
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

    /// `cargo test manual_camera_names -- --ignored --nocapture`
    #[test]
    #[ignore = "manual: lists the cameras the OS reports, in worker index order"]
    fn manual_camera_names() {
        for (index, name) in super::camera_names().iter().enumerate() {
            println!("camera {index}: {name}");
        }
    }

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
