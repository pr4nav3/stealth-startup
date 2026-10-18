//! Turns one foreground sample into the thing being supervised: an app, a website, or one
//! of the browser rows. A confirmed address always wins; titles are a narrow fallback.

use crate::activity::ActivitySample;
use crate::website::{self, Address};
use serde::Serialize;
use std::time::Duration;

/// How long a confirmed site survives an unreadable or edited address in the same tab.
const GRACE: Duration = Duration::from_millis(1500);
const OTHER_WEBSITES_KEY: &str = "other-websites";
pub const PAUSED_KEY: &str = "paused";
pub const UNKNOWN_KEY: &str = "untracked";
pub const UNKNOWN_LABEL: &str = "Device asleep / unknown";
pub const SUPERVISOR_KEY: &str = "supervisor";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    /// An app. A browser's own pages (New Tab, settings, history, files, extensions) are the
    /// browser app itself, so they share its row, its time, and its rules.
    App,
    Website,
    OtherWebsites,
}

/// Why the activity was chosen. Stored with spans to measure how often fallbacks run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    App,
    Address,
    Grace,
    Title,
    None,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Activity {
    pub kind: ActivityKind,
    pub key: String,
    pub label: String,
    #[serde(skip)]
    pub app_id: String,
    #[serde(skip)]
    pub app_label: String,
    #[serde(skip)]
    pub source: Source,
}

impl Activity {
    pub fn app(name: &str) -> Self {
        Self {
            kind: ActivityKind::App,
            key: name.to_lowercase(),
            label: name.to_string(),
            app_id: name.to_lowercase(),
            app_label: name.to_string(),
            source: Source::App,
        }
    }

    #[cfg(test)]
    pub fn website(browser: &str, key: &str) -> Self {
        Self::in_browser(ActivityKind::Website, key, browser, browser, Source::Address)
    }

    /// Rebuilds the parts rules and categories need from a stored span.
    pub fn from_stored(kind: &str, key: &str, label: &str, app_id: &str) -> Self {
        let kind = ActivityKind::from_key(kind);
        Self {
            kind,
            key: key.to_string(),
            label: label.to_string(),
            app_id: app_id.to_lowercase(),
            app_label: if kind == ActivityKind::App { label } else { app_id }.to_string(),
            source: Source::None,
        }
    }

    /// Supervisor's own windows: recorded as Neutral (or Away), never judged by rules.
    pub fn supervisor() -> Self {
        Self::app("Supervisor")
    }

    /// The session was on hold. Fills the timeline; never counted toward any score.
    pub fn paused() -> Self {
        Self::marker(PAUSED_KEY, "Paused")
    }

    /// The machine slept, froze, or would not report a window.
    pub fn unknown() -> Self {
        Self::marker(UNKNOWN_KEY, UNKNOWN_LABEL)
    }

    fn marker(key: &str, label: &str) -> Self {
        Self {
            kind: ActivityKind::App,
            key: key.to_string(),
            label: label.to_string(),
            app_id: key.to_string(),
            app_label: label.to_string(),
            source: Source::None,
        }
    }

    pub fn same_place(&self, other: &Activity) -> bool {
        self.kind == other.kind && self.key == other.key
    }

    fn in_browser(kind: ActivityKind, key: &str, app_id: &str, app_label: &str, source: Source) -> Self {
        let label = match kind {
            ActivityKind::Website => website::site_label(key),
            ActivityKind::OtherWebsites => "Other websites".to_string(),
            ActivityKind::App => app_label.to_string(),
        };
        let key = match kind {
            ActivityKind::Website => key,
            ActivityKind::OtherWebsites => OTHER_WEBSITES_KEY,
            _ => app_id,
        };
        Self {
            kind,
            key: key.to_string(),
            label,
            app_id: app_id.to_string(),
            app_label: app_label.to_string(),
            source,
        }
    }
}

