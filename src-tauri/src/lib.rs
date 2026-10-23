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
use storage::{HistoryWeek, WeekRecap, SessionReport, StoredPhraseLibrary, TrendsData};
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

struct AppState {
    sessions: Mutex<SessionManager>,
    voice_download: Arc<Mutex<voice::VoiceDownloadState>>,
    voice_recording: Mutex<voice::VoiceRecordingState>,
    voice_worker: voice::VoiceWorker,
    webcam: webcam::WebcamPreview,
    started_at: Instant,
    identity: SupervisorIdentity,
}

#[tauri::command]
async fn start_session(
    input: StartSessionInput,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let mut sessions = lock_sessions(&state)?;

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
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.snapshot(now))
}

#[tauri::command]
fn pause_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let snapshot = lock_sessions(&state)?.pause(now);
    sync_session_webcam(&app);
    Ok(snapshot)
}

#[tauri::command]
fn resume_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let snapshot = lock_sessions(&state)?.resume(now);
    sync_session_webcam(&app);
    Ok(snapshot)
}

#[tauri::command]
fn snooze_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.snooze(now, 5 * 60))
}

#[tauri::command]
fn stop_session(app: AppHandle, state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    let snapshot = lock_sessions(&state)?.stop(now);

    hide_status_window(&app);
    sync_session_webcam(&app);
    Ok(snapshot)
}

#[tauri::command]
async fn flag_current_distracting(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.flag_current(now))
}

#[tauri::command]
async fn allow_current_for_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.allow_current_for_session(now))
}

#[tauri::command]
async fn allow_current_always(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.allow_current_always(now))
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
fn get_history_week(week_start: String, state: State<'_, AppState>) -> Result<HistoryWeek, String> {
    lock_sessions(&state)?.history_week(week_start)
}

#[tauri::command]
fn get_week_recap(start: String, state: State<'_, AppState>) -> Result<WeekRecap, String> {
    lock_sessions(&state)?.week_recap(start)
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
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.dismiss_storage_notice(now))
}

#[tauri::command]
fn get_settings(state: State<'_, AppState>) -> Result<SessionSettings, String> {
    Ok(lock_sessions(&state)?.settings())
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
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.update_rules(rules, now))
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
    let now = state.started_at.elapsed();
    let snapshot = lock_sessions(&state)?.set_status_window_enabled(true, now);

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
    let snapshot = lock_sessions(&state)?.set_status_window_enabled(enabled, now);
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
    let now = state.started_at.elapsed();
    let snapshot = lock_sessions(&state)?.set_status_window_on_top(enabled, now);

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
                started_at: Instant::now(),
                identity: supervisor_identity(),
            });

            setup_status_window(app)?;
            setup_tray(app)?;
            start_session_ticker(app.handle().clone());
            start_voice_worker_ticker(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            start_session,
            get_session_snapshot,
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
            get_session_report,
            get_trends,
            delete_session,
            delete_all_history,
            get_history_size,
            dismiss_storage_notice,
            get_settings,
            get_rules,
            resolve_rule_input,
            get_site_names,
            set_site_name,
            get_app_names,
            set_app_name,
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
            delete_voice_clip,
            start_webcam_preview,
            stop_webcam_preview
        ])
        .build(tauri::generate_context!())
        .expect("failed to run Supervisor")
        .run(|app, event| {
            // Managed state is never dropped (the ticker threads hold AppHandles), so stop the
            // voice worker here; outside Windows nothing else kills it when the app exits.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<AppState>();
                // Cancel first: a release alone waits for a running clip to finish.
                voice::cancel_generation(&state.voice_worker);
                voice::release_worker(&state.voice_worker);
                webcam::stop_preview(&state.webcam);
            }
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
    .build()?;

    Ok(())
}

fn setup_tray(app: &mut tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show Supervisor", true, None::<&str>)?;
    let show_status =
        MenuItem::with_id(app, "show_status", "Show Status Window", true, None::<&str>)?;
    let pin_status = MenuItem::with_id(app, "pin_status", "Pin Status Window", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause_resume", "Pause/Resume", true, None::<&str>)?;
    let snooze = MenuItem::with_id(app, "snooze", "Mute Nudges 5 min", true, None::<&str>)?;
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
                with_sessions(app, |sessions, now| {
                    sessions.stop(now);
                });
                hide_status_window(app);
            }
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

fn counted_new_nudge(seen: &mut (u64, u64), id: u64, nudges: u64) -> bool {
    let before = if seen.0 == id { seen.1 } else { 0 };
    *seen = (id, nudges);
    nudges > before
}

fn start_session_ticker(app: AppHandle) {
    let mut last_loop = app.state::<AppState>().started_at.elapsed();
    let mut nudges_seen = (0, 0);
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(1));

        let (ended_after_tick, should_poll) = {
            let state = app.state::<AppState>();
            let now = state.started_at.elapsed();
            let mut sessions = state.sessions.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            // This loop runs every second; a long silence means the machine slept or froze.
            if now.saturating_sub(last_loop) > session::LOST_TIME_AFTER {
                sessions.record_lost_time(last_loop, now);
            }
            last_loop = now;
            let had_current = sessions.has_current();
            let snapshot = sessions.tick(now);
            // A phone nudge can come from a webcam frame or this tick; both show up here.
            if new_nudge(&mut nudges_seen, &snapshot) {
                let _ = app.emit(NUDGE_EVENT, ());
            }
            (
                had_current && matches!(snapshot.status, session::SessionStatus::Idle),
                sessions.should_poll_activity(),
            )
        };
        if ended_after_tick {
            hide_status_window(&app);
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
            sessions.record_capture_gap(now);
            let had_current = sessions.has_current();
            let snapshot = sessions.tick(now);
            if had_current && matches!(snapshot.status, session::SessionStatus::Idle) {
                hide_status_window(&app);
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
            if new_nudge(&mut nudges_seen, &snapshot) {
                let _ = app.emit(NUDGE_EVENT, ());
            }
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
    }
}

fn with_sessions(
    app: &tauri::AppHandle,
    action: impl FnOnce(&mut SessionManager, std::time::Duration),
) {
    let state = app.state::<AppState>();
    let now = state.started_at.elapsed();
    if let Ok(mut sessions) = state.sessions.lock() {
        action(&mut sessions, now);
    };
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
