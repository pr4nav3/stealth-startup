mod activity;
mod focus;
mod session;
mod storage;
mod voice;
mod voice_runtime;
mod webcam;

use activity::ActivitySample;
use focus::{FocusEngine, FocusSnapshot};
use session::{
    SessionManager, SessionSettings, SessionSnapshot, SessionSummary, SettingsUpdate,
    StartSessionInput, SupervisorIdentity,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use storage::{SessionReport, SessionStorage, StoredPhraseLibrary, TrendsData};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder,
};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

const MAIN_WINDOW_LABEL: &str = "main";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 228.0;
const OVERLAY_HEIGHT: f64 = 214.0;
const OVERLAY_MARGIN: i32 = 18;

struct AppState {
    focus_engine: Mutex<FocusEngine>,
    sessions: Mutex<SessionManager>,
    storage: Mutex<SessionStorage>,
    voice_download: Arc<Mutex<voice::VoiceDownloadState>>,
    voice_recording: Mutex<voice::VoiceRecordingState>,
    webcam: webcam::WebcamPreview,
    started_at: Instant,
    identity: SupervisorIdentity,
}

#[tauri::command]
fn get_activity_sample(state: State<'_, AppState>) -> Result<ActivitySample, String> {
    current_activity_sample(&state)
}

#[tauri::command]
fn get_focus_snapshot(state: State<'_, AppState>) -> Result<FocusSnapshot, String> {
    let sample = current_activity_sample(&state)?;
    let mut engine = state
        .focus_engine
        .lock()
        .map_err(|_| "focus engine unavailable".to_string())?;

    Ok(engine.record_sample(&sample, state.started_at.elapsed()))
}

#[tauri::command]
fn start_session(
    input: StartSessionInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    sessions.start(input, now, utc_now()?, Some(&*storage))?;
    let should_show_status = sessions.status_window_enabled();
    let snapshot = sessions.snapshot(now);
    drop(sessions);

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
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.snapshot(now))
}

#[tauri::command]
fn pause_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    Ok(sessions.pause(now, Some(&*storage)))
}

#[tauri::command]
fn resume_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    Ok(sessions.resume(now, Some(&*storage)))
}

#[tauri::command]
fn snooze_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    Ok(sessions.snooze(now, 5 * 60, Some(&*storage)))
}

#[tauri::command]
fn clear_snooze(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    Ok(sessions.clear_snooze(now, Some(&*storage)))
}

#[tauri::command]
fn stop_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    let snapshot = sessions.stop(now, &utc_now()?, Some(&*storage));
    drop(sessions);

    hide_status_window(&app);
    Ok(snapshot)
}

#[tauri::command]
fn mark_current_relevant(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    sessions.mark_current(focus::RuleClassification::Focused, now, Some(&*storage))
}

#[tauri::command]
fn mark_current_distracting(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;

    sessions.mark_current(focus::RuleClassification::Distracting, now, Some(&*storage))
}

#[tauri::command]
fn update_session_rules(
    rules: Vec<focus::AppRule>,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    sessions.update_current_rules(rules, now, Some(&*storage))
}

#[tauri::command]
fn get_history(state: State<'_, AppState>) -> Result<Vec<SessionSummary>, String> {
    Ok(lock_sessions(&state)?.history())
}

#[tauri::command]
fn get_session_report(id: u64, state: State<'_, AppState>) -> Result<SessionReport, String> {
    Ok(lock_storage(&state)?.get_session_report(id)?)
}

#[tauri::command]
fn get_trends(state: State<'_, AppState>) -> Result<TrendsData, String> {
    Ok(lock_storage(&state)?.get_trends()?)
}

#[tauri::command]
fn delete_session(id: u64, state: State<'_, AppState>) -> Result<(), String> {
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    storage.delete_session(id)?;
    sessions.refresh_history_cache(&storage);
    Ok(())
}

#[tauri::command]
fn delete_all_history(state: State<'_, AppState>) -> Result<(), String> {
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    storage.delete_all_history()?;
    sessions.refresh_history_cache(&storage);
    Ok(())
}

