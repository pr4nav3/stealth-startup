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
    /// Native identity (see `app_identity`): a packaged app's AUMID, a bundle id, or the
    /// program's maker and name. Kept apart from the label, which can change.
    #[serde(skip)]
    pub app_identity: String,
    /// The history key `app_identity` was given. `None` until assigned; the stem stands in.
    #[serde(skip)]
    pub app_key: Option<String>,
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
    /// macOS: window titles and browser addresses are read through Accessibility.
    pub accessibility: PermissionState,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionState {
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Granted,
    #[cfg_attr(target_os = "windows", allow(dead_code))]
    Missing,
    #[allow(dead_code)]
    NotRequired,
}

pub fn current_activity_sample() -> Result<ActivitySample, String> {
    // The capture calls Cocoa every second from a plain thread; without a pool the objects it
    // autoreleases are only freed when the thread exits, so memory would grow all session.
    #[cfg(target_os = "macos")]
    return objc2::rc::autoreleasepool(|_| capture_foreground(capture_permissions(), idle_seconds()));
    #[cfg(not(target_os = "macos"))]
    capture_foreground(capture_permissions(), idle_seconds())
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
        #[cfg(target_os = "macos")]
        if crate::macos::screen_locked() {
            return Err("the screen is locked".to_string());
        }

        let active_window = match get_active_window() {
            Ok(window) => window,
            // macOS keeps an app in front with no window on screen: its last window closed,
            // everything minimized, or the desktop clicked. Like the Windows desktop, that is
            // shell UI: the window the user came from keeps the time.
            #[cfg(target_os = "macos")]
            Err(_) => match crate::macos::frontmost_app() {
                Some((process_id, name, process_path)) => active_win_pos_rs::ActiveWindow {
                    title: String::new(),
                    process_path,
                    app_name: name,
                    window_id: format!("app-{process_id}"),
                    process_id,
                    position: active_win_pos_rs::WindowPosition { x: 0.0, y: 0.0, width: 0.0, height: 0.0 },
                },
                None => return Err("could not read the active window".to_string()),
            },
            #[cfg(not(target_os = "macos"))]
            Err(_) => return Err("could not read the active window".to_string()),
        };
        #[cfg(target_os = "macos")]
        let windowless = active_window.window_id.starts_with("app-");

        #[allow(unused_mut)]
        let mut sample =
            sample_from_active_window(active_window, captured_at, permissions, idle_seconds);

        #[cfg(target_os = "macos")]
        {
            use activity_normalize::{classify_macos_surface, ForegroundDisposition};

            if windowless {
                sample.shell_surface = true;
                return Ok(sample);
            }
            // Without Screen Recording the window list gives no titles; Accessibility does.
            // Before classifying: a Finder folder is told from the desktop by its title.
            if sample.window_title.is_none() {
                sample.window_title = browser_macos::focused_window_title(sample.process_id)
                    .and_then(|title| clean_title(&sample.app_name, &title));
            }
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
            if !sample.shell_surface && crate::website::is_browser(&sample.app_name.to_lowercase()) {
                sample.browser = Some(browser_macos::read_browser(sample.process_id));
            }
        }

        Ok(sample)
    }
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
    let app_name = activity_normalize::strip_zero_width(&active_window.app_name).trim().to_string();
    let window_title = clean_title(&app_name, &active_window.title);
    ActivitySample {
        captured_at,
        platform: platform(),
        app_label: activity_normalize::app_label(
            &app_name,
            bundle_display_name(&active_window.process_path).as_deref(),
        ),
        app_identity: app_identity(&active_window.process_path.to_string_lossy(), &app_name),
        app_key: None,
        app_name,
        process_path: active_window.process_path.to_string_lossy().into_owned(),
        process_id: active_window.process_id,
        window_title,
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
    }
}

/// What the Finder and the Dock call an app: its bundle's file name ("Visual Studio Code" for
/// `Visual Studio Code.app`). The window list names the process instead, which for many apps
/// is an internal name ("Code", "Electron"). Not a bundle: `None`, and the process name stands.
#[cfg(not(target_os = "windows"))]
fn bundle_display_name(path: &std::path::Path) -> Option<String> {
    if !path.extension()?.to_str()?.eq_ignore_ascii_case("app") {
        return None;
    }
    let name = activity_normalize::strip_zero_width(path.file_stem()?.to_str()?);
    Some(name.trim().to_string()).filter(|name| !name.is_empty())
}

