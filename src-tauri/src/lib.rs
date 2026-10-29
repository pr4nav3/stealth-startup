mod activity;
mod app_identity;
mod attribution;
mod focus;
mod session;
mod storage;
mod voice;
mod voice_runtime;
#[cfg(target_os = "macos")]
mod macos;
mod webcam;
mod website;

use activity::ActivitySample;
use session::{
    SessionManager, SessionSettings, SessionSnapshot, SessionSummary, SettingsUpdate,
    StartSessionInput, SupervisorIdentity,
};
use storage::{DailyFocus, HistoryWeek, MonthRecap, WeekRecap, SessionReport, StoredPhraseLibrary, TrendPeriod, TrendUnit, TrendsData};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder,
};

const MAIN_WINDOW_LABEL: &str = "main";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 312.0;
const OVERLAY_HEIGHT: f64 = 286.0;
const OVERLAY_MARGIN: i32 = 18;

/// Session time: a monotonic clock that keeps running while the computer sleeps, so a closed
/// lid shows up as a gap (lost time, an expired timer) instead of vanishing. `Instant` does
/// that on Windows but not on macOS, where it stops during sleep.
#[derive(Clone, Copy)]
struct SessionClock {
    start: Duration,
}

impl SessionClock {
    fn now() -> Self {
        Self { start: Self::read() }
    }

    fn elapsed(&self) -> Duration {
        Self::read().saturating_sub(self.start)
    }

    #[cfg(target_os = "macos")]
    fn read() -> Duration {
        // On Darwin CLOCK_MONOTONIC counts sleep (mach_continuous_time); CLOCK_UPTIME_RAW,
        // which `Instant` uses, does not.
        let mut time = libc::timespec { tv_sec: 0, tv_nsec: 0 };
        unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
        Duration::new(time.tv_sec as u64, time.tv_nsec as u32)
    }

    #[cfg(not(target_os = "macos"))]
    fn read() -> Duration {
        static ORIGIN: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        ORIGIN.get_or_init(Instant::now).elapsed()
    }
}

struct AppState {
    sessions: Mutex<SessionManager>,
    voice_download: Arc<Mutex<voice::VoiceDownloadState>>,
    voice_recording: Mutex<voice::VoiceRecordingState>,
    voice_worker: voice::VoiceWorker,
    webcam: webcam::WebcamPreview,
    started_at: SessionClock,
    identity: SupervisorIdentity,
    /// The tray's pause item, renamed by the ticker to what it would do now.
    tray_pause: Mutex<Option<MenuItem<tauri::Wry>>>,
}

/// What the tray's pause item would do for this session.
fn tray_pause_label(snapshot: &SessionSnapshot) -> String {
    let Some(current) = snapshot.current.as_ref() else {
        return "Pause".to_string();
    };
    match current.status {
        session::SessionStatus::Paused => match current.pomodoro.as_ref() {
            Some(pomodoro) if pomodoro.phase != "work" => format!("Start round {}", pomodoro.round + 1),
            _ => "Resume".to_string(),
        },
        _ => "Pause".to_string(),
    }
}

fn set_tray_pause_label(app: &AppHandle, last: &mut String, label: String) {
    if *last == label {
        return;
    }
    let state = app.state::<AppState>();
    if let Ok(item) = state.tray_pause.lock() {
        if let Some(item) = item.as_ref() {
            let _ = item.set_text(&label);
        }
    }
    *last = label;
}

#[tauri::command]
async fn start_session(
    input: StartSessionInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();

    sessions.start(input, now);
    let should_show_status = sessions.status_window_enabled();
    let snapshot = sessions.snapshot(now);
    drop(sessions);
    voice::release_worker(&state.voice_worker);
    sync_session_webcam(&app);

    if should_show_status {
        show_status_window_internal(&app)?;
        show_main_window(&app);
    } else {
        hide_status_window(&app);
    }
    Ok(snapshot)
}

#[tauri::command]
fn get_session_snapshot(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.snapshot(now))
}

#[tauri::command]
fn pause_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    let snapshot = sessions.pause(now);
    drop(sessions);
    sync_session_webcam(&app);
    Ok(snapshot)
}

#[tauri::command]
fn resume_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    let snapshot = sessions.resume(now);
    drop(sessions);
    sync_session_webcam(&app);
    Ok(snapshot)
}

#[tauri::command]
fn snooze_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.snooze(now, 5 * 60))
}

#[tauri::command]
fn stop_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    let ended = sessions.has_current();
    let snapshot = sessions.stop(now);
    drop(sessions);

    if ended {
        session_ended(&app);
    } else {
        hide_status_window(&app);
    }
    sync_session_webcam(&app);
    Ok(snapshot)
}

#[tauri::command]
async fn flag_current_distracting(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.flag_current(now))
}

#[tauri::command]
async fn allow_current_for_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.allow_current_for_session(now))
}

#[tauri::command]
async fn allow_current_always(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.allow_current_always(now))
}

#[tauri::command]
fn get_history(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    Ok(lock_sessions(&state)?.history())
}

#[tauri::command]
fn get_session_report(id: u64, state: State<'_, AppState>) -> Result<SessionReport, String> {
    lock_sessions(&state)?.session_report(id)
}

#[tauri::command]
fn get_trends(state: State<'_, AppState>) -> Result<TrendsData, String> {
    lock_sessions(&state)?.trends()
}

#[tauri::command]
fn get_trend_periods(unit: TrendUnit, state: State<'_, AppState>) -> Result<Vec<TrendPeriod>, String> {
    lock_sessions(&state)?.trend_periods(unit)
}

#[tauri::command]
fn get_focus_days(start: String, end: String, state: State<'_, AppState>) -> Result<Vec<DailyFocus>, String> {
    lock_sessions(&state)?.focus_days(start, end)
}

