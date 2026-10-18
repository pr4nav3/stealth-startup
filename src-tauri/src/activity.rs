#[path = "activity_normalize.rs"]
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
mod activity_normalize;
#[cfg(target_os = "windows")]
#[path = "activity_win32.rs"]
mod activity_win32;
// Each platform reads the browser address its own way behind the same `read_browser` shape.
#[cfg(target_os = "macos")]
#[path = "browser_macos.rs"]
mod browser_macos;
#[cfg(target_os = "windows")]
#[path = "browser_win32.rs"]
mod browser_win32;

#[cfg(target_os = "windows")]
use activity_normalize::idle_elapsed_seconds;
#[cfg(not(target_os = "windows"))]
use activity_normalize::normalize_title_text;

use serde::Serialize;
use std::collections::HashMap;
#[cfg(not(target_os = "windows"))]
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActivitySample {
    pub captured_at: String,
    pub platform: &'static str,
    pub app_name: String,
    pub app_label: String,
    pub process_path: String,
    pub process_id: u64,
    /// Read for attribution only; never sent to the UI or stored.
    #[serde(skip)]
    pub window_title: Option<String>,
    pub window_id: String,
    pub bounds: Bounds,
    pub idle_seconds: Option<f64>,
    pub permissions: CapturePermissions,
    /// OS shell UI (Start, Search, Alt-Tab, taskbar...). Sessions keep the previous window.
    pub shell_surface: bool,
    /// Set only when a browser is in front.
    #[serde(skip)]
    pub browser: Option<BrowserRead>,
    /// The app in front is playing media, so a user who isn't typing may be watching or
    /// listening. Looked up only once input has stopped; `false` until then.
    #[serde(skip)]
    pub media_playing: bool,
}

/// What the browser's address bar said. `address: None` means it could not be read.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BrowserRead {
    pub address: Option<String>,
    /// The address bar has keyboard focus, so its text may be typing, not the page.
    pub editing: bool,
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
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Missing,
    #[allow(dead_code)]
    NotRequired,
}

pub fn current_activity_sample() -> Result<ActivitySample, String> {
    let mut sample = capture_foreground(capture_permissions(), idle_seconds())?;
    let input_stopped = sample
        .idle_seconds
        .is_some_and(|seconds| seconds >= crate::focus::AWAY_AFTER_SECONDS as f64);
    if input_stopped && !sample.shell_surface {
        sample.media_playing = media_playing_in_front(&sample);
    }
    Ok(sample)
}

fn capture_foreground(
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
) -> Result<ActivitySample, String> {
    #[cfg(target_os = "windows")]
    {
        activity_win32::capture_current(permissions, idle_seconds)
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
            if crate::website::is_browser(&sample.app_name.to_lowercase()) {
                sample.browser = Some(browser_macos::read_browser(sample.process_id));
            }
        }

        Ok(sample)
    }
}

/// Windows: the system media controls. macOS: a "keep the display awake" power assertion,
/// which browsers hold only while video plays on screen and call apps hold during calls.
#[cfg(target_os = "windows")]
fn media_playing_in_front(sample: &ActivitySample) -> bool {
    activity_win32::media_playing_in_front(sample)
}

#[cfg(target_os = "macos")]
fn media_playing_in_front(sample: &ActivitySample) -> bool {
    crate::macos::keeps_display_awake(sample.process_id)
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
fn media_playing_in_front(_sample: &ActivitySample) -> bool {
    false
}

/// Resolve old app keys against processes that Windows can still identify.
pub fn running_app_labels(keys: &[String]) -> HashMap<String, String> {
    #[cfg(target_os = "windows")]
    {
        activity_win32::running_app_labels(keys)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = keys;
        HashMap::new()
    }
}

#[cfg(not(target_os = "windows"))]
fn sample_from_active_window(
    active_window: active_win_pos_rs::ActiveWindow,
    captured_at: String,
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
) -> ActivitySample {
    ActivitySample {
        captured_at,
        platform: platform(),
        app_label: activity_normalize::app_label(&active_window.app_name, None),
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
        browser: None,
        media_playing: false,
    }
}

#[cfg(not(target_os = "windows"))]
fn non_empty(value: String) -> Option<String> {
    normalize_title_text(&value)
}

#[cfg(not(target_os = "windows"))]
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
    #[allow(unused_imports)]
    use super::*;

    #[cfg(not(target_os = "windows"))]
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

    #[cfg(not(target_os = "windows"))]
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

    /// Switch between browsers, tabs, and apps while it runs:
    /// `cargo test manual_foreground_probe -- --ignored --nocapture`
    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual: prints what each sample resolves to, every 500ms for 30s"]
    fn manual_foreground_probe() {
        use std::time::{Duration, Instant};

        let started = Instant::now();
        let mut resolver = crate::attribution::Resolver::default();
        let samples = std::env::var("PROBE_SAMPLES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(60);
        for _ in 0..samples {
            let before = Instant::now();
            match current_activity_sample() {
                Ok(sample) => {
                    let took = before.elapsed();
                    let activity = resolver.resolve(&sample, started.elapsed());
                    println!(
                        "{:>5}ms  {:<24} {:?} -> {:?} {} [{:?}]",
                        took.as_millis(),
                        sample.app_label,
                        sample.browser,
                        activity.kind,
                        activity.label,
                        activity.source,
                    );
                }
                Err(error) => println!("ERR: {error}"),
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    #[cfg(not(target_os = "windows"))]
    fn active_window(title: &str) -> active_win_pos_rs::ActiveWindow {
        active_win_pos_rs::ActiveWindow {
            title: title.to_string(),
            process_path: std::path::PathBuf::from("/Applications/Test.app"),
            app_name: "Test".to_string(),
            window_id: "window-1".to_string(),
            process_id: 42,
            position: active_win_pos_rs::WindowPosition {
                x: 10.0,
                y: 20.0,
                width: 800.0,
                height: 600.0,
            },
        }
    }
}