impl ActivityKind {
    pub fn key(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Website => "website",
            Self::OtherWebsites => "other_websites",
        }
    }

    pub fn from_key(value: &str) -> Self {
        match value {
            "website" => Self::Website,
            "other_websites" => Self::OtherWebsites,
            _ => Self::App,
        }
    }
}

impl Source {
    pub fn key(self) -> &'static str {
        match self {
            Self::App => "app",
            Self::Address => "address",
            Self::Grace => "grace",
            Self::Title => "title",
            Self::None => "none",
        }
    }
}

/// The app's history key once assigned (see `app_identity`), else its program name.
pub fn app_id(sample: &ActivitySample) -> String {
    sample.app_key.clone().unwrap_or_else(|| sample.app_name.to_lowercase())
}

#[derive(Debug)]
struct Confirmed {
    activity: Activity,
    window_id: String,
    title: Option<String>,
    at: Duration,
}

/// Remembers just enough between samples for grace and the title-stability check.
#[derive(Debug, Default)]
pub struct Resolver {
    confirmed: Option<Confirmed>,
    last_window: Option<(String, Option<String>)>,
}

impl Resolver {
    pub fn resolve(&mut self, sample: &ActivitySample, now: Duration) -> Activity {
        let app_id = app_id(sample);
        let title = sample.window_title.as_deref();
        let window = (sample.window_id.clone(), sample.window_title.clone());
        let title_held = self.last_window.as_ref() == Some(&window);
        self.last_window = Some(window);

        if !website::is_browser(&app_id) {
            return own_app(sample, &app_id, Source::App);
        }

        let read = sample.browser.clone().unwrap_or_default();
        let in_browser = |kind, key: &str, source| {
            Activity::in_browser(kind, key, &app_id, &sample.app_label, source)
        };

        let address = read.address.as_deref().map(str::trim);
        // An empty or unread bar is a New Tab only when the title says so too. It counts even
        // while the cursor waits in the bar, since nothing is typed yet.
        let new_tab = matches!(address, None | Some("")) && title.is_some_and(website::is_new_tab_title);
        if !read.editing || new_tab {
            let committed = match address {
                _ if new_tab => {
                    let source = if address.is_some() { Source::Address } else { Source::Title };
                    Some(own_app(sample, &app_id, source))
                }
                Some(address) if !address.is_empty() => match website::classify_address(address) {
                    Address::Website(key) => Some(in_browser(ActivityKind::Website, &key, Source::Address)),
                    Address::BrowserPage | Address::Local => Some(own_app(sample, &app_id, Source::Address)),
                    Address::NotAddress => None,
                },
                _ => None,
            };
            if let Some(activity) = committed {
                self.confirmed = Some(Confirmed {
                    activity: activity.clone(),
                    window_id: sample.window_id.clone(),
                    title: sample.window_title.clone(),
                    at: now,
                });
                return activity;
            }
        }

        // A different window or a new title means a different tab or page: no grace.
        if let Some(confirmed) = &self.confirmed {
            if confirmed.window_id == sample.window_id
                && confirmed.title == sample.window_title
                && now.saturating_sub(confirmed.at) <= GRACE
            {
                let mut activity = confirmed.activity.clone();
                activity.source = Source::Grace;
                return activity;
            }
        }

        // Titles flicker while a page loads; trust one only once it held for a sample.
        if !read.editing && title_held {
            if let Some(key) = title.and_then(website::site_from_title) {
                return in_browser(ActivityKind::Website, key, Source::Title);
            }
        }
        in_browser(ActivityKind::OtherWebsites, "", Source::None)
    }
}

