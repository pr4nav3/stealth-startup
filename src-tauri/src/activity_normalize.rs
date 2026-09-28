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

const NEUTRAL_EXE_NAMES: &[&str] = &[
    "searchhost",
    "startmenuexperiencehost",
    "shellexperiencehost",
    "textinputhost",
];

const NEUTRAL_WINDOW_CLASSES: &[&str] = &[
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
    "XamlExplorerHostIslandWindow",
    "MultitaskingViewFrame",
    "TopLevelWindowForOverflowXamlIsland",
    "Windows.UI.Core.CoreWindow",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForegroundDisposition {
    Sample,
    Neutral,
    NoSample,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawForegroundIdentity {
    pub exe_name: String,
    pub window_class: String,
    pub window_title: String,
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

    if exe == "applicationframehost" {
        // Frame host with only shell chrome and no page title is usually Start/search chrome.
        if title.is_none() {
            return ForegroundDisposition::Neutral;
        }
        return ForegroundDisposition::Sample;
    }

    if is_neutral_window_class(class) {
        return ForegroundDisposition::Neutral;
    }

    if is_neutral_exe_name(&exe) {
        return ForegroundDisposition::Neutral;
    }

    if exe == "explorer" && is_desktop_shell_class(class) {
        return ForegroundDisposition::Neutral;
    }

    ForegroundDisposition::Sample
}

pub fn is_neutral_window_class(class: &str) -> bool {
    NEUTRAL_WINDOW_CLASSES
        .iter()
        .any(|candidate| class.eq_ignore_ascii_case(candidate))
}

pub fn is_neutral_exe_name(exe_lower: &str) -> bool {
    NEUTRAL_EXE_NAMES.iter().any(|candidate| *candidate == exe_lower)
}

pub fn is_desktop_shell_class(class: &str) -> bool {
    class.eq_ignore_ascii_case("Progman") || class.eq_ignore_ascii_case("WorkerW")
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

    #[test]
    fn classifies_shell_surfaces_as_neutral() {
        let desktop = RawForegroundIdentity {
            exe_name: "explorer.exe".to_string(),
            window_class: "WorkerW".to_string(),
            window_title: String::new(),
        };
        assert_eq!(
            classify_foreground_surface(&desktop),
            ForegroundDisposition::Neutral
        );

        let task_view = RawForegroundIdentity {
            exe_name: "explorer.exe".to_string(),
            window_class: "MultitaskingViewFrame".to_string(),
            window_title: String::new(),
        };
        assert_eq!(
            classify_foreground_surface(&task_view),
            ForegroundDisposition::Neutral
        );
    }

    #[test]
    fn lock_screen_is_no_sample() {
        let lock = RawForegroundIdentity {
            exe_name: "LockApp.exe".to_string(),
            window_class: "Windows.UI.Core.CoreWindow".to_string(),
            window_title: String::new(),
        };
        assert_eq!(classify_foreground_surface(&lock), ForegroundDisposition::NoSample);
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
