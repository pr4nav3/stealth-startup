use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter};

const WORKER_EVENT: &str = "webcam-frame";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct WebcamFrame {
    pub face_in_frame: bool,
    pub gaze: Option<String>,
    #[serde(default)]
    pub person_in_frame: bool,
    #[serde(default)]
    pub present: bool,
    pub phone_in_frame: bool,
    #[serde(default)]
    pub looking_at_phone: bool,
    #[serde(default)]
    pub phone_count: u32,
    #[serde(default)]
    pub phone_confidence: f32,
    #[serde(default)]
    pub jpeg: String,
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
            error: Some(message.into()),
        }
    }
}

pub struct WebcamPreview {
    stop: Mutex<Arc<AtomicBool>>,
    child: Mutex<Option<Child>>,
}

impl Default for WebcamPreview {
    fn default() -> Self {
        Self {
            stop: Mutex::new(Arc::new(AtomicBool::new(false))),
            child: Mutex::new(None),
        }
    }
}

impl Drop for WebcamPreview {
    fn drop(&mut self) {
        stop_preview(self);
    }
}

impl WebcamPreview {
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

pub fn start_preview(app: AppHandle, state: &WebcamPreview) -> Result<(), String> {
    if state.is_running() {
        return Ok(());
    }
    stop_preview(state);

    let (python, script) = worker_paths()?;
    let mut command = Command::new(&python);
    command
        .arg("-u")
        .arg(&script)
        .arg("--preview")
        .arg("--fps")
        .arg("8")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .env("PYTHONUNBUFFERED", "1")
        .env("ORT_LOG_SEVERITY_LEVEL", "3")
        .current_dir(script.parent().unwrap_or_else(|| Path::new(".")));

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

    std::thread::spawn(move || read_worker(app, stdout, stop));
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

fn read_worker<R: std::io::Read>(app: AppHandle, stdout: R, stop: Arc<AtomicBool>) {
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
        let _ = app.emit(
            WORKER_EVENT,
            WebcamFrame::error_frame("Webcam worker stopped"),
        );
    }
}

fn worker_paths() -> Result<(PathBuf, PathBuf), String> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .canonicalize()
        .map_err(|error| error.to_string())?;
    let script = root.join("webcam_bakeoff").join("run_tracker.py");
    if !script.is_file() {
        return Err("webcam_bakeoff/run_tracker.py is missing".to_string());
    }

    let python = if cfg!(windows) {
        root.join(".venv-webcam").join("Scripts").join("python.exe")
    } else {
        root.join(".venv-webcam").join("bin").join("python")
    };
    if !python.is_file() {
        return Err(
            "Webcam venv missing. Run: python -m venv .venv-webcam && pip install -r webcam_bakeoff/requirements.txt && python webcam_bakeoff/download_models.py"
                .to_string(),
        );
    }
    Ok((python, script))
}

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
    fn parses_null_gaze_and_error() {
        let line = r#"{"face_in_frame":false,"gaze":null,"phone_in_frame":false,"looking_at_phone":false,"error":"could not open camera 0"}"#;
        let frame: WebcamFrame = serde_json::from_str(line).unwrap();
        assert!(!frame.face_in_frame);
        assert_eq!(frame.gaze, None);
        assert_eq!(frame.error.as_deref(), Some("could not open camera 0"));
    }
}
