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
    place: Option<(ActivityKind, String)>,
    window_started_at: Duration,
    /// Set while the place's clock is paused (idle, or Supervisor in front).
    held_since: Option<Duration>,
    last_nudge_at: Option<Duration>,
    /// Muted engines keep judging time but never ask for a nudge.
    muted: bool,
}

impl FocusEngine {
    pub fn new(config: FocusConfig, rules: Vec<AppRule>) -> Self {
        Self {
            config,
            rules,
            place: None,
            window_started_at: Duration::ZERO,
            held_since: None,
            last_nudge_at: None,
            muted: false,
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
            return self.snapshot(
                sample,
                activity,
                FocusState::Idle,
                RuleClassification::Neutral,
                None,
                false,
                None,
                now,
            );
        }

        // Back on the same place: carry on where it left off, minus the held time.
        if let Some(since) = self.held_since.take() {
            self.window_started_at += now.saturating_sub(since);
        }

        let matched_rule = matching_rule(&self.rules, activity).cloned();
        let classification = matched_rule
            .as_ref()
            .map(|rule| rule.classification)
            .unwrap_or(RuleClassification::Neutral);
        let seconds_on_window = elapsed_seconds(now, self.window_started_at);
        let state = self.state_for_classification(&classification, seconds_on_window);
        let next_nudge_in_seconds = if self.muted {
            None
        } else {
            self.next_nudge_in_seconds(state, seconds_on_window, now)
        };
        let should_nudge = next_nudge_in_seconds == Some(0);

        if should_nudge {
            self.last_nudge_at = Some(now);
        }

        self.snapshot(
            sample,
            activity,
            state,
            classification,
            matched_rule,
            should_nudge,
            next_nudge_in_seconds,
            now,
        )
    }