#[tauri::command]
fn get_history_week(week_start: String, state: State<'_, AppState>) -> Result<HistoryWeek, String> {
    lock_sessions(&state)?.history_week(week_start)
}

#[tauri::command]
fn search_sessions(query: String, state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    lock_sessions(&state)?.search_sessions(query)
}

#[tauri::command]
fn get_week_recap(start: String, state: State<'_, AppState>) -> Result<WeekRecap, String> {
    lock_sessions(&state)?.week_recap(start)
}

/// The calendar month containing `start`, for the share card.
#[tauri::command]
fn get_month_recap(start: String, state: State<'_, AppState>) -> Result<MonthRecap, String> {
    lock_sessions(&state)?.month_recap(start)
}

#[tauri::command]
fn delete_session(id: u64, state: State<'_, AppState>) -> Result<(), String> {
    lock_sessions(&state)?.delete_session(id)
}

#[tauri::command]
fn get_history_size(state: State<'_, AppState>) -> Result<u64, String> {
    lock_sessions(&state)?.history_bytes()
}

#[tauri::command]
fn delete_all_history(state: State<'_, AppState>) -> Result<(), String> {
    lock_sessions(&state)?.delete_all_history()
}

#[tauri::command]
fn dismiss_storage_notice(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.dismiss_storage_notice(now))
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SessionSettings, String> {
    Ok(lock_sessions(&state)?.settings())
}

#[derive(serde::Serialize)]
struct CaptureAccess {
    platform: &'static str,
    permissions: activity::CapturePermissions,
}

/// What the OS lets Supervisor see, for the Getting started card. On macOS the first call
/// shows the system's Accessibility prompt.
#[tauri::command]
fn get_capture_access() -> CaptureAccess {
    CaptureAccess {
        platform: std::env::consts::OS,
        permissions: activity::capture_permissions(),
    }
}

#[tauri::command]
fn open_accessibility_settings() -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    // `open` returns at once; waiting for it leaves no zombie process per click.
    std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
        .status()
        .map(drop)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn get_rules(state: State<'_, AppState>) -> Result<Vec<focus::AppRule>, String> {
    Ok(lock_sessions(&state)?.rules())
}

/// Turns what the user typed into a website or app rule, or says why it can't be one.
#[tauri::command]
fn resolve_rule_input(text: String, state: State<'_, AppState>) -> Result<focus::AppRule, String> {
    let names = lock_sessions(&state)?.app_names().unwrap_or_default();
    let rule = focus::AppRule::from_input_with_apps(&text, &names)?;
    if rule.names_a_browser() {
        return Err("A browser can't be a distraction on its own. Add the website instead (youtube.com).".to_string());
    }
    Ok(rule)
}

#[tauri::command]
fn get_site_names(state: State<'_, AppState>) -> Result<HashMap<String, String>, String> {
    Ok(lock_sessions(&state)?.site_names())
}

#[tauri::command]
fn set_site_name(
    key: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<HashMap<String, String>, String> {
    lock_sessions(&state)?.set_site_name(&key, &name)
}

#[tauri::command]
fn get_app_names(state: State<'_, AppState>) -> Result<HashMap<String, String>, String> {
    lock_sessions(&state)?.app_names()
}

#[tauri::command]
fn review_candidates(id: u64, state: State<'_, AppState>) -> Result<Vec<storage::RecapApp>, String> {
    lock_sessions(&state)?.review_candidates(id)
}

#[tauri::command]
fn review_session(
    id: u64,
    distractions: Vec<focus::AppRule>,
    work: Vec<String>,
    state: State<'_, AppState>,
) -> Result<session::RulesSaved, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    sessions.review_session(id, distractions, work, now)
}

#[tauri::command]
fn set_session_exception(rule: focus::AppRule, allowed: bool, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.set_session_exception(rule, allowed, now))
}

#[tauri::command]
fn reclassify_session_app(
    id: u64,
    kind: String,
    key: String,
    distraction: bool,
    state: State<'_, AppState>,
) -> Result<SessionReport, String> {
    lock_sessions(&state)?.reclassify_session_app(id, kind, key, distraction)
}

#[tauri::command]
fn set_app_name(
    key: String,
    name: String,
    state: State<'_, AppState>,
) -> Result<HashMap<String, String>, String> {
    lock_sessions(&state)?.set_app_name(&key, &name)
}

#[tauri::command]
async fn update_rules(
    rules: Vec<focus::AppRule>,
    state: State<'_, AppState>,
) -> Result<session::RulesSaved, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    Ok(sessions.update_rules(rules, now))
}

#[tauri::command]
fn get_phrase_library(state: State<'_, AppState>) -> Result<StoredPhraseLibrary, String> {
    lock_sessions(&state)?.load_phrase_library()
}

#[tauri::command]
fn update_phrase_library(
    library: StoredPhraseLibrary,
    state: State<'_, AppState>,
) -> Result<StoredPhraseLibrary, String> {
    lock_sessions(&state)?.save_phrase_library(library)
}

#[tauri::command]
fn update_settings(
    update: SettingsUpdate,
    state: State<'_, AppState>,
) -> Result<SessionSettings, String> {
    Ok(lock_sessions(&state)?.update_settings(update))
}

#[tauri::command]
fn show_status_window(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    let snapshot = sessions.set_status_window_enabled(true, now);
    drop(sessions);

    show_status_window_internal(&app)?;
    Ok(snapshot)
}

#[tauri::command]
fn set_status_window_enabled(
    enabled: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    let snapshot = sessions.set_status_window_enabled(enabled, now);
    drop(sessions);
    let should_show_status = enabled && snapshot.current.is_some();

    if should_show_status {
        show_status_window_internal(&app)?;
    } else {
        hide_status_window(&app);
    }
    Ok(snapshot)
}