#[tauri::command]
fn scrub_span_context(state: State<'_, AppState>) -> Result<(), String> {
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    storage.scrub_span_context()?;
    sessions.refresh_history_cache(&storage);
    Ok(())
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SessionSettings, String> {
    Ok(lock_sessions(&state)?.settings())
}

#[tauri::command]
fn get_rules(state: State<'_, AppState>) -> Result<Vec<focus::AppRule>, String> {
    Ok(lock_sessions(&state)?.rules())
}

#[tauri::command]
fn update_rules(
    rules: Vec<focus::AppRule>,
    state: State<'_, AppState>,
) -> Result<Vec<focus::AppRule>, String> {
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    sessions.update_rules(rules, Some(&*storage))
}

#[tauri::command]
fn get_phrase_library(state: State<'_, AppState>) -> Result<StoredPhraseLibrary, String> {
    lock_storage(&state)?.load_phrase_library()
}

#[tauri::command]
fn update_phrase_library(
    library: StoredPhraseLibrary,
    state: State<'_, AppState>,
) -> Result<StoredPhraseLibrary, String> {
    lock_storage(&state)?.save_phrase_library(library)
}

#[tauri::command]
fn update_settings(
    update: SettingsUpdate,
    state: State<'_, AppState>,
) -> Result<SessionSettings, String> {
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    sessions.update_settings(update, Some(&*storage))
}

#[tauri::command]
fn show_status_window(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    let snapshot = sessions.set_status_window_enabled(true, now, Some(&*storage))?;
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
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    let snapshot = sessions.set_status_window_enabled(enabled, now, Some(&*storage))?;
    let should_show_status = enabled && snapshot.current.is_some();
    drop(sessions);

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
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;
    let storage = lock_storage(&state)?;
    let snapshot = sessions.set_status_window_on_top(enabled, now, Some(&*storage))?;
    drop(sessions);

    set_status_window_on_top_internal(&app, enabled)?;
    if enabled {
        show_status_window_internal(&app)?;
    }
    Ok(snapshot)
}

#[tauri::command]
fn get_voice_backend_status(app: AppHandle) -> Result<voice::VoiceBackendStatus, String> {
    voice::get_status(&app)
}

#[tauri::command]
async fn download_voice_cloning_model(app: AppHandle) -> Result<voice::VoiceModelStatus, String> {
    tauri::async_runtime::spawn_blocking(move || voice::download_model(app))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
fn start_voice_model_download(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceDownloadSnapshot, String> {
    voice::start_model_download(app, Arc::clone(&state.voice_download))
}

#[tauri::command]
fn get_voice_model_download(
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
fn delete_voice_cloning_model(app: AppHandle) -> Result<voice::VoiceModelStatus, String> {
    voice::delete_model(&app)
}

#[tauri::command]
fn save_voice_reference(
    app: AppHandle,
    input: voice::SaveVoiceReferenceInput,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::save_reference(&app, input)
}

#[tauri::command]
fn save_voice_reference_from_path(
    app: AppHandle,
    input: voice::SaveVoiceReferenceFromPathInput,
) -> Result<voice::VoiceProfileSummary, String> {
    voice::save_reference_from_path(&app, input)
}

#[tauri::command]
fn start_voice_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<voice::VoiceRecordingSnapshot, String> {
    voice::start_recording(&app, &state.voice_recording)
}

#[tauri::command]
fn stop_voice_recording(
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
    input: voice::GenerateVoiceClipsInput,
) -> Result<voice::VoiceProfileSummary, String> {
    tauri::async_runtime::spawn_blocking(move || voice::generate_clips(app, input))
        .await
        .map_err(|error| error.to_string())?
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

#[tauri::command]
fn start_webcam_preview(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    webcam::start_preview(app, &state.webcam)
}

#[tauri::command]
fn stop_webcam_preview(state: State<'_, AppState>) -> Result<(), String> {
    webcam::stop_preview(&state.webcam);
    Ok(())
}

fn lock_sessions<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, SessionManager>, String> {
    state
        .sessions
        .lock()
        .map_err(|_| "session manager unavailable".to_string())
}

fn lock_storage<'a>(
    state: &'a State<'_, AppState>,
) -> Result<std::sync::MutexGuard<'a, SessionStorage>, String> {
    state
        .storage
        .lock()
        .map_err(|_| "session storage unavailable".to_string())
}

fn database_path(app: &tauri::App) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| error.to_string())
        .map(|dir| dir.join("sessions.db"))
}

fn utc_now() -> Result<String, String> {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())
}

