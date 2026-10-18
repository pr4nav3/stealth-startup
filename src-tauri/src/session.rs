use crate::activity::ActivitySample;
use crate::attribution::{Activity, ActivityKind, Resolver, Source};
use crate::focus::{
    starter_rules, AppRule, FocusConfig, FocusEngine, FocusSnapshot, FocusState,
    RuleClassification, RuleTarget,
};
use crate::storage::{
    focus_state_from_key, focus_state_key, open_or_reset, HistoryWeek, PeriodRecap, SessionReport,
    SessionStorage, StorageNotice, TrendsData,
};
use crate::website;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

pub const HISTORY_CACHE_LIMIT: usize = 20;
/// The ticker runs every second. A longer silence means the machine slept or froze.
pub const LOST_TIME_AFTER: Duration = Duration::from_secs(15);
/// The worker already smooths frames (k-of-n), so the phone counts from its first report. It
/// ends once the worker has not seen it for this long, so a dropped frame doesn't split it.
const PHONE_CLEAR_AFTER: Duration = Duration::from_secs(2);
/// No frame for this long means the camera is not watching (worker stopped or camera lost).
const CAMERA_STALE_AFTER: Duration = Duration::from_secs(3);
/// Nobody in view, and no input, for this long is Away (dated back to when they left).
pub const CAMERA_AWAY_AFTER: Duration = Duration::from_secs(60);
/// A span state of its own, apart from the focus states: the phone outranks the window.
const PHONE_STATE: &str = "phone";

pub struct SessionManager {
    settings: SessionSettings,
    persisted_rules: Vec<AppRule>,
    current: Option<Session>,
    history: Vec<SessionSummary>,
    /// `None` means history is off for this run; `startup_notice` says why.
    storage: Option<SessionStorage>,
    startup_notice: Option<StorageNotice>,
    /// The latest mark that rescored past sessions, for the main window to announce.
    rescore: Option<RescoreNotice>,
}

impl SessionManager {
    pub fn open(database_path: Result<PathBuf, String>) -> Self {
        let (opened, startup_notice) = open_or_reset(database_path);
        match opened {
            Some((storage, state)) => Self {
                settings: state.settings,
                persisted_rules: state.rules,
                current: None,
                history: state.history,
                storage: Some(storage),
                startup_notice,
                rescore: None,
            },
            None => Self::without_storage(startup_notice),
        }
    }

    fn without_storage(startup_notice: Option<StorageNotice>) -> Self {
        Self {
            settings: SessionSettings::default(),
            persisted_rules: starter_rules(),
            current: None,
            history: Vec::new(),
            storage: None,
            startup_notice,
            rescore: None,
        }
    }

    #[cfg(test)]
    fn with_storage(storage: SessionStorage) -> Self {
        Self {
            storage: Some(storage),
            ..Self::without_storage(None)
        }
    }

    pub fn history(&self) -> Vec<SessionSummary> {
        self.history.clone()
    }

    pub fn session_report(&self, id: u64) -> Result<SessionReport, String> {
        self.storage()?.get_session_report(id)
    }

    pub fn trends(&self) -> Result<TrendsData, String> {
        self.storage()?.get_trends()
    }

    pub fn history_week(&self, week_start: String) -> Result<HistoryWeek, String> {
        self.storage()?.get_history_week(&week_start)
    }

    pub fn period_recap(&self, period: String, start: String) -> Result<PeriodRecap, String> {
        self.storage()?.get_period_recap(&period, &start)
    }

    pub fn delete_session(&mut self, id: u64) -> Result<(), String> {
        self.storage()?.delete_session(id)?;
        self.refresh_history();
        Ok(())
    }

    pub fn history_bytes(&self) -> Result<u64, String> {
        self.storage()?.history_bytes()
    }

    pub fn delete_all_history(&mut self) -> Result<(), String> {
        self.storage()?.delete_all_history()?;
        self.refresh_history();
        Ok(())
    }

    /// Clears the one-time "started a fresh database" card. Other notices stay until fixed.
    pub fn dismiss_storage_notice(&mut self, now: Duration) -> SessionSnapshot {
        if matches!(self.startup_notice, Some(StorageNotice::Reset { .. })) {
            self.startup_notice = None;
        }
        self.snapshot(now)
    }

    fn storage(&self) -> Result<&SessionStorage, String> {
        self.storage
            .as_ref()
            .ok_or_else(|| "History is off: Supervisor could not open its storage.".to_string())
    }

    fn storage_notice(&self) -> Option<StorageNotice> {
        match self.storage.as_ref().and_then(SessionStorage::failure) {
            Some(reason) => Some(StorageNotice::Failed { reason }),
            None => self.startup_notice.clone(),
        }
    }

    fn refresh_history(&mut self) {
        let Some(storage) = self.storage.as_ref() else {
            return;
        };
        match storage.list_history(HISTORY_CACHE_LIMIT) {
            Ok(history) => self.history = history,
            Err(error) => storage.record_failure("read history", error),
        }
    }

    fn save_settings(&self) {
        save(self.storage.as_ref(), "save settings", |storage| {
            storage.save_settings(&self.settings)
        });
    }

    /// Returns the past sessions whose scores the save changed.
    fn save_rules(&mut self, rules: Vec<AppRule>) -> Vec<u64> {
        let mut rescored = Vec::new();
        save(self.storage.as_ref(), "save rules", |storage| {
            rescored = storage.replace_rules(&rules)?;
            Ok(())
        });
        self.persisted_rules = rules;
        if !rescored.is_empty() {
            self.refresh_history();
        }
        rescored
    }

    pub fn rules(&self) -> Vec<AppRule> {
        keep_current_rules(self.persisted_rules.clone())
    }

    pub fn update_rules(&mut self, rules: Vec<AppRule>) -> RulesSaved {
        let rules = clean_rules(rules);
        let rescored_sessions = self.save_rules(rules.clone());
        RulesSaved { rules, rescored_sessions }
    }

    pub fn site_names(&self) -> HashMap<String, String> {
        match self.storage.as_ref() {
            Some(storage) => storage.site_names(),
            None => website::preset_names()
                .map(|(key, label)| (key.to_string(), label.to_string()))
                .collect(),
        }
    }

    pub fn set_site_name(&mut self, key: &str, name: &str) -> Result<HashMap<String, String>, String> {
        self.storage()?.set_site_name(key, name)?;
        self.refresh_history();
        Ok(self.site_names())
    }

    pub fn app_names(&self) -> Result<HashMap<String, String>, String> {
        self.storage()?.app_names()
    }

    pub fn set_app_name(&mut self, key: &str, name: &str) -> Result<HashMap<String, String>, String> {
        self.storage()?.set_app_name(key, name)?;
        self.refresh_history();
        self.app_names()
    }

    pub fn load_phrase_library(&self) -> Result<crate::storage::StoredPhraseLibrary, String> {
        self.storage()?.load_phrase_library()
    }

    pub fn save_phrase_library(
        &mut self,
        library: crate::storage::StoredPhraseLibrary,
    ) -> Result<crate::storage::StoredPhraseLibrary, String> {
        self.storage()?.save_phrase_library(library)
    }
}

#[derive(Clone, Debug)]
pub struct SupervisorIdentity {
    pub process_id: u64,
    pub process_path: String,
}

impl SupervisorIdentity {
    pub fn matches(&self, sample: &ActivitySample) -> bool {
        sample.process_id == self.process_id || paths_match(&sample.process_path, &self.process_path)
    }
}

impl SessionManager {
    pub fn start(&mut self, input: StartSessionInput, now: Duration) {
        // A duplicate Start command must not orphan the session already being recorded.
        if self.current.is_some() {
            return;
        }
        let strictness = input.strictness.unwrap_or(self.settings.default_strictness);
        let duration_seconds = input.duration_minutes.clamp(5, 180) * 60;
        let goal = clean_goal(input.goal);
        let rules = clean_rules(
            input
                .rules
                .unwrap_or_else(|| self.persisted_rules.clone()),
        );
        let started_at = utc_now();
        // Id 0 marks a session that is not in the database (history off or the insert failed).
        let session_id = match self.storage.as_ref() {
            Some(storage) => storage
                .insert_session(&goal, strictness, &started_at, duration_seconds)
                .unwrap_or_else(|error| {
                    storage.record_failure("start session", error);
                    0
                }),
            None => 0,
        };
        self.current = Some(Session::new(
            session_id,
            goal,
            duration_seconds,
            strictness,
            rules.clone(),
            now,
        ));
        self.save_rules(rules);
        self.settings.status_window_on_top = true;
        self.save_settings();
    }

    pub fn record_sample(
        &mut self,
        sample: ActivitySample,
        now: Duration,
        supervisor: &SupervisorIdentity,
    ) -> SessionSnapshot {
        self.tick_current(now);
        self.finish_expired(now);
        if let Some(session) = self.current.as_mut() {
            session.record_sample(sample, now, supervisor, self.storage.as_ref());
        }
        self.tick_current(now);
        self.finish_expired(now);
        self.snapshot(now)
    }

    /// The ticker went silent from `from` to `now` (sleep or a freeze): nobody observed it.
    pub fn record_lost_time(&mut self, from: Duration, now: Duration) {
        if let Some(session) = self.current.as_mut() {
            if session.is_recording() && now > from {
                session.record_lost_time(from, now, self.storage.as_ref());
            }
        }
    }

    pub fn record_capture_gap(&mut self, now: Duration) {
        if let Some(session) = self.current.as_mut() {
            session.record_capture_gap(now, self.storage.as_ref());
        }
    }

    pub fn tick(&mut self, now: Duration) -> SessionSnapshot {
        self.tick_current(now);
        self.finish_expired(now);
        self.snapshot(now)
    }

    /// One webcam reading (two a second, five while a phone may be in view). Ignored unless the
    /// user turned the webcam on.
    pub fn record_webcam(&mut self, phone: bool, present: bool, now: Duration) {
        if !self.settings.webcam_enabled {
            return;
        }
        if let Some(session) = self.current.as_mut() {
            session.record_webcam(phone, present, now, self.storage.as_ref());
        }
    }