#[tauri::command]
fn set_status_window_on_top(
    enabled: bool,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let mut sessions = lock_sessions(&state)?;
    let now = state.started_at.elapsed();
    let snapshot = sessions.set_status_window_on_top(enabled, now);
    drop(sessions);

    set_status_window_on_top_internal(&app, enabled)?;
    if enabled {
        show_status_window_internal(&app)?;
    }
    Ok(snapshot)
}

#[tauri::command]
async fn get_voice_backend_status(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceBackendStatus, String> {
    voice::get_status(&app, &state.voice_worker)
}

/// Async so it runs off the main thread: the check hashes a gigabyte of model files.
#[tauri::command]
async fn start_voice_model_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceDownloadSnapshot, String> {
    // A check can install a new runtime; a worker still running the old one would load half
    // its files from the new version (a running program's folder can be swapped on a Mac).
    if state.voice_worker.is_busy() {
        return Err("A clip is generating. Wait for it or cancel it first.".to_string());
    }
    voice::release_worker(&state.voice_worker);
    voice::start_model_download(app, Arc::clone(&state.voice_download))
}

#[tauri::command]
async fn get_voice_model_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceDownloadSnapshot, String> {
    voice::model_download_snapshot(&app, &state.voice_download)
}

#[tauri::command]
fn cancel_voice_model_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceDownloadSnapshot, String> {
    voice::cancel_model_download(&app, &state.voice_download)
}

#[tauri::command]
async fn delete_voice_cloning_model(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceModelStatus, String> {
    // The worker holds the model and runtime files open; deleting under it fails on Windows.
    if state.voice_worker.is_busy() {
        return Err("A clip is generating. Wait for it or cancel it first.".to_string());
    }
    if voice::download_active(&state.voice_download) {
        return Err("A download is running. Cancel it first.".to_string());
    }
    voice::release_worker(&state.voice_worker);
    voice::delete_model(&app)
}

#[tauri::command]
fn save_voice_reference_from_path(
    app: AppHandle,
    input: voice::SaveVoiceReferenceFromPathInput,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::save_reference_from_path(&app, input)
}

#[tauri::command]
async fn start_voice_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceRecordingSnapshot, String> {
    voice::start_recording(&app, &state.voice_recording)
}

#[tauri::command]
async fn stop_voice_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceRecordingSnapshot, String> {
    voice::stop_recording(&app, &state.voice_recording)
}

#[tauri::command]
fn clear_voice_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceRecordingSnapshot, String> {
    voice::clear_recording(&app, &state.voice_recording)
}

#[tauri::command]
async fn generate_voice_clips(
    app: AppHandle,
    state: State<'_, AppState>,
    input: voice::GenerateVoiceClipsInput,
) -> Result<voice::VoiceProfileSummary, String> {
    let worker = state.voice_worker.clone();
    tauri::async_runtime::spawn_blocking(move || voice::generate_clips(app, &worker, input))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn release_voice_worker(state: State<'_, AppState>) {
    voice::release_worker(&state.voice_worker);
}

#[tauri::command]
fn cancel_voice_generation(state: State<'_, AppState>) {
    voice::cancel_generation(&state.voice_worker);
}

#[tauri::command]
fn list_voice_profiles(app: AppHandle) -> Result<Vec<voice::VoiceProfileSummary>, String> {
    voice::list_profiles(&app)
}

#[tauri::command]
fn delete_voice_profile(
    app: AppHandle,
    profile_id: String,
) -> Result<Vec<voice::VoiceProfileSummary>, String> {
    voice::delete_profile(&app, profile_id)
}

#[tauri::command]
fn set_voice_tone(
    app: AppHandle,
    profile_id: String,
    tone: String,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::set_profile_tone(&app, profile_id, tone)
}

#[tauri::command]
fn rename_voice_profile(
    app: AppHandle,
    profile_id: String,
    display_name: String,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::rename_profile(&app, profile_id, display_name)
}

#[tauri::command]
fn delete_voice_profile_clips(
    app: AppHandle,
    profile_id: String,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::delete_profile_clips(&app, profile_id)
}

#[tauri::command]
fn delete_voice_clip(
    app: AppHandle,
    input: voice::DeleteVoiceClipInput,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::delete_clip(&app, input)
}

// Runs off the main thread: on macOS it can wait for the camera permission prompt.
#[tauri::command(async)]
fn start_webcam_preview(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let settings = lock_sessions(&state)?.settings();
    webcam::start_worker(app, &state.webcam, settings.webcam_gpu, settings.webcam_camera, true, on_webcam_frame)
}

/// Enumeration can take a moment on a slow driver, so it runs off the main thread.
#[tauri::command(async)]
fn list_cameras() -> Vec<String> {
    webcam::camera_names()
}

/// A session that still wants the camera gets a worker without pictures back.
#[tauri::command]
fn stop_webcam_preview(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    webcam::stop_preview(&state.webcam);
    sync_session_webcam(&app);
    Ok(())
}

fn on_webcam_frame(app: &AppHandle, frame: &webcam::WebcamFrame) {
    if frame.error.is_none() {
        with_sessions(app, |sessions, now| {
            sessions.record_webcam(frame.phone_in_frame, frame.present, now);
        });
    }
}

/// Runs the webcam exactly while a session records with it turned on. The preview worker is
/// the Settings page's to stop. Starting happens off this thread (it can wait on a permission
/// prompt), and waits a while after a failure instead of retrying every second.
fn sync_session_webcam(app: &AppHandle) {
    let state = app.state::<AppState>();
    let Ok((wants, settings)) = lock_sessions(&state).map(|sessions| (sessions.wants_webcam(), sessions.settings())) else {
        return;
    };
    let gpu = settings.webcam_gpu;
    let camera = settings.webcam_camera;
    let webcam = &state.webcam;
    // GPU mode that failed once this run would fail again every 30 s; CPU always works.
    let gpu = gpu && !webcam.gpu_failed();
    if wants && webcam.is_stalled() {
        // Frozen, not crashed: kill it so the next tick starts a fresh one.
        eprintln!("webcam: worker sent nothing for a while; restarting it");
        webcam::stop_preview(webcam);
        return;
    }
    if wants {
        if !webcam.is_running() && !webcam.is_starting() && !webcam.recently_failed() {
            let app = app.clone();
            std::thread::spawn(move || {
                let state = app.state::<AppState>();
                if let Err(error) = webcam::start_worker(app.clone(), &state.webcam, gpu, camera, false, on_webcam_frame) {
                    eprintln!("webcam: {error}");
                }
            });
        }
    } else if webcam.is_running() && !webcam.is_preview() {
        webcam::stop_preview(webcam);
    }
}

fn lock_sessions<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, SessionManager>, String> {
    // A panic under the lock must not take the whole app down with it.
    Ok(state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
}

fn database_path(app: &tauri::App) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| error.to_string())
        .map(|dir| dir.join("sessions.db"))
}

pub fn run() {
    #[cfg(all(debug_assertions, target_os = "macos"))]
    if let Ok(path) = std::env::var("SUPERVISOR_PROBE") {
        activity::probe_to_file(&path);
        std::process::exit(0);
    }
    // Before Tauri starts any thread; see `capture_local_offset`.
    storage::capture_local_offset();
    tauri::Builder::default()
        // Must be the first plugin: a second launch focuses this window and exits before
        // it can open the database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .setup(|app| {
            app.manage(AppState {
                sessions: Mutex::new(SessionManager::open(database_path(app))),
                voice_download: Arc::new(Mutex::new(voice::VoiceDownloadState::default())),
                voice_recording: Mutex::new(voice::VoiceRecordingState::default()),
                voice_worker: voice::VoiceWorker::new(),
                webcam: webcam::WebcamPreview::default(),
                started_at: SessionClock::now(),
                identity: supervisor_identity(),
                tray_pause: Mutex::new(None),
            });

            // A focus tracker works while it is out of sight. Without this, App Nap slows the
            // 1 s ticker and the hidden page that plays nudges once Supervisor is covered.
            #[cfg(target_os = "macos")]
            {
                use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
                let activity = NSProcessInfo::processInfo().beginActivityWithOptions_reason(
                    NSActivityOptions::UserInitiatedAllowingIdleSystemSleep,
                    &NSString::from_str("Tracking a focus session"),
                );
                // Held for the life of the app.
                std::mem::forget(activity);
            }
            setup_status_window(app)?;
            if let Some(main) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                size_page_exactly(&main);
            }
            setup_tray(app)?;
            start_session_ticker(app.handle().clone());
            start_voice_worker_ticker(app.handle().clone());
            #[cfg(debug_assertions)]
            start_automation(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_session,
            get_session_snapshot,
            quit_app,
            pause_session,
            resume_session,
            snooze_session,
            stop_session,
            flag_current_distracting,
            allow_current_for_session,
            allow_current_always,
            get_history,
            get_history_week,
            get_week_recap,
            get_month_recap,
            search_sessions,
            get_session_report,
            get_trends,
            get_trend_periods,
            get_focus_days,
            delete_session,
            delete_all_history,
            get_history_size,
            dismiss_storage_notice,
            get_settings,
            get_capture_access,
            open_accessibility_settings,
            get_rules,
            resolve_rule_input,
            get_site_names,
            set_site_name,
            get_app_names,
            set_app_name,
            review_candidates,
            review_session,
            reclassify_session_app,
            set_session_exception,
            update_rules,
            get_phrase_library,
            update_phrase_library,
            update_settings,
            set_status_window_size,
            show_status_window,
            set_status_window_enabled,
            set_status_window_on_top,
            get_voice_backend_status,
            start_voice_model_download,
            get_voice_model_download,
            cancel_voice_model_download,
            delete_voice_cloning_model,
            save_voice_reference_from_path,
            start_voice_recording,
            stop_voice_recording,
            clear_voice_recording,
            generate_voice_clips,
            release_voice_worker,
            cancel_voice_generation,
            list_voice_profiles,
            delete_voice_profile,
            delete_voice_profile_clips,
            set_voice_tone,
            rename_voice_profile,
            delete_voice_clip,
            start_webcam_preview,
            stop_webcam_preview,
            list_cameras,
            automation_result,
            automation_tray,
            automation_input,
            automation_media,
            automation_snapshot
        ])
        .build(tauri::generate_context!())
        .expect("failed to run Supervisor")
        .run(|app, event| match event {
            // Closing the main window hides it, as on any Mac app: the session keeps running,
            // nudges keep playing (the page plays them), and the tray or Dock brings it back.
            // Destroying it would leave nothing for "Show Supervisor" to show.
            tauri::RunEvent::WindowEvent {
                label,
                event: tauri::WindowEvent::CloseRequested { api, .. },
                ..
            } if label == MAIN_WINDOW_LABEL => {
                api.prevent_close();
                if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
                    let _ = window.hide();
                }
                // A Settings camera preview has no reason to run with nothing showing it.
                let state = app.state::<AppState>();
                if state.webcam.is_preview() {
                    webcam::stop_preview(&state.webcam);
                }
            }
            // A Dock click (or a second launch, which macOS turns into this) shows the window.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { .. } => show_main_window(app),
            // Managed state is never dropped (the ticker threads hold AppHandles), so stop the
            // voice worker here; outside Windows nothing else kills it when the app exits.
            tauri::RunEvent::Exit => {
                let state = app.state::<AppState>();
                // Quitting mid-session ends it as stopped, saving its last seconds, rather than
                // leaving it open for the next launch to mark abandoned.
                let mut sessions = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                let now = state.started_at.elapsed();
                sessions.stop(now);
                drop(sessions);
                // Cancel first: a release alone waits for a running clip to finish.
                voice::cancel_generation(&state.voice_worker);
                voice::release_worker(&state.voice_worker);
                webcam::stop_preview(&state.webcam);
            }
            _ => {}
        });
}