fn utc_now_fallback() -> String {
    utc_now().unwrap_or_else(|error| {
        eprintln!("supervisor storage: utc_now: {error}");
        OffsetDateTime::UNIX_EPOCH
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
    })
}

fn bootstrap_sessions_from_storage(
    storage: SessionStorage,
) -> Result<(SessionStorage, SessionManager), String> {
    let settings = storage.load_settings()?;
    let rules = storage.load_rules()?;
    let next_id = storage.next_session_id()?;
    let history = storage
        .list_history(session::HISTORY_CACHE_LIMIT)
        .unwrap_or_else(|error| {
            eprintln!("supervisor storage: load history cache: {error}");
            Vec::new()
        });
    Ok((
        storage,
        SessionManager::bootstrap(settings, rules, next_id, history),
    ))
}

fn open_storage_for_app(app: &tauri::App) -> SessionStorage {
    let path = match database_path(app) {
        Ok(path) => path,
        Err(error) => {
            eprintln!("supervisor storage: app data path: {error}");
            return SessionStorage::open_in_memory().expect("in-memory session database must open");
        }
    };
    match SessionStorage::open(&path) {
        Ok(storage) => storage,
        Err(error) => {
            eprintln!("supervisor storage: open disk database: {error}");
            SessionStorage::open_in_memory().expect("in-memory session database must open")
        }
    }
}

fn init_app_storage_and_sessions(app: &tauri::App) -> (SessionStorage, SessionManager) {
    let disk = open_storage_for_app(app);
    match bootstrap_sessions_from_storage(disk) {
        Ok(pair) => pair,
        Err(error) => {
            eprintln!("supervisor storage: bootstrap failed, using in-memory database: {error}");
            let storage =
                SessionStorage::open_in_memory().expect("in-memory session database must open");
            bootstrap_sessions_from_storage(storage).unwrap_or_else(|fallback_error| {
                eprintln!("supervisor storage: in-memory bootstrap: {fallback_error}");
                let storage =
                    SessionStorage::open_in_memory().expect("in-memory session database must open");
                (storage, SessionManager::default())
            })
        }
    }
}

pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let (storage, sessions) = init_app_storage_and_sessions(app);

            app.manage(AppState {
                focus_engine: Mutex::new(FocusEngine::default()),
                sessions: Mutex::new(sessions),
                storage: Mutex::new(storage),
                voice_download: Arc::new(Mutex::new(voice::VoiceDownloadState::default())),
                voice_recording: Mutex::new(voice::VoiceRecordingState::default()),
                webcam: webcam::WebcamPreview::default(),
                started_at: Instant::now(),
                identity: supervisor_identity(),
            });

            setup_status_window(app)?;
            apply_window_icons(app);
            setup_tray(app)?;
            start_session_ticker(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_activity_sample,
            get_focus_snapshot,
            start_session,
            get_session_snapshot,
            pause_session,
            resume_session,
            snooze_session,
            clear_snooze,
            stop_session,
            mark_current_relevant,
            mark_current_distracting,
            update_session_rules,
            get_history,
            get_session_report,
            get_trends,
            delete_session,
            delete_all_history,
            scrub_span_context,
            get_settings,
            get_rules,
            update_rules,
            get_phrase_library,
            update_phrase_library,
            update_settings,
            show_status_window,
            set_status_window_enabled,
            set_status_window_on_top,
            set_status_window_size,
            get_voice_backend_status,
            download_voice_cloning_model,
            start_voice_model_download,
            get_voice_model_download,
            cancel_voice_model_download,
            delete_voice_cloning_model,
            save_voice_reference,
            save_voice_reference_from_path,
            start_voice_recording,
            stop_voice_recording,
            clear_voice_recording,
            generate_voice_clips,
            list_voice_profiles,
            delete_voice_profile,
            delete_voice_profile_clips,
            delete_voice_clip,
            start_webcam_preview,
            stop_webcam_preview
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Supervisor");
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
    .build()?;

    Ok(())
}