    /// The webcam runs while a session records (Active or Muted) and the user turned it on.
    pub fn wants_webcam(&self) -> bool {
        self.settings.webcam_enabled && self.should_poll_activity()
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

    pub fn pause(&mut self, now: Duration) -> SessionSnapshot {
        if let Some(session) = self.current.as_mut() {
            session.pause(now, self.storage.as_ref());
        }
        self.snapshot(now)
    }

    pub fn resume(&mut self, now: Duration) -> SessionSnapshot {
        if let Some(session) = self.current.as_mut() {
            session.resume(now, self.storage.as_ref());
        }
        self.snapshot(now)
    }

    pub fn toggle_pause(&mut self, now: Duration) {
        let storage = self.storage.as_ref();
        let Some(session) = self.current.as_mut() else {
            return;
        };
        match session.status {
            SessionStatus::Paused => session.resume(now, storage),
            SessionStatus::Active | SessionStatus::Snoozed => session.pause(now, storage),
            _ => {}
        }
    }

    pub fn snooze(&mut self, now: Duration, seconds: u64) -> SessionSnapshot {
        if let Some(session) = self.current.as_mut() {
            session.snooze(now, seconds, self.storage.as_ref());
        }
        self.snapshot(now)
    }

    pub fn update_current_rules(&mut self, rules: Vec<AppRule>, now: Duration) -> SessionSnapshot {
        let rules = clean_rules(rules);
        if let Some(session) = self.current.as_mut() {
            session.replace_rules(rules, now, self.storage.as_ref());
        }
        self.snapshot(now)
    }

    pub fn stop(&mut self, now: Duration) -> SessionSnapshot {
        self.finish_current(now, "stopped");
        self.snapshot(now)
    }

    pub fn mark_current(&mut self, classification: RuleClassification, now: Duration) -> SessionSnapshot {
        let Some(session) = self.current.as_mut() else {
            return self.snapshot(now);
        };
        session.mark_current(classification, now, self.storage.as_ref());
        let rules = session.rules.clone();
        let rescored = self.save_rules(rules);
        if !rescored.is_empty() {
            let seq = self.rescore.as_ref().map_or(0, |notice| notice.seq) + 1;
            self.rescore = Some(RescoreNotice { seq, sessions: rescored.len() as u64 });
        }
        self.snapshot(now)
    }

    pub fn settings(&self) -> SessionSettings {
        self.settings.clone()
    }

    pub fn update_settings(&mut self, update: SettingsUpdate) -> SessionSettings {
        if let Some(strictness) = update.default_strictness {
            self.settings.default_strictness = strictness;
        }
        if let Some(enabled) = update.status_window_on_top {
            self.settings.status_window_on_top = enabled;
        }
        if let Some(enabled) = update.webcam_gpu {
            self.settings.webcam_gpu = enabled;
        }
        if let Some(enabled) = update.webcam_enabled {
            self.settings.webcam_enabled = enabled;
        }
        if let Some(voice_profile) = update.voice_profile {
            let voice_profile = voice_profile.trim();
            if !voice_profile.is_empty() {
                self.settings.voice_profile = voice_profile.chars().take(120).collect();
            }
        }
        self.save_settings();
        self.settings.clone()
    }

    pub fn set_status_window_on_top(&mut self, enabled: bool, now: Duration) -> SessionSnapshot {
        self.settings.status_window_on_top = enabled;
        self.save_settings();
        self.snapshot(now)
    }

    pub fn set_status_window_enabled(&mut self, enabled: bool, now: Duration) -> SessionSnapshot {
        self.settings.status_window_enabled = enabled;
        self.save_settings();
        self.snapshot(now)
    }

    pub fn snapshot(&self, now: Duration) -> SessionSnapshot {
        SessionSnapshot {
            status: self
                .current
                .as_ref()
                .map(|session| session.status)
                .unwrap_or(SessionStatus::Idle),
            settings: self.settings.clone(),
            current: self.current.as_ref().map(|session| session.snapshot(now)),
            history: self.history.clone(),
            storage_notice: self.storage_notice(),
            rescore: self.rescore.clone(),
        }
    }

    fn finish_current(&mut self, now: Duration, reason: &str) {
        let Some(mut session) = self.current.take() else {
            return;
        };
        // A delayed tick (for example after sleep) must not credit time beyond the timer.
        let now = if session.is_expired(now) {
            Duration::from_secs(session.started_at_seconds + session.timer_held_seconds + session.duration_seconds)
        } else {
            now
        };
        session.finish(now, self.storage.as_ref());
        if session.id != 0 {
            let ended_at = utc_now();
            let elapsed_seconds = session.timer_elapsed_seconds(now);
            save(self.storage.as_ref(), "close session", |storage| {
                storage.close_session(session.id, &ended_at, reason, elapsed_seconds)
            });
        }
        self.refresh_history();
    }

    fn tick_current(&mut self, now: Duration) {
        if let Some(session) = self.current.as_mut() {
            session.tick(now, self.storage.as_ref());
        }
    }

    fn finish_expired(&mut self, now: Duration) {
        if self
            .current
            .as_ref()
            .is_some_and(|session| session.is_expired(now))
        {
            self.finish_current(now, "completed");
        }
    }
}

/// Runs a write against storage when history is on; a failure is recorded for the UI, never
/// allowed to stop the session.
fn save(
    storage: Option<&SessionStorage>,
    what: &str,
    write: impl FnOnce(&SessionStorage) -> Result<(), String>,
) {
    if let Some(storage) = storage {
        if let Err(error) = write(storage) {
            storage.record_failure(what, error);
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
    snoozed_until_seconds: Option<u64>,
    metrics: SessionMetrics,
    evidence: Vec<EvidenceEvent>,
    latest_focus: Option<FocusSnapshot>,
    rules: Vec<AppRule>,
    engine: FocusEngine,
    last_accounted_at: Duration,
    /// The state of Supervisor's own window while it is in front (Neutral or Idle).
    supervisor_state: Option<FocusState>,
    timer_held_seconds: u64,
    timer_hold_started_at_seconds: Option<u64>,
    open_span: Option<OpenSpan>,
    last_window_sample: Option<ActivitySample>,
    resolver: Resolver,
    last_span_checkpoint: Duration,
    /// Session offset where the last span ended. Every span starts here, so the stored
    /// timeline tiles the session with no holes.
    recorded_until: u64,
    /// The last span written, so a short unclaimed tail can be given back to it.
    last_closed: Option<OpenSpan>,
    /// Last time media playing in front, or the webcam, showed the user was there; Away never
    /// starts earlier.
    present_at: Duration,
    /// When window reads started failing, while they still are.
    capture_failing_since: Option<Duration>,
    camera: Camera,
}

/// What the webcam last said. Only flags and times: frames never reach the session.
#[derive(Debug, Default)]
struct Camera {
    last_frame_at: Option<Duration>,
    present: bool,
    absent_since: Option<Duration>,
    /// While a phone is in view: when it was first seen. Its span is the open span.
    phone_since: Option<Duration>,
    phone_seen_at: Duration,
}

#[derive(Clone, Debug)]
struct OpenSpan {
    activity: Activity,
    /// Lets an address read correct a title guess made in the same window.
    window_id: String,
    window_title: Option<String>,
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
    ) -> Self {
        Self {
            id,
            goal,
            duration_seconds,
            strictness,
            status: SessionStatus::Active,
            started_at_seconds: now.as_secs(),
            snoozed_until_seconds: None,
            metrics: SessionMetrics::default(),
            evidence: Vec::new(),
            latest_focus: None,
            rules: rules.clone(),
            engine: FocusEngine::new(strictness.focus_config(), rules),
            last_accounted_at: now,
            supervisor_state: None,
            timer_held_seconds: 0,
            timer_hold_started_at_seconds: None,
            open_span: None,
            last_window_sample: None,
            resolver: Resolver::default(),
            last_span_checkpoint: now,
            recorded_until: 0,
            last_closed: None,
            present_at: now,
            capture_failing_since: None,
            camera: Camera::default(),
        }
    }

    fn tick(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.status == SessionStatus::Paused {
            self.checkpoint_open_span(now, storage);
        }
        if self.is_recording() {
            self.update_phone(now, storage);
        }
        if self.status == SessionStatus::Snoozed
            && self
                .snoozed_until_seconds
                .is_some_and(|until| now.as_secs() >= until)
        {
            self.unmute();
            self.push_event(now, "Nudges unmuted", "WATCHING");
        }
    }

    /// Muted sessions record exactly like active ones; only nudges stop.
    fn is_recording(&self) -> bool {
        matches!(self.status, SessionStatus::Active | SessionStatus::Snoozed)
    }

    fn fresh_engine(&self) -> FocusEngine {
        let mut engine = FocusEngine::new(self.strictness.focus_config(), self.rules.clone());
        engine.set_muted(self.status == SessionStatus::Snoozed);
        engine
    }

    fn unmute(&mut self) {
        self.status = SessionStatus::Active;
        self.snoozed_until_seconds = None;
        self.engine.set_muted(false);
    }

    fn record_sample(
        &mut self,
        sample: ActivitySample,
        now: Duration,
        supervisor: &SupervisorIdentity,
        storage: Option<&SessionStorage>,
    ) {
        self.capture_failing_since = None;
        self.tick(now, storage);
        if !self.is_recording() {
            return;
        }

        // Alt-Tab, Start, Search, taskbar, and desktop stand in for the window the user came from.
        let sample = if sample.shell_surface {
            let Some(previous) = self.last_window_sample.clone() else {
                return;
            };
            ActivitySample {
                captured_at: sample.captured_at,
                idle_seconds: sample.idle_seconds,
                // Whatever played earlier may have stopped; don't keep an idle user present on it.
                media_playing: false,
                ..previous
            }
        } else {
            self.last_window_sample = Some(sample.clone());
            sample
        };

        // A phone in hand outranks the window in front until it is put down.
        if self.phone_active() {
            self.account_phone(now);
            self.checkpoint_open_span(now, storage);
            return;
        }
        let camera_away = self.camera_away(&sample, now);
        self.engine.set_camera(self.camera_sees_someone(now), camera_away);

        // Supervisor's own window is never judged by rules and is not focus: Neutral, or Away
        // after the same stretch without input as any other window.
        if supervisor.matches(&sample) {
            let state = if self.engine.is_away(&sample) { FocusState::Idle } else { FocusState::Neutral };
            let span_at = if state == FocusState::Idle {
                self.backdate_away(&sample, now)
            } else {
                now
            };
            if self.open_span_is_capture_gap() {
                self.account_open_gap(now);
            } else if state == FocusState::Idle {
                self.account(now, FocusState::Idle);
            } else {
                self.account_current(now);
            }
            self.supervisor_state = Some(state);
            // A glance at Supervisor pauses the site's clock; it doesn't restart it.
            self.engine.hold(now);
            self.sync_span(span_at, "", None, &Activity::supervisor(), state, storage);
            self.checkpoint_open_span(now, storage);
            return;
        }

        if let Some(state) = self.supervisor_state.take() {
            self.account(now, state);
        }

        let mut sample = sample;
        if let Some(storage) = storage {
            if sample.app_identity.is_empty() {
                sample.app_identity =
                    crate::app_identity::fallback_identity(&sample.process_path, &sample.app_name);
            }
            sample.app_key = Some(storage.app_key(&sample.app_identity, &sample.app_name, &sample.app_label));
        }
        let activity = self.resolver.resolve(&sample, now);
        let mut display_activity = activity.clone();
        if let (ActivityKind::Website, Some(storage)) = (activity.kind, storage) {
            display_activity.label = storage.site_label(&activity.key);
        }
        if let (ActivityKind::App, Some(storage)) = (activity.kind, storage) {
            display_activity.label = storage.app_label(&activity.key, &activity.label);
        }
        if sample.media_playing {
            self.present_at = now;
        }
        let focus = self.engine.record_sample(&sample, &display_activity, now);
        let span_at = if focus.state == FocusState::Idle {
            self.backdate_away(&sample, now)
        } else {
            now
        };
        self.account_elapsed(now, focus.state, &activity);
        self.sync_span(
            span_at,
            &sample.window_id,
            sample.window_title.as_deref(),
            &activity,
            focus.state,
            storage,
        );

        if focus.should_nudge {
            self.metrics.nudges += 1;
            self.push_event(now, "Voice nudge queued", "VOICE 01");
            self.persist_event(now, "nudge", Some("voice"), storage);
        }

        self.push_focus_event(now, &focus);
        self.latest_focus = Some(focus);
        self.checkpoint_open_span(now, storage);
    }

    fn pause(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if !self.is_recording() {
            return;
        }
        self.account_current(now);
        self.start_timer_hold(now);
        self.close_open_span(now, storage);
        self.open_marker_span(Activity::paused(), "paused");
        self.camera = Camera::default();
        self.unmute();
        self.status = SessionStatus::Paused;
        self.push_event(now, "Session paused", "HOLD");
        self.persist_event(now, "pause", None, storage);
    }

    fn resume(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.status == SessionStatus::Paused {
            self.close_open_span(now, storage);
            self.finish_timer_hold(now);
            self.status = SessionStatus::Active;
            self.last_accounted_at = now;
            self.engine = self.fresh_engine();
            self.supervisor_state = None;
            self.push_event(now, "Session resumed", "LIVE");
            self.persist_event(now, "resume", None, storage);
        } else if self.status == SessionStatus::Snoozed {
            self.unmute();
            self.push_event(now, "Nudges unmuted", "LIVE");
            self.persist_event(now, "unsnooze", None, storage);
        }
    }

    /// Mute is not a break: time keeps being recorded and judged. Pause is the break.
    fn snooze(&mut self, now: Duration, seconds: u64, storage: Option<&SessionStorage>) {
        if !self.is_recording() {
            return;
        }
        self.status = SessionStatus::Snoozed;
        self.snoozed_until_seconds = Some(now.as_secs() + seconds);
        self.engine.set_muted(true);
        if let Some(focus) = self.latest_focus.as_mut() {
            focus.should_nudge = false;
            focus.next_nudge_in_seconds = None;
        }
        self.push_event(now, "Nudges muted", &format!("{} MIN", seconds / 60));
        self.persist_event(now, "snooze", Some(&seconds.to_string()), storage);
    }

    fn replace_rules(&mut self, rules: Vec<AppRule>, now: Duration, storage: Option<&SessionStorage>) {
        if self.is_recording() {
            self.account_current(now);
            if self.supervisor_state.is_none() && !self.phone_active() {
                self.close_open_span(now, storage);
            }
        }
        self.rules = rules.clone();
        self.engine.replace_rules(rules);
        self.latest_focus = None;
        self.last_accounted_at = now;
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
        if !focus.markable {
            return;
        }
        let rule = match focus.activity.kind {
            ActivityKind::Website => AppRule::website(&focus.activity.key, classification),
            _ => AppRule::app(&focus.activity.app_id, classification),
        };
        if self.is_recording() {
            self.account_current(now);
        } else {
            self.last_accounted_at = now;
        }

        let marked_rule = rule.clone();
        // A phone span stays a phone span; the rule applies once it is put down.
        let on_phone = self.phone_active();
        let recorded_activity = self.open_span.as_ref().map(|span| span.activity.clone());
        if !on_phone {
            self.close_open_span(now, storage);
        }
        self.rules.insert(0, rule);
        self.rules = keep_current_rules(std::mem::take(&mut self.rules));
        self.engine.replace_rules(self.rules.clone());
        self.last_accounted_at = now;
        self.latest_focus = Some(marked_focus(
            &focus,
            marked_rule,
            classification,
            self.strictness,
        ));
        if self.is_recording() && self.supervisor_state.is_none() && !on_phone {
            if let (Some(activity), Some(marked)) = (recorded_activity, self.latest_focus.as_ref()) {
                self.sync_span(now, &focus.sample.window_id,
                    focus.sample.window_title.as_deref(), &activity, marked.state, storage);
            }
        }
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
        if self.open_span_is_capture_gap() {
            self.account_open_gap(now);
        } else if self.is_recording() {
            self.account_current(now);
        } else {
            self.last_accounted_at = now;
        }
        self.close_open_span(now, storage);
        self.claim_tail(now, storage);
        self.finish_timer_hold(now);
        self.status = SessionStatus::Complete;
        self.push_event(now, "Session saved", "LOCAL");
    }

    fn session_offset_seconds(&self, now: Duration) -> u64 {
        now.as_secs().saturating_sub(self.started_at_seconds)
    }

    /// A read that fails during a switch (Alt-Tab to a minimised window, a virtual desktop
    /// change) stays with the window before, like any switch. Only a failure that lasts
    /// `LOST_TIME_AFTER` becomes lost time, dated from when it began.
    fn record_capture_gap(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if !self.is_recording() || now <= self.last_accounted_at {
            return;
        }
        // The phone span doesn't need the window.
        if self.phone_active() {
            self.capture_failing_since = None;
            return;
        }
        let since = *self.capture_failing_since.get_or_insert(self.last_accounted_at);
        if now.saturating_sub(since) >= LOST_TIME_AFTER {
            self.record_lost_time(since, now, storage);
        }
    }

    /// Time from `from` to `now` that nobody observed: a failed capture, sleep, or a freeze.
    fn record_lost_time(&mut self, from: Duration, now: Duration, storage: Option<&SessionStorage>) {
        if !self.open_span_is_capture_gap() {
            if self.last_accounted_at < from {
                self.account_current(from);
            }
            self.close_open_span(from, storage);
            self.open_marker_span(Activity::unknown(), "unknown");
            self.last_accounted_at = self.last_accounted_at.max(from);
            self.camera = Camera::default();
        }
        self.account_open_gap(now);
        self.checkpoint_open_span(now, storage);
    }

    fn open_marker_span(&mut self, activity: Activity, state: &str) {
        self.open_span = Some(OpenSpan {
            activity,
            window_id: String::new(),
            window_title: None,
            state: state.to_string(),
            started_offset_seconds: self.recorded_until,
        });
    }

    /// Nothing may end the session unrecorded. A short unclaimed tail (a rule edit just before
    /// Stop) belongs to the span before it; anything else is lost time.
    fn claim_tail(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        let end = self.session_offset_seconds(now);
        if self.recorded_until >= end {
            return;
        }
        let gap = end - self.recorded_until;
        match self.last_closed.clone() {
            Some(span) if gap <= LOST_TIME_AFTER.as_secs() => self.open_span = Some(span),
            _ => self.open_marker_span(Activity::unknown(), "unknown"),
        }
        self.close_open_span(now, storage);
    }

    fn sync_span(
        &mut self,
        close_at: Duration,
        window_id: &str,
        window_title: Option<&str>,
        activity: &Activity,
        state: FocusState,
        storage: Option<&SessionStorage>,
    ) {
        let state_key = focus_state_key(state);
        if let Some(open) = self.open_span.as_mut() {
            // The address arrived for the page a title guessed at: the guess gives way.
            if open.activity.source == Source::Title
                && activity.source == Source::Address
                && open.window_id == window_id
                && open.window_title.as_deref() == window_title
            {
                open.activity = activity.clone();
            }
            if open.activity.same_place(activity) && open.state == state_key {
                return;
            }
        }
        self.close_open_span(close_at, storage);
        self.open_span = Some(OpenSpan {
            activity: activity.clone(),
            window_id: window_id.to_string(),
            window_title: window_title.map(str::to_string),
            state: state_key.to_string(),
            started_offset_seconds: self.recorded_until,
        });
    }

    fn close_open_span(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        let Some(open) = self.open_span.take() else {
            return;
        };
        let end_offset = self.session_offset_seconds(now).max(open.started_offset_seconds);
        let seconds = end_offset - open.started_offset_seconds;
        self.recorded_until = self.recorded_until.max(end_offset);
        if seconds > 0 {
            self.last_closed = Some(open.clone());
        }
        if self.id == 0 {
            return;
        }
        save(storage, "save span", |storage| {
            storage.checkpoint_span(
                self.id,
                &open.activity,
                &open.state,
                open.started_offset_seconds,
                seconds,
            )
        });
    }

    fn checkpoint_open_span(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.id == 0 || now.saturating_sub(self.last_span_checkpoint) < Duration::from_secs(15) {
            return;
        }
        let Some(open) = self.open_span.as_ref() else { return };
        let seconds = self.session_offset_seconds(now).saturating_sub(open.started_offset_seconds);
        save(storage, "checkpoint span", |storage| {
            storage.checkpoint_span(self.id, &open.activity, &open.state,
                open.started_offset_seconds, seconds)
        });
        self.last_span_checkpoint = now;
    }

    fn persist_event(
        &self,
        now: Duration,
        kind: &str,
        detail: Option<&str>,
        storage: Option<&SessionStorage>,
    ) {
        if self.id == 0 {
            return;
        }
        save(storage, "save event", |storage| {
            storage.insert_event(self.id, self.session_offset_seconds(now), kind, detail)
        });
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
            camera: self.camera_status(now),
        }
    }

    fn is_expired(&self, now: Duration) -> bool {
        self.raw_timer_elapsed_seconds(now) >= self.duration_seconds
    }

    /// Credits time up to `now` to whatever was in front (or the phone in hand).
    fn account_current(&mut self, now: Duration) {
        if self.phone_active() {
            self.account_phone(now);
            return;
        }
        match self.supervisor_state {
            Some(state) => self.account(now, state),
            None => self.account_latest(now),
        }
    }

    fn account_latest(&mut self, now: Duration) {
        if let Some(focus) = self.latest_focus.clone() {
            self.account(now, focus.state);
        } else {
            self.last_accounted_at = now;
        }
    }

    fn account(&mut self, now: Duration, state: FocusState) {
        let delta = now.as_secs().saturating_sub(self.last_accounted_at.as_secs());
        if delta == 0 {
            return;
        }
        self.add_seconds(state, delta);
        self.last_accounted_at = now;
    }

    fn account_elapsed(&mut self, now: Duration, new_state: FocusState, _new_activity: &Activity) {
        if self.open_span_is_capture_gap() {
            self.account_open_gap(now);
            return;
        }
        let delta = now.as_secs().saturating_sub(self.last_accounted_at.as_secs());
        if delta == 0 {
            return;
        }
        if new_state == FocusState::Idle {
            self.add_seconds(FocusState::Idle, delta);
            self.last_accounted_at = now;
            return;
        }
        if let Some(previous) = self.latest_focus.clone() {
            self.add_seconds(previous.state, delta);
            self.last_accounted_at = now;
            return;
        }
        // No observation yet: do not claim the startup interval belonged to this app.
        self.last_accounted_at = now;
    }

    /// Phone time is distracted time, and also counted on its own.
    fn account_phone(&mut self, now: Duration) {
        let delta = now.as_secs().saturating_sub(self.last_accounted_at.as_secs());
        if delta == 0 {
            return;
        }
        self.metrics.distracted_seconds += delta;
        self.metrics.phone_seconds += delta;
        self.last_accounted_at = now;
    }

    fn account_open_gap(&mut self, now: Duration) {
        let delta = now.as_secs().saturating_sub(self.last_accounted_at.as_secs());
        if delta == 0 {
            return;
        }
        self.add_seconds(FocusState::Idle, delta);
        self.last_accounted_at = now;
    }

    fn open_span_is_capture_gap(&self) -> bool {
        self.open_span.as_ref().is_some_and(|span| span.state == "unknown")
    }

    /// Away starts at the last input, not when the threshold noticed it: the minutes already
    /// credited to the window in front go back to Away. Only within the current span, and
    /// never before media playing in front last showed the user was there. Returns where the
    /// Away span begins.
    fn backdate_away(&mut self, sample: &ActivitySample, now: Duration) -> Duration {
        let boundary = self.last_accounted_at;
        let Some(span) = self.open_span.as_ref() else {
            return boundary;
        };
        // Already away, or a pause or lost-time marker: nothing was credited to give back.
        let Some(state) = focus_state_from_key(&span.state).filter(|state| *state != FocusState::Idle) else {
            return boundary;
        };
        let span_start = Duration::from_secs(self.started_at_seconds + span.started_offset_seconds);
        let last_input = now.saturating_sub(Duration::from_secs_f64(sample.idle_seconds.unwrap_or(0.0)));
        let left_at = Duration::from_secs(last_input.max(span_start).max(self.present_at).as_secs());
        // The site's nudge clock stops at the same moment.
        self.engine.hold(left_at);
        let reclaimed = boundary.as_secs().saturating_sub(left_at.as_secs());
        if reclaimed == 0 {
            return boundary;
        }
        let credited = self.metric_mut(state);
        *credited = credited.saturating_sub(reclaimed);
        self.add_seconds(FocusState::Idle, reclaimed);
        left_at
    }

    fn add_seconds(&mut self, state: FocusState, delta: u64) {
        *self.metric_mut(state) += delta;
    }

    fn metric_mut(&mut self, state: FocusState) -> &mut u64 {
        match state {
            FocusState::Focused => &mut self.metrics.focused_seconds,
            FocusState::Neutral => &mut self.metrics.neutral_seconds,
            FocusState::Suspicious => &mut self.metrics.suspicious_seconds,
            FocusState::Distracted => &mut self.metrics.distracted_seconds,
            FocusState::Idle => &mut self.metrics.idle_seconds,
        }
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

    fn record_webcam(&mut self, phone: bool, present: bool, now: Duration, storage: Option<&SessionStorage>) {
        if !self.is_recording() {
            return;
        }
        // Frames and ticks take the lock in either order; never step back in time.
        let now = now.max(self.last_accounted_at);
        self.camera.last_frame_at = Some(now);
        self.camera.present = present || phone;
        if self.camera.present {
            self.camera.absent_since = None;
            self.present_at = self.present_at.max(now);
        } else {
            self.camera.absent_since.get_or_insert(now);
        }
        if phone {
            self.camera.phone_seen_at = now;
            if self.camera.phone_since.is_none() {
                self.start_phone(now, storage);
            }
        }
        self.update_phone(now, storage);
    }

    fn phone_active(&self) -> bool {
        self.camera.phone_since.is_some()
    }

    fn camera_live(&self, now: Duration) -> bool {
        self.camera
            .last_frame_at
            .is_some_and(|at| now.saturating_sub(at) <= CAMERA_STALE_AFTER)
    }

    fn camera_sees_someone(&self, now: Duration) -> bool {
        self.camera_live(now) && self.camera.present
    }

    /// Nobody in view for `CAMERA_AWAY_AFTER`, and no input since they left. Input always wins:
    /// someone typing is there even if the camera misses them.
    fn camera_away(&self, sample: &ActivitySample, now: Duration) -> bool {
        let Some(since) = self.camera.absent_since.filter(|_| self.camera_live(now)) else {
            return false;
        };
        let gone = now.saturating_sub(since);
        gone >= CAMERA_AWAY_AFTER && sample.idle_seconds.unwrap_or(0.0) >= gone.as_secs_f64()
    }

    /// The phone gets a span of its own from the first frame that saw it.
    fn start_phone(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        if self.open_span_is_capture_gap() {
            self.account_open_gap(now);
        } else {
            self.account_current(now);
        }
        self.supervisor_state = None;
        self.capture_failing_since = None;
        // Phone time doesn't count toward the site in front.
        self.engine.hold(now);
        self.close_open_span(now, storage);
        self.open_marker_span(Activity::phone(), PHONE_STATE);
        self.camera.phone_since = Some(now);
        self.push_event(now, "Phone in view", "WEBCAM");
    }

    /// Ends the phone once no frame has seen it for a moment (put down, or the camera
    /// stopped), else nudges when due: after the strictness grace, never while muted, and on
    /// the cooldown every nudge shares.
    fn update_phone(&mut self, now: Duration, storage: Option<&SessionStorage>) {
        let Some(since) = self.camera.phone_since else {
            return;
        };
        self.account_phone(now);
        if now.saturating_sub(self.camera.phone_seen_at) >= PHONE_CLEAR_AFTER {
            self.close_open_span(now, storage);
            self.camera.phone_since = None;
            self.push_event(now, "Phone put down", "WEBCAM");
            return;
        }
        if now.saturating_sub(since).as_secs() >= self.phone_grace_seconds() && self.engine.nudge_ready(now) {
            self.engine.note_nudge(now);
            self.metrics.nudges += 1;
            self.push_event(now, "Phone nudge queued", "VOICE 01");
            self.persist_event(now, "nudge", Some("phone"), storage);
        }
    }

    /// A phone in hand is a distraction with no doubt about it, so it skips the long wait a
    /// site gets: it is distracted, and nudged, after the strictness's Slipping grace.
    fn phone_grace_seconds(&self) -> u64 {
        self.strictness.focus_config().distracted_after_seconds
    }

    fn camera_status(&self, now: Duration) -> CameraStatus {
        let phone_seconds = self
            .camera
            .phone_since
            .map_or(0, |since| now.saturating_sub(since).as_secs());
        CameraStatus {
            live: self.camera_live(now),
            present: self.camera_sees_someone(now),
            phone: self.phone_active(),
            phone_seconds,
            phone_distracted: self.phone_active() && phone_seconds >= self.phone_grace_seconds(),
        }
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
    #[serde(default)]
    pub webcam_gpu: Option<bool>,
    #[serde(default)]
    pub webcam_enabled: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SessionSettings {
    pub default_strictness: Strictness,
    pub voice_profile: String,
    pub status_window_enabled: bool,
    pub status_window_on_top: bool,
    pub webcam_gpu: bool,
    /// Runs the webcam during sessions (phone and presence). Off until the user turns it on.
    pub webcam_enabled: bool,
}

impl Default for SessionSettings {
    fn default() -> Self {
        Self {
            default_strictness: Strictness::Normal,
            voice_profile: "Starter voice".to_string(),
            status_window_enabled: true,
            status_window_on_top: true,
            webcam_gpu: false,
            webcam_enabled: false,
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
                distracted_after_seconds: 15,
                nudge_after_seconds: 90,
                nudge_cooldown_seconds: 180,
            },
            Strictness::Normal => FocusConfig::default(),
            Strictness::Strict => FocusConfig {
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
    pub storage_notice: Option<StorageNotice>,
    pub rescore: Option<RescoreNotice>,
}

/// A mark during a session rescored past sessions. `seq` lets a window announce it once.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RescoreNotice {
    pub seq: u64,
    pub sessions: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RulesSaved {
    pub rules: Vec<AppRule>,
    /// Past sessions whose scores changed, for the "re-scored" notice.
    pub rescored_sessions: Vec<u64>,
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
    pub camera: CameraStatus,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct CameraStatus {
    /// Frames are arriving.
    pub live: bool,
    pub present: bool,
    pub phone: bool,
    /// How long this phone has been in view.
    pub phone_seconds: u64,
    /// Past the grace: the popup shows and the phone is nudged when due.
    pub phone_distracted: bool,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct SessionMetrics {
    pub focused_seconds: u64,
    pub neutral_seconds: u64,
    pub suspicious_seconds: u64,
    pub distracted_seconds: u64,
    pub idle_seconds: u64,
    /// Part of `distracted_seconds`.
    pub phone_seconds: u64,
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
    /// Relevant time only.
    pub focus_seconds: u64,
    pub neutral_seconds: u64,
    pub unrated_seconds: u64,
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
    keep_current_rules(
        rules
            .into_iter()
            .filter_map(|mut rule| {
                let value = rule.value.trim();
                if value.is_empty() {
                    return None;
                }
                rule.value = value.chars().take(160).collect();
                if rule.target == RuleTarget::Website {
                    rule.value = rule.value.to_lowercase();
                }
                Some(rule)
            })
            .collect(),
    )
    .into_iter()
    .take(100)
    .collect()
}

/// The first rule per target wins; rules naming a browser are dropped (browsers are Neutral).
fn keep_current_rules(rules: Vec<AppRule>) -> Vec<AppRule> {
    let mut seen = std::collections::HashSet::new();
    rules
        .into_iter()
        .filter(|rule| !rule.names_a_browser())
        .filter(|rule| seen.insert((rule.target, rule.value.trim().to_lowercase())))
        .collect()
}

fn focus_event_label(focus: &FocusSnapshot) -> String {
    focus.activity.label.clone()
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
        RuleTarget::App => "APP",
        RuleTarget::Website => "SITE",
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

fn paths_match(left: &str, right: &str) -> bool {
    !left.trim().is_empty() && left.eq_ignore_ascii_case(right)
}

pub fn utc_now() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .expect("RFC 3339 formatting only fails for years outside 0000-9999")
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
    FocusSnapshot {
        matched_rule: Some(matched_rule),
        sample: focus.sample.clone(),
        activity: focus.activity.clone(),
        markable: focus.markable,
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

    fn test_manager() -> SessionManager {
        SessionManager::with_storage(SessionStorage::open_in_memory().expect("open db"))
    }

    #[test]
    fn start_pause_resume_stop_creates_history() {
        let mut manager = test_manager();
        manager
            .start(
                input("Write tests", Strictness::Normal),
                seconds(0),
            );
        record(&mut manager, sample("Code", "Supervisor", 0.0), seconds(1));
        manager.pause(seconds(11));
        manager.resume(seconds(20));
        record(&mut manager, sample("Code", "Supervisor", 0.0), seconds(30));
        let snapshot = manager.stop(seconds(40));

        assert_eq!(snapshot.status, SessionStatus::Idle);
        assert_eq!(snapshot.history.len(), 1);
        assert_eq!(snapshot.history[0].goal, "Write tests");
        assert!(snapshot.history[0].unrated_seconds > 0);
    }

    #[test]
    fn marking_announces_only_past_sessions_it_rescored() {
        let mut manager = test_manager();
        manager.start(input("Past", Strictness::Normal), seconds(0));
        record(&mut manager, sample("Notion", "Plan", 0.0), seconds(0));
        record(&mut manager, sample("Notion", "Plan", 0.0), seconds(30));
        manager.stop(seconds(60));

        manager.start(input("Now", Strictness::Normal), seconds(100));
        record(&mut manager, sample("Figma", "Plan", 0.0), seconds(100));
        // Figma has no past time, so nothing in history changes.
        assert_eq!(manager.mark_current(RuleClassification::Focused, seconds(110)).rescore, None);
        record(&mut manager, sample("Notion", "Plan", 0.0), seconds(120));
        let marked = manager.mark_current(RuleClassification::Focused, seconds(130));
        assert_eq!(marked.rescore, Some(RescoreNotice { seq: 1, sessions: 1 }));
        assert_eq!(marked.history[0].focus_seconds, 60);
    }

    #[test]
    fn coming_back_to_the_same_site_resumes_instead_of_restarting() {
        let mut manager = test_manager();
        manager.start(input("Back", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        for t in 0..=20 {
            record(&mut manager, site("youtube.com", 0.0), seconds(t));
        }
        // A 3s glance at Supervisor, then a break from 30s: neither counts toward YouTube.
        manager.record_sample(sample_with_pid("Supervisor", "Supervisor", 7, 0.0), seconds(20), &identity());
        let back = record(&mut manager, site("youtube.com", 0.0), seconds(23)).current.unwrap().focus.unwrap();
        assert_eq!(back.state, FocusState::Distracted);
        record(&mut manager, site("youtube.com", 0.0), seconds(30));
        record(&mut manager, site("youtube.com", 180.0), seconds(210));
        let resumed = record(&mut manager, site("youtube.com", 0.0), seconds(220)).current.unwrap().focus.unwrap();
        assert_eq!(resumed.state, FocusState::Distracted);
        // 27s on YouTube so far (0-20, 23-30), so the 60s nudge is 33s away, not 60.
        assert_eq!(resumed.next_nudge_in_seconds, Some(33));

        manager.stop(seconds(225));
        let slipping = manager.session_report(id).unwrap().spans.iter()
            .filter(|span| span.state == "suspicious").count();
        assert_eq!(slipping, 1);
    }

    #[test]
    fn duplicate_start_keeps_the_running_session() {
        let mut manager = test_manager();
        manager.start(input("Original", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        manager.start(input("Replacement", Strictness::Strict), seconds(10));
        let current = manager.snapshot(seconds(10)).current.unwrap();
        assert_eq!(current.id, id);
        assert_eq!(current.goal, "Original");
    }

    #[test]
    fn a_delayed_expiry_cannot_record_more_time_than_the_session_timer() {
        let mut manager = test_manager();
        manager.start(input_minutes("Deadline", Strictness::Normal, 5), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        manager.tick(seconds(1000));
        let report = manager.session_report(id).unwrap();
        assert_eq!(report.duration_seconds, 300);
        assert_eq!(report.unrated_seconds, 300);
    }

    #[test]
    fn marking_an_app_changes_the_saved_state_at_the_mark_time() {
        let mut manager = test_manager();
        manager.start(input("Mark", Strictness::Normal), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(10));
        manager.mark_current(RuleClassification::Distracting, seconds(12));
        let id = manager.snapshot(seconds(12)).current.unwrap().id;
        manager.stop(seconds(20));
        let report = manager.session_report(id).unwrap();
        let bounds: Vec<_> = report.spans.iter().map(|span| (span.started_offset_seconds, span.seconds)).collect();
        assert_eq!(bounds, vec![(0, 12), (12, 8)]);
        // The rating also reaches the unrated time before the mark.
        assert_eq!(report.distracted_seconds, 20);
    }

    #[test]
    fn app_and_website_rules_with_the_same_text_are_distinct() {
        let rules = clean_rules(vec![
            AppRule::app("example.com", RuleClassification::Focused),
            AppRule::website("example.com", RuleClassification::Distracting),
        ]);
        assert_eq!(rules.len(), 2);
    }

    #[test]
    fn checkpoints_recover_a_long_open_span_without_duplicate_rows() {
        let mut manager = test_manager();
        manager.start(input("Checkpoint", Strictness::Normal), seconds(0));
        for at in [0, 15, 30, 44] {
            record(&mut manager, sample("Code", "Plan", 0.0), seconds(at));
        }
        let id = manager.snapshot(seconds(44)).current.unwrap().id;
        // Simulate the recovery path without finishing the in-memory session.
        manager.storage.as_ref().unwrap().recover_orphans().unwrap();
        let recovered = manager.session_report(id).unwrap();
        assert_eq!(recovered.spans.len(), 1);
        assert_eq!(recovered.unrated_seconds, 30);
        assert_eq!(recovered.ended_reason.as_deref(), Some("abandoned"));
        manager.stop(seconds(45));
        let finished = manager.session_report(id).unwrap();
        assert_eq!(finished.spans.len(), 1);
        assert_eq!(finished.spans[0].seconds, 45);
    }

    #[test]
    fn fractional_ticks_and_capture_recovery_agree_with_saved_time() {
        let mut manager = test_manager();
        manager.start(input("Timing", Strictness::Normal), seconds(0));
        for millis in [0, 1100, 2200, 3300, 4400, 5500, 6600, 7700, 8800, 9900, 11000] {
            record(&mut manager, sample("Code", "Plan", 0.0), Duration::from_millis(millis));
        }
        // Reads fail from 11s; at 30s that has lasted long enough to be lost time.
        manager.record_capture_gap(seconds(30));
        let snapshot = record(&mut manager, sample("Code", "Plan", 0.0), seconds(35));
        let current = snapshot.current.unwrap();
        assert_eq!(current.metrics.neutral_seconds, 11);
        assert_eq!(current.metrics.idle_seconds, 24);
        manager.stop(seconds(35));
        let report = manager.session_report(current.id).unwrap();
        assert_eq!(report.unrated_seconds, 11);
        assert_eq!(report.idle_seconds, 24);
    }

    #[test]
    fn a_failed_read_during_alt_tab_stays_with_the_window_before() {
        let mut manager = test_manager();
        manager.start(input("Switch", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        for t in 0..=10 {
            record(&mut manager, sample("Code", "Plan", 0.0), seconds(t));
        }
        // A minimised window in front for a moment while switching.
        manager.record_capture_gap(seconds(11));
        manager.record_capture_gap(seconds(12));
        record(&mut manager, sample("Notes", "Todo", 0.0), seconds(13));
        manager.stop(seconds(20));

        let report = manager.session_report(id).unwrap();
        let bounds: Vec<_> = report.spans.iter()
            .map(|span| (span.started_offset_seconds, span.seconds, span.activity_key.as_str()))
            .collect();
        assert_eq!(bounds, vec![(0, 13, "code"), (13, 7, "notes")]);
        assert_eq!(report.idle_seconds, 0);
    }

    #[test]
    fn changing_tabs_does_not_rewrite_a_previous_title_guess() {
        let mut manager = test_manager();
        manager.start(input("Tabs", Strictness::Normal), seconds(0));
        let unread = sample("chrome", "Issue - GitHub", 0.0);
        record(&mut manager, unread.clone(), seconds(0));
        record(&mut manager, unread, seconds(1));
        let id = manager.snapshot(seconds(1)).current.unwrap().id;
        record(&mut manager, site("example.com", 0.0), seconds(10));
        manager.stop(seconds(20));
        let report = manager.session_report(id).unwrap();
        assert!(report.spans.iter().any(|span| span.activity_key == "github.com" && span.seconds == 9));
    }

    #[test]
    fn starting_session_pins_status_window_again() {
        let mut manager = test_manager();
        manager
            .set_status_window_on_top(false, seconds(0));
        manager
            .start(
                input("Write tests", Strictness::Normal),
                seconds(1),
            );

        assert!(manager.snapshot(seconds(1)).settings.status_window_on_top);
    }

    #[test]
    fn side_window_enabled_setting_is_independent_from_pinning() {
        let mut manager = test_manager();
        manager
            .set_status_window_enabled(false, seconds(0));
        manager
            .set_status_window_on_top(false, seconds(0));
        manager
            .start(
                input("Write tests", Strictness::Normal),
                seconds(1),
            );
        let settings = manager.snapshot(seconds(1)).settings;

        assert!(!settings.status_window_enabled);
        assert!(settings.status_window_on_top);
    }

    #[test]
    fn session_start_uses_provided_rules() {
        let mut manager = test_manager();
        manager.start(
            StartSessionInput {
                goal: "Read docs".to_string(),
                duration_minutes: 25,
                strictness: Some(Strictness::Strict),
                rules: Some(vec![AppRule::website("chrome.com", RuleClassification::Focused)]),
            },
            seconds(0),
        );

        let snapshot = record(
            &mut manager,
            site("https://developer.chrome.com/docs/extensions", 0.0),
            seconds(10),
        );
        let focus = snapshot.current.unwrap().focus.unwrap();

        assert_eq!(focus.state, FocusState::Focused);
        assert_eq!(focus.matched_rule.unwrap().value, "chrome.com");
    }

    #[test]
    fn snooze_prevents_nudges_until_expiry() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        manager.snooze(seconds(1), 300);

        let snoozed = record(&mut manager, site("youtube.com", 0.0), seconds(40));
        assert_eq!(snoozed.status, SessionStatus::Snoozed);
        let current = snoozed.current.unwrap();
        assert!(!current.focus.unwrap().should_nudge);
        assert_eq!(current.metrics.nudges, 0);

        // Still drifting when the mute runs out: the nudge it held back arrives.
        let resumed = record(&mut manager, site("youtube.com", 0.0), seconds(301));
        assert_eq!(resumed.status, SessionStatus::Active);
        assert!(resumed.current.unwrap().focus.unwrap().should_nudge);
    }

    #[test]
    fn muted_time_is_still_recorded() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Strict), seconds(0));
        record(&mut manager, site("docs.google.com", 0.0), seconds(0));
        manager.snooze(seconds(10), 300);
        record(&mut manager, site("youtube.com", 0.0), seconds(20));
        record(&mut manager, site("youtube.com", 0.0), seconds(60));
        let snapshot = manager.stop(seconds(90));
        let session = &snapshot.history[0];

        assert_eq!(session.duration_seconds, 90);
        assert_eq!(session.idle_seconds, 0);
        assert_eq!(session.unrated_seconds + session.distracted_seconds, 90);
        assert!(session.distracted_seconds >= 30);
        assert_eq!(session.nudges, 0);
    }

    #[test]
    fn idle_does_not_accumulate_distracted_time() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        let snapshot = record(
            &mut manager,
            site("youtube.com", 200.0),
            seconds(200),
        );
        let metrics = snapshot.current.unwrap().metrics;

        assert_eq!(metrics.distracted_seconds, 0);
        assert_eq!(metrics.idle_seconds, 200);
    }

    #[test]
    fn a_thinking_pause_stays_with_the_window_in_front() {
        let mut manager = test_manager();
        manager.start(input("Think", Strictness::Strict), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        manager.mark_current(RuleClassification::Focused, seconds(0));
        for t in 0..=60 {
            record(&mut manager, sample("Code", "plan.md", 0.0), seconds(t));
        }
        // Two and a half minutes reading without touching anything, then typing again.
        for t in 61..=210 {
            record(&mut manager, sample("Code", "plan.md", (t - 60) as f64), seconds(t));
        }
        record(&mut manager, sample("Code", "plan.md", 0.0), seconds(211));
        manager.stop(seconds(220));

        let report = manager.session_report(id).unwrap();
        assert_eq!(report.idle_seconds, 0);
        assert!(report.spans.iter().all(|span| span.state != "idle"));
    }

    fn webcam_manager() -> SessionManager {
        let mut manager = test_manager();
        manager.update_settings(SettingsUpdate {
            default_strictness: None,
            status_window_on_top: None,
            voice_profile: None,
            webcam_gpu: None,
            webcam_enabled: Some(true),
        });
        manager
    }

    fn bounds(manager: &SessionManager, id: u64) -> Vec<(u64, u64, String, String)> {
        manager.session_report(id).unwrap().spans.into_iter()
            .map(|span| (span.started_offset_seconds, span.seconds, span.activity_key, span.state))
            .collect()
    }

    fn span(start: u64, seconds: u64, key: &str, state: &str) -> (u64, u64, String, String) {
        (start, seconds, key.to_string(), state.to_string())
    }

    #[test]
    fn a_phone_interrupts_the_window_from_its_first_frame() {
        let mut manager = webcam_manager();
        manager.start(input("Phone", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        for t in 0..=30 {
            manager.record_webcam((10..20).contains(&t), true, seconds(t));
            record(&mut manager, sample("Code", "plan.md", 0.0), seconds(t));
        }
        manager.stop(seconds(30));

        // Last seen at 19s; gone for two seconds at 21s.
        assert_eq!(bounds(&manager, id), vec![
            span(0, 10, "code", "unrated"),
            span(10, 11, "phone", "phone"),
            span(21, 9, "code", "unrated"),
        ]);
        let report = manager.session_report(id).unwrap();
        assert_eq!(report.phone_seconds, 11);
        assert_eq!(report.distracted_seconds, 11);
        // Normal: nudged eight seconds in.
        assert_eq!(report.nudge_count, 1);
        assert!(report.events.iter().any(|event| event.kind == "nudge"
            && event.detail.as_deref() == Some("phone") && event.at_offset_seconds == 18));
        assert!(report.apps.iter().any(|app| app.group == "Distracting" && app.key == "phone" && !app.ratable));
    }

    #[test]
    fn the_phone_shows_live_and_is_distracted_after_the_strictness_grace() {
        let mut manager = webcam_manager();
        manager.start(input("Phone", Strictness::Strict), seconds(0));
        record(&mut manager, sample("Code", "plan.md", 0.0), seconds(0));
        manager.record_webcam(true, true, seconds(1));
        let camera = manager.snapshot(seconds(1)).current.unwrap().camera;
        assert!(camera.live && camera.phone && !camera.phone_distracted);
        for t in 2..=6 {
            manager.record_webcam(true, true, seconds(t));
        }
        let current = manager.snapshot(seconds(6)).current.unwrap();
        assert!(current.camera.phone_distracted);
        assert_eq!(current.metrics.nudges, 1);
        assert_eq!(current.metrics.phone_seconds, 5);
    }

    #[test]
    fn a_muted_session_records_the_phone_but_never_nudges() {
        let mut manager = webcam_manager();
        manager.start(input("Phone", Strictness::Strict), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        record(&mut manager, sample("Code", "plan.md", 0.0), seconds(0));
        manager.snooze(seconds(0), 300);
        for t in 1..=60 {
            manager.record_webcam(true, true, seconds(t));
            record(&mut manager, sample("Code", "plan.md", 0.0), seconds(t));
        }
        let snapshot = manager.stop(seconds(61));
        assert_eq!(snapshot.history[0].nudges, 0);
        assert_eq!(manager.session_report(id).unwrap().phone_seconds, 60);
    }

    #[test]
    fn window_and_phone_nudges_share_one_cooldown() {
        let mut manager = webcam_manager();
        manager.start(input("Phone", Strictness::Strict), seconds(0));
        let nudges = |manager: &SessionManager, t| manager.snapshot(seconds(t)).current.unwrap().metrics.nudges;
        for t in 0..=210 {
            // Strict: YouTube is nudged at 30s; the phone from 31s to 125s.
            manager.record_webcam((31..=125).contains(&t), true, seconds(t));
            record(&mut manager, site("youtube.com", 0.0), seconds(t));
            match t {
                30 => assert_eq!(nudges(&manager, t), 1),
                // The phone waits out the 90s cooldown instead of nudging 5s in.
                119 => assert_eq!(nudges(&manager, t), 1),
                120 => assert_eq!(nudges(&manager, t), 2),
                // And YouTube, back in front, waits out the phone's.
                209 => assert_eq!(nudges(&manager, t), 2),
                210 => assert_eq!(nudges(&manager, t), 3),
                _ => {}
            }
        }
    }

    #[test]
    fn frames_are_ignored_while_the_webcam_is_off_or_the_session_paused() {
        let mut manager = test_manager();
        manager.start(input("Off", Strictness::Strict), seconds(0));
        manager.record_webcam(true, true, seconds(1));
        assert!(!manager.snapshot(seconds(1)).current.unwrap().camera.phone);
        assert!(!manager.wants_webcam());

        let mut manager = webcam_manager();
        manager.start(input("Paused", Strictness::Strict), seconds(0));
        assert!(manager.wants_webcam());
        manager.record_webcam(true, true, seconds(1));
        manager.pause(seconds(2));
        assert!(!manager.wants_webcam());
        manager.record_webcam(true, true, seconds(3));
        assert!(!manager.snapshot(seconds(3)).current.unwrap().camera.phone);
    }

    #[test]
    fn the_phone_ends_two_seconds_after_the_last_frame_that_saw_it() {
        let mut manager = webcam_manager();
        manager.start(input("Lost", Strictness::Normal), seconds(0));
        for t in 0..=2 {
            manager.record_webcam(true, true, seconds(t));
        }
        // The camera stops sending.
        manager.tick(seconds(3));
        assert!(manager.snapshot(seconds(3)).current.unwrap().camera.phone);
        manager.tick(seconds(4));
        assert!(!manager.snapshot(seconds(4)).current.unwrap().camera.phone);
    }

    #[test]
    fn someone_in_view_is_present_without_input_and_leaving_is_away_sooner() {
        let mut manager = webcam_manager();
        manager.start(input("Read", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        // Reading for five minutes without touching anything, in view the whole time.
        for t in 0..=300 {
            manager.record_webcam(false, true, seconds(t));
            record(&mut manager, sample("Code", "plan.md", t as f64), seconds(t));
        }
        // Typing until 320s, in view until 340s, then gone; Away a minute later, from 340s.
        for t in 301..=500 {
            manager.record_webcam(false, t <= 340, seconds(t));
            let idle = if t <= 320 { 0.0 } else { (t - 320) as f64 };
            record(&mut manager, sample("Code", "plan.md", idle), seconds(t));
        }
        manager.stop(seconds(500));
        assert_eq!(bounds(&manager, id), vec![
            span(0, 340, "code", "unrated"),
            span(340, 160, "code", "idle"),
        ]);
    }

    #[test]
    fn a_real_break_is_away_from_the_last_input() {
        let mut manager = test_manager();
        manager.start(input("Break", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        for t in 0..=60 {
            record(&mut manager, sample("Code", "plan.md", 0.0), seconds(t));
        }
        // Last input at 60s; the break is only noticed three minutes later.
        for t in 61..=300 {
            record(&mut manager, sample("Code", "plan.md", (t - 60) as f64), seconds(t));
        }
        manager.stop(seconds(300));

        let report = manager.session_report(id).unwrap();
        let bounds: Vec<_> = report.spans.iter()
            .map(|span| (span.started_offset_seconds, span.seconds, span.state.as_str()))
            .collect();
        assert_eq!(bounds, vec![(0, 60, "unrated"), (60, 240, "idle")]);
        assert_eq!(report.idle_seconds, 240);
    }

    #[test]
    fn away_never_starts_before_media_last_showed_someone_there() {
        let mut manager = test_manager();
        manager.start(input("Watch", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        // Watching without input until the video ends at 600s, then nobody is there.
        for t in 1..=600 {
            let watching = ActivitySample { media_playing: true, ..site("youtube.com", t as f64) };
            record(&mut manager, watching, seconds(t));
        }
        for t in 601..=900 {
            record(&mut manager, site("youtube.com", t as f64), seconds(t));
        }
        manager.stop(seconds(900));

        let report = manager.session_report(id).unwrap();
        assert_eq!(report.idle_seconds, 300);
        // Slipping and Distracted together: every watched second stays on YouTube.
        assert_eq!(report.distracted_seconds, 600);
    }

    #[test]
    fn supervisor_owned_samples_do_not_record_activity() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        let supervisor = identity();
        let snapshot = manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(20),
            &supervisor,
        );
        let current = snapshot.current.unwrap();

        assert_eq!(metric_total(&current.metrics), 0);
        assert_eq!(current.metrics.nudges, 0);
        assert!(current.evidence.is_empty());
        assert!(current.focus.is_none());
    }

    #[test]
    fn supervisor_foreground_freezes_after_accounting_previous_window() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(10));
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(20),
            &supervisor,
        );
        let snapshot = manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(60),
            &supervisor,
        );
        let metrics = snapshot.current.unwrap().metrics;

        // YouTube keeps its 20s; the 40s in Supervisor is Neutral, never judged.
        assert_eq!(metric_total(&metrics), 60);
        assert_eq!(metrics.neutral_seconds, 40);
        assert_eq!(metrics.nudges, 0);
    }

    #[test]
    fn supervisor_time_is_a_neutral_app_row() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        record(&mut manager, sample_with_pid("Supervisor", "Supervisor", 7, 0.0), seconds(1));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(30));
        record(&mut manager, sample_with_pid("Supervisor", "Supervisor", 7, 0.0), seconds(40));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(45));
        manager.stop(seconds(60));
        let report = manager.session_report(id).expect("report");

        let supervisor: Vec<_> = report.apps.iter().filter(|app| app.key == "supervisor").collect();
        assert_eq!(supervisor.len(), 1);
        assert_eq!(supervisor[0].group, "Neutral");
        assert_eq!(supervisor[0].seconds, 35);
        assert!(report.spans.iter().filter(|span| span.activity_key == "supervisor")
            .all(|span| span.state == "neutral"));
        assert_eq!(report.focused_seconds, 0);
    }

    #[test]
    fn leaving_supervisor_in_front_goes_away_from_the_last_input() {
        let mut manager = test_manager();
        manager.start(input("Left it open", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(0));
        // Opens History at 30s, reads it until 60s, then walks away.
        for t in 30..=60 {
            record(&mut manager, sample_with_pid("Supervisor", "Supervisor", 7, 0.0), seconds(t));
        }
        for t in 61..=400 {
            record(&mut manager, sample_with_pid("Supervisor", "Supervisor", 7, (t - 60) as f64), seconds(t));
        }
        manager.stop(seconds(400));

        let report = manager.session_report(id).unwrap();
        let supervisor: Vec<_> = report.spans.iter()
            .filter(|span| span.activity_key == "supervisor")
            .map(|span| (span.started_offset_seconds, span.seconds, span.state.as_str()))
            .collect();
        assert_eq!(supervisor, vec![(30, 30, "neutral"), (60, 340, "idle")]);
        assert_eq!(report.idle_seconds, 340);
    }

    #[test]
    fn paused_time_is_recorded_but_never_scored() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(0));
        manager.pause(seconds(30));
        manager.resume(seconds(90));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(91));
        manager.stop(seconds(120));
        let report = manager.session_report(id).expect("report");

        let paused: u64 = report.spans.iter().filter(|span| span.state == "paused")
            .map(|span| span.seconds).sum();
        assert_eq!(paused, 60);
        assert_eq!(report.duration_seconds, 60);
        assert_eq!(report.unrated_seconds, 60);
        assert!(report.apps.iter().all(|app| app.key != "paused"));
    }

    #[test]
    fn sleep_is_recorded_as_lost_time_not_the_last_app() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Normal), seconds(0));
        let id = manager.snapshot(seconds(0)).current.unwrap().id;
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(10));
        manager.record_lost_time(seconds(10), seconds(610));
        record(&mut manager, site("youtube.com", 0.0), seconds(610));
        manager.stop(seconds(620));
        let report = manager.session_report(id).expect("report");

        let lost: u64 = report.spans.iter().filter(|span| span.label == "Device asleep / unknown")
            .map(|span| span.seconds).sum();
        assert_eq!(lost, 600);
        assert_eq!(report.distracted_seconds, 20);
    }

    /// The core promise of the report: from Start to Stop, every second belongs to exactly one
    /// recorded segment. Random event sequences must never leave a hole or an overlap.
    #[test]
    fn every_second_of_a_session_is_recorded() {
        let mut seeds_with_phone = 0;
        for seed in 1..=300u64 {
            let mut state = seed;
            let mut next = move |n: u64| {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                (state >> 33) % n
            };
            let mut manager = webcam_manager();
            manager.start(input_minutes("Fuzz", Strictness::Strict, 180), seconds(0));
            let id = manager.snapshot(seconds(0)).current.unwrap().id;
            let mut now = 0u64;
            for _ in 0..160 {
                now += 1 + next(4);
                let t = seconds(now);
                match next(19) {
                    0..=3 => {
                        let app = ["Code", "Slack", "Notes"][next(3) as usize];
                        record(&mut manager, sample(app, "window", 0.0), t);
                    }
                    4 | 5 => {
                        let address = ["youtube.com", "github.com"][next(2) as usize];
                        record(&mut manager, site(address, 0.0), t);
                    }
                    6 => {
                        record(&mut manager, sample("Code", "window", 500.0), t);
                    }
                    7 => {
                        record(&mut manager, sample_with_pid("Supervisor", "Supervisor", 7, 0.0), t);
                    }
                    8 => {
                        record(&mut manager, shell_sample(), t);
                    }
                    9 => {
                        if next(2) == 0 {
                            manager.pause(t);
                        } else {
                            manager.resume(t);
                        }
                    }
                    10 => {
                        manager.snooze(t, 300);
                    }
                    11 => {
                        let mark = if next(2) == 0 {
                            RuleClassification::Focused
                        } else {
                            RuleClassification::Distracting
                        };
                        manager.mark_current(mark, t);
                    }
                    12 => {
                        manager.record_capture_gap(t);
                    }
                    13 => {
                        let rules = manager.rules();
                        manager.update_current_rules(rules, t);
                    }
                    // Webcam readings arrive between ticks, a few a second.
                    14 | 15 => {
                        for quarter in 0..1 + next(8) {
                            manager.record_webcam(true, true, t + Duration::from_millis(250 * quarter));
                        }
                    }
                    16 => {
                        manager.record_webcam(false, true, t + Duration::from_millis(500));
                    }
                    17 => {
                        manager.record_webcam(false, false, t + Duration::from_millis(750));
                    }
                    _ => {
                        now += 20 + next(100);
                        manager.record_lost_time(t, seconds(now));
                    }
                }
                manager.tick(seconds(now));
            }
            let rules = manager.rules();
            manager.update_current_rules(rules, seconds(now));
            now += 1;
            manager.stop(seconds(now));

            let report = manager.session_report(id).expect("report");
            let spans = report.spans;
            let mut cursor = 0;
            for span in &spans {
                assert_eq!(span.started_offset_seconds, cursor, "seed {seed}: hole or overlap at {cursor}s");
                cursor += span.seconds;
            }
            assert_eq!(cursor, now, "seed {seed}: timeline ends at {cursor}s, session at {now}s");
            assert_eq!(manager.storage.as_ref().unwrap().repaired_seconds(), 0, "seed {seed}: holes were repaired");
            let phone: u64 = spans.iter().filter(|span| span.state == "phone").map(|span| span.seconds).sum();
            assert_eq!(report.phone_seconds, phone, "seed {seed}");
            seeds_with_phone += u64::from(phone > 0);
        }
        assert!(seeds_with_phone > 100, "only {seeds_with_phone} seeds recorded a phone");
    }

    #[test]
    fn timer_advances_while_supervisor_foreground_without_accounting_it() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(10));
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(20),
            &supervisor,
        );

        let current = manager.tick(seconds(60)).current.unwrap();

        assert_eq!(current.elapsed_seconds, 60);
        assert_eq!(current.remaining_seconds, 24 * 60);
        assert_eq!(metric_total(&current.metrics), 20);
    }

    #[test]
    fn timer_holds_while_paused_but_not_snoozed() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        manager.pause(seconds(10));

        assert_eq!(
            manager.tick(seconds(70)).current.unwrap().elapsed_seconds,
            10
        );

        manager.resume(seconds(70));
        assert_eq!(
            manager.tick(seconds(80)).current.unwrap().elapsed_seconds,
            20
        );

        manager.snooze(seconds(80), 300);
        assert_eq!(
            manager.tick(seconds(200)).current.unwrap().elapsed_seconds,
            140
        );
    }

    #[test]
    fn clock_expiry_finishes_session_even_on_supervisor_window() {
        let mut manager = test_manager();
        manager.start(
            input_minutes("Focus", Strictness::Strict, 5),
            seconds(0),
        );
        record(&mut manager, site("docs.google.com", 0.0), seconds(0));
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(1),
            &supervisor,
        );

        let snapshot = manager.tick(seconds(300));

        assert_eq!(snapshot.status, SessionStatus::Idle);
        assert_eq!(snapshot.history.len(), 1);
        assert_eq!(snapshot.history[0].duration_seconds, 300);
    }

    #[test]
    fn muting_while_paused_keeps_the_break() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(6));
        manager.pause(seconds(10));
        assert_eq!(manager.snooze(seconds(70), 300).status, SessionStatus::Paused);
        let snapshot = manager.stop(seconds(80));