fn setup_status_window(app: &mut tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        OVERLAY_WINDOW_LABEL,
        WebviewUrl::App("index.html?view=overlay".into()),
    )
    .title("Supervisor Status")
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT)
    .resizable(false)
    .maximizable(false)
    .minimizable(true)
    .closable(false)
    .decorations(true)
    .always_on_top(true)
    .skip_taskbar(false)
    .accept_first_mouse(true)
    .focused(false)
    .visible(false)
    // A distraction in a full-screen app or on another desktop must show its card there.
    .visible_on_all_workspaces(true)
    .build()?;

    #[cfg(target_os = "macos")]
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        use objc2_app_kit::{NSWindow, NSWindowCollectionBehavior};
        if let Ok(pointer) = window.ns_window() {
            let ns_window: &NSWindow = unsafe { &*pointer.cast::<NSWindow>() };
            ns_window.setCollectionBehavior(
                ns_window.collectionBehavior()
                    | NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
        }
    }

    Ok(())
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Supervisor", true, None::<&str>)?;
    let show_status =
        MenuItem::with_id(app, "show_status", "Show Status Window", true, None::<&str>)?;
    let pin_status = MenuItem::with_id(app, "pin_status", "Pin Status Window", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_resume", "Pause", true, None::<&str>)?;
    let snooze = MenuItem::with_id(app, "snooze", "Mute for 5 min", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "End Session", true, None::<&str>)?;
    if let Ok(mut slot) = app.state::<AppState>().tray_pause.lock() {
        *slot = Some(pause.clone());
    }
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &show_status,
            &pin_status,
            &pause,
            &snooze,
            &stop,
            &quit,
        ],
    )?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| tray_action(app, event.id.as_ref()))
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