    pub fn replace_rules(&mut self, rules: Vec<AppRule>) {
        self.rules = rules;
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

    fn state_for_classification(
        &self,
        classification: &RuleClassification,
        seconds_on_window: u64,
    ) -> FocusState {
        match classification {
            RuleClassification::Focused => FocusState::Focused,
            RuleClassification::Neutral => FocusState::Neutral,
            RuleClassification::Distracting
                if seconds_on_window < self.config.distracted_after_seconds =>
            {
                FocusState::Suspicious
            }
            RuleClassification::Distracting => FocusState::Distracted,
        }
    }

    /// No input past the threshold is Away, unless media is playing in the app in front.
    pub fn is_away(&self, sample: &ActivitySample) -> bool {
        sample.idle_seconds.is_some_and(|seconds| {
            seconds >= AWAY_AFTER_SECONDS as f64
                && !(sample.media_playing && seconds < MEDIA_PRESENCE_LIMIT_SECONDS)
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

    #[allow(clippy::too_many_arguments)]
    fn snapshot(
        &self,
        sample: &ActivitySample,
        activity: &Activity,
        state: FocusState,
        classification: RuleClassification,
        matched_rule: Option<AppRule>,
        should_nudge: bool,
        next_nudge_in_seconds: Option<u64>,
        now: Duration,
    ) -> FocusSnapshot {
        FocusSnapshot {
            sample: sample.clone(),
            activity: activity.clone(),
            state,
            classification,
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
    pub state: FocusState,
    pub classification: RuleClassification,
    pub matched_rule: Option<AppRule>,
    pub seconds_on_current_window: u64,
    pub should_nudge: bool,
    pub next_nudge_in_seconds: Option<u64>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum FocusState {
    Focused,
    Neutral,
    Suspicious,
    Distracted,
    Idle,
}

/// `value` is a website key ("youtube.com") or an app name ("Spotify", "Terminal").
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AppRule {
    pub target: RuleTarget,
    pub value: String,
    pub classification: RuleClassification,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum RuleTarget {
    App,
    Website,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuleClassification {
    Focused,
    Neutral,
    Distracting,
}

impl AppRule {
    pub fn app(value: &str, classification: RuleClassification) -> Self {
        Self {
            target: RuleTarget::App,
            value: value.trim().to_string(),
            classification,
        }
    }

    pub fn website(key: &str, classification: RuleClassification) -> Self {
        Self {
            target: RuleTarget::Website,
            value: key.trim().to_lowercase(),
            classification,
        }
    }

    /// A typed rule: an address or verified site name becomes a website rule, a plain name an
    /// app rule. Anything else (a page title, a path) is refused rather than guessed at.
    pub fn from_input(text: &str, classification: RuleClassification) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if let Some(key) = website::website_from_input(text) {
            return Some(Self::website(&key, classification));
        }
        is_plain_name(text).then(|| Self::app(text, classification))
    }

    pub fn from_input_with_apps(
        text: &str,
        classification: RuleClassification,
        names: &std::collections::HashMap<String, String>,
    ) -> Option<Self> {
        let text = text.trim();
        // Known executable keys may contain dots (WhatsApp.Root); resolve them before URLs.
        if let Some(key) = names.keys().find(|key| key.eq_ignore_ascii_case(text)) {
            return Some(Self::app(key, classification));
        }
        let mut matches = names.iter().filter(|(_, name)| name.eq_ignore_ascii_case(text));
        if let Some((key, _)) = matches.next() {
            if matches.next().is_none() {
                return Some(Self::app(key, classification));
            }
        }
        Self::from_input(text, classification)
    }

    /// An app rule also covers that browser's pages that are not an identified website.
    pub fn matches(&self, activity: &Activity) -> bool {
        let value = self.value.as_str();
        match self.target {
            RuleTarget::Website => {
                activity.kind == ActivityKind::Website && activity.key.eq_ignore_ascii_case(value)
            }
            RuleTarget::App => {
                activity.kind != ActivityKind::Website
                    && (activity.app_id.eq_ignore_ascii_case(value)
                        || activity.app_label.eq_ignore_ascii_case(value))
            }
        }
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
        .map(|key| AppRule::website(key, RuleClassification::Distracting))
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
            let rule = AppRule::from_input_with_apps(text, RuleClassification::Focused, &names).unwrap();
            assert_eq!(rule.target, RuleTarget::App);
            assert_eq!(rule.value, "whatsapp.root");
        }
        assert_eq!(AppRule::from_input_with_apps("https://whatsapp.com", RuleClassification::Focused, &names)
            .unwrap().target, RuleTarget::Website);
    }

    #[test]
    fn first_matching_rule_scores_the_activity() {
        let mut engine = test_engine(vec![
            AppRule::app("Code", RuleClassification::Focused),
            AppRule::website("youtube.com", RuleClassification::Distracting),
        ]);

        let snapshot = observe(&mut engine, &Activity::app("Code"), 0.0, 0);

        assert_eq!(snapshot.state, FocusState::Focused);
        assert_eq!(snapshot.matched_rule.unwrap().value, "Code");
    }

    #[test]
    fn website_rules_beat_app_rules_on_the_browser() {
        let rules = vec![
            AppRule::app("Google Chrome", RuleClassification::Focused),
            AppRule::website("youtube.com", RuleClassification::Distracting),
        ];
        let mut youtube = Activity::website("chrome", "youtube.com");
        youtube.app_label = "Google Chrome".to_string();
        assert_eq!(
            matching_rule(&rules, &youtube).map(|rule| rule.classification),
            Some(RuleClassification::Distracting)
        );

        let mut github = Activity::website("chrome", "github.com");
        github.app_label = "Google Chrome".to_string();
        assert_eq!(matching_rule(&rules, &github), None);

        let mut other = github.clone();
        other.kind = ActivityKind::OtherWebsites;
        other.key = "other-websites".to_string();
        assert_eq!(
            matching_rule(&rules, &other).map(|rule| rule.classification),
            Some(RuleClassification::Focused)
        );
    }

    #[test]
    fn app_rules_match_the_id_or_the_label() {
        let rule = AppRule::app("Visual Studio Code", RuleClassification::Focused);
        let mut code = Activity::app("Code");
        assert!(!rule.matches(&code));
        code.app_label = "Visual Studio Code".to_string();
        assert!(rule.matches(&code));
        assert!(AppRule::app("code", RuleClassification::Focused).matches(&code));
    }

    #[test]
    fn typed_rules_become_sites_apps_or_nothing() {
        let rule = |text| AppRule::from_input(text, RuleClassification::Distracting);
        assert_eq!(rule("YouTube").map(|r| (r.target, r.value)), Some((RuleTarget::Website, "youtube.com".into())));
        assert_eq!(rule("https://m.reddit.com/r/x").map(|r| r.value), Some("reddit.com".into()));
        assert_eq!(rule("Spotify").map(|r| r.target), Some(RuleTarget::App));
        assert_eq!(rule("Watch Later - YouTube"), None);
        assert_eq!(rule("C:\\Games\\game.exe"), None);
    }

    #[test]
    fn distracting_rule_moves_through_suspicious_before_distracted() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com", RuleClassification::Distracting)]);
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
        let mut engine = test_engine(vec![AppRule::website("youtube.com", RuleClassification::Distracting)]);
        let youtube = Activity::website("chrome", "youtube.com");

        assert!(!observe(&mut engine, &youtube, 0.0, 100).should_nudge);
        assert!(!observe(&mut engine, &youtube, 0.0, 109).should_nudge);
        assert!(observe(&mut engine, &youtube, 0.0, 110).should_nudge);
    }

    #[test]
    fn idle_samples_do_not_nudge_or_count_toward_the_site() {
        let mut engine = test_engine(vec![AppRule::website("youtube.com", RuleClassification::Distracting)]);
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
        let mut engine = test_engine(vec![AppRule::website("youtube.com", RuleClassification::Distracting)]);
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
        let mut engine = test_engine(vec![AppRule::website("youtube.com", RuleClassification::Distracting)]);
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
