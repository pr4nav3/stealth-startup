#[path = "activity_normalize.rs"]
mod activity_normalize;
#[cfg(target_os = "windows")]
#[path = "activity_win32.rs"]
mod activity_win32;

use activity_normalize::{
    classify_foreground_surface, idle_elapsed_seconds, is_neutral_window_class,
    normalize_title_text, ForegroundDisposition, RawForegroundIdentity,
};
use serde::ser::{SerializeStruct, Serializer};
use serde::Serialize;
#[cfg(not(target_os = "windows"))]
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone, Debug, PartialEq)]
pub struct ActivitySample {
    pub captured_at: String,
    pub platform: &'static str,
    pub app_name: String,
    pub process_path: String,
    pub process_id: u64,
    pub window_title: Option<String>,
    pub window_id: String,
    pub bounds: Bounds,
    pub idle_seconds: Option<f64>,
    pub permissions: CapturePermissions,
}

impl Serialize for ActivitySample {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("ActivitySample", 12)?;
        state.serialize_field("captured_at", &self.captured_at)?;
        state.serialize_field("platform", &self.platform)?;
        state.serialize_field("app_name", &self.app_name)?;
        state.serialize_field("process_path", &self.process_path)?;
        state.serialize_field("process_id", &self.process_id)?;
        state.serialize_field("window_title", &self.window_title)?;
        state.serialize_field("window_id", &self.window_id)?;
        state.serialize_field("bounds", &self.bounds)?;
        state.serialize_field("idle_seconds", &self.idle_seconds)?;
        state.serialize_field("permissions", &self.permissions)?;
        state.serialize_field("capture_kind", capture_kind_label(self))?;
        if let Some(window_class) = window_class_hint(self) {
            state.serialize_field("window_class", &window_class)?;
        }
        state.end()
    }
}

pub fn should_treat_as_neutral(sample: &ActivitySample) -> bool {
    if sample.platform != "windows" {
        return false;
    }

    matches!(
        classify_foreground_surface(&foreground_identity_from_sample(sample)),
        ForegroundDisposition::Neutral
    )
}

fn capture_kind_label(sample: &ActivitySample) -> &'static str {
    if should_treat_as_neutral(sample) {
        "neutral"
    } else {
        "normal"
    }
}

fn window_class_hint(sample: &ActivitySample) -> Option<String> {
    if is_neutral_window_class(&sample.app_name) {
        return Some(sample.app_name.clone());
    }
    None
}

fn foreground_identity_from_sample(sample: &ActivitySample) -> RawForegroundIdentity {
    let exe_name = if let Some(file_name) = std::path::Path::new(&sample.process_path)
        .file_name()
        .and_then(|name| name.to_str())
    {
        file_name.to_string()
    } else if sample.app_name.to_ascii_lowercase().ends_with(".exe") {
        sample.app_name.clone()
    } else {
        format!("{}.exe", sample.app_name)
    };

    let window_class = if is_neutral_window_class(&sample.app_name) {
        sample.app_name.clone()
    } else {
        String::new()
    };

    RawForegroundIdentity {
        exe_name,
        window_class,
        window_title: sample.window_title.clone().unwrap_or_default(),
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CapturePermissions {
    pub screen_recording: PermissionState,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    Granted,
    Missing,
    #[allow(dead_code)]
    NotRequired,
}

pub fn current_activity_sample() -> Result<ActivitySample, String> {
    let permissions = capture_permissions();
    let idle_seconds = idle_seconds();

    #[cfg(target_os = "windows")]
    {
        return activity_win32::capture_current(permissions, idle_seconds);
    }

    #[cfg(not(target_os = "windows"))]
    {
        use active_win_pos_rs::get_active_window;

        let captured_at = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .map_err(|error| error.to_string())?;
        let active_window =
            get_active_window().map_err(|_| "could not read the active window".to_string())?;

        Ok(sample_from_active_window(
            active_window,
            captured_at,
            permissions,
            idle_seconds,
        ))
    }
}

fn sample_from_active_window(
    active_window: active_win_pos_rs::ActiveWindow,
    captured_at: String,
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
) -> ActivitySample {
    ActivitySample {
        captured_at,
        platform: platform(),
        app_name: active_window.app_name,
        process_path: active_window.process_path.to_string_lossy().into_owned(),
        process_id: active_window.process_id,
        window_title: non_empty(active_window.title),
        window_id: active_window.window_id,
        bounds: Bounds {
            x: active_window.position.x,
            y: active_window.position.y,
            width: active_window.position.width,
            height: active_window.position.height,
        },
        idle_seconds,
        permissions,
    }
}

fn non_empty(value: String) -> Option<String> {
    normalize_title_text(&value)
}

fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "unknown"
    }
}