/// What each tray menu item does. One function so the debug hook can run the same code.
fn tray_action(app: &AppHandle, id: &str) {
    match id {
        "show" => show_main_window(app),
        "show_status" => {
            with_sessions(app, |sessions, now| {
                sessions.set_status_window_enabled(true, now);
            });
            let _ = show_status_window_internal(app);
        }
        "pin_status" => {
            with_sessions(app, |sessions, now| {
                sessions.set_status_window_enabled(true, now);
                sessions.set_status_window_on_top(true, now);
            });
            let _ = set_status_window_on_top_internal(app, true);
            let _ = show_status_window_internal(app);
        }
        "pause_resume" => with_sessions(app, |sessions, now| sessions.toggle_pause(now)),
        "snooze" => with_sessions(app, |sessions, now| {
            sessions.snooze(now, 5 * 60);
        }),
        "stop" => {
            let mut ended = false;
            with_sessions(app, |sessions, now| {
                ended = sessions.has_current();
                sessions.stop(now);
            });
            if ended {
                session_ended(app);
            } else {
                hide_status_window(app);
            }
        }
        "quit" => quit(app),
        _ => {}
    }
}

/// Debug builds only, for the automation hook: runs a tray menu item by id.
#[tauri::command]
fn automation_tray(id: String, app: AppHandle) -> Result<(), String> {
    if !cfg!(debug_assertions) || std::env::var_os("SUPERVISOR_AUTOMATION").is_none() {
        return Err("automation is off".to_string());
    }
    tray_action(&app, &id);
    Ok(())
}

/// Debug builds only, for the automation hook: a keystroke or click as the user would make
/// it, posted by this process (which holds the Accessibility grant). Lets the Mac pass drive
/// Spotlight, Cmd-Tab and Mission Control without a person at the keyboard.
#[tauri::command]
fn automation_input(
    kind: String,
    key: Option<u16>,
    cmd: Option<bool>,
    ctrl: Option<bool>,
    alt: Option<bool>,
    x: Option<f64>,
    y: Option<f64>,
    hold_ms: Option<u64>,
) -> Result<(), String> {
    if !cfg!(debug_assertions) || std::env::var_os("SUPERVISOR_AUTOMATION").is_none() {
        return Err("automation is off".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGMouseButton};
        let hold = Duration::from_millis(hold_ms.unwrap_or(60));
        let post = |event: Option<objc2_core_foundation::CFRetained<CGEvent>>| {
            CGEvent::post(CGEventTapLocation::HIDEventTap, event.as_deref());
        };
        match kind.as_str() {
            "key" => {
                let key = key.ok_or("key needed")?;
                let mut flags = CGEventFlags::empty();
                if cmd.unwrap_or(false) { flags |= CGEventFlags(1 << 20); }
                if ctrl.unwrap_or(false) { flags |= CGEventFlags::MaskControl; }
                if alt.unwrap_or(false) { flags |= CGEventFlags::MaskAlternate; }
                let down = CGEvent::new_keyboard_event(None, key, true);
                CGEvent::set_flags(down.as_deref(), flags);
                post(down);
                std::thread::sleep(hold);
                let up = CGEvent::new_keyboard_event(None, key, false);
                CGEvent::set_flags(up.as_deref(), flags);
                post(up);
            }
            "click" => {
                let point = objc2_core_foundation::CGPoint::new(x.ok_or("x needed")?, y.ok_or("y needed")?);
                post(CGEvent::new_mouse_event(None, CGEventType::MouseMoved, point, CGMouseButton::Left));
                std::thread::sleep(Duration::from_millis(80));
                post(CGEvent::new_mouse_event(None, CGEventType::LeftMouseDown, point, CGMouseButton::Left));
                std::thread::sleep(hold);
                post(CGEvent::new_mouse_event(None, CGEventType::LeftMouseUp, point, CGMouseButton::Left));
            }
            other => return Err(format!("unknown input kind {other}")),
        }
        return Ok(());
    }
    #[allow(unreachable_code)]
    {
        let _ = (kind, key, cmd, ctrl, alt, x, y, hold_ms);
        Err("only on macOS".to_string())
    }
}

