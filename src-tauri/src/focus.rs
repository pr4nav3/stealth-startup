use crate::activity::ActivitySample;
use crate::attribution::{Activity, ActivityKind};
use crate::website;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// No keyboard or mouse input for this long means the user is away. Shorter pauses (reading,
/// thinking, writing on paper) stay with the window in front. A real break is dated back to
/// the last input once noticed, so waiting this long costs no accuracy. It is the same for
/// every strictness: whether someone is at the computer is a fact, not a preference.
/// Capture looks for media playing only past this point, so that lookup is rare.
pub const AWAY_AFTER_SECONDS: u64 = 180;

/// Media playing in the app in front means the user is watching, listening, or on a call, for
/// at most this long without input. Past it, autoplay or a forgotten call counts as Away.
const MEDIA_PRESENCE_LIMIT_SECONDS: f64 = 45.0 * 60.0;

#[derive(Debug)]
pub struct FocusEngine {
    config: FocusConfig,
    rules: Vec<AppRule>,
    /// Distractions allowed for this session: a rule they name does not match.
    exceptions: Vec<AppRule>,
    place: Option<(ActivityKind, String)>,
    window_started_at: Duration,
    /// Set while the place's clock is paused (idle, or Supervisor in front).
    held_since: Option<Duration>,
    last_nudge_at: Option<Duration>,
    /// Muted engines keep judging time but never ask for a nudge.
    muted: bool,
    /// From the webcam: someone is in view (counts like media playing), or nobody has been
    /// in view, without input, long enough to be away.
    camera_present: bool,
    camera_away: bool,
}

impl FocusEngine {
    pub fn new(config: FocusConfig, rules: Vec<AppRule>) -> Self {
        Self {
            config,
            rules,
            exceptions: Vec::new(),
            place: None,
            window_started_at: Duration::ZERO,
            held_since: None,
            last_nudge_at: None,
            muted: false,
            camera_present: false,
            camera_away: false,
        }
    }

    /// The timer runs per app or website, so new tabs and pages on one site keep counting.
    pub fn record_sample(
        &mut self,
        sample: &ActivitySample,
        activity: &Activity,
        now: Duration,
    ) -> FocusSnapshot {
        if self
            .place
            .as_ref()
            .is_none_or(|(kind, key)| *kind != activity.kind || *key != activity.key)
        {
            self.place = Some((activity.kind, activity.key.clone()));
            self.window_started_at = now;
            self.held_since = None;
        }

        if self.is_away(sample) {
            self.hold(now);
            return self.snapshot(sample, activity, FocusState::Idle, None, false, None, now);
        }

        // Back on the same place: carry on where it left off, minus the held time.
        if let Some(since) = self.held_since.take() {
            self.window_started_at += now.saturating_sub(since);
        }

        let matched_rule = self.matching_rule(activity).cloned();
        let seconds_on_window = elapsed_seconds(now, self.window_started_at);
        let state = self.state_for_rule(matched_rule.is_some(), seconds_on_window);
        let next_nudge_in_seconds = if self.muted {
            None
        } else {
            self.next_nudge_in_seconds(state, seconds_on_window, now)
        };
        let should_nudge = next_nudge_in_seconds == Some(0);

        if should_nudge {
            self.last_nudge_at = Some(now);
        }

        self.snapshot(sample, activity, state, matched_rule, should_nudge, next_nudge_in_seconds, now)
    }

    pub fn replace_rules(&mut self, rules: Vec<AppRule>) {
        self.rules = rules;
    }

    pub fn set_exceptions(&mut self, exceptions: Vec<AppRule>) {
        self.exceptions = exceptions;
    }

    /// The distraction rule for this activity, unless an exception for this session names it.
    fn matching_rule(&self, activity: &Activity) -> Option<&AppRule> {
        if matching_rule(&self.exceptions, activity).is_some() {
            return None;
        }
        matching_rule(&self.rules, activity)
    }

