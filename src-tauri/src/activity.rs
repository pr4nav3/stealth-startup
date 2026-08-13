use active_win_pos_rs::{get_active_window, ActiveWindow};
use serde::Serialize;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone, Debug, Serialize, PartialEq)]
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
    let captured_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())?;
    let active_window =
        get_active_window().map_err(|_| "could not read the active window".to_string())?;

    Ok(sample_from_active_window(
        active_window,
        captured_at,
        capture_permissions(),
        idle_seconds(),
    ))
}

fn sample_from_active_window(
    active_window: ActiveWindow,
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
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
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
    seconds.is_finite().then(|| round_seconds(seconds))
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

    let elapsed_ms = unsafe { GetTickCount() }.wrapping_sub(input.dw_time);
    Some(round_seconds(f64::from(elapsed_ms) / 1000.0))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn idle_seconds() -> Option<f64> {
    None
}

fn round_seconds(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
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

    fn active_window(title: &str) -> ActiveWindow {
        ActiveWindow {
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