fn apply_window_icons(app: &tauri::App) {
    let Some(icon) = app.default_window_icon() else {
        return;
    };
    for label in [MAIN_WINDOW_LABEL, OVERLAY_WINDOW_LABEL] {
        if let Some(window) = app.get_webview_window(label) {
            let _ = window.set_icon(icon.clone());
        }
    }
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Supervisor", true, None::<&str>)?;
    let show_status =
        MenuItem::with_id(app, "show_status", "Show Status Window", true, None::<&str>)?;
    let pin_status = MenuItem::with_id(app, "pin_status", "Pin Status Window", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_resume", "Pause/Resume", true, None::<&str>)?;
    let snooze = MenuItem::with_id(app, "snooze", "Snooze 5 min", true, None::<&str>)?;
    let stop = MenuItem::with_id(app, "stop", "Stop Session", true, None::<&str>)?;
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
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "show_status" => {
                with_sessions(app, |sessions, now, storage| {
                    let _ = sessions.set_status_window_enabled(true, now, Some(storage));
                });
                let _ = show_status_window_internal(app);
            }
            "pin_status" => {
                with_sessions(app, |sessions, now, storage| {
                    let _ = sessions.set_status_window_enabled(true, now, Some(storage));
                    let _ = sessions.set_status_window_on_top(true, now, Some(storage));
                });
                let _ = set_status_window_on_top_internal(app, true);
                let _ = show_status_window_internal(app);
            }
            "pause_resume" => with_sessions(app, |sessions, now, storage| {
                sessions.toggle_pause(now, Some(storage));
            }),
            "snooze" => with_sessions(app, |sessions, now, storage| {
                sessions.snooze(now, 5 * 60, Some(storage));
            }),
            "stop" => with_sessions(app, |sessions, now, storage| {
                let ended_at = utc_now_fallback();
                sessions.stop(now, &ended_at, Some(storage));
                hide_status_window(app);
            }),
            "quit" => app.exit(0),
            _ => {}
        })
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
    let height = height.clamp(120.0, 960.0);
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|error| error.to_string())?;
    if center {
        let _ = window.set_always_on_top(true);
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
        tauri::PhysicalSize::new(
            (width * scale).round() as u32,
            (height * scale).round() as u32,
        )
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

fn start_session_ticker(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));

        let (ended_after_tick, should_poll) = {
            let state = app.state::<AppState>();
            let now = state.started_at.elapsed();
            let Ok(mut sessions) = state.sessions.lock() else {
                continue;
            };
            let Ok(storage) = state.storage.lock() else {
                continue;
            };
            let had_current = sessions.has_current();
            let snapshot = sessions.tick(now, Some(&*storage));
            (
                had_current && matches!(snapshot.status, session::SessionStatus::Idle),
                sessions.should_poll_activity(),
            )
        };
        if ended_after_tick {
            hide_status_window(&app);
        }
        if !should_poll {
            continue;
        }

        let Ok(sample) = ({
            let state = app.state::<AppState>();
            current_activity_sample(&state)
        }) else {
            continue;
        };

        let ended = {
            let state = app.state::<AppState>();
            let now = state.started_at.elapsed();
            let identity = state.identity.clone();
            let Ok(mut sessions) = state.sessions.lock() else {
                continue;
            };
            let Ok(storage) = state.storage.lock() else {
                continue;
            };
            let was_polling = sessions.should_poll_activity();
            let snapshot = sessions.record_sample(sample, now, &identity, Some(&*storage));
            was_polling && matches!(snapshot.status, session::SessionStatus::Idle)
        };

        if ended {
            hide_status_window(&app);
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
        app_name: "Supervisor".to_string(),
    }
}

fn with_sessions(
    app: &tauri::AppHandle,
    action: impl FnOnce(&mut SessionManager, std::time::Duration, &SessionStorage),
) {
    let state = app.state::<AppState>();
    let now = state.started_at.elapsed();
    if let Ok(mut sessions) = state.sessions.lock() {
        if let Ok(storage) = state.storage.lock() {
            action(&mut sessions, now, &storage);
        }
    };
}