    /// Pauses the current place's clock from `since` (the earliest hold wins, and never before
    /// arriving). Coming back to the same place is not a new arrival, so it gets no second
    /// grace period and its nudge countdown resumes rather than restarts.
    pub fn hold(&mut self, since: Duration) {
        let since = since.max(self.window_started_at);
        self.held_since = Some(self.held_since.map_or(since, |held| held.min(since)));
    }

    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
    }

    pub fn set_camera(&mut self, present: bool, away: bool) {
        self.camera_present = present;
        self.camera_away = away;
    }

    /// Every nudge, window or phone, shares one cooldown, so two never play back to back.
    pub fn nudge_ready(&self, now: Duration) -> bool {
        !self.muted
            && self.last_nudge_at.is_none_or(|last| {
                elapsed_seconds(now, last) >= self.config.nudge_cooldown_seconds
            })
    }

    /// A nudge from outside the engine (the phone) starts the shared cooldown.
    pub fn note_nudge(&mut self, now: Duration) {
        self.last_nudge_at = Some(now);
    }

    /// No rule is work. A distraction is Suspicious inside the grace, then Distracted.
    fn state_for_rule(&self, distracting: bool, seconds_on_window: u64) -> FocusState {
        if !distracting {
            FocusState::Focused
        } else if seconds_on_window < self.config.distracted_after_seconds {
            FocusState::Suspicious
        } else {
            FocusState::Distracted
        }
    }

    /// No input past the threshold is Away, unless media is playing in the app in front or the
    /// webcam sees someone. The webcam seeing nobody makes it Away sooner.
    pub fn is_away(&self, sample: &ActivitySample) -> bool {
        self.camera_away
            || sample.idle_seconds.is_some_and(|seconds| {
                seconds >= AWAY_AFTER_SECONDS as f64
                    && !((sample.media_playing || self.camera_present)
                        && seconds < MEDIA_PRESENCE_LIMIT_SECONDS)
            })
    }

    fn next_nudge_in_seconds(
        &self,
        state: FocusState,
        seconds_on_window: u64,
        now: Duration,
    ) -> Option<u64> {
        if state != FocusState::Distracted {
            return None;
        }

        let first_nudge_wait = self
            .config
            .nudge_after_seconds
            .saturating_sub(seconds_on_window);
        let cooldown_wait = self
            .last_nudge_at
            .map(|last| {
                self.config
                    .nudge_cooldown_seconds
                    .saturating_sub(elapsed_seconds(now, last))
            })
            .unwrap_or(0);

        Some(first_nudge_wait.max(cooldown_wait))
    }

    fn snapshot(
        &self,
        sample: &ActivitySample,
        activity: &Activity,
        state: FocusState,
        matched_rule: Option<AppRule>,
        should_nudge: bool,
        next_nudge_in_seconds: Option<u64>,
        now: Duration,
    ) -> FocusSnapshot {
        FocusSnapshot {
            sample: sample.clone(),
            activity: activity.clone(),
            markable: is_markable(activity),
            state,
            distracting: matched_rule.is_some(),
            matched_rule,
            seconds_on_current_window: elapsed_seconds(now, self.window_started_at),
            should_nudge,
            next_nudge_in_seconds,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FocusConfig {
    pub distracted_after_seconds: u64,
    pub nudge_after_seconds: u64,
    pub nudge_cooldown_seconds: u64,
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self {
            distracted_after_seconds: 8,
            nudge_after_seconds: 60,
            nudge_cooldown_seconds: 120,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct FocusSnapshot {
    pub sample: ActivitySample,
    pub activity: Activity,
    /// Whether "mark current" can name exactly this (see `is_markable`).
    pub markable: bool,
    pub state: FocusState,
    /// A distraction rule matched (and no exception lifted it).
    pub distracting: bool,
    pub matched_rule: Option<AppRule>,
    pub seconds_on_current_window: u64,
    pub should_nudge: bool,
    pub next_nudge_in_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FocusState {
    Focused,
    Suspicious,
    Distracted,
    Idle,
}

/// A distraction: `value` is a website key ("youtube.com") or an app name ("Spotify").
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AppRule {
    pub target: RuleTarget,
    pub value: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RuleTarget {
    App,
    Website,
}

impl AppRule {
    pub fn app(value: &str) -> Self {
        Self { target: RuleTarget::App, value: value.trim().to_string() }
    }

    pub fn website(key: &str) -> Self {
        Self { target: RuleTarget::Website, value: key.trim().to_lowercase() }
    }

    /// A typed rule: an address or verified site name becomes a website rule, a plain name an
    /// app rule. Anything else (a page title, a path) is refused rather than guessed at.
    pub fn from_input(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(key) = website::website_from_input(text) {
            return Some(Self::website(&key));
        }
        is_plain_name(text).then(|| Self::app(text))
    }

    /// Like `from_input`, but a known app's key or name becomes a rule on that app's key. A
    /// name two apps share is refused: a rule on it would rate both.
    pub fn from_input_with_apps(
        text: &str,
        names: &std::collections::HashMap<String, String>,
    ) -> Result<Self, String> {
        let text = text.trim();
        // Known executable keys may contain dots (WhatsApp.Root); resolve them before URLs.
        if let Some(key) = names.keys().find(|key| key.eq_ignore_ascii_case(text)) {
            return Ok(Self::app(key));
        }
        let matches: Vec<&String> = names.iter()
            .filter(|(_, name)| name.eq_ignore_ascii_case(text))
            .map(|(key, _)| key)
            .collect();
        match matches.as_slice() {
            [key] => return Ok(Self::app(key)),
            [] => {}
            _ => return Err(format!(
                "More than one app is called {text}. Rate the one you mean from History, or rename one first."
            )),
        }
        Self::from_input(text).ok_or_else(|| {
            "Enter a website (youtube.com) or an app name (Spotify), not a page title or path".to_string()
        })
    }

    /// No rule names a browser: sites are judged one by one, and a browser's own pages and
    /// unidentified sites are never distractions.
    pub fn names_a_browser(&self) -> bool {
        self.target == RuleTarget::App && website::is_browser(&self.value.trim().to_lowercase())
    }

    pub fn matches(&self, activity: &Activity) -> bool {
        let value = self.value.as_str();
        match self.target {
            RuleTarget::Website => {
                activity.kind == ActivityKind::Website && activity.key.eq_ignore_ascii_case(value)
            }
            RuleTarget::App => {
                activity.kind != ActivityKind::Website
                    && !website::is_browser(&activity.app_id)
                    && (activity.app_id.eq_ignore_ascii_case(value)
                        || (names_by_label(value, &activity.app_id)
                            && activity.app_label.eq_ignore_ascii_case(value)))
            }
        }
    }
}

/// Whether an app rule may match by the app's name. Keys are lowercase without spaces, so a
/// value like that names one app's key and never a second app that happens to share the name
/// ("code" is not every app labeled "Code"). Nor does a rule on `app` reach `app#2`.
fn names_by_label(value: &str, app_id: &str) -> bool {
    let key_shaped = !value.chars().any(|ch| ch.is_uppercase() || ch.is_whitespace());
    !key_shaped && !crate::app_identity::stem(app_id).eq_ignore_ascii_case(value)
}

/// A website, or an app that isn't a browser. Browser pages and unidentified sites have no
/// rule that would mean only them.
pub fn is_markable(activity: &Activity) -> bool {
    match activity.kind {
        ActivityKind::Website => true,
        ActivityKind::App => !website::is_browser(&activity.app_id),
        ActivityKind::OtherWebsites => false,
    }
}

/// Website rules first: a site rule beats a rule on the browser showing it.
pub fn matching_rule<'a>(rules: &'a [AppRule], activity: &Activity) -> Option<&'a AppRule> {
    rules
        .iter()
        .find(|rule| rule.target == RuleTarget::Website && rule.matches(activity))
        .or_else(|| {
            rules
                .iter()
                .find(|rule| rule.target == RuleTarget::App && rule.matches(activity))
        })
}

pub fn is_plain_name(text: &str) -> bool {
    const SEPARATORS: &[&str] = &[" - ", " — ", " – ", " | ", "/", "\\", ":"];
    text.chars().count() <= 40 && !SEPARATORS.iter().any(|sep| text.contains(sep))
}

pub fn starter_rules() -> Vec<AppRule> {
    ["youtube.com", "netflix.com", "tiktok.com", "instagram.com", "reddit.com"]
        .into_iter()
        .map(AppRule::website)
        .collect()
}

fn elapsed_seconds(now: Duration, since: Duration) -> u64 {
    now.saturating_sub(since).as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Bounds, CapturePermissions, PermissionState};

    #[test]
    fn known_dotted_app_keys_are_not_websites() {
        let names = std::collections::HashMap::from([
            ("whatsapp.root".to_string(), "WhatsApp".to_string()),
        ]);
        for text in ["WhatsApp.Root", "WhatsApp"] {
            let rule = AppRule::from_input_with_apps(text, &names).unwrap();
            assert_eq!(rule.target, RuleTarget::App);
            assert_eq!(rule.value, "whatsapp.root");
        }
        assert_eq!(AppRule::from_input_with_apps("https://whatsapp.com", &names)
            .unwrap().target, RuleTarget::Website);
    }

    #[test]
    fn a_name_two_apps_share_is_refused() {
        let names = std::collections::HashMap::from([
            ("gl".to_string(), "Game Launcher".to_string()),
            ("gamelauncher".to_string(), "Game Launcher".to_string()),
        ]);
        assert!(AppRule::from_input_with_apps("Game Launcher", &names).is_err());
        // Each key still names exactly one.
        let rule = AppRule::from_input_with_apps("GL", &names).unwrap();
        assert_eq!(rule.value, "gl");
    }

    #[test]
    fn a_rule_on_one_app_never_reaches_another_with_the_same_name() {
        let first = AppRule::app("app");
        let mut second = Activity::app("App");
        second.app_id = "app#2".into();
        second.key = "app#2".into();
        assert!(first.matches(&Activity::app("App")));
        assert!(!first.matches(&second));
        // Even a name rule written in the app's own case stays off the second app.
        assert!(!AppRule::app("App").matches(&second));
        assert!(AppRule::app("app#2").matches(&second));

        // A key rule never matches a different program that is merely labeled the same.
        let mut codium = Activity::app("Code");
        codium.app_id = "vscodium".into();
        codium.key = "vscodium".into();
        assert!(!AppRule::app("code").matches(&codium));
    }

    #[test]
    fn a_second_browser_install_is_still_a_browser() {
        let mut canary = Activity::app("Google Chrome Canary");
        canary.app_id = "chrome#2".into();
        canary.key = "chrome#2".into();
        assert!(!is_markable(&canary));
        assert!(AppRule::app("chrome#2").names_a_browser());
        assert!(!AppRule::app("chrome#2").matches(&canary));
    }

    #[test]
    fn no_rule_is_work_and_a_rule_is_a_distraction() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com")]);

        let work = observe(&mut engine, &Activity::app("Code"), 0.0, 0);
        assert_eq!(work.state, FocusState::Focused);
        assert!(!work.distracting);
        assert_eq!(work.matched_rule, None);

        let youtube = observe(&mut engine, &Activity::website("chrome", "youtube.com"), 0.0, 1);
        assert!(youtube.distracting);
        assert_eq!(youtube.matched_rule.unwrap().value, "youtube.com");
    }

    #[test]
    fn an_exception_lifts_a_rule_for_the_session() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com"), AppRule::app("Slack")]);
        engine.set_exceptions(vec![AppRule::website("youtube.com")]);

        let youtube = observe(&mut engine, &Activity::website("chrome", "youtube.com"), 0.0, 0);
        assert_eq!(youtube.state, FocusState::Focused);
        assert!(!youtube.distracting);
        assert_eq!(observe(&mut engine, &Activity::app("Slack"), 0.0, 1).state, FocusState::Suspicious);

        engine.set_exceptions(Vec::new());
        assert!(observe(&mut engine, &Activity::website("chrome", "youtube.com"), 0.0, 2).distracting);
    }

    #[test]
    fn browser_rules_never_apply_only_site_rules_do() {
        let rules = vec![
            AppRule::app("Google Chrome"),
            AppRule::website("youtube.com"),
        ];
        let mut youtube = Activity::website("chrome", "youtube.com");
        youtube.app_label = "Google Chrome".to_string();
        assert_eq!(matching_rule(&rules, &youtube).map(|rule| rule.value.as_str()), Some("youtube.com"));

        let mut github = Activity::website("chrome", "github.com");
        github.app_label = "Google Chrome".to_string();
        assert_eq!(matching_rule(&rules, &github), None);

        // No rule reaches a browser: the Chrome rule covers neither its pages nor
        // unidentified sites.
        let mut other = github.clone();
        other.kind = ActivityKind::OtherWebsites;
        other.key = "other-websites".to_string();
        assert_eq!(matching_rule(&rules, &other), None);
        let mut pages = github.clone();
        pages.kind = ActivityKind::App;
        pages.key = "chrome".to_string();
        assert_eq!(matching_rule(&rules, &pages), None);
        assert!(rules[0].names_a_browser());
    }

    #[test]
    fn app_rules_match_the_id_or_the_label() {
        let rule = AppRule::app("Visual Studio Code");
        let mut code = Activity::app("Code");
        assert!(!rule.matches(&code));
        code.app_label = "Visual Studio Code".to_string();
        assert!(rule.matches(&code));
        assert!(AppRule::app("code").matches(&code));
    }

    #[test]
    fn typed_rules_become_sites_apps_or_nothing() {
        let rule = |text| AppRule::from_input(text);
        assert_eq!(rule("YouTube").map(|r| (r.target, r.value)), Some((RuleTarget::Website, "youtube.com".into())));
        assert_eq!(rule("https://m.reddit.com/r/x").map(|r| r.value), Some("reddit.com".into()));
        assert_eq!(rule("Spotify").map(|r| r.target), Some(RuleTarget::App));
        assert_eq!(rule("Watch Later - YouTube"), None);
        assert_eq!(rule("C:\\Games\\game.exe"), None);
    }

    #[test]
    fn distracting_rule_moves_through_suspicious_before_distracted() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com")]);
        let youtube = Activity::website("chrome", "youtube.com");

        let first = observe(&mut engine, &youtube, 0.0, 0);
        let later = observe(&mut engine, &youtube, 0.0, 4);
        let distracted = observe(&mut engine, &youtube, 0.0, 5);

        assert_eq!(first.state, FocusState::Suspicious);
        assert_eq!(later.state, FocusState::Suspicious);
        assert_eq!(distracted.state, FocusState::Distracted);
    }

    #[test]
    fn app_switch_does_not_nudge_immediately() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com")]);
        let youtube = Activity::website("chrome", "youtube.com");

        assert!(!observe(&mut engine, &youtube, 0.0, 100).should_nudge);
        assert!(!observe(&mut engine, &youtube, 0.0, 109).should_nudge);
        assert!(observe(&mut engine, &youtube, 0.0, 110).should_nudge);
    }

    #[test]
    fn idle_samples_do_not_nudge_or_count_toward_the_site() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com")]);
        let youtube = Activity::website("chrome", "youtube.com");

        let idle = observe(&mut engine, &youtube, 200.0, 100);
        let returned = observe(&mut engine, &youtube, 0.0, 101);

        assert_eq!(idle.state, FocusState::Idle);
        assert!(!idle.should_nudge);
        assert_eq!(returned.state, FocusState::Suspicious);
        assert!(!returned.should_nudge);
    }

    #[test]
    fn media_in_front_keeps_an_idle_user_present_until_the_limit() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com")]);
        let youtube = Activity::website("chrome", "youtube.com");

        assert_eq!(observe_media(&mut engine, &youtube, 200.0, true, 0).state, FocusState::Suspicious);
        let watching = observe_media(&mut engine, &youtube, 220.0, true, 20);
        assert_eq!(watching.state, FocusState::Distracted);
        assert!(watching.should_nudge);
        // 45 minutes without input: autoplay with nobody there.
        assert_eq!(observe_media(&mut engine, &youtube, 2700.0, true, 40).state, FocusState::Idle);
    }

    #[test]
    fn nudge_cooldown_blocks_repeated_nudges() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com")]);
        let youtube = Activity::website("chrome", "youtube.com");

        assert!(!observe(&mut engine, &youtube, 0.0, 0).should_nudge);
        assert!(observe(&mut engine, &youtube, 0.0, 10).should_nudge);
        let cooled_down = observe(&mut engine, &youtube, 0.0, 12);
        assert!(!cooled_down.should_nudge);
        assert_eq!(cooled_down.next_nudge_in_seconds, Some(28));
        assert!(observe(&mut engine, &youtube, 0.0, 40).should_nudge);
    }

    fn test_engine(rules: Vec<AppRule>) -> FocusEngine {
        FocusEngine::new(
            FocusConfig {
                distracted_after_seconds: 5,
                nudge_after_seconds: 10,
                nudge_cooldown_seconds: 30,
            },
            rules,
        )
    }

    fn observe(engine: &mut FocusEngine, activity: &Activity, idle_seconds: f64, at: u64) -> FocusSnapshot {
        observe_media(engine, activity, idle_seconds, false, at)
    }

    fn observe_media(
        engine: &mut FocusEngine,
        activity: &Activity,
        idle_seconds: f64,
        media_playing: bool,
        at: u64,
    ) -> FocusSnapshot {
        let sample = ActivitySample {
            captured_at: "2026-08-12T00:00:00Z".to_string(),
            platform: "macos",
            app_name: activity.app_id.clone(),
            app_label: activity.app_label.clone(),
            process_path: format!("/Applications/{}.app", activity.app_id),
            app_identity: String::new(),
            app_key: None,
            process_id: 42,
            window_title: None,
            window_id: "window-1".to_string(),
            bounds: Bounds { x: 0.0, y: 0.0, width: 1440.0, height: 900.0 },
            idle_seconds: Some(idle_seconds),
            permissions: CapturePermissions { screen_recording: PermissionState::Granted },
            shell_surface: false,
            browser: None,
            media_playing,
        };
        engine.record_sample(&sample, activity, Duration::from_secs(at))
    }
}