/// macOS: the bundle identifier, so a renamed app stays one app. Elsewhere: the program path.
#[cfg(not(target_os = "windows"))]
fn app_identity(process_path: &str, app_name: &str) -> String {
    #[cfg(target_os = "macos")]
    if let Some(id) = crate::macos::bundle_identifier(process_path) {
        return crate::app_identity::macos_identity(Some(&id), process_path);
    }
    crate::app_identity::fallback_identity(process_path, app_name)
}

/// The title as attribution reads it: blank and placeholder titles are `None`, and a
/// browser's name and profile come off the end ("YouTube - Google Chrome – Alex" is
/// "YouTube"), so title fallbacks and the New Tab check see the page's own title.
#[cfg(not(target_os = "windows"))]
fn clean_title(app_name: &str, raw: &str) -> Option<String> {
    activity_normalize::normalize_title_for_browser(app_name, raw).or_else(|| normalize_title_text(raw))
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

/// The Mac twin of `manual_foreground_probe`, run from the bundled app so it has the app's
/// permissions: `SUPERVISOR_PROBE=/tmp/probe.txt open -a Supervisor.app`, in a debug build.
/// Switch between browsers, tabs, and apps while it runs.
#[cfg(all(debug_assertions, target_os = "macos"))]
pub fn probe_to_file(path: &str) {
    use std::io::Write;
    use std::time::{Duration, Instant};

    let Ok(mut file) = std::fs::File::create(path) else {
        return;
    };
    let _ = writeln!(
        file,
        "accessibility={} screen_recording={}",
        browser_macos::accessibility_granted(),
        crate::macos::screen_recording_granted()
    );
    let started = Instant::now();
    let mut resolver = crate::attribution::Resolver::default();
    let mut described = std::collections::HashSet::new();
    for _ in 0..40 {
        let before = Instant::now();
        match current_activity_sample() {
            Ok(sample) => {
                let activity = resolver.resolve(&sample, started.elapsed());
                let _ = writeln!(
                    file,
                    "{:>5}ms  {:<20} {:<40} title={:?} browser={:?} -> {:?} {} [{:?}]",
                    before.elapsed().as_millis(),
                    sample.app_label,
                    sample.process_path,
                    sample.window_title,
                    sample.browser,
                    activity.kind,
                    activity.label,
                    activity.source,
                );
                if sample.browser.as_ref().is_some_and(|read| read.address.is_none())
                    && described.insert(sample.process_id)
                {
                    let _ = writeln!(file, "{}", browser_macos::describe_focused_window(sample.process_id, 4));
                }
            }
            Err(error) => {
                let _ = writeln!(file, "ERR: {error}");
            }
        }
        let _ = file.flush();
        std::thread::sleep(Duration::from_millis(500));
    }
}

pub fn capture_permissions() -> CapturePermissions {
    CapturePermissions {
        screen_recording: screen_recording_permission(),
        accessibility: accessibility_permission(),
    }
}

#[cfg(target_os = "macos")]
fn accessibility_permission() -> PermissionState {
    if browser_macos::request_accessibility() {
        PermissionState::Granted
    } else {
        PermissionState::Missing
    }
}

#[cfg(not(target_os = "macos"))]
fn accessibility_permission() -> PermissionState {
    PermissionState::NotRequired
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
                accessibility: PermissionState::NotRequired,
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
    fn apps_are_named_by_their_bundle_not_their_process() {
        let name = |path: &str| bundle_display_name(std::path::Path::new(path));
        assert_eq!(name("/Applications/Visual Studio Code.app").as_deref(), Some("Visual Studio Code"));
        assert_eq!(name("/Applications/\u{200e}WhatsApp.app").as_deref(), Some("WhatsApp"));
        assert_eq!(name("/usr/local/bin/tool"), None);
        let mut window = active_window("main.rs");
        window.app_name = "Code".to_string();
        window.process_path = std::path::PathBuf::from("/Applications/Visual Studio Code.app");
        let sample = sample_from_active_window(
            window,
            "2026-10-25T00:00:00Z".to_string(),
            CapturePermissions { screen_recording: PermissionState::Granted, accessibility: PermissionState::Granted },
            None,
        );
        assert_eq!(sample.app_label, "Visual Studio Code");
        // The key stays the process name, so history and rules keyed on it carry on.
        assert_eq!(sample.app_name, "Code");
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn trims_title_and_preserves_bounds() {
        let sample = sample_from_active_window(
            active_window("  Plan.md  "),
            "2026-08-12T00:00:00Z".to_string(),
            CapturePermissions {
                screen_recording: PermissionState::Granted,
                accessibility: PermissionState::NotRequired,
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