        assert_eq!(snapshot.history[0].distracted_seconds, 10);
        assert_eq!(snapshot.history[0].duration_seconds, 10);
    }

    #[test]
    fn app_switch_does_not_nudge_immediately_inside_session() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Normal),
            seconds(0),
        );
        assert!(
            !manager
                .record_sample(site("youtube.com", 0.0), seconds(0), &identity())
                .current
                .unwrap()
                .focus
                .unwrap()
                .should_nudge
        );
        assert!(
            !manager
                .record_sample(site("youtube.com", 0.0), seconds(59), &identity())
                .current
                .unwrap()
                .focus
                .unwrap()
                .should_nudge
        );
        assert!(
            manager
                .record_sample(site("youtube.com", 0.0), seconds(60), &identity())
                .current
                .unwrap()
                .focus
                .unwrap()
                .should_nudge
        );
    }

    #[test]
    fn shell_surfaces_keep_the_previous_window() {
        let mut manager = test_manager();
        manager
            .start(input("Focus", Strictness::Normal), seconds(0));
        let before = record(&mut manager, sample("Code", "main.rs", 0.0), seconds(1))
            .current
            .unwrap()
            .focus
            .unwrap();
        let during = record(&mut manager, shell_sample(), seconds(5))
            .current
            .unwrap()
            .focus
            .unwrap();

        assert_eq!(during.sample.app_name, "Code");
        assert_eq!(during.sample.window_title.as_deref(), Some("main.rs"));
        assert_eq!(during.state, before.state);
        assert_eq!(during.classification, before.classification);
    }

    #[test]
    fn alt_tab_does_not_delay_or_hide_a_nudge() {
        let mut manager = test_manager();
        manager
            .start(input("Focus", Strictness::Normal), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        for at in [30, 40, 50] {
            let focus = record(&mut manager, shell_sample(), seconds(at))
                .current
                .unwrap()
                .focus
                .unwrap();
            assert_eq!(focus.activity.key, "youtube.com");
            assert_ne!(focus.state, FocusState::Neutral);
        }
        let nudge = record(&mut manager, shell_sample(), seconds(60))
            .current
            .unwrap()
            .focus
            .unwrap();

        assert!(nudge.should_nudge);
    }

    #[test]
    fn shell_surface_before_any_window_is_skipped() {
        let mut manager = test_manager();
        manager
            .start(input("Focus", Strictness::Normal), seconds(0));
        let snapshot = record(&mut manager, shell_sample(), seconds(1));

        assert!(snapshot.current.unwrap().focus.is_none());
    }

    #[test]
    fn mark_current_relevant_wins_over_starter_rules() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(
            &mut manager,
            site("youtube.com/watch", 0.0),
            seconds(0),
        );
        let marked = manager.mark_current(RuleClassification::Focused, seconds(1));
        assert_eq!(
            marked.current.unwrap().focus.unwrap().state,
            FocusState::Focused
        );

        let snapshot = record(
            &mut manager,
            site("youtube.com/watch", 0.0),
            seconds(40),
        );

        assert_eq!(
            snapshot.current.unwrap().focus.unwrap().state,
            FocusState::Focused
        );
    }

    #[test]
    fn mark_current_distracting_refreshes_cached_focus_immediately() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, sample("Notes", "Plan", 0.0), seconds(0));
        record(&mut manager, sample("Notes", "Plan", 0.0), seconds(10));

        let snapshot = manager.mark_current(RuleClassification::Distracting, seconds(11));
        let focus = snapshot.current.unwrap().focus.unwrap();

        assert_eq!(focus.state, FocusState::Distracted);
        assert_eq!(focus.classification, RuleClassification::Distracting);
        assert_eq!(focus.matched_rule.unwrap().value, "notes");
    }

    #[test]
    fn mark_current_uses_latest_external_window_after_supervisor_focus() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(
            &mut manager,
            site("youtube.com/watch", 0.0),
            seconds(0),
        );
        let supervisor = identity();
        manager.record_sample(
            sample_with_pid("Supervisor", "Supervisor", 7, 0.0),
            seconds(1),
            &supervisor,
        );
        manager.mark_current(RuleClassification::Focused, seconds(2));
        let snapshot = record(
            &mut manager,
            site("youtube.com/watch", 0.0),
            seconds(3),
        );

        assert_eq!(
            snapshot.current.unwrap().focus.unwrap().state,
            FocusState::Focused
        );
    }

    #[test]
    fn snapshot_reads_do_not_advance_metrics() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(10));
        let first = manager.snapshot(seconds(10));
        let second = manager.snapshot(seconds(20));

        assert_eq!(
            first.current.unwrap().metrics,
            second.current.unwrap().metrics
        );
    }

    #[test]
    fn evidence_records_decision_metadata() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        let snapshot = record(&mut manager, site("youtube.com", 0.0), seconds(10));
        let current = snapshot.current.unwrap();
        let event = current.evidence.first().unwrap();

        assert_eq!(event.label, "YouTube");
        assert!(event.value.contains("DISTRACTED"));
        assert!(event.value.contains("SITE youtube.com"));
        assert!(event.value.contains("NUDGE ARMED"));
    }

    #[test]
    fn history_cache_refreshes_after_close_and_delete() {
        let mut manager = test_manager();
        manager.start(input("Cache test", Strictness::Normal), seconds(0));
        record(&mut manager, sample("Code", "main.rs", 0.0), seconds(10));
        manager.stop(seconds(11));

        let history = manager.history();
        assert_eq!(history.len(), 1);
        assert_eq!(manager.snapshot(seconds(11)).history.len(), 1);

        manager.delete_session(history[0].id).expect("delete session");
        assert!(manager.history().is_empty());
        assert!(manager.snapshot(seconds(11)).history.is_empty());
        assert_eq!(manager.snapshot(seconds(11)).storage_notice, None);
    }

    #[test]
    fn app_switch_keeps_elapsed_time_on_previous_app() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Strict), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(10));
        let metrics = record(&mut manager, site("youtube.com", 0.0), seconds(25))
            .current
            .unwrap()
            .metrics;
        assert_eq!(metrics.neutral_seconds, 25);
        assert_eq!(metrics.suspicious_seconds, 0);
        assert_eq!(metrics.distracted_seconds, 0);
    }

    #[test]
    fn capture_gap_does_not_extend_the_last_app() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Strict), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(10));
        manager.record_capture_gap(seconds(40));
        let id = manager.snapshot(seconds(40)).current.unwrap().id;
        manager.stop(seconds(40));
        let report = manager.session_report(id).expect("report");
        let code_seconds: u64 = report
            .spans
            .iter()
            .filter(|span| span.label == "Code")
            .map(|span| span.seconds)
            .sum();
        let gap_seconds: u64 = report
            .spans
            .iter()
            .filter(|span| span.label == "Device asleep / unknown")
            .map(|span| span.seconds)
            .sum();
        assert_eq!(code_seconds, 10);
        assert_eq!(gap_seconds, 30);
        assert_eq!(report.idle_seconds, 30);
    }

    #[test]
    fn delete_all_history_keeps_the_active_session() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Normal), seconds(0));
        record(&mut manager, sample("Code", "Plan", 0.0), seconds(10));
        let id = manager.snapshot(seconds(10)).current.unwrap().id;
        assert!(manager
            .delete_session(id)
            .unwrap_err()
            .contains("Stop the session"));
        manager.delete_all_history().unwrap();
        manager.stop(seconds(11));
        let report = manager.session_report(id).expect("active session survives");
        assert_eq!(report.ended_reason.as_deref(), Some("stopped"));
        manager.delete_all_history().unwrap();
        assert!(manager.session_report(id).is_err());
    }

    #[test]
    fn history_off_still_runs_sessions() {
        let notice = StorageNotice::Off { reason: "disk full".to_string() };
        let mut manager = SessionManager::without_storage(Some(notice.clone()));
        manager.start(input("Focus", Strictness::Normal), seconds(0));
        record(&mut manager, sample("Code", "main.rs", 0.0), seconds(10));
        assert!(manager.snapshot(seconds(10)).current.is_some());

        let snapshot = manager.stop(seconds(11));
        assert_eq!(snapshot.status, SessionStatus::Idle);
        assert!(snapshot.history.is_empty());
        assert_eq!(snapshot.storage_notice, Some(notice.clone()));
        assert!(manager.trends().is_err());
        assert_eq!(manager.dismiss_storage_notice(seconds(12)).storage_notice, Some(notice));
    }

    #[test]
    fn reset_notice_is_dismissible() {
        let notice = StorageNotice::Reset {
            reason: "damaged".to_string(),
            moved_to: "sessions.broken-1.db".to_string(),
        };
        let mut manager = SessionManager {
            startup_notice: Some(notice),
            ..test_manager()
        };
        assert!(manager.snapshot(seconds(0)).storage_notice.is_some());
        assert_eq!(manager.dismiss_storage_notice(seconds(0)).storage_notice, None);
    }

    #[test]
    fn stop_summary_includes_top_hints_and_nudges() {
        let mut manager = test_manager();
        manager.start(
            input("Focus", Strictness::Strict),
            seconds(0),
        );
        record(&mut manager, site("youtube.com", 0.0), seconds(0));
        record(&mut manager, site("youtube.com", 0.0), seconds(30));
        let snapshot = manager.stop(seconds(31));

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
        manager.record_sample(sample, now, &identity())
    }

    fn identity() -> SupervisorIdentity {
        SupervisorIdentity {
            process_id: 7,
            process_path: "/Applications/Supervisor.app".to_string(),
        }
    }

    #[test]
    fn another_app_named_supervisor_is_tracked() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Strict), seconds(0));
        let other = ActivitySample {
            process_path: "/Applications/Other/Supervisor.app".to_string(),
            ..sample("Supervisor", "Someone else's app", 0.0)
        };
        let snapshot = record(&mut manager, other, seconds(5));

        let focus = snapshot.current.unwrap().focus.unwrap();
        // Never filed under Supervisor's own row.
        assert_eq!(focus.activity.key, "supervisor#2");
    }

    #[test]
    fn two_programs_with_one_name_are_two_apps_and_keep_their_keys() {
        let mut manager = test_manager();
        let mut start = input("Focus", Strictness::Normal);
        start.rules = Some(vec![AppRule::app("app", RuleClassification::Distracting)]);
        manager.start(start, seconds(0));
        let alpha = ActivitySample {
            process_path: "/Vendors/Alpha/App.app".to_string(),
            ..sample("App", "Alpha", 0.0)
        };
        let beta = ActivitySample {
            process_path: "/Vendors/Beta/App.app".to_string(),
            ..sample("App", "Beta", 0.0)
        };
        let focus = |snapshot: SessionSnapshot| snapshot.current.unwrap().focus.unwrap();
        let a = focus(record(&mut manager, alpha.clone(), seconds(0)));
        let b = focus(record(&mut manager, beta, seconds(5)));
        assert_eq!((a.activity.key.as_str(), a.classification), ("app", RuleClassification::Distracting));
        // The rule on the first app does not reach the second, and the two never look alike.
        assert_eq!((b.activity.key.as_str(), b.classification), ("app#2", RuleClassification::Neutral));
        assert_eq!(b.activity.label, "App (2)");
        assert_eq!(focus(record(&mut manager, alpha, seconds(10))).activity.key, "app");
    }

    fn sample(app_name: &str, title: &str, idle_seconds: f64) -> ActivitySample {
        sample_with_pid(app_name, title, 42, idle_seconds)
    }

    fn site(address: &str, idle_seconds: f64) -> ActivitySample {
        ActivitySample {
            browser: Some(crate::activity::BrowserRead {
                address: Some(address.to_string()),
                editing: false,
            }),
            ..sample("chrome", address, idle_seconds)
        }
    }

    #[test]
    fn a_title_guess_is_corrected_when_the_address_arrives() {
        let mut manager = test_manager();
        manager.start(input("Focus", Strictness::Normal), seconds(0));
        let unread = sample("chrome", "Issue #42 - GitHub", 0.0);
        record(&mut manager, unread.clone(), seconds(0));
        let guess = record(&mut manager, unread.clone(), seconds(1));
        assert_eq!(guess.current.unwrap().focus.unwrap().activity.key, "github.com");

        let confirmed = ActivitySample {
            browser: Some(crate::activity::BrowserRead {
                address: Some("example.com/issues/42".to_string()),
                editing: false,
            }),
            ..unread
        };
        record(&mut manager, confirmed, seconds(5));
        let id = manager.snapshot(seconds(5)).current.unwrap().id;
        manager.stop(seconds(10));

        let labels: Vec<String> = manager
            .session_report(id)
            .expect("report")
            .spans
            .into_iter()
            .map(|span| span.label)
            .collect();
        assert!(!labels.iter().any(|label| label == "GitHub"), "{labels:?}");
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
            app_label: app_name.to_string(),
            process_path: format!("/Applications/{app_name}.app"),
            app_identity: String::new(),
            app_key: None,
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
            shell_surface: false,
            browser: None,
            media_playing: false,
        }
    }

    fn shell_sample() -> ActivitySample {
        ActivitySample {
            platform: "windows",
            app_name: "explorer".to_string(),
            process_path: "C:\\Windows\\explorer.exe".to_string(),
            process_id: 99,
            window_title: None,
            window_id: "shell".to_string(),
            shell_surface: true,
            ..sample("unused", "unused", 0.0)
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
