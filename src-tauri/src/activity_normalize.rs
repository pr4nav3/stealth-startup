//! Pure normalization and surface classification for activity samples (all platforms).

/// Zero-width spaces and bidi marks: macOS app names carry a leading U+200E ("‎WhatsApp").
/// The zero-width joiner stays: it holds emoji sequences together.
const ZERO_WIDTH_CHARS: [char; 5] = ['\u{200b}', '\u{200c}', '\u{200e}', '\u{200f}', '\u{feff}'];

const BROWSER_EXE_SUFFIXES: &[(&str, &[&str])] = &[
    (
        "chrome",
        &[" - Google Chrome", " — Google Chrome", " - Chromium", " — Chromium"],
    ),
    (
        // "msedge" on Windows; the macOS app is "Microsoft Edge".
        "edge",
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

/// "New Tab" stays: attribution recognizes it as a browser page.
const PLACEHOLDER_TITLES: &[&str] = &["", "untitled", "about:blank"];

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
    pub is_system_shell: bool,
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

    // ShellHost is a Windows shell process under System32, outside SystemApps.
    if exe == "shellhost" && identity.is_system_shell {
        return ForegroundDisposition::ShellSurface;
    }

    if identity.is_system_app {
        return ForegroundDisposition::ShellSurface;
    }

    if exe == "explorer" && !class.eq_ignore_ascii_case(FILE_EXPLORER_WINDOW_CLASS) {
        return ForegroundDisposition::ShellSurface;
    }

    if exe == "applicationframehost" {
        // A Store-app frame whose app could not be resolved. Every Store app shares this host,
        // so crediting it would mix them up; the window the user came from keeps the time.
        return ForegroundDisposition::ShellSurface;
    }

    ForegroundDisposition::Sample
}

/// macOS system UI runs from the CoreServices folder. Finder folders stay real windows.
#[cfg_attr(target_os = "windows", allow(dead_code))]
pub fn classify_macos_surface(
    app_path: &str,
    window_title: Option<&str>,
    core_services: &str,
) -> ForegroundDisposition {
    let at = |bundle: &str| {
        let dir = format!("{}/{bundle}", core_services.trim_end_matches('/'));
        app_path.eq_ignore_ascii_case(&dir) || is_inside_dir(app_path, &dir)
    };
    if !is_inside_dir(app_path, core_services) || at("Applications") {
        return ForegroundDisposition::Sample;
    }
    if at("loginwindow.app") {
        return ForegroundDisposition::NoSample;
    }
    if at("Finder.app") && window_title.is_some() {
        return ForegroundDisposition::Sample;
    }
    ForegroundDisposition::ShellSurface
}

/// Case-insensitive "is `path` inside `dir`", as Windows and macOS paths are.
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
            // macOS Chromium browsers add the profile after the name with an en dash:
            // "YouTube - Google Chrome – Alex (Work)".
            if let Some(index) = normalized_title.rfind(normalized_suffix.as_str()) {
                let rest = &normalized_title[index + normalized_suffix.len()..];
                if rest.starts_with(" – ") || rest.starts_with(" - ") {
                    return normalized_title[..index].trim_end().to_owned();
                }
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

    let markers: &[&str] = if browser_key == "edge" {
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

/// The name people know an app by: the exe's own description, with every shell as "Terminal".
pub fn app_label(app_name: &str, description: Option<&str>) -> String {
    const TERMINALS: &[&str] = &[
        "cmd", "powershell", "pwsh", "windowsterminal", "wt", "conhost", "openconsole", "terminal",
    ];
    if TERMINALS.contains(&exe_stem_lower(app_name).as_str()) {
        return "Terminal".to_string();
    }
    description
        .map(str::trim)
        .filter(|description| !description.is_empty())
        .unwrap_or(app_name)
        .to_string()
}

/// What Edge reports for a site that gives no media details. Which tab plays can't be told.
const UNTITLED_BROWSER_MEDIA: &[&str] = &["a site is playing media"];

/// Whether a playing media session belongs to the window in front. `source_app` is the
/// session's app id ("Spotify.exe", "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify", "MSEdge").
/// A browser plays for all its tabs, so it counts when the media title is in the window title,
/// which names the active tab, or when it won't say what plays. Other apps also match by their
/// exe name in the app id.
pub fn media_is_in_front(
    app_name: &str,
    window_title: Option<&str>,
    in_browser: bool,
    source_app: &str,
    media_title: &str,
) -> bool {
    let media_title = media_title.trim().to_lowercase();
    // Very short titles ("A", "Go") would match almost any window title.
    let title_in_front = media_title.chars().count() >= 3
        && window_title.is_some_and(|title| title.to_lowercase().contains(&media_title));
    let app = exe_stem_lower(app_name);
    let same_app = !app.is_empty()
        && source_app
            .split(|ch: char| !ch.is_alphanumeric())
            .any(|part| part.eq_ignore_ascii_case(&app));
    if in_browser {
        return title_in_front || (same_app && UNTITLED_BROWSER_MEDIA.contains(&media_title.as_str()));
    }
    title_in_front || same_app
}

fn exe_stem_lower(exe_name: &str) -> String {
    let stem = std::path::Path::new(exe_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(exe_name);
    stem.to_ascii_lowercase()
}

pub fn strip_zero_width(value: &str) -> String {
    value
        .chars()
        .filter(|ch| !ZERO_WIDTH_CHARS.contains(ch))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_media_counts_only_for_the_tab_in_front() {
        let youtube = Some("Lo-fi beats to study to - YouTube");
        assert!(media_is_in_front("chrome", youtube, true, "Chrome", "Lo-fi beats to study to"));
        // Music in a background tab while reading mail: the browser plays, but not in front.
        assert!(!media_is_in_front("chrome", Some("Inbox - Gmail"), true, "Chrome", "Lo-fi beats to study to"));
        assert!(!media_is_in_front("chrome", youtube, true, "Chrome", ""));
        assert!(!media_is_in_front("chrome", youtube, true, "Chrome", "Lo"));
    }

    #[test]
    fn untitled_browser_media_counts_for_that_browser_in_front() {
        let page = Some("Lecture 4 | Some University");
        assert!(media_is_in_front("msedge", page, true, "MSEdge", "A site is playing media"));
        // Edge playing behind Chrome is not Chrome playing.
        assert!(!media_is_in_front("chrome", page, true, "MSEdge", "A site is playing media"));
    }

    #[test]
    fn app_media_counts_by_its_app_id_or_title() {
        assert!(media_is_in_front("Spotify", Some("Spotify Premium"), false, "Spotify.exe", "Song"));
        assert!(media_is_in_front(
            "Spotify",
            Some("Spotify Premium"),
            false,
            "SpotifyAB.SpotifyMusic_zpdnekdrzrea0!Spotify",
            "Song",
        ));
        assert!(media_is_in_front("vlc", Some("Lecture 4.mp4 - VLC media player"), false, "", "Lecture 4.mp4"));
        // Spotify playing behind an editor is not the editor playing.
        assert!(!media_is_in_front("Code", Some("main.rs - Supervisor"), false, "Spotify.exe", "Song"));
        // Part of an app id is not an app id.
        assert!(!media_is_in_front("App", Some("Main window"), false, "Microsoft.WindowsApps!Player", "Song"));
    }

    #[test]
    fn placeholder_titles_become_none() {
        assert_eq!(normalize_title_text("   "), None);
        assert_eq!(normalize_title_text("about:blank"), None);
        assert_eq!(normalize_title_text("New Tab").as_deref(), Some("New Tab"));
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
    fn strips_macos_browser_names_and_profiles() {
        // Titles as the macOS Accessibility API reports them, with the app names macOS uses.
        assert_eq!(
            normalize_title_for_browser("Google Chrome", "YouTube - Google Chrome – Alex (Work)").as_deref(),
            Some("YouTube")
        );
        assert_eq!(
            normalize_title_for_browser("Google Chrome", "New Tab - Google Chrome – Alex (Work)").as_deref(),
            Some("New Tab")
        );
        assert_eq!(normalize_title_for_browser("Microsoft Edge", "Inbox - Microsoft Edge").as_deref(), Some("Inbox"));
        assert_eq!(normalize_title_for_browser("Brave Browser", "Rust - Brave").as_deref(), Some("Rust"));
        assert_eq!(normalize_title_for_browser("Firefox", "Docs — Mozilla Firefox").as_deref(), Some("Docs"));
        // Safari puts nothing after the page title.
        assert_eq!(normalize_title_for_browser("Safari", "Start Page").as_deref(), Some("Start Page"));
        // A page whose own title mentions the browser keeps it.
        assert_eq!(
            normalize_title_for_browser("Google Chrome", "Why Google Chrome is slow - Google Chrome").as_deref(),
            Some("Why Google Chrome is slow")
        );
    }

    fn identity(exe: &str, class: &str, title: &str, is_system_app: bool) -> RawForegroundIdentity {
        RawForegroundIdentity {
            exe_name: exe.to_string(),
            window_class: class.to_string(),
            window_title: title.to_string(),
            is_system_app,
            is_system_shell: exe.eq_ignore_ascii_case("ShellHost.exe"),
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
        assert_eq!(
            classify_foreground_surface(&identity("ShellHost.exe", "ShellHost", "", false)),
            ForegroundDisposition::ShellSurface
        );
        let mut unrelated = identity("ShellHost.exe", "AppWindow", "Work", false);
        unrelated.is_system_shell = false;
        assert_eq!(classify_foreground_surface(&unrelated), ForegroundDisposition::Sample);
    }

    #[test]
    fn store_apps_are_not_hidden_by_their_window_class() {
        assert_eq!(
            classify_foreground_surface(&identity("Spotify.exe", "Windows.UI.Core.CoreWindow", "Spotify", false)),
            ForegroundDisposition::Sample
        );
    }

    #[test]
    fn an_unresolved_store_app_frame_is_never_credited() {
        for title in ["", "Calculator", "Settings"] {
            assert_eq!(
                classify_foreground_surface(&identity("ApplicationFrameHost.exe", "ApplicationFrameWindow", title, false)),
                ForegroundDisposition::ShellSurface,
                "{title}"
            );
        }
    }

    #[test]
    fn lock_screen_is_no_sample() {
        assert_eq!(
            classify_foreground_surface(&identity("LockApp.exe", "Windows.UI.Core.CoreWindow", "", true)),
            ForegroundDisposition::NoSample
        );
    }

    #[test]
    fn macos_system_ui_is_a_shell_surface_but_apps_and_finder_folders_are_not() {
        let core = "/System/Library/CoreServices";
        let classify = |path: &str, title: Option<&str>| classify_macos_surface(path, title, core);
        assert_eq!(
            classify("/System/Library/CoreServices/Dock.app", None),
            ForegroundDisposition::ShellSurface
        );
        assert_eq!(
            classify("/System/Library/CoreServices/Finder.app", Some("Downloads")),
            ForegroundDisposition::Sample
        );
        assert_eq!(
            classify("/System/Library/CoreServices/loginwindow.app", None),
            ForegroundDisposition::NoSample
        );
        assert_eq!(
            classify("/Applications/Safari.app", Some("Docs")),
            ForegroundDisposition::Sample
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

    #[test]
    fn app_label_prefers_the_description_and_names_shells_terminal() {
        assert_eq!(app_label("Code", Some("Visual Studio Code")), "Visual Studio Code");
        assert_eq!(app_label("cmd", Some("Windows Command Processor")), "Terminal");
        assert_eq!(app_label("Thorium", Some("  ")), "Thorium");
    }
}