/// Debug builds only, for the automation hook: whether `pid` keeps the display awake (what
/// media presence reads on macOS), so the check can be driven against a playing video.
#[tauri::command]
fn automation_media(pid: u64) -> Result<bool, String> {
    if !cfg!(debug_assertions) || std::env::var_os("SUPERVISOR_AUTOMATION").is_none() {
        return Err("automation is off".to_string());
    }
    #[cfg(target_os = "macos")]
    {
        return Ok(macos::keeps_display_awake(pid));
    }
    #[allow(unreachable_code)]
    {
        let _ = pid;
        Err("only on macOS".to_string())
    }
}

/// Debug builds only, for the automation hook: the window's page rendered by its own
/// webview to a TIFF at `path` (no Screen Recording grant needed). `sips` turns it into PNG.
#[tauri::command]
fn automation_snapshot(label: String, path: String, height: Option<f64>, app: AppHandle) -> Result<(), String> {
    if !cfg!(debug_assertions) || std::env::var_os("SUPERVISOR_AUTOMATION").is_none() {
        return Err("automation is off".to_string());
    }
    let window = app.get_webview_window(&label).ok_or("no such window")?;
    window
        .with_webview(move |webview| {
            #[cfg(target_os = "macos")]
            unsafe {
                use objc2_app_kit::NSImage;
                use objc2_foundation::{NSError, NSString};
                use objc2_web_kit::WKWebView;
                let web_view: &WKWebView = &*webview.inner().cast::<WKWebView>();
                // Only the page the user sees: the view's bounds run on under the title bar.
                let configuration = objc2_web_kit::WKSnapshotConfiguration::new(objc2::MainThreadMarker::new_unchecked());
                if let Some(height) = height {
                    let bounds = web_view.bounds();
                    configuration.setRect(objc2_core_foundation::CGRect::new(
                        objc2_core_foundation::CGPoint::new(0.0, 0.0),
                        objc2_core_foundation::CGSize::new(bounds.size.width, height),
                    ));
                }
                let handler = block2::RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
                    if let Some(image) = image.as_ref() {
                        if let Some(tiff) = image.TIFFRepresentation() {
                            let _ = tiff.writeToFile_atomically(&NSString::from_str(&path), true);
                        }
                    }
                });
                web_view.takeSnapshotWithConfiguration_completionHandler(Some(&configuration), &handler);
            }
            #[cfg(not(target_os = "macos"))]
            {
                let _ = (webview, path, height);
            }
        })
        .map_err(|error| error.to_string())
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW_LABEL) {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn show_status_window_internal(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let pin = app
            .state::<AppState>()
            .sessions
            .lock()
            .map(|sessions| sessions.status_window_on_top())
            .unwrap_or(true);
        let _ = window.unminimize();
        let _ = window.set_always_on_bottom(false);
        window
            .set_always_on_top(pin)
            .map_err(|error| error.to_string())?;
        position_status_window(app)?;
        window.show().map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn set_status_window_on_top_internal(app: &AppHandle, enabled: bool) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let _ = window.set_always_on_bottom(false);
        if enabled {
            window
                .set_always_on_top(true)
                .map_err(|error| error.to_string())?;
        } else {
            window
                .set_always_on_top(false)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn hide_status_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) {
        let _ = window.hide();
    }
}

/// A session ended, run out or stopped: the status window goes, and the main window plays a
/// closing line. Sent once per session, from wherever it ended.
const SESSION_END_EVENT: &str = "session-end";

fn session_ended(app: &AppHandle) {
    hide_status_window(app);
    let _ = app.emit(SESSION_END_EVENT, ());
}

/// Quit while a session is running asks first, in the main window ("End this session and
/// quit?"); its answer comes back as `quit_app`. With no session it quits at once.
const QUIT_REQUESTED_EVENT: &str = "quit-requested";

fn quit(app: &AppHandle) {
    let state = app.state::<AppState>();
    let running = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).has_current();
    if !running {
        app.exit(0);
        return;
    }
    show_main_window(app);
    let _ = app.emit(QUIT_REQUESTED_EVENT, ());
}

/// "End and quit": the exit handler ends the session as stopped, without a closing line.
#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn set_status_window_size(
    width: f64,
    height: f64,
    center: bool,
    app: AppHandle,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return Ok(());
    };
    let width = width.clamp(200.0, 800.0);
    let height = height.clamp(120.0, 960.0) + titlebar_inset(&window);
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|error| error.to_string())?;
    // A card (centred) shows whether or not the session timer is on: with the timer off the
    // window is hidden the rest of the time, so it comes out for the card and goes back after.
    let timer_shown = app
        .try_state::<AppState>()
        .map(|state| state.sessions.lock().map(|sessions| sessions.status_window_enabled()).unwrap_or(true))
        .unwrap_or(true);
    if center {
        let _ = window.set_always_on_top(true);
        if !timer_shown {
            let _ = window.show();
        }
    } else if !timer_shown {
        let _ = window.hide();
    } else if let Some(state) = app.try_state::<AppState>() {
        let pin = state
            .sessions
            .lock()
            .map(|sessions| sessions.status_window_on_top())
            .unwrap_or(true);
        let _ = window.set_always_on_top(pin);
    }
    place_status_window(&app, Some((width, height)), center)
}

