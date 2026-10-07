#[path = "activity_normalize.rs"]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod activity_normalize;
#[cfg(target_os = "windows")]
#[path = "activity_win32.rs"]
mod activity_win32;

#[cfg(target_os = "windows")]
use activity_normalize::idle_elapsed_seconds;
use activity_normalize::normalize_title_text;

pub use activity_normalize::mark_subject;
use serde::Serialize;
#[cfg(not(target_os = "windows"))]
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone, Debug, PartialEq, Serialize)]
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
    /// OS shell UI (Start, Search, Alt-Tab, taskbar...). Sessions keep the previous window.
    pub shell_surface: bool,
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

        #[allow(unused_mut)]
        let mut sample =
            sample_from_active_window(active_window, captured_at, permissions, idle_seconds);

        #[cfg(target_os = "macos")]
        {
            use activity_normalize::{classify_macos_surface, ForegroundDisposition};

            let core_services = crate::macos::core_services_dir()
                .ok_or("macOS did not report its CoreServices folder")?;
            match classify_macos_surface(
                &sample.process_path,
                sample.window_title.as_deref(),
                core_services,
            ) {
                ForegroundDisposition::NoSample => {
                    return Err("the login window is in front".to_string());
                }
                ForegroundDisposition::ShellSurface => sample.shell_surface = true,
                ForegroundDisposition::Sample => {}
            }
        }

        Ok(sample)
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
        shell_surface: false,
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
    crate::macos::idle_seconds().map(activity_normalize::round_seconds)
}

#[cfg(target_os = "windows")]
fn idle_seconds() -> Option<f64> {
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut input = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if !unsafe { GetLastInputInfo(&mut input) }.as_bool() {
        return None;
    }

    Some(idle_elapsed_seconds(input.dwTime, unsafe { GetTickCount() }))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn idle_seconds() -> Option<f64> {
    None
}

#[cfg(target_os = "macos")]
fn macos_screen_recording_granted() -> bool {
    crate::macos::screen_recording_granted()
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
