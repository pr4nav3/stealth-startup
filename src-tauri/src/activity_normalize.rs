//! Pure normalization and surface classification for activity samples (all platforms).

const ZERO_WIDTH_CHARS: [char; 3] = ['\u{200b}', '\u{200c}', '\u{feff}'];

const BROWSER_EXE_SUFFIXES: &[(&str, &[&str])] = &[
    (
        "chrome",
        &[" - Google Chrome", " — Google Chrome", " - Chromium", " — Chromium"],
    ),
    (
        "msedge",
        &[
            " - Microsoft Edge",
            " — Microsoft Edge",
            " - Microsoft​ Edge",
            " — Microsoft​ Edge",
        ],
    ),
    ("firefox", &[" — Mozilla Firefox", " - Mozilla Firefox"]),
    ("brave", &[" - Brave", " — Brave"]),
    ("opera", &[" - Opera", " — Opera", " - Opera GX", " — Opera GX"]),
    ("arc", &[" - Arc", " — Arc"]),
];

const PLACEHOLDER_TITLES: &[&str] = &[
    "",
    "untitled",
    "new tab",
    "start page",
    "about:blank",
    "chrome://newtab",
    "edge://newtab",
];

/// The window class File Explorer folder windows have used for decades. Every other top-level
/// window owned by explorer.exe is shell UI: desktop, taskbar, Alt-Tab, tray, Run dialog.
const FILE_EXPLORER_WINDOW_CLASS: &str = "CabinetWClass";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForegroundDisposition {
    Sample,
    /// Shell UI (Start, Search, Alt-Tab, taskbar...): the session keeps the previous window.
    ShellSurface,
    NoSample,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawForegroundIdentity {
    pub exe_name: String,
    pub window_class: String,
    pub window_title: String,
    /// The process runs from `<Windows>\SystemApps`, where Windows installs its shell UI
    /// (Start, Search, notification center, touch keyboard, system dialogs).
    pub is_system_app: bool,
}

pub fn classify_foreground_surface(identity: &RawForegroundIdentity) -> ForegroundDisposition {
    let exe = exe_stem_lower(&identity.exe_name);
    let class = identity.window_class.as_str();
    let title = normalize_title_text(&identity.window_title);

    if identity.exe_name.is_empty() && title.is_none() && class.is_empty() {
        return ForegroundDisposition::NoSample;
    }

    if exe == "lockapp" || exe == "logonui" || exe == "credentialuibroker" {
        return ForegroundDisposition::NoSample;
    }

    if identity.is_system_app {
        return ForegroundDisposition::ShellSurface;
    }

    if exe == "explorer" && !class.eq_ignore_ascii_case(FILE_EXPLORER_WINDOW_CLASS) {
        return ForegroundDisposition::ShellSurface;
    }

    if exe == "applicationframehost" && title.is_none() {
        // A Store-app frame whose app could not be resolved and has no title: nothing to credit.
        return ForegroundDisposition::ShellSurface;
    }

    ForegroundDisposition::Sample
}

/// Case-insensitive "is `path` inside `dir`", as Windows paths are.
pub fn is_inside_dir(path: &str, dir: &str) -> bool {
    let dir = dir.trim_end_matches(['\\', '/']);
    path.len() > dir.len()
        && path.is_char_boundary(dir.len())
        && path[..dir.len()].eq_ignore_ascii_case(dir)
        && path[dir.len()..].starts_with(['\\', '/'])
}

pub fn normalize_title_text(raw: &str) -> Option<String> {
    let cleaned = strip_zero_width(raw);
    let stripped = cleaned.trim();
    if stripped.is_empty() {
        return None;
    }
    let lower = stripped.to_ascii_lowercase();
    if PLACEHOLDER_TITLES.iter().any(|placeholder| *placeholder == lower) {
        return None;
    }
    Some(stripped.to_owned())
}

pub fn normalize_title_for_browser(exe_name: &str, raw_title: &str) -> Option<String> {
    let base = normalize_title_text(raw_title)?;
    let without_suffix = strip_browser_suffix_for_exe(exe_name, &base);
    let without_profile = strip_browser_profile_segment(&without_suffix);
    normalize_title_text(&without_profile)
}

pub fn strip_browser_suffix_for_exe(exe_name: &str, title: &str) -> String {
    let exe_key = exe_stem_lower(exe_name);
    let normalized_title = strip_zero_width(title);

    for (browser_key, suffixes) in BROWSER_EXE_SUFFIXES {
        if exe_key != *browser_key && !exe_key.contains(browser_key) {
            continue;
        }
        for suffix in *suffixes {
            let normalized_suffix = strip_zero_width(suffix);
            if let Some(prefix) = normalized_title.strip_suffix(normalized_suffix.as_str()) {
                return prefix.trim_end().to_owned();
            }
        }
        if let Some(stripped) = strip_profile_browser_suffix(&normalized_title, browser_key) {
            return stripped;
        }
    }

    normalized_title.trim().to_owned()
}

fn strip_browser_profile_segment(title: &str) -> String {
    const MARKER: &str = " - Profile ";
    if let Some(idx) = title.rfind(MARKER) {
        return title[..idx].trim_end().to_owned();
    }
    title.to_owned()
}

fn strip_profile_browser_suffix(title: &str, browser_key: &str) -> Option<String> {
    let edge_markers = [
        " - Microsoft Edge",
        " — Microsoft Edge",
        " - Microsoft​ Edge",
        " — Microsoft​ Edge",
    ];
    let chrome_markers = [" - Google Chrome", " — Google Chrome"];

    let markers: &[&str] = if browser_key == "msedge" {
        &edge_markers
    } else if browser_key == "chrome" {
        &chrome_markers
    } else {
        return None;
    };

    for marker in markers {
        let normalized_marker = strip_zero_width(marker);
        if let Some(idx) = title.rfind(normalized_marker.as_str()) {
            let prefix = title[..idx].trim_end();
            if prefix.contains(" - Profile ") || prefix.contains(" — Profile ") {
                return Some(prefix.to_owned());
            }
        }
    }
    None
}

pub fn app_name_from_process_path(path: &str, fallback_class: &str) -> String {
    let trimmed = path.trim();
    if !trimmed.is_empty() {
        if let Some(name) = std::path::Path::new(trimmed)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
        {
            return name.to_owned();
        }
    }

    if !fallback_class.is_empty() {
        return fallback_class.to_owned();
    }

    "unknown-window".to_owned()
}

pub fn idle_elapsed_seconds(last_input_tick: u32, now_tick: u32) -> f64 {
    let elapsed_ms = now_tick.wrapping_sub(last_input_tick);
    round_seconds(f64::from(elapsed_ms) / 1000.0)
}

pub fn round_seconds(seconds: f64) -> f64 {
    (seconds * 10.0).round() / 10.0
}

fn exe_stem_lower(exe_name: &str) -> String {
    let stem = std::path::Path::new(exe_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(exe_name);
    stem.to_ascii_lowercase()
}

fn strip_zero_width(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ZERO_WIDTH_CHARS.contains(ch))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placeholder_titles_become_none() {
        assert_eq!(normalize_title_text("   "), None);
        assert_eq!(normalize_title_text("New Tab"), None);
        assert_eq!(normalize_title_text("about:blank"), None);
    }

    #[test]
    fn strips_chrome_and_edge_suffixes() {
        assert_eq!(
            normalize_title_for_browser("chrome.exe", "Plan.md - Google Chrome"),
            Some("Plan.md".to_string())
        );
        assert_eq!(
            normalize_title_for_browser("msedge.exe", "Docs — Microsoft​ Edge"),
            Some("Docs".to_string())
        );
        assert_eq!(
            normalize_title_for_browser(
                "msedge.exe",
                "Inbox - Profile 1 - Microsoft Edge",
            ),
            Some("Inbox".to_string())
        );
    }

    fn identity(exe: &str, class: &str, title: &str, is_system_app: bool) -> RawForegroundIdentity {
        RawForegroundIdentity {
            exe_name: exe.to_string(),
            window_class: class.to_string(),
            window_title: title.to_string(),
            is_system_app,
        }
    }

    #[test]
    fn explorer_shell_windows_are_shell_surfaces_but_folders_are_not() {
        for class in ["WorkerW", "Shell_TrayWnd", "XamlExplorerHostIslandWindow", "SomeFutureClass"] {
            assert_eq!(
                classify_foreground_surface(&identity("explorer.exe", class, "", false)),
                ForegroundDisposition::ShellSurface,
                "{class}"
            );
        }
        assert_eq!(
            classify_foreground_surface(&identity("Explorer.EXE", "CabinetWClass", "Downloads", false)),
            ForegroundDisposition::Sample
        );
    }

    #[test]
    fn system_apps_are_shell_surfaces_whatever_their_name() {
        for exe in ["SearchHost.exe", "StartMenuExperienceHost.exe", "RenamedIn2030.exe"] {
            assert_eq!(
                classify_foreground_surface(&identity(exe, "Windows.UI.Core.CoreWindow", "", true)),
                ForegroundDisposition::ShellSurface,
                "{exe}"
            );
        }
    }

    #[test]
    fn store_apps_are_not_hidden_by_their_window_class() {
        assert_eq!(
            classify_foreground_surface(&identity("Spotify.exe", "Windows.UI.Core.CoreWindow", "Spotify", false)),
            ForegroundDisposition::Sample
        );
    }

    #[test]
    fn lock_screen_is_no_sample() {
        assert_eq!(
            classify_foreground_surface(&identity("LockApp.exe", "Windows.UI.Core.CoreWindow", "", true)),
            ForegroundDisposition::NoSample
        );
    }

    #[test]
    fn inside_dir_matches_whole_folder_names_only() {
        let apps = r"C:\WINDOWS\SystemApps";
        assert!(is_inside_dir(r"c:\windows\systemapps\Search\SearchHost.exe", apps));
        assert!(is_inside_dir(r"C:\WINDOWS\SystemApps\x.exe", r"C:\Windows\SystemApps\"));
        assert!(!is_inside_dir(r"C:\WINDOWS\SystemAppsX\x.exe", apps));
        assert!(!is_inside_dir(r"C:\WINDOWS\SystemApps", apps));
        assert!(!is_inside_dir(r"C:\Program Files\app.exe", apps));
    }

    #[test]
    fn idle_tick_wraparound_uses_wrapping_sub() {
        assert_eq!(idle_elapsed_seconds(0xffff_0000, 0xffff_bb80), 48.0);
    }

    #[test]
    fn app_name_never_empty_with_class_fallback() {
        assert_eq!(
            app_name_from_process_path("", "TaskManagerWindow"),
            "TaskManagerWindow"
        );
        assert_eq!(
            app_name_from_process_path("", ""),
            "unknown-window"
        );
    }
}