/// How much of a window's "inner" height the title bar takes. On macOS the window's content
/// view runs under the title bar (Tauri's full-size content view), the page is laid out below
/// it, and so a window asked for 286 shows 258 of page. On Windows the inner size is the page.
fn titlebar_inset(window: &tauri::WebviewWindow) -> f64 {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSWindow;
        if let Ok(pointer) = window.ns_window() {
            let ns_window: &NSWindow = unsafe { &*pointer.cast::<NSWindow>() };
            let frame = ns_window.frame();
            let content = ns_window.contentLayoutRect();
            return (frame.size.height - content.size.height).max(0.0);
        }
    }
    let _ = window;
    0.0
}

/// Give the page the size the config asked for, title bar not included.
fn size_page_exactly(window: &tauri::WebviewWindow) {
    let inset = titlebar_inset(window);
    if inset <= 0.0 {
        return;
    }
    let Ok(scale) = window.scale_factor() else { return };
    let Ok(inner) = window.inner_size() else { return };
    let logical: tauri::LogicalSize<f64> = inner.to_logical(scale);
    let _ = window.set_size(tauri::LogicalSize::new(logical.width, logical.height + inset));
}

fn position_status_window(app: &AppHandle) -> Result<(), String> {
    place_status_window(app, None, false)
}

fn place_status_window(
    app: &AppHandle,
    logical: Option<(f64, f64)>,
    center: bool,
) -> Result<(), String> {
    let Some(window) = app.get_webview_window(OVERLAY_WINDOW_LABEL) else {
        return Ok(());
    };
    let monitor = app
        .get_webview_window(MAIN_WINDOW_LABEL)
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| app.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return Ok(());
    };
    let area = monitor.work_area();
    let scale = window
        .scale_factor()
        .unwrap_or_else(|_| monitor.scale_factor());
    let margin = (f64::from(OVERLAY_MARGIN) * scale).round() as i32;
    let fallback_size = tauri::PhysicalSize::new(
        (OVERLAY_WIDTH * scale).round() as u32,
        (OVERLAY_HEIGHT * scale).round() as u32,
    );
    let size = if let Some((width, height)) = logical {
        tauri::PhysicalSize::new((width * scale).round() as u32, (height * scale).round() as u32)
    } else {
        window.outer_size().unwrap_or(fallback_size)
    };
    let min_x = area.position.x + margin;
    let max_x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
    let (x, y) = if center {
        (
            area.position.x + (area.size.width as i32 - size.width as i32) / 2,
            area.position.y + (area.size.height as i32 - size.height as i32) / 2,
        )
    } else {
        (max_x.max(min_x), area.position.y + margin)
    };

    window
        .set_position(tauri::PhysicalPosition::new(x, y))
        .map_err(|error| error.to_string())
}

fn start_voice_worker_ticker(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(5));
        let state = app.state::<AppState>();
        voice::tick_worker(&state.voice_worker);
    });
}

/// Tells the main window the moment a nudge is due. Its own poll is a webview timer, which the
/// OS throttles while the window is minimized or hidden (up to a minute late); this thread isn't.
const NUDGE_EVENT: &str = "session-nudge";

/// Whether the snapshot has a nudge the main window hasn't been told about. `seen` is the last
/// announced (session id, nudge count).
fn new_nudge(seen: &mut (u64, u64), snapshot: &SessionSnapshot) -> bool {
    let Some(current) = snapshot.current.as_ref() else {
        return false;
    };
    counted_new_nudge(seen, current.id, current.metrics.nudges)
}

/// A Pomodoro round ended ("break") or its break ran out ("break_over"): the main window plays
/// a line for it and the status window shows its card. Sent once per moment, like a nudge.
const MOMENT_EVENT: &str = "session-moment";

fn new_moment(seen: &mut (u64, u64), snapshot: &SessionSnapshot) -> Option<&'static str> {
    let current = snapshot.current.as_ref()?;
    let pomodoro = current.pomodoro.as_ref()?;
    counted_new_nudge(seen, current.id, pomodoro.moments).then_some(pomodoro.phase)
}

fn counted_new_nudge(seen: &mut (u64, u64), id: u64, nudges: u64) -> bool {
    let before = if seen.0 == id { seen.1 } else { 0 };
    *seen = (id, nudges);
    nudges > before
}

/// Debug builds only: with `SUPERVISOR_AUTOMATION=<dir>`, a script dropped at `<dir>/cmd.js`
/// runs inside the main window's page (as the body of an async function; `return` a
/// JSON-serializable value) and its result lands in `<dir>/result.json`. This is how the
/// app's own flows are driven end to end on a Mac, where no WebDriver exists.
#[cfg(debug_assertions)]
fn start_automation(app: AppHandle) {
    let Some(dir) = std::env::var_os("SUPERVISOR_AUTOMATION").map(PathBuf::from) else {
        return;
    };
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(200));
        let script_path = dir.join("cmd.js");
        let Ok(script) = std::fs::read_to_string(&script_path) else {
            continue;
        };
        let _ = std::fs::remove_file(&script_path);
        let _ = std::fs::remove_file(dir.join("result.json"));
        // A first line `//window=overlay` targets another window; the main window otherwise.
        let label = script
            .lines()
            .next()
            .and_then(|line| line.trim().strip_prefix("//window="))
            .map(str::trim)
            .unwrap_or(MAIN_WINDOW_LABEL)
            .to_string();
        let Some(window) = app.get_webview_window(&label) else {
            let _ = std::fs::write(dir.join("result.json"), format!("{{\"error\":\"no window {label}\"}}"));
            continue;
        };
        let wrapped = format!(
            "(async () => {{ let out; try {{ out = {{ ok: await (async () => {{ {script} }})() }}; }} \
             catch (error) {{ out = {{ error: String(error && error.stack || error) }}; }} \
             window.__TAURI_INTERNALS__.invoke('automation_result', {{ text: JSON.stringify(out) }}); }})();"
        );
        if let Err(error) = window.eval(&wrapped) {
            let _ = std::fs::write(dir.join("result.json"), format!("{{\"error\":{:?}}}", error.to_string()));
        }
    });
}

