use crate::activity::{ActivitySample, should_treat_as_neutral};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug)]
pub struct FocusEngine {
    config: FocusConfig,
    rules: Vec<AppRule>,
    window_key: Option<WindowKey>,
    window_started_at: Duration,
    last_nudge_at: Option<Duration>,
}

impl Default for FocusEngine {
    fn default() -> Self {
        Self::new(FocusConfig::default(), starter_rules())
    }
}

impl FocusEngine {
    pub fn new(config: FocusConfig, rules: Vec<AppRule>) -> Self {
        Self {
            config,
            rules,
            window_key: None,
            window_started_at: Duration::ZERO,
            last_nudge_at: None,
        }
    }

    pub fn record_sample(&mut self, sample: &ActivitySample, now: Duration) -> FocusSnapshot {
        if should_treat_as_neutral(sample) {
            self.window_key = None;
            self.window_started_at = now;
            return self.snapshot(
                sample,
                FocusState::Neutral,
                RuleClassification::Neutral,
                None,
                false,
                None,
                now,
            );
        }

        let key = WindowKey::from_sample(sample);
        if self.window_key.as_ref() != Some(&key) {
            self.window_key = Some(key);
            self.window_started_at = now;
        }

        if self.is_idle(sample) {
            self.window_started_at = now;
            return self.snapshot(
                sample,
                FocusState::Idle,
                RuleClassification::Neutral,
                None,
                false,
                None,
                now,
            );
        }

        let matched_rule = self.rules.iter().find(|rule| rule.matches(sample)).cloned();
        let classification = matched_rule
            .as_ref()
            .map(|rule| rule.classification.clone())
            .unwrap_or(RuleClassification::Neutral);
        let seconds_on_window = elapsed_seconds(now, self.window_started_at);
        let state = self.state_for_classification(&classification, seconds_on_window);
        let next_nudge_in_seconds = self.next_nudge_in_seconds(state, seconds_on_window, now);
        let should_nudge = next_nudge_in_seconds == Some(0);

        if should_nudge {
            self.last_nudge_at = Some(now);
        }

        self.snapshot(
            sample,
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

    fn is_idle(&self, sample: &ActivitySample) -> bool {
        sample
            .idle_seconds
            .is_some_and(|seconds| seconds >= self.config.idle_after_seconds as f64)
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
        state: FocusState,
        classification: RuleClassification,
        matched_rule: Option<AppRule>,
        should_nudge: bool,
        next_nudge_in_seconds: Option<u64>,
        now: Duration,
    ) -> FocusSnapshot {
        FocusSnapshot {
            sample: sample.clone(),
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
    pub idle_after_seconds: u64,
    pub distracted_after_seconds: u64,
    pub nudge_after_seconds: u64,
    pub nudge_cooldown_seconds: u64,
}

impl Default for FocusConfig {
    fn default() -> Self {
        Self {
            idle_after_seconds: 60,
            distracted_after_seconds: 8,
            nudge_after_seconds: 60,
            nudge_cooldown_seconds: 120,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct FocusSnapshot {
    pub sample: ActivitySample,
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

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct AppRule {
    pub target: RuleTarget,
    pub value: String,
    pub classification: RuleClassification,
}

impl AppRule {
    pub fn title_contains(value: &str, classification: RuleClassification) -> Self {
        Self {
            target: RuleTarget::WindowTitle,
            value: value.to_string(),
            classification,
        }
    }

    pub fn app_name(value: &str, classification: RuleClassification) -> Self {
        Self {
            target: RuleTarget::AppName,
            value: value.to_string(),
            classification,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuleTarget {
    #[allow(dead_code)]
    AppName,
    #[allow(dead_code)]
    ProcessPath,
    WindowTitle,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RuleClassification {
    #[allow(dead_code)]
    Focused,
    Neutral,
    Distracting,
}

impl AppRule {
    fn matches(&self, sample: &ActivitySample) -> bool {
        let value = self.value.trim();
        if value.is_empty() {
            return false;
        }

        match self.target {
            RuleTarget::AppName => sample.app_name.eq_ignore_ascii_case(value),
            RuleTarget::ProcessPath => contains_ignore_case(&sample.process_path, value),
            RuleTarget::WindowTitle => sample
                .window_title
                .as_ref()
                .is_some_and(|title| contains_ignore_case(title, value)),
        }
    }
}

pub fn starter_rules() -> Vec<AppRule> {
    ["YouTube", "Netflix", "TikTok", "Instagram", "Reddit"]
        .into_iter()
        .map(|title| AppRule::title_contains(title, RuleClassification::Distracting))
        .collect()
}

fn elapsed_seconds(now: Duration, since: Duration) -> u64 {
    now.saturating_sub(since).as_secs()
}

fn contains_ignore_case(haystack: &str, needle: &str) -> bool {
    // ponytail: O(n) lowercase matching is fine for tens of rules; compile matchers if users add hundreds.
    haystack.to_lowercase().contains(&needle.to_lowercase())
}

#[derive(Debug, PartialEq)]
struct WindowKey {
    app_name: String,
    process_path: String,
    process_id: u64,
    window_id: String,
    window_title: Option<String>,
}

impl WindowKey {
    fn from_sample(sample: &ActivitySample) -> Self {
        Self {
            app_name: sample.app_name.clone(),
            process_path: sample.process_path.clone(),
            process_id: sample.process_id,
            window_id: sample.window_id.clone(),
            window_title: sample.window_title.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Bounds, CapturePermissions, PermissionState};

    #[test]
    fn first_matching_rule_scores_the_sample() {
        let mut engine = test_engine(vec![
            AppRule::app_name("Code", RuleClassification::Focused),
            AppRule::title_contains("YouTube", RuleClassification::Distracting),
        ]);

        let snapshot = engine.record_sample(&sample("Code", "YouTube API docs", 0.0), seconds(0));

        assert_eq!(snapshot.state, FocusState::Focused);
        assert_eq!(snapshot.classification, RuleClassification::Focused);
        assert_eq!(snapshot.matched_rule.unwrap().target, RuleTarget::AppName);
    }

    #[test]
    fn distracting_rule_moves_through_suspicious_before_distracted() {
        let mut engine = test_engine(vec![AppRule::title_contains(
            "YouTube",
            RuleClassification::Distracting,
        )]);

        let first = engine.record_sample(&sample("Chrome", "YouTube", 0.0), seconds(0));
        let later = engine.record_sample(&sample("Chrome", "YouTube", 0.0), seconds(4));
        let distracted = engine.record_sample(&sample("Chrome", "YouTube", 0.0), seconds(5));

        assert_eq!(first.state, FocusState::Suspicious);
        assert_eq!(later.state, FocusState::Suspicious);
        assert_eq!(distracted.state, FocusState::Distracted);
    }

    #[test]
    fn app_switch_does_not_nudge_immediately() {
        let mut engine = test_engine(vec![AppRule::title_contains(
            "YouTube",
            RuleClassification::Distracting,
        )]);

        assert!(
            !engine
                .record_sample(&sample("Chrome", "YouTube", 0.0), seconds(100))
                .should_nudge
        );
        assert!(
            !engine
                .record_sample(&sample("Chrome", "YouTube", 0.0), seconds(109))
                .should_nudge
        );
        assert!(
            engine
                .record_sample(&sample("Chrome", "YouTube", 0.0), seconds(110))
                .should_nudge
        );
    }

    #[test]
    fn idle_samples_do_not_nudge_and_reset_distraction_timer() {
        let mut engine = test_engine(vec![AppRule::title_contains(
            "YouTube",
            RuleClassification::Distracting,
        )]);

        let idle = engine.record_sample(&sample("Chrome", "YouTube", 120.0), seconds(100));
        let returned = engine.record_sample(&sample("Chrome", "YouTube", 0.0), seconds(101));

        assert_eq!(idle.state, FocusState::Idle);
        assert!(!idle.should_nudge);
        assert_eq!(returned.state, FocusState::Suspicious);
        assert!(!returned.should_nudge);
    }

    #[test]
    fn nudge_cooldown_blocks_repeated_nudges() {
        let mut engine = test_engine(vec![AppRule::title_contains(
            "YouTube",
            RuleClassification::Distracting,
        )]);

        assert!(
            !engine
                .record_sample(&sample("Chrome", "YouTube", 0.0), seconds(0))
                .should_nudge
        );
        assert!(
            engine
                .record_sample(&sample("Chrome", "YouTube", 0.0), seconds(10))
                .should_nudge
        );
        let cooled_down = engine.record_sample(&sample("Chrome", "YouTube", 0.0), seconds(12));
        assert!(!cooled_down.should_nudge);
        assert_eq!(cooled_down.next_nudge_in_seconds, Some(28));
        assert!(
            engine
                .record_sample(&sample("Chrome", "YouTube", 0.0), seconds(40))
                .should_nudge
        );
    }

    fn test_engine(rules: Vec<AppRule>) -> FocusEngine {
        FocusEngine::new(
            FocusConfig {
                idle_after_seconds: 60,
                distracted_after_seconds: 5,
                nudge_after_seconds: 10,
                nudge_cooldown_seconds: 30,
            },
            rules,
        )
    }

    fn sample(app_name: &str, title: &str, idle_seconds: f64) -> ActivitySample {
        ActivitySample {
            captured_at: "2026-08-12T00:00:00Z".to_string(),
            platform: "macos",
            app_name: app_name.to_string(),
            process_path: format!("/Applications/{app_name}.app"),
            process_id: 42,
            window_title: Some(title.to_string()),
            window_id: "window-1".to_string(),
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 1440.0,
                height: 900.0,
            },
            idle_seconds: Some(idle_seconds),
            permissions: CapturePermissions {
                screen_recording: PermissionState::Granted,
            },
        }
    }

    #[test]
    fn neutral_capture_kind_never_drifts() {
        let mut engine = test_engine(vec![AppRule::title_contains(
            "YouTube",
            RuleClassification::Distracting,
        )]);

        let neutral = ActivitySample {
            captured_at: "2026-08-12T00:00:00Z".to_string(),
            platform: "windows",
            app_name: "SearchHost".to_string(),
            process_path: "C:\\Windows\\SystemApps\\MicrosoftWindows.Client.CBS_cw5n1h2txyewy\\SearchHost.exe".to_string(),
            process_id: 42,
            window_title: Some("Search".to_string()),
            window_id: "window-1".to_string(),
            bounds: Bounds {
                x: 0.0,
                y: 0.0,
                width: 1440.0,
                height: 900.0,
            },
            idle_seconds: Some(0.0),
            permissions: CapturePermissions {
                screen_recording: PermissionState::Granted,
            },
        };
        let snapshot = engine.record_sample(&neutral, seconds(120));

        assert_eq!(snapshot.state, FocusState::Neutral);
        assert_eq!(snapshot.classification, RuleClassification::Neutral);
        assert!(!snapshot.should_nudge);
    }

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }
}