/// The app in front as itself, under its app id.
fn own_app(sample: &ActivitySample, app_id: &str, source: Source) -> Activity {
    let mut activity = Activity::app(&sample.app_label);
    activity.key = app_id.to_string();
    activity.app_id = app_id.to_string();
    activity.source = source;
    activity
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{BrowserRead, Bounds, CapturePermissions, PermissionState};

    fn sample(app: &str, window: &str, title: &str, address: Option<&str>, editing: bool) -> ActivitySample {
        ActivitySample {
            captured_at: "2026-10-15T12:00:00Z".to_string(),
            platform: "windows",
            app_name: app.to_string(),
            app_label: if app == "chrome" { "Google Chrome" } else { app }.to_string(),
            process_path: format!("C:\\Apps\\{app}.exe"),
            app_identity: String::new(),
            app_key: None,
            process_id: 1,
            window_title: (!title.is_empty()).then(|| title.to_string()),
            window_id: window.to_string(),
            bounds: Bounds { x: 0.0, y: 0.0, width: 800.0, height: 600.0 },
            idle_seconds: Some(0.0),
            permissions: CapturePermissions { screen_recording: PermissionState::NotRequired },
            shell_surface: false,
            browser: Some(BrowserRead {
                address: address.map(str::to_string),
                editing,
            }),
            media_playing: false,
        }
    }

    fn chrome(title: &str, address: Option<&str>) -> ActivitySample {
        sample("chrome", "1", title, address, false)
    }

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn address_wins_over_title() {
        let mut resolver = Resolver::default();
        let github = resolver.resolve(&chrome("Issue #42 - GitHub", Some("github.com/openai/codex/issues/42")), at(0));
        assert_eq!((github.kind, github.key.as_str(), github.label.as_str()), (ActivityKind::Website, "github.com", "GitHub"));
        assert_eq!(github.source, Source::Address);

        let example = resolver.resolve(&chrome("Video - YouTube", Some("example.com/watch")), at(1000));
        assert_eq!(example.key, "example.com");
    }

    #[test]
    fn page_titles_never_split_or_merge_sites() {
        let mut resolver = Resolver::default();
        let keys: Vec<String> = ["Pulls", "Issues", "Actions", "Settings"]
            .iter()
            .enumerate()
            .map(|(i, title)| resolver.resolve(&chrome(title, Some("https://github.com/x")), at(i as u64 * 1000)).key)
            .collect();
        assert!(keys.iter().all(|key| key == "github.com"));

        let a = resolver.resolve(&chrome("Dashboard", Some("vercel.com/dashboard")), at(5000));
        let b = resolver.resolve(&chrome("Dashboard", Some("supabase.com/dashboard")), at(6000));
        assert!(!a.same_place(&b));
    }

    #[test]
    fn browser_pages_are_the_browser_itself() {
        let mut resolver = Resolver::default();
        let history = resolver.resolve(&chrome("History", Some("chrome://history")), at(0));
        let new_tab = resolver.resolve(&chrome("New Tab", Some("")), at(1000));
        assert_eq!((history.kind, history.key.as_str()), (ActivityKind::App, "chrome"));
        assert!(history.same_place(&new_tab));
        assert_eq!(history.label, "Google Chrome");
    }

    #[test]
    fn a_new_tab_waiting_for_an_address_is_the_browser() {
        let mut resolver = Resolver::default();
        let waiting = resolver.resolve(&sample("chrome", "1", "New Tab", Some(""), true), at(0));
        assert_eq!((waiting.kind, waiting.key.as_str()), (ActivityKind::App, "chrome"));
        // Without the address (macOS until its reader exists), the title alone says so.
        let unread = resolver.resolve(&chrome("New Tab", None), at(1000));
        assert_eq!((unread.kind, unread.source), (ActivityKind::App, Source::Title));
    }

    #[test]
    fn an_empty_address_alone_is_not_a_new_tab() {
        let mut resolver = Resolver::default();
        let page = resolver.resolve(&chrome("Untitled page", Some("")), at(0));
        assert_eq!(page.kind, ActivityKind::OtherWebsites);
    }

    #[test]
    fn local_files_stay_under_the_browser() {
        let mut resolver = Resolver::default();
        let file = resolver.resolve(&chrome("notes.html", Some("file:///C:/notes.html")), at(0));
        assert_eq!((file.kind, file.key.as_str()), (ActivityKind::App, "chrome"));
    }

    #[test]
    fn editing_keeps_the_same_tab_briefly_then_expires() {
        let mut resolver = Resolver::default();
        resolver.resolve(&chrome("Issue - GitHub", Some("github.com/a")), at(0));
        let typing = sample("chrome", "1", "Issue - GitHub", Some("how to"), true);
        let held = resolver.resolve(&typing, at(1000));
        assert_eq!((held.key.as_str(), held.source), ("github.com", Source::Grace));
        let expired = resolver.resolve(&typing, at(2600));
        assert_eq!(expired.kind, ActivityKind::OtherWebsites);
    }

    #[test]
    fn grace_never_crosses_a_tab_or_window_switch() {
        let mut resolver = Resolver::default();
        resolver.resolve(&chrome("Issue - GitHub", Some("github.com/a")), at(0));
        let new_tab = resolver.resolve(&chrome("Some article", None), at(500));
        assert_eq!(new_tab.kind, ActivityKind::OtherWebsites);

        resolver.resolve(&chrome("Issue - GitHub", Some("github.com/a")), at(1000));
        let other_window = resolver.resolve(&sample("chrome", "2", "Issue - GitHub", None, false), at(1200));
        assert_ne!(other_window.source, Source::Grace);
    }

    #[test]
    fn unreadable_address_uses_a_held_verified_title_or_other_websites() {
        let mut resolver = Resolver::default();
        let first = resolver.resolve(&chrome("Issue #42 - GitHub", None), at(0));
        assert_eq!(first.kind, ActivityKind::OtherWebsites);
        let held = resolver.resolve(&chrome("Issue #42 - GitHub", None), at(1000));
        assert_eq!((held.key.as_str(), held.source), ("github.com", Source::Title));

        resolver.resolve(&chrome("Article - Something", None), at(2000));
        let unknown = resolver.resolve(&chrome("Article - Something", None), at(3000));
        assert_eq!(unknown.kind, ActivityKind::OtherWebsites);
    }

    #[test]
    fn a_long_read_failure_settles_on_other_websites() {
        let mut resolver = Resolver::default();
        resolver.resolve(&chrome("Feed", Some("reddit.com")), at(0));
        let late = (1..10)
            .map(|second| resolver.resolve(&chrome("Feed", None), at(second * 1000)))
            .last()
            .expect("samples");
        assert_eq!(late.kind, ActivityKind::OtherWebsites);
    }

    #[test]
    fn the_assigned_app_key_names_the_app_and_its_browser_pages() {
        let mut resolver = Resolver::default();
        let mut canary = chrome("History", Some("chrome://history"));
        canary.app_key = Some("chrome#2".to_string());
        let pages = resolver.resolve(&canary, at(0));
        assert_eq!((pages.kind, pages.key.as_str()), (ActivityKind::App, "chrome#2"));
        canary.browser = Some(BrowserRead { address: Some("github.com/x".into()), editing: false });
        let site = resolver.resolve(&canary, at(1000));
        assert_eq!((site.kind, site.app_id.as_str()), (ActivityKind::Website, "chrome#2"));

        let mut tool = sample("tool", "9", "Main", None, false);
        tool.browser = None;
        tool.app_key = Some("tool#2".to_string());
        assert_eq!(resolver.resolve(&tool, at(2000)).key, "tool#2");
    }

    #[test]
    fn native_apps_use_the_process_identity() {
        let mut resolver = Resolver::default();
        let mut code = sample("Code", "9", "main.rs - Supervisor", None, false);
        code.app_label = "Visual Studio Code".to_string();
        code.browser = None;
        let activity = resolver.resolve(&code, at(0));
        assert_eq!((activity.kind, activity.key.as_str()), (ActivityKind::App, "code"));
        assert_eq!(activity.label, "Visual Studio Code");
    }
}