/// The receiving end of `start_automation`; inert in release builds and without the variable.
#[tauri::command]
fn automation_result(text: String) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Err("automation is off".to_string());
    }
    let dir = std::env::var_os("SUPERVISOR_AUTOMATION").map(PathBuf::from).ok_or("automation is off")?;
    let tmp = dir.join("result.json.part");
    std::fs::write(&tmp, text).map_err(|error| error.to_string())?;
    std::fs::rename(&tmp, dir.join("result.json")).map_err(|error| error.to_string())
}

fn start_session_ticker(app: AppHandle) {
    let mut last_loop = app.state::<AppState>().started_at.elapsed();
    let mut nudges_seen = (0, 0);
    let mut moments_seen = (0, 0);
    let mut was_running = false;
    let mut tray_label = String::new();
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));

        let (ended_after_tick, should_poll, pause_label) = {
            let state = app.state::<AppState>();
            let now = state.started_at.elapsed();
            let mut sessions = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            // This loop runs every second; a long silence means the machine slept or froze.
            if now.saturating_sub(last_loop) > session::LOST_TIME_AFTER {
                sessions.record_lost_time(last_loop, now);
            }
            last_loop = now;
            let had_current = sessions.has_current();
            // A new session starts its counts over, whatever its id (0 with history off).
            if had_current && !was_running {
                nudges_seen = (0, 0);
                moments_seen = (0, 0);
            }
            was_running = had_current;
            let snapshot = sessions.tick(now);
            // A phone nudge can come from a webcam frame or this tick; both show up here.
            if new_nudge(&mut nudges_seen, &snapshot) {
                let _ = app.emit(NUDGE_EVENT, ());
            }
            if let Some(phase) = new_moment(&mut moments_seen, &snapshot) {
                let _ = app.emit(MOMENT_EVENT, phase);
            }
            (
                had_current && matches!(snapshot.status, session::SessionStatus::Idle),
                sessions.should_poll_activity(),
                tray_pause_label(&snapshot),
            )
        };
        set_tray_pause_label(&app, &mut tray_label, pause_label);
        if ended_after_tick {
            session_ended(&app);
        }
        sync_session_webcam(&app);
        if !should_poll {
            continue;
        }

        let Ok(sample) = ({
            let state = app.state::<AppState>();
            current_activity_sample(&state)
        }) else {
            let state = app.state::<AppState>();
            let now = state.started_at.elapsed();
            let mut sessions = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            last_loop = now;
            sessions.record_capture_gap(now);
            let had_current = sessions.has_current();
            let snapshot = sessions.tick(now);
            if had_current && matches!(snapshot.status, session::SessionStatus::Idle) {
                session_ended(&app);
            }
            continue;
        };

        let ended = {
            let state = app.state::<AppState>();
            let now = state.started_at.elapsed();
            let identity = state.identity.clone();
            let mut sessions = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let was_polling = sessions.should_poll_activity();
            let snapshot = sessions.record_sample(sample, now, &identity);
            // A slow capture (a hung Accessibility read) is not the machine sleeping.
            last_loop = now;
            if new_nudge(&mut nudges_seen, &snapshot) {
                let _ = app.emit(NUDGE_EVENT, ());
            }
            if let Some(phase) = new_moment(&mut moments_seen, &snapshot) {
                let _ = app.emit(MOMENT_EVENT, phase);
            }
            was_polling && matches!(snapshot.status, session::SessionStatus::Idle)
        };

        if ended {
            session_ended(&app);
        }
    });
}

fn current_activity_sample(_state: &State<'_, AppState>) -> Result<ActivitySample, String> {
    activity::current_activity_sample()
}

fn supervisor_identity() -> SupervisorIdentity {
    SupervisorIdentity {
        process_id: u64::from(std::process::id()),
        process_path: std::env::current_exe()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_default(),
    }
}

fn with_sessions(
    app: &tauri::AppHandle,
    action: impl FnOnce(&mut SessionManager, std::time::Duration),
) {
    let state = app.state::<AppState>();
    // The lock first, then the clock (see the commands); and a panic under the lock must not
    // make every tray action and webcam frame vanish for the rest of the run.
    let mut sessions = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let now = state.started_at.elapsed();
    action(&mut sessions, now);
}

#[cfg(test)]
mod tests {
    use super::counted_new_nudge;

    #[test]
    fn each_nudge_is_announced_once_per_session() {
        let mut seen = (0, 0);
        assert!(!counted_new_nudge(&mut seen, 1, 0));
        assert!(counted_new_nudge(&mut seen, 1, 1));
        assert!(!counted_new_nudge(&mut seen, 1, 1));
        assert!(counted_new_nudge(&mut seen, 1, 2));
        // A new session starts counting from zero, even if the old one had more.
        assert!(!counted_new_nudge(&mut seen, 2, 0));
        assert!(counted_new_nudge(&mut seen, 2, 1));
    }
}
