use crate::activity::ActivitySample;
use crate::focus::{
    starter_rules, AppRule, FocusConfig, FocusEngine, FocusSnapshot, FocusState,
    RuleClassification, RuleTarget,
};
use crate::storage::{focus_state_key, span_category, SessionStorage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

#[derive(Debug)]
pub struct SessionManager {
    settings: SessionSettings,
    persisted_rules: Vec<AppRule>,
    current: Option<Session>,
    history: Vec<SessionSummary>,
    next_id: u64,
}

impl Default for SessionManager {
    fn default() -> Self {
        Self {
            settings: SessionSettings::default(),
            persisted_rules: starter_rules(),
            current: None,
            history: Vec::new(),
            next_id: 1,
        }
    }
}

impl SessionManager {
    pub fn bootstrap(
        settings: SessionSettings,
        rules: Vec<AppRule>,
        next_id: u64,
    ) -> Self {
        Self {
            settings,
            persisted_rules: rules,
            current: None,
            history: Vec::new(),
            next_id,
        }
    }
}

#[derive(Clone, Debug)]
pub struct SupervisorIdentity {
    pub process_id: u64,
    pub process_path: String,
    pub app_name: String,
}

impl SupervisorIdentity {
    pub fn matches(&self, sample: &ActivitySample) -> bool {
        // ponytail: Supervisor window detection is a V1 process/app-name heuristic; upgrade with
        // explicit Tauri window handles if platform metadata proves ambiguous.
        sample.process_id == self.process_id
            || paths_match(&sample.process_path, &self.process_path)
            || sample.app_name.eq_ignore_ascii_case(&self.app_name)
            || sample.app_name.eq_ignore_ascii_case("Supervisor")
    }
}

impl SessionManager {
    pub fn start(
        &mut self,
        input: StartSessionInput,
        now: Duration,
        started_at: String,
        storage: Option<&SessionStorage>,
    ) -> Result<(), String> {
        let strictness = input.strictness.unwrap_or(self.settings.default_strictness);
        let duration_seconds = input.duration_minutes.clamp(5, 180) * 60;
        let goal = clean_goal(input.goal);
        let rules = clean_rules(
            input
                .rules
                .unwrap_or_else(|| self.persisted_rules.clone()),
        );
        let session_id = if let Some(storage) = storage {
            let id = storage.next_session_id()?;
            storage.insert_session(id, &goal, strictness, &started_at, duration_seconds)?;
            id
        } else {
            let id = self.next_id;
            self.next_id += 1;
            id
        };
        let session = Session::new(
            session_id,
            goal,
            duration_seconds,
            strictness,
            rules.clone(),
            now,
            started_at,
        );

        if let Some(storage) = storage {
            storage.replace_rules(&rules)?;
            self.persisted_rules = rules;
        } else {
            self.next_id = session_id + 1;
        }
        self.settings.status_window_on_top = true;
        if let Some(storage) = storage {
            storage.save_settings(&self.settings)?;
        }
        self.current = Some(session);
        Ok(())
    }

    pub fn record_sample(
        &mut self,
        sample: ActivitySample,
        now: Duration,
        supervisor: &SupervisorIdentity,
        storage: Option<&SessionStorage>,
    ) -> SessionSnapshot {
        self.tick_current(now, storage);
        self.finish_expired(now, storage);

        if let Some(session) = self.current.as_mut() {
            session.record_sample(sample, now, supervisor, storage);
        }

        self.tick_current(now, storage);
        self.finish_expired(now, storage);
        self.snapshot(now, storage)
    }

    pub fn tick(&mut self, now: Duration, storage: Option<&SessionStorage>) -> SessionSnapshot {
        self.tick_current(now, storage);
        self.finish_expired(now, storage);
        self.snapshot(now, storage)
    }

    pub fn should_poll_activity(&self) -> bool {
        self.current.as_ref().is_some_and(|session| {
            matches!(
                session.status,
                SessionStatus::Active | SessionStatus::Snoozed
            )
        })
    }

    pub fn has_current(&self) -> bool {
        self.current.is_some()
    }

    pub fn status_window_on_top(&self) -> bool {
        self.settings.status_window_on_top
    }

    pub fn status_window_enabled(&self) -> bool {
        self.settings.status_window_enabled
    }

    pub fn pause(
        &mut self,
        now: Duration,
        storage: Option<&SessionStorage>,
    ) -> SessionSnapshot {
        if let Some(session) = self.current.as_mut() {
            session.pause(now, storage);
        }
        self.snapshot(now, storage)
    }

    pub fn resume(
        &mut self,
        now: Duration,
        storage: Option<&SessionStorage>,
    ) -> SessionSnapshot {
        if let Some(session) = self.current.as_mut() {
            session.resume(now, storage);
        }
        self.snapshot(now, storage)
    }

    pub fn toggle_pause(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        match self.current.as_ref().map(|session| session.status) {
            Some(SessionStatus::Paused) => {
                if let Some(session) = self.current.as_mut() {
                    session.resume(now, storage);
                }
            }
            Some(SessionStatus::Active | SessionStatus::Snoozed) => {
                if let Some(session) = self.current.as_mut() {
                    session.pause(now, storage);
                }
            }
            _ => {}
        }
    }

    pub fn snooze(
        &mut self,
        now: Duration,
        seconds: u64,
        storage: Option<&SessionStorage>,
    ) -> SessionSnapshot {
        if let Some(session) = self.current.as_mut() {
            session.snooze(now, seconds, storage);
        }
        self.snapshot(now, storage)
    }

    pub fn stop(
        &mut self,
        now: Duration,
        ended_at: &str,
        storage: Option<&SessionStorage>,
    ) -> SessionSnapshot {
        self.finish_current(now, "stopped", ended_at, storage);
        self.snapshot(now, storage)
    }

    pub fn mark_current(
        &mut self,
        classification: RuleClassification,
        now: Duration,
        storage: Option<&SessionStorage>,
    ) -> Result<SessionSnapshot, String> {
        if let Some(session) = self.current.as_mut() {
            session.mark_current(classification, now, storage);
        }
        if let Some(storage) = storage {
            if let Some(session) = self.current.as_ref() {
                storage.replace_rules(&session.rules)?;
                self.persisted_rules = session.rules.clone();
            }
        }
        Ok(self.snapshot(now, storage))
    }

    pub fn settings(&self) -> SessionSettings {
        self.settings.clone()
    }

    pub fn update_settings(
        &mut self,
        update: SettingsUpdate,
        storage: Option<&SessionStorage>,
    ) -> Result<SessionSettings, String> {
        if let Some(strictness) = update.default_strictness {
            self.settings.default_strictness = strictness;
        }
        if let Some(enabled) = update.status_window_on_top {
            self.settings.status_window_on_top = enabled;
        }
        if let Some(voice_profile) = update.voice_profile {
            let voice_profile = voice_profile.trim();
            if !voice_profile.is_empty() {
                self.settings.voice_profile = voice_profile.chars().take(120).collect();
            }
        }
        if let Some(storage) = storage {
            storage.save_settings(&self.settings)?;
        }
        Ok(self.settings.clone())
    }

    pub fn set_status_window_on_top(
        &mut self,
        enabled: bool,
        now: Duration,
        storage: Option<&SessionStorage>,
    ) -> Result<SessionSnapshot, String> {
        self.settings.status_window_on_top = enabled;
        if let Some(storage) = storage {
            storage.save_settings(&self.settings)?;
        }
        Ok(self.snapshot(now, storage))
    }

    pub fn set_status_window_enabled(
        &mut self,
        enabled: bool,
        now: Duration,
        storage: Option<&SessionStorage>,
    ) -> Result<SessionSnapshot, String> {
        self.settings.status_window_enabled = enabled;
        if let Some(storage) = storage {
            storage.save_settings(&self.settings)?;
        }
        Ok(self.snapshot(now, storage))
    }

    pub fn history(&self, storage: Option<&SessionStorage>) -> Vec<SessionSummary> {
        if let Some(storage) = storage {
            storage.list_history(20).unwrap_or_default()
        } else {
            self.history.clone()
        }
    }

    pub fn snapshot(&self, now: Duration, storage: Option<&SessionStorage>) -> SessionSnapshot {
        SessionSnapshot {
            status: self
                .current
                .as_ref()
                .map(|session| session.status)
                .unwrap_or(SessionStatus::Idle),
            settings: self.settings.clone(),
            current: self.current.as_ref().map(|session| session.snapshot(now)),
            history: self.history(storage),
        }
    }

    fn finish_current(
        &mut self,
        now: Duration,
        reason: &str,
        ended_at: &str,
        storage: Option<&SessionStorage>,
    ) {
        if let Some(mut session) = self.current.take() {
            session.finish(now, storage);
            if let Some(storage) = storage {
                let _ = storage.close_session(session.id, ended_at, reason);
            } else {
                self.history.insert(0, session.summary(now));
                self.history.truncate(20);
            }
        }
    }

    fn tick_current(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if let Some(session) = self.current.as_mut() {
            session.tick(now, storage);
        }
    }

    fn finish_expired(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self
            .current
            .as_ref()
            .is_some_and(|session| session.is_expired(now))
        {
            let ended_at = storage
                .and_then(|_| utc_now_rfc3339().ok())
                .unwrap_or_else(|| "expired".to_string());
            self.finish_current(now, "completed", &ended_at, storage);
        }
    }
}

#[derive(Debug)]
struct Session {
    id: u64,
    goal: String,
    duration_seconds: u64,
    strictness: Strictness,
    status: SessionStatus,
    started_at_seconds: u64,
    started_at: String,
    snoozed_until_seconds: Option<u64>,
    metrics: SessionMetrics,
    evidence: Vec<EvidenceEvent>,
    app_seconds: HashMap<String, u64>,
    latest_focus: Option<FocusSnapshot>,
    rules: Vec<AppRule>,
    engine: FocusEngine,
    last_accounted_at: Duration,
    observing_supervisor: bool,
    timer_held_seconds: u64,
    timer_hold_started_at_seconds: Option<u64>,
    open_span: Option<OpenSpan>,
    last_span_sample: Option<ActivitySample>,
    last_span_state: Option<FocusState>,
    last_span_classification: Option<RuleClassification>,
}

#[derive(Clone, Debug)]
struct OpenSpan {
    app: String,
    context: Option<String>,
    category: String,
    state: String,
    started_offset_seconds: u64,
}

impl Session {
    fn new(
        id: u64,
        goal: String,
        duration_seconds: u64,
        strictness: Strictness,
        rules: Vec<AppRule>,
        now: Duration,
        started_at: String,
    ) -> Self {
        Self {
            id,
            goal,
            duration_seconds,
            strictness,
            status: SessionStatus::Active,
            started_at_seconds: now.as_secs(),
            started_at,
            snoozed_until_seconds: None,
            metrics: SessionMetrics::default(),
            evidence: Vec::new(),
            app_seconds: HashMap::new(),
            latest_focus: None,
            rules: rules.clone(),
            engine: FocusEngine::new(strictness.focus_config(), rules),
            last_accounted_at: now,
            observing_supervisor: false,
            timer_held_seconds: 0,
            timer_hold_started_at_seconds: None,
            open_span: None,
            last_span_sample: None,
            last_span_state: None,
            last_span_classification: None,
        }
    }

    fn tick(&mut self, now: Duration, _storage: Option<&SessionStorage>) {
        if self.status == SessionStatus::Snoozed
            && self
                .snoozed_until_seconds
                .is_some_and(|until| now.as_secs() >= until)
        {
            self.status = SessionStatus::Active;
            self.snoozed_until_seconds = None;
            self.last_accounted_at = now;
            self.engine = FocusEngine::new(self.strictness.focus_config(), self.rules.clone());
            self.observing_supervisor = false;
            self.push_event(now, "Snooze expired", "WATCHING");
        }
    }

    fn record_sample(
        &mut self,
        sample: ActivitySample,
        now: Duration,
        supervisor: &SupervisorIdentity,
        storage: Option<&SessionStorage>,
    ) {
        self.tick(now, storage);
        if self.status == SessionStatus::Snoozed {
            return;
        }

        if self.status != SessionStatus::Active {
            return;
        }

        if supervisor.matches(&sample) {
            if !self.observing_supervisor {
                self.account_latest(now);
                self.close_open_span(now, storage);
                self.observing_supervisor = true;
            }
            self.last_accounted_at = now;
            return;
        }

        if self.observing_supervisor {
            self.observing_supervisor = false;
            self.last_accounted_at = now;
            self.engine = FocusEngine::new(self.strictness.focus_config(), self.rules.clone());
        }

        let focus = self.engine.record_sample(&sample, now);
        self.account(now, focus.state, &sample);
        self.sync_span(
            now,
            &sample,
            focus.state,
            focus.classification,
            storage,
        );

        if focus.should_nudge {
            self.metrics.nudges += 1;
            self.push_event(now, "Voice nudge queued", "VOICE 01");
            self.persist_event(now, "nudge", Some("voice"), storage);
        }

        self.push_focus_event(now, &focus);
        self.latest_focus = Some(focus);
    }

    fn pause(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.status == SessionStatus::Active {
            if self.observing_supervisor {
                self.last_accounted_at = now;
            } else {
                self.account_latest(now);
            }
            self.start_timer_hold(now);
            self.close_open_span(now, storage);
            self.status = SessionStatus::Paused;
            self.push_event(now, "Session paused", "HOLD");
            self.persist_event(now, "pause", None, storage);
        } else if self.status == SessionStatus::Snoozed {
            self.close_open_span(now, storage);
            self.status = SessionStatus::Paused;
            self.snoozed_until_seconds = None;
            self.last_accounted_at = now;
            self.observing_supervisor = false;
            self.push_event(now, "Session paused", "HOLD");
            self.persist_event(now, "pause", None, storage);
        }
    }

    fn resume(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.status == SessionStatus::Paused {
            self.finish_timer_hold(now);
            self.status = SessionStatus::Active;
            self.last_accounted_at = now;
            self.engine = FocusEngine::new(self.strictness.focus_config(), self.rules.clone());
            self.observing_supervisor = false;
            self.push_event(now, "Session resumed", "LIVE");
            self.persist_event(now, "resume", None, storage);
        } else if self.status == SessionStatus::Snoozed {
            self.status = SessionStatus::Active;
            self.snoozed_until_seconds = None;
            self.last_accounted_at = now;
            self.engine = FocusEngine::new(self.strictness.focus_config(), self.rules.clone());
            self.observing_supervisor = false;
            self.push_event(now, "Snooze cleared", "LIVE");
            self.persist_event(now, "resume", None, storage);
        }
    }

    fn snooze(&mut self, now: Duration, seconds: u64, storage: Option<&SessionStorage>) {
        if matches!(self.status, SessionStatus::Active | SessionStatus::Paused) {
            if self.status == SessionStatus::Paused {
                self.finish_timer_hold(now);
            } else if !self.observing_supervisor {
                self.account_latest(now);
            } else {
                self.last_accounted_at = now;
            }
            self.status = SessionStatus::Snoozed;
            self.snoozed_until_seconds = Some(now.as_secs() + seconds);
            self.last_accounted_at = now;
            self.close_open_span(now, storage);
            self.observing_supervisor = false;
            self.push_event(now, "Snoozed nudges", &format!("{} MIN", seconds / 60));
            self.persist_event(now, "snooze", Some(&seconds.to_string()), storage);
        }
    }

    fn mark_current(
        &mut self,
        classification: RuleClassification,
        now: Duration,
        storage: Option<&SessionStorage>,
    ) {
        let Some(focus) = self.latest_focus.clone() else {
            return;
        };
        if self.status == SessionStatus::Active && !self.observing_supervisor {
            self.account_latest(now);
        } else {
            self.last_accounted_at = now;
        }
        let sample = &focus.sample;
        let rule = sample
            .window_title
            .as_ref()
            .filter(|title| !title.trim().is_empty())
            .map(|title| AppRule::title_contains(title, classification))
            .unwrap_or_else(|| AppRule::app_name(&sample.app_name, classification));

        let marked_rule = rule.clone();
        self.rules.insert(0, rule);
        self.engine.replace_rules(self.rules.clone());
        self.last_accounted_at = now;
        self.latest_focus = Some(marked_focus(
            &focus,
            marked_rule,
            classification,
            self.strictness,
        ));
        self.push_event(
            now,
            match classification {
                RuleClassification::Focused => "Marked relevant",
                RuleClassification::Distracting => "Marked distracting",
                RuleClassification::Neutral => "Marked neutral",
            },
            "RULE SAVED",
        );
        match classification {
            RuleClassification::Focused => {
                self.persist_event(now, "marked_relevant", None, storage);
            }
            RuleClassification::Distracting => {
                self.persist_event(now, "marked_distracting", None, storage);
            }
            RuleClassification::Neutral => {}
        }
    }

    fn finish(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.status == SessionStatus::Active && !self.observing_supervisor {
            self.account_latest(now);
        } else {
            self.last_accounted_at = now;
        }
        self.close_open_span(now, storage);
        self.finish_timer_hold(now);
        self.status = SessionStatus::Complete;
        self.push_event(now, "Session saved", "LOCAL");
    }

    fn session_offset_seconds(&self, now: Duration) -> u64 {
        now.as_secs().saturating_sub(self.started_at_seconds)
    }

    fn sync_span(
        &mut self,
        now: Duration,
        sample: &ActivitySample,
        state: FocusState,
        classification: RuleClassification,
        storage: Option<&SessionStorage>,
    ) {
        let state_key = focus_state_key(state);
        let category = span_category(
            &sample.app_name,
            sample.window_title.as_deref(),
            state_key,
            classification,
        );
        let context = sample
            .window_title
            .as_ref()
            .filter(|title| !title.trim().is_empty())
            .cloned();
        let same = self.open_span.as_ref().is_some_and(|open| {
            open.app == sample.app_name
                && open.context == context
                && open.state == state_key
                && open.category == category
        });
        if same {
            self.last_span_sample = Some(sample.clone());
            self.last_span_state = Some(state);
            self.last_span_classification = Some(classification);
            return;
        }
        self.close_open_span(now, storage);
        let offset = self.session_offset_seconds(now);
        self.open_span = Some(OpenSpan {
            app: sample.app_name.clone(),
            context,
            category: category.to_string(),
            state: state_key.to_string(),
            started_offset_seconds: offset,
        });
        self.last_span_sample = Some(sample.clone());
        self.last_span_state = Some(state);
        self.last_span_classification = Some(classification);
    }

    fn close_open_span(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        let Some(open) = self.open_span.take() else {
            return;
        };
        let end_offset = self.session_offset_seconds(now);
        let seconds = end_offset.saturating_sub(open.started_offset_seconds);
        if let Some(storage) = storage {
            let _ = storage.insert_span(
                self.id,
                &open.app,
                open.context.as_deref(),
                &open.category,
                &open.state,
                open.started_offset_seconds,
                seconds,
            );
        }
    }

    fn persist_event(
        &self,
        now: Duration,
        kind: &str,
        detail: Option<&str>,
        storage: Option<&SessionStorage>,
    ) {
        if let Some(storage) = storage {
            let _ = storage.insert_event(
                self.id,
                self.session_offset_seconds(now),
                kind,
                detail,
            );
        }
    }

    fn snapshot(&self, now: Duration) -> CurrentSession {
        let elapsed_seconds = self.timer_elapsed_seconds(now);
        CurrentSession {
            id: self.id,
            goal: self.goal.clone(),
            duration_seconds: self.duration_seconds,
            elapsed_seconds,
            remaining_seconds: self.duration_seconds.saturating_sub(elapsed_seconds),
            strictness: self.strictness,
            status: self.status,
            snoozed_until_seconds: self.snoozed_until_seconds,
            metrics: self.metrics.clone(),
            evidence: self.evidence.clone(),
            focus: self.latest_focus.clone(),
            rules: self.rules.clone(),
        }
    }

    fn summary(&self, now: Duration) -> SessionSummary {
        SessionSummary {
            id: self.id,
            goal: self.goal.clone(),
            started_at: self.started_at.clone(),
            duration_seconds: self.timer_elapsed_seconds(now),
            focus_seconds: self.metrics.focused_seconds + self.metrics.neutral_seconds,
            distracted_seconds: self.metrics.distracted_seconds,
            idle_seconds: self.metrics.idle_seconds,
            nudges: self.metrics.nudges,
            top_hints: top_hints(&self.app_seconds),
        }
    }

    fn is_expired(&self, now: Duration) -> bool {
        self.raw_timer_elapsed_seconds(now) >= self.duration_seconds
    }

    fn account_latest(&mut self, now: Duration) {
        if let Some(focus) = self.latest_focus.clone() {
            self.account(now, focus.state, &focus.sample);
        } else {
            self.last_accounted_at = now;
        }
    }

    fn account(&mut self, now: Duration, state: FocusState, sample: &ActivitySample) {
        let delta = now.saturating_sub(self.last_accounted_at).as_secs();
        if delta == 0 {
            return;
        }

        match state {
            FocusState::Focused => self.metrics.focused_seconds += delta,
            FocusState::Neutral => self.metrics.neutral_seconds += delta,
            FocusState::Suspicious => self.metrics.suspicious_seconds += delta,
            FocusState::Distracted => self.metrics.distracted_seconds += delta,
            FocusState::Idle => self.metrics.idle_seconds += delta,
        }

        *self.app_seconds.entry(sample_hint(sample)).or_insert(0) += delta;
        self.last_accounted_at = now;
    }

    fn push_focus_event(&mut self, now: Duration, focus: &FocusSnapshot) {
        let label = focus_event_label(focus);
        let value = focus_event_value(focus);

        if self
            .evidence
            .first()
            .is_some_and(|event| event.label == label && event.value == value)
        {
            return;
        }

        self.push_event(now, &label, &value);
    }

    fn push_event(&mut self, now: Duration, label: &str, value: &str) {
        self.evidence.insert(
            0,
            EvidenceEvent {
                at_seconds: now.as_secs().saturating_sub(self.started_at_seconds),
                label: label.to_string(),
                value: value.to_string(),
            },
        );
        self.evidence.truncate(8);
    }

    fn start_timer_hold(&mut self, now: Duration) {
        if self.timer_hold_started_at_seconds.is_none() {
            self.timer_hold_started_at_seconds = Some(now.as_secs());
        }
    }

    fn finish_timer_hold(&mut self, now: Duration) {
        if let Some(started_at) = self.timer_hold_started_at_seconds.take() {
            self.timer_held_seconds += now.as_secs().saturating_sub(started_at);
        }
    }

    fn timer_elapsed_seconds(&self, now: Duration) -> u64 {
        self.raw_timer_elapsed_seconds(now)
            .min(self.duration_seconds)
    }

    fn raw_timer_elapsed_seconds(&self, now: Duration) -> u64 {
        let current_hold_seconds = self
            .timer_hold_started_at_seconds
            .map(|started_at| now.as_secs().saturating_sub(started_at))
            .unwrap_or(0);

        now.as_secs()
            .saturating_sub(self.started_at_seconds)
            .saturating_sub(self.timer_held_seconds + current_hold_seconds)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct StartSessionInput {
    pub goal: String,
    pub duration_minutes: u64,
    pub strictness: Option<Strictness>,
    pub rules: Option<Vec<AppRule>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SettingsUpdate {
    pub default_strictness: Option<Strictness>,
    pub status_window_on_top: Option<bool>,
    pub voice_profile: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SessionSettings {
    pub default_strictness: Strictness,
    pub voice_profile: String,
    pub status_window_enabled: bool,
    pub status_window_on_top: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            default_strictness: Strictness::Normal,
            voice_profile: "Starter voice".to_string(),
            status_window_enabled: true,
            status_window_on_top: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Strictness {
    Gentle,
    Normal,
    Strict,
}

impl Strictness {
    fn focus_config(self) -> FocusConfig {
        match self {
            Strictness::Gentle => FocusConfig {
                idle_after_seconds: 90,
                distracted_after_seconds: 15,
                nudge_after_seconds: 90,
                nudge_cooldown_seconds: 180,
            },
            Strictness::Normal => FocusConfig::default(),
            Strictness::Strict => FocusConfig {
                idle_after_seconds: 45,
                distracted_after_seconds: 5,
                nudge_after_seconds: 30,
                nudge_cooldown_seconds: 90,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Active,
    Paused,
    Snoozed,
    Complete,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SessionSnapshot {
    pub status: SessionStatus,
    pub settings: SessionSettings,
    pub current: Option<CurrentSession>,
    pub history: Vec<SessionSummary>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct CurrentSession {
    pub id: u64,
    pub goal: String,
    pub duration_seconds: u64,
    pub elapsed_seconds: u64,
    pub remaining_seconds: u64,
    pub strictness: Strictness,
    pub status: SessionStatus,
    pub snoozed_until_seconds: Option<u64>,
    pub metrics: SessionMetrics,
    pub evidence: Vec<EvidenceEvent>,
    pub focus: Option<FocusSnapshot>,
    pub rules: Vec<AppRule>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SessionMetrics {
    pub focused_seconds: u64,
    pub neutral_seconds: u64,
    pub suspicious_seconds: u64,
    pub distracted_seconds: u64,
    pub idle_seconds: u64,
    pub nudges: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct EvidenceEvent {
    pub at_seconds: u64,
    pub label: String,
    pub value: String,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct SessionSummary {
    pub id: u64,
    pub goal: String,
    pub started_at: String,
    pub duration_seconds: u64,
    pub focus_seconds: u64,
    pub distracted_seconds: u64,
    pub idle_seconds: u64,
    pub nudges: u64,
    pub top_hints: Vec<String>,
}

fn clean_goal(goal: String) -> String {
    let goal = goal.trim();
    if goal.is_empty() {
        "Do the work".to_string()
    } else {
        goal.chars().take(120).collect()
    }
}

fn clean_rules(rules: Vec<AppRule>) -> Vec<AppRule> {
    rules
        .into_iter()
        .filter_map(|mut rule| {
            let value = rule.value.trim();
            if value.is_empty() {
                return None;
            }
            rule.value = value.chars().take(160).collect();
            Some(rule)
        })
        .take(100)
        .collect()
}

fn sample_hint(sample: &ActivitySample) -> String {
    sample
        .window_title
        .as_ref()
        .filter(|title| !title.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| sample.app_name.clone())
}

fn focus_event_label(focus: &FocusSnapshot) -> String {
    let app = focus.sample.app_name.trim();
    let title = focus
        .sample
        .window_title
        .as_deref()
        .map(str::trim)
        .filter(|title| !title.is_empty());

    match title {
        Some(title) if !title.eq_ignore_ascii_case(app) => {
            format!("{} / {}", focus.sample.app_name, title)
        }
        _ => focus.sample.app_name.clone(),
    }
}

fn focus_event_value(focus: &FocusSnapshot) -> String {
    format!(
        "{} / {} / {}",
        state_label(focus.state),
        rule_hint(focus.matched_rule.as_ref()),
        nudge_hint(focus)
    )
}

fn state_label(state: FocusState) -> &'static str {
    match state {
        FocusState::Focused => "FOCUSED",
        FocusState::Neutral => "NEUTRAL",
        FocusState::Suspicious => "SUSPICIOUS",
        FocusState::Distracted => "DISTRACTED",
        FocusState::Idle => "IDLE",
    }
}

fn rule_hint(rule: Option<&AppRule>) -> String {
    rule.map(|rule| format!("{} {}", rule_target_label(&rule.target), rule.value))
        .unwrap_or_else(|| "NO RULE".to_string())
}

fn rule_target_label(target: &RuleTarget) -> &'static str {
    match target {
        RuleTarget::AppName => "APP",
        RuleTarget::ProcessPath => "PATH",
        RuleTarget::WindowTitle => "TITLE",
    }
}

fn nudge_hint(focus: &FocusSnapshot) -> &'static str {
    if focus.should_nudge {
        "NUDGE NOW"
    } else if focus.next_nudge_in_seconds.is_some() {
        "NUDGE ARMED"
    } else {
        "NO NUDGE"
    }
}

fn top_hints(app_seconds: &HashMap<String, u64>) -> Vec<String> {
    let mut pairs: Vec<_> = app_seconds.iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
    pairs
        .into_iter()
        .take(3)
        .map(|(hint, _)| hint.clone())
        .collect()
}

fn paths_match(left: &str, right: &str) -> bool {
    !left.trim().is_empty() && left.eq_ignore_ascii_case(right)
}

fn utc_now_rfc3339() -> Result<String, String> {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| error.to_string())
}

fn marked_focus(
    focus: &FocusSnapshot,
    matched_rule: AppRule,
    classification: RuleClassification,
    strictness: Strictness,
) -> FocusSnapshot {
    let state = marked_focus_state(
        classification,
        focus.seconds_on_current_window,
        strictness.focus_config(),
    );
    let sample = focus.sample.clone();

    FocusSnapshot {
        matched_rule: Some(matched_rule),
        sample,
        state,
        classification,
        seconds_on_current_window: focus.seconds_on_current_window,
        should_nudge: false,
        next_nudge_in_seconds: None,
    }
}

fn marked_focus_state(
    classification: RuleClassification,
    seconds_on_window: u64,
    config: FocusConfig,
) -> FocusState {
    match classification {
        RuleClassification::Focused => FocusState::Focused,
        RuleClassification::Neutral => FocusState::Neutral,
        RuleClassification::Distracting if seconds_on_window < config.distracted_after_seconds => {
            FocusState::Suspicious
        }
        RuleClassification::Distracting => FocusState::Distracted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{Bounds, CapturePermissions, PermissionState};

    const NO_STORAGE: Option<&SessionStorage> = None;

    #[test]
    fn start_pause_resume_stop_creates_history() {
        let mut manager = SessionManager::default();
        manager
            .start(
                input("Write tests", Strictness::Normal),
                seconds(0),
                "start".to_string(),
                NO_STORAGE,
            )
            .unwrap();
        record(&mut manager, sample("Code", "Supervisor", 0.0), seconds(1));
        manager.pause(seconds(11), NO_STORAGE);
        manager.resume(seconds(20), NO_STORAGE);
        record(&mut manager, sample("Code", "Supervisor", 0.0), seconds(30));
        let snapshot = manager.stop(seconds(40), "end", NO_STORAGE);

        assert_eq!(snapshot.status, SessionStatus::Idle);
        assert_eq!(snapshot.history.len(), 1);
        assert_eq!(snapshot.history[0].goal, "Write tests");
        assert!(snapshot.history[0].focus_seconds > 0);
    }

    #[test]
    fn starting_session_pins_status_window_again() {
        let mut manager = SessionManager::default();
        manager
            .set_status_window_on_top(false, seconds(0), NO_STORAGE)
            .unwrap();
        manager
            .start(
                input("Write tests", Strictness::Normal),
                seconds(1),
                "start".to_string(),
                NO_STORAGE,
            )
            .unwrap();

        assert!(manager.snapshot(seconds(1), NO_STORAGE).settings.status_window_on_top);
    }

    #[test]
    fn side_window_enabled_setting_is_independent_from_pinning() {
        let mut manager = SessionManager::default();
        manager
            .set_status_window_enabled(false, seconds(0), NO_STORAGE)
            .unwrap();
        manager
            .set_status_window_on_top(false, seconds(0), NO_STORAGE)
            .unwrap();
        manager
            .start(
                input("Write tests", Strictness::Normal),
                seconds(1),
                "start".to_string(),
                NO_STORAGE,
            )
            .unwrap();
        let settings = manager.snapshot(seconds(1), NO_STORAGE).settings;

        assert!(!settings.status_window_enabled);
        assert!(settings.status_window_on_top);
    }

    #[test]
    fn session_start_uses_provided_rules() {
        let mut manager = SessionManager::default();
        manager.start(
            StartSessionInput {
                goal: "Read docs".to_string(),
                duration_minutes: 25,
                strictness: Some(Strictness::Strict),
                rules: Some(vec![AppRule::title_contains(
                    "developer.chrome.com",
                    RuleClassification::Focused,
                )]),
            },
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();

        let snapshot = record(
            &mut manager,
            sample("Google Chrome", "developer.chrome.com / Tabs API", 0.0),
            seconds(10),
        );
        let focus = snapshot.current.unwrap().focus.unwrap();

        assert_eq!(focus.state, FocusState::Focused);
        assert_eq!(focus.matched_rule.unwrap().value, "developer.chrome.com");
    }

    #[test]
    fn snooze_prevents_nudges_until_expiry() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        manager.snooze(seconds(1), 300, NO_STORAGE);

        let snoozed = record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(40));
        assert_eq!(snoozed.status, SessionStatus::Snoozed);
        assert!(!snoozed.current.unwrap().focus.unwrap().should_nudge);

        let resumed = record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(301));
        assert_eq!(resumed.status, SessionStatus::Active);
        assert!(!resumed.current.unwrap().focus.unwrap().should_nudge);
    }

    #[test]
    fn idle_does_not_accumulate_distracted_time() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        let snapshot = record(
            &mut manager,
            sample("Chrome", "YouTube", 120.0),
            seconds(60),
        );
        let metrics = snapshot.current.unwrap().metrics;

        assert_eq!(metrics.distracted_seconds, 0);
        assert_eq!(metrics.idle_seconds, 60);
    }

    #[test]
    fn supervisor_owned_samples_do_not_record_activity() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        let supervisor = identity();
        let snapshot = manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(20),
            &supervisor,
            NO_STORAGE,
        );
        let current = snapshot.current.unwrap();

        assert_eq!(metric_total(&current.metrics), 0);
        assert_eq!(current.metrics.nudges, 0);
        assert!(current.evidence.is_empty());
        assert!(current.focus.is_none());
    }

    #[test]
    fn supervisor_foreground_freezes_after_accounting_previous_window() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(10));
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(20),
            &supervisor,
            NO_STORAGE,
        );
        let snapshot = manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(60),
            &supervisor,
            NO_STORAGE,
        );

        assert_eq!(metric_total(&snapshot.current.unwrap().metrics), 20);
    }

    #[test]
    fn timer_advances_while_supervisor_foreground_without_accounting_it() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(10));
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(20),
            &supervisor,
            NO_STORAGE,
        );

        let current = manager.tick(seconds(60), NO_STORAGE).current.unwrap();

        assert_eq!(current.elapsed_seconds, 60);
        assert_eq!(current.remaining_seconds, 24 * 60);
        assert_eq!(metric_total(&current.metrics), 20);
    }

    #[test]
    fn timer_holds_while_paused_but_not_snoozed() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        manager.pause(seconds(10), NO_STORAGE);

        assert_eq!(
            manager.tick(seconds(70), NO_STORAGE).current.unwrap().elapsed_seconds,
            10
        );

        manager.resume(seconds(70), NO_STORAGE);
        assert_eq!(
            manager.tick(seconds(80), NO_STORAGE).current.unwrap().elapsed_seconds,
            20
        );

        manager.snooze(seconds(80), 300, NO_STORAGE);
        assert_eq!(
            manager.tick(seconds(200), NO_STORAGE).current.unwrap().elapsed_seconds,
            140
        );
    }

    #[test]
    fn clock_expiry_finishes_session_even_on_supervisor_window() {
        let mut manager = SessionManager::default();
        manager.start(
            input_minutes("Focus", Strictness::Strict, 5),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "Docs", 0.0), seconds(0));
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(1),
            &supervisor,
            NO_STORAGE,
        );

        let snapshot = manager.tick(seconds(300), NO_STORAGE);

        assert_eq!(snapshot.status, SessionStatus::Idle);
        assert_eq!(snapshot.history.len(), 1);
        assert_eq!(snapshot.history[0].duration_seconds, 300);
    }

    #[test]
    fn paused_then_snoozed_time_is_not_accounted() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(6));
        manager.pause(seconds(10), NO_STORAGE);
        manager.snooze(seconds(70), 300, NO_STORAGE);
        let snapshot = manager.stop(seconds(80), "end", NO_STORAGE);

        assert_eq!(snapshot.history[0].distracted_seconds, 10);
        assert_eq!(snapshot.history[0].duration_seconds, 20);
    }

    #[test]
    fn app_switch_does_not_nudge_immediately_inside_session() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Normal),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        assert!(
            !manager
                .record_sample(sample("Chrome", "YouTube", 0.0), seconds(0), &identity(), NO_STORAGE)
                .current
                .unwrap()
                .focus
                .unwrap()
                .should_nudge
        );
        assert!(
            !manager
                .record_sample(sample("Chrome", "YouTube", 0.0), seconds(59), &identity(), NO_STORAGE)
                .current
                .unwrap()
                .focus
                .unwrap()
                .should_nudge
        );
        assert!(
            manager
                .record_sample(sample("Chrome", "YouTube", 0.0), seconds(60), &identity(), NO_STORAGE)
                .current
                .unwrap()
                .focus
                .unwrap()
                .should_nudge
        );
    }

    #[test]
    fn mark_current_relevant_wins_over_starter_rules() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(
            &mut manager,
            sample("Chrome", "YouTube API docs", 0.0),
            seconds(0),
        );
        let marked = manager.mark_current(RuleClassification::Focused, seconds(1), NO_STORAGE).unwrap();
        assert_eq!(
            marked.current.unwrap().focus.unwrap().state,
            FocusState::Focused
        );

        let snapshot = record(
            &mut manager,
            sample("Chrome", "YouTube API docs", 0.0),
            seconds(40),
        );

        assert_eq!(
            snapshot.current.unwrap().focus.unwrap().state,
            FocusState::Focused
        );
    }

    #[test]
    fn mark_current_distracting_refreshes_cached_focus_immediately() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Notes", "Plan", 0.0), seconds(0));
        record(&mut manager, sample("Notes", "Plan", 0.0), seconds(10));

        let snapshot = manager.mark_current(RuleClassification::Distracting, seconds(11), NO_STORAGE).unwrap();
        let focus = snapshot.current.unwrap().focus.unwrap();

        assert_eq!(focus.state, FocusState::Distracted);
        assert_eq!(focus.classification, RuleClassification::Distracting);
        assert_eq!(focus.matched_rule.unwrap().value, "Plan");
    }

    #[test]
    fn mark_current_uses_latest_external_window_after_supervisor_focus() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(
            &mut manager,
            sample("Chrome", "YouTube API docs", 0.0),
            seconds(0),
        );
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(1),
            &supervisor,
            NO_STORAGE,
        );
        manager.mark_current(RuleClassification::Focused, seconds(2), NO_STORAGE).unwrap();
        let snapshot = record(
            &mut manager,
            sample("Chrome", "YouTube API docs", 0.0),
            seconds(3),
        );

        assert_eq!(
            snapshot.current.unwrap().focus.unwrap().state,
            FocusState::Focused
        );
    }

    #[test]
    fn snapshot_reads_do_not_advance_metrics() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(10));
        let first = manager.snapshot(seconds(10), NO_STORAGE);
        let second = manager.snapshot(seconds(20), NO_STORAGE);

        assert_eq!(
            first.current.unwrap().metrics,
            second.current.unwrap().metrics
        );
    }

    #[test]
    fn evidence_records_decision_metadata() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        let snapshot = record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(10));
        let current = snapshot.current.unwrap();
        let event = current.evidence.first().unwrap();

        assert_eq!(event.label, "Chrome / YouTube");
        assert!(event.value.contains("DISTRACTED"));
        assert!(event.value.contains("TITLE YouTube"));
        assert!(event.value.contains("NUDGE ARMED"));
    }

    #[test]
    fn stop_summary_includes_top_hints_and_nudges() {
        let mut manager = SessionManager::default();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
            "start".to_string(),
            NO_STORAGE,
        )
        .unwrap();
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(0));
        record(&mut manager, sample("Chrome", "YouTube", 0.0), seconds(30));
        let snapshot = manager.stop(seconds(31), "end", NO_STORAGE);

        assert_eq!(snapshot.history[0].top_hints, vec!["YouTube"]);
        assert_eq!(snapshot.history[0].nudges, 1);
    }

    fn input(goal: &str, strictness: Strictness) -> StartSessionInput {
        input_minutes(goal, strictness, 25)
    }

    fn input_minutes(
        goal: &str,
        strictness: Strictness,
        duration_minutes: u64,
    ) -> StartSessionInput {
        StartSessionInput {
            goal: goal.to_string(),
            duration_minutes,
            strictness: Some(strictness),
            rules: None,
        }
    }

    fn record(
        manager: &mut SessionManager,
        sample: ActivitySample,
        now: Duration,
    ) -> SessionSnapshot {
        manager.record_sample(sample, now, &identity(), NO_STORAGE)
    }

    fn identity() -> SupervisorIdentity {
        SupervisorIdentity {
            process_id: 7,
            process_path: "/Applications/Supervisor.app".to_string(),
            app_name: "Supervisor".to_string(),
        }
    }

    fn sample(app_name: &str, title: &str, idle_seconds: f64) -> ActivitySample {
        sample_with_pid(app_name, title, 42, idle_seconds)
    }

    fn sample_with_pid(
        app_name: &str,
        title: &str,
        process_id: u64,
        idle_seconds: f64,
    ) -> ActivitySample {
        ActivitySample {
            captured_at: "2026-08-12T00:00:00Z".to_string(),
            platform: "macos",
            app_name: app_name.to_string(),
            process_path: format!("/Applications/{app_name}.app"),
            process_id,
            window_title: Some(title.to_string()),
            window_id: title.to_string(),
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

    fn seconds(value: u64) -> Duration {
        Duration::from_secs(value)
    }

    fn metric_total(metrics: &SessionMetrics) -> u64 {
        metrics.focused_seconds
            + metrics.neutral_seconds
            + metrics.suspicious_seconds
            + metrics.distracted_seconds
            + metrics.idle_seconds
    }
}
