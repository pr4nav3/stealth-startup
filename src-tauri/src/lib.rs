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
use storage::{SessionReport, TrendsData};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder,
};

const MAIN_WINDOW_LABEL: &str = "main";
const OVERLAY_WINDOW_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 312.0;
const OVERLAY_HEIGHT: f64 = 286.0;
const OVERLAY_MARGIN: i32 = 18;

struct AppState {
    focus_engine: Mutex<FocusEngine>,
    sessions: Mutex<SessionManager>,
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

    sessions.start(input, now);
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
    Ok(lock_sessions(&state)?.pause(now))
}

#[tauri::command]
fn resume_session(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.resume(now))
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
    Ok(snapshot)
}

#[tauri::command]
fn mark_current_relevant(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.mark_current(focus::RuleClassification::Focused, now))
}

#[tauri::command]
fn mark_current_distracting(state: State<'_, AppState>) -> Result<SessionSnapshot, String> {
    let now = state.started_at.elapsed();
    Ok(lock_sessions(&state)?.mark_current(focus::RuleClassification::Distracting, now))
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
fn delete_session(id: u64, state: State<'_, AppState>) -> Result<(), String> {
    lock_sessions(&state)?.delete_session(id)
}

#[tauri::command]
fn delete_all_history(state: State<'_, AppState>) -> Result<(), String> {
    lock_sessions(&state)?.delete_all_history()
}

#[tauri::command]
fn scrub_span_context(state: State<'_, AppState>) -> Result<(), String> {
    lock_sessions(&state)?.scrub_span_context()
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

fn database_path(app: &tauri::App) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map_err(|error| error.to_string())
        .map(|dir| dir.join("sessions.db"))
}

pub fn run() {
    tauri::Builder::default()
        // Must be the first plugin: a second launch focuses this window and exits before
        // it can open the database.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .setup(|app| {
            app.manage(AppState {
                focus_engine: Mutex::new(FocusEngine::default()),
                sessions: Mutex::new(SessionManager::open(database_path(app))),
                voice_download: Arc::new(Mutex::new(voice::VoiceDownloadState::default())),
                voice_recording: Mutex::new(voice::VoiceRecordingState::default()),
                webcam: webcam::WebcamPreview::default(),
                started_at: Instant::now(),
                identity: supervisor_identity(),
            });

            setup_status_window(app)?;
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
            stop_session,
            mark_current_relevant,
            mark_current_distracting,
            get_history,
            get_session_report,
            get_trends,
            delete_session,
            delete_all_history,
            scrub_span_context,
            dismiss_storage_notice,
            get_settings,
            update_settings,
            show_status_window,
            set_status_window_enabled,
            set_status_window_on_top,
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

fn position_status_window(app: &AppHandle) -> Result<(), String> {
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
    let size = window.outer_size().unwrap_or(fallback_size);
    let min_x = area.position.x + margin;
    let max_x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
    let x = max_x.max(min_x);
    let y = area.position.y + margin;

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
            let had_current = sessions.has_current();
            let snapshot = sessions.tick(now);
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
            let was_polling = sessions.should_poll_activity();
            let snapshot = sessions.record_sample(sample, now, &identity);
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