fn capture_permissions() -> CapturePermissions {
    CapturePermissions {
        screen_recording: screen_recording_permission(),
    }
}

#[cfg(target_os = "macos")]
fn screen_recording_permission() -> PermissionState {
    if macos_screen_recording_granted() {
        PermissionState::Granted
    } else {
        PermissionState::Missing
    }
}

#[cfg(not(target_os = "macos"))]
fn screen_recording_permission() -> PermissionState {
    PermissionState::NotRequired
}

#[cfg(target_os = "macos")]
fn idle_seconds() -> Option<f64> {
    const HID_SYSTEM_STATE: u32 = 1;
    const ANY_INPUT_EVENT: u32 = u32::MAX;

    let seconds =
        unsafe { CGEventSourceSecondsSinceLastEventType(HID_SYSTEM_STATE, ANY_INPUT_EVENT) };
    seconds.is_finite().then(|| activity_normalize::round_seconds(seconds))
}

#[cfg(target_os = "windows")]
fn idle_seconds() -> Option<f64> {
    let mut input = LastInputInfo {
        cb_size: std::mem::size_of::<LastInputInfo>() as u32,
        dw_time: 0,
    };

    let ok = unsafe { GetLastInputInfo(&mut input) };
    if ok == 0 {
        return None;
    }

    Some(idle_elapsed_seconds(
        input.dw_time,
        unsafe { GetTickCount() },
    ))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn idle_seconds() -> Option<f64> {
    None
}

#[cfg(target_os = "macos")]
fn macos_screen_recording_granted() -> bool {
    unsafe { CGPreflightScreenCaptureAccess() }
}

#[cfg(target_os = "macos")]
#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGEventSourceSecondsSinceLastEventType(source_state_id: u32, event_type: u32) -> f64;
}

#[cfg(target_os = "windows")]
#[repr(C)]
struct LastInputInfo {
    cb_size: u32,
    dw_time: u32,
}

#[cfg(target_os = "windows")]
#[link(name = "User32")]
unsafe extern "system" {
    fn GetLastInputInfo(input: *mut LastInputInfo) -> i32;
}

#[cfg(target_os = "windows")]
#[link(name = "Kernel32")]
unsafe extern "system" {
    fn GetTickCount() -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use active_win_pos_rs::WindowPosition;
    use std::path::PathBuf;

    #[test]
    fn normalizes_empty_title_to_none() {
        let sample = sample_from_active_window(
            active_window("  "),
            "2026-08-12T00:00:00Z".to_string(),
            CapturePermissions {
                screen_recording: PermissionState::Missing,
            },
            Some(1.24),
        );

        assert_eq!(sample.window_title, None);
        assert_eq!(sample.idle_seconds, Some(1.24));
        assert_eq!(
            sample.permissions.screen_recording,
            PermissionState::Missing
        );
    }

    #[test]
    fn trims_title_and_preserves_bounds() {
        let sample = sample_from_active_window(
            active_window("  Plan.md  "),
            "2026-08-12T00:00:00Z".to_string(),
            CapturePermissions {
                screen_recording: PermissionState::Granted,
            },
            None,
        );

        assert_eq!(sample.window_title.as_deref(), Some("Plan.md"));
        assert_eq!(
            sample.bounds,
            Bounds {
                x: 10.0,
                y: 20.0,
                width: 800.0,
                height: 600.0,
            }
        );
    }

    #[cfg(all(test, target_os = "windows"))]
    #[test]
    #[ignore = "manual: prints foreground samples every 500ms"]
    fn manual_foreground_probe() {
        use std::{thread, time::Duration};

        for _ in 0..20 {
            match current_activity_sample() {
                Ok(sample) => println!("{sample:#?}"),
                Err(error) => println!("ERR: {error}"),
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    fn active_window(title: &str) -> active_win_pos_rs::ActiveWindow {
        active_win_pos_rs::ActiveWindow {
            title: title.to_string(),
            process_path: PathBuf::from("/Applications/Test.app"),
            app_name: "Test".to_string(),
            window_id: "window-1".to_string(),
            process_id: 42,
            position: WindowPosition {
                x: 10.0,
                y: 20.0,
                width: 800.0,
                height: 600.0,
            },
        }
    }
}
