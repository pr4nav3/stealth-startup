use crate::activity::{ActivitySample, Bounds, BrowserRead, CapturePermissions};
use crate::website::is_browser;
use super::activity_normalize::{
    app_label, app_name_from_process_path, classify_foreground_surface, is_inside_dir,
    media_is_in_front, normalize_title_for_browser, normalize_title_text, ForegroundDisposition,
    RawForegroundIdentity,
};
use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use windows::core::{w, BOOL, HSTRING, PCWSTR, PWSTR};
use windows::ApplicationModel::AppInfo;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSessionManager as MediaSessions,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as PlaybackStatus,
};
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, MAX_PATH, RECT, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::Storage::FileSystem::{
    GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
    TH32CS_SNAPPROCESS,
};
use windows::Win32::System::SystemInformation::GetSystemWindowsDirectoryW;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumChildWindows, GetAncestor, GetClassNameW, GetForegroundWindow, GetGUIThreadInfo, GetWindow,
    GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    GA_PARENT, GW_OWNER, GUITHREADINFO,
};

/// Longest path Windows can return (the extended-length path limit).
const PROCESS_IMAGE_PATH_CHARS: usize = 32768;

/// Whether the window in front is playing media, per the system media controls (what the
/// volume flyout's player shows). A failed read counts as not playing: idle stays Away.
pub(super) fn media_playing_in_front(sample: &ActivitySample) -> bool {
    let in_browser = is_browser(&sample.app_name.to_lowercase());
    playing_media().unwrap_or_default().iter().any(|(source_app, title)| {
        media_is_in_front(&sample.app_name, sample.window_title.as_deref(), in_browser, source_app, title)
    })
}

/// App id and media title of every session that is playing right now.
fn playing_media() -> windows::core::Result<Vec<(String, String)>> {
    let mut playing = Vec::new();
    for session in MediaSessions::RequestAsync()?.get()?.GetSessions()? {
        if session.GetPlaybackInfo()?.PlaybackStatus()? != PlaybackStatus::Playing {
            continue;
        }
        let title = session.TryGetMediaPropertiesAsync()?.get()?.Title()?;
        playing.push((session.SourceAppUserModelId()?.to_string(), title.to_string()));
    }
    Ok(playing)
}

/// Use the same OS metadata as foreground capture to relabel older app history.
pub(super) fn running_app_labels(keys: &[String]) -> HashMap<String, String> {
    let wanted: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
    let mut labels = HashMap::new();
    if wanted.is_empty() {
        return labels;
    }
    let Ok(snapshot) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else {
        return labels;
    };
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            let end = entry.szExeFile.iter().position(|&unit| unit == 0).unwrap_or(entry.szExeFile.len());
            let exe = String::from_utf16_lossy(&entry.szExeFile[..end]);
            let key = std::path::Path::new(&exe)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or(&exe)
                .to_lowercase();
            if wanted.contains(key.as_str()) && !labels.contains_key(&key) {
                if let Some(path) = query_process_path(entry.th32ProcessID) {
                    let label = cached_app_label(&path, &key, entry.th32ProcessID);
                    labels.insert(key, label);
                }
            }
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snapshot) };
    labels
}

pub fn capture_current(
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
) -> Result<ActivitySample, String> {
    let captured_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())?;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Err("no foreground window".to_string());
    }

    if is_cloaked(hwnd) || unsafe { IsIconic(hwnd) }.as_bool() {
        return Err("foreground window is not usable".to_string());
    }

    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return Err("foreground window is not visible".to_string());
    }

    let window_class = class_name(hwnd);
    let raw_title = read_wide_string(|buffer| unsafe { GetWindowTextW(hwnd, buffer) });
    let process_id = window_process_id(hwnd);
    if process_id == 0 {
        return Err("foreground window has no process".to_string());
    }

    let (mut process_id, mut process_path, mut exe_name) =
        resolve_process_identity(hwnd, process_id);
    if belongs_to_this_process(hwnd) {
        process_id = std::process::id();
        if let Ok(path) = std::env::current_exe() {
            process_path = path.to_string_lossy().into_owned();
            exe_name = file_name_from_path(&process_path);
        }
    }
    if process_path.is_empty() {
        return Err("foreground process could not be identified".to_string());
    }

    let disposition = classify_foreground_surface(&RawForegroundIdentity {
        exe_name: exe_name.clone(),
        window_class: window_class.clone(),
        window_title: raw_title.clone(),
        is_system_app: system_apps_dir().is_some_and(|dir| is_inside_dir(&process_path, dir)),
        is_system_shell: system_apps_dir()
            .and_then(|dir| dir.strip_suffix("\\SystemApps"))
            .is_some_and(|windows| is_inside_dir(&process_path, &format!("{windows}\\System32"))),
    });

    let shell_surface = match disposition {
        ForegroundDisposition::NoSample => {
            return Err("foreground surface is not classifiable".to_string());
        }
        ForegroundDisposition::ShellSurface => true,
        ForegroundDisposition::Sample => false,
    };

    Ok(build_sample(
        captured_at,
        permissions,
        idle_seconds,
        hwnd,
        process_id,
        process_path,
        exe_name,
        window_class,
        raw_title,
        shell_surface,
    ))
}

/// `<Windows>\SystemApps`, asked from Windows once (the Windows folder is not always `C:\Windows`).
fn system_apps_dir() -> Option<&'static str> {
    static DIR: OnceLock<Option<String>> = OnceLock::new();
    DIR.get_or_init(|| {
        let mut buffer = vec![0u16; MAX_PATH as usize];
        let length = unsafe { GetSystemWindowsDirectoryW(Some(&mut buffer)) } as usize;
        (length > 0 && length < buffer.len())
            .then(|| format!(r"{}\SystemApps", String::from_utf16_lossy(&buffer[..length])))
    })
    .as_deref()
}

fn build_sample(
    captured_at: String,
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
    hwnd: HWND,
    process_id: u32,
    process_path: String,
    exe_name: String,
    window_class: String,
    raw_title: String,
    shell_surface: bool,
) -> ActivitySample {
    let mut app_name = app_name_from_process_path(&process_path, &window_class);
    if app_name.is_empty() {
        app_name = exe_name.clone();
    }
    let window_title = normalize_title_for_browser(&exe_name, &raw_title)
        .or_else(|| normalize_title_text(&raw_title));
    let browser = (!shell_surface && is_browser(&app_name.to_lowercase())).then(|| {
        let read = super::browser_win32::read_browser(hwnd);
        // The user may have switched windows during the read; don't credit the new one.
        if unsafe { GetForegroundWindow() } == hwnd {
            read
        } else {
            BrowserRead::default()
        }
    });

    ActivitySample {
        captured_at,
        platform: "windows",
        app_label: cached_app_label(&process_path, &app_name, process_id),
        app_name,
        process_path,
        process_id: process_id as u64,
        window_title,
        window_id: (hwnd.0 as isize).to_string(),
        bounds: read_bounds(hwnd),
        idle_seconds,
        permissions,
        shell_surface,
        browser,
        media_playing: false,
    }
}

/// The exe's FileDescription ("Visual Studio Code" for Code.exe), read once per program.
fn cached_app_label(process_path: &str, app_name: &str, process_id: u32) -> String {
    static LABELS: OnceLock<Mutex<HashMap<String, String>>> = OnceLock::new();
    let mut labels = LABELS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(label) = labels.get(process_path) {
        return label.clone();
    }
    let version_name = file_version_name(process_path);
    let package_name = package_display_name(process_id);
    let name = package_name.as_deref().filter(|name| !identifier_like(name))
        .or(version_name.as_deref())
        .or(package_name.as_deref());
    let label = app_label(app_name, name);
    if labels.len() >= 256 {
        labels.clear();
    }
    labels.insert(process_path.to_string(), label.clone());
    label
}

/// Windows resolves the localized display name of a packaged app from its AUMID.
fn package_display_name(process_id: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let result = (|| {
        let mut length = 0u32;
        if unsafe { GetApplicationUserModelId(handle, &mut length, None) } != ERROR_INSUFFICIENT_BUFFER
            || length == 0 || length > 4096
        {
            return None;
        }
        let mut buffer = vec![0u16; length as usize];
        if unsafe { GetApplicationUserModelId(handle, &mut length, Some(PWSTR(buffer.as_mut_ptr()))) }.0 != 0 {
            return None;
        }
        let id = String::from_utf16_lossy(&buffer[..(length as usize).saturating_sub(1)]);
        let info = AppInfo::GetFromAppUserModelId(&HSTRING::from(id)).ok()?;
        let name = info.DisplayInfo().ok()?.DisplayName().ok()?.to_string();
        Some(name.trim().to_string()).filter(|name| !name.is_empty())
    })();
    let _ = unsafe { CloseHandle(handle) };
    result
}

fn file_version_name(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }
    let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
    let path = PCWSTR(wide.as_ptr());
    unsafe {
        let size = GetFileVersionInfoSizeW(path, None);
        if size == 0 {
            return None;
        }
        let mut data = vec![0u8; size as usize];
        GetFileVersionInfoW(path, None, size, data.as_mut_ptr().cast()).ok()?;

        let mut value: *mut c_void = std::ptr::null_mut();
        let mut length = 0u32;
        // The first language/codepage pair names the string table; US English as a fallback.
        let table = if VerQueryValueW(
            data.as_ptr().cast(),
            w!("\\VarFileInfo\\Translation"),
            &mut value,
            &mut length,
        )
        .as_bool()
            && length >= 4
        {
            let pair = value as *const u16;
            format!("{:04x}{:04x}", *pair, *pair.add(1))
        } else {
            "040904b0".to_string()
        };
        let mut read = |field: &str| {
            let query: Vec<u16> = format!("\\StringFileInfo\\{table}\\{field}")
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            if !VerQueryValueW(data.as_ptr().cast(), PCWSTR(query.as_ptr()), &mut value, &mut length)
                .as_bool()
                || length == 0
            {
                return None;
            }
            let text = std::slice::from_raw_parts(value as *const u16, length as usize);
            let text = String::from_utf16_lossy(text);
            Some(text.trim_end_matches('\0').trim().to_string()).filter(|text| !text.is_empty())
        };
        let description = read("FileDescription");
        let product = read("ProductName");
        match (description, product) {
            (Some(description), Some(product))
                if identifier_like(&description) && !identifier_like(&product) => Some(product),
            (Some(description), _) => Some(description),
            (None, product) => product,
        }
    }
}

fn identifier_like(name: &str) -> bool {
    !name.contains(char::is_whitespace) && (name.contains('.') || name.contains('_'))
}

fn resolve_process_identity(hwnd: HWND, process_id: u32) -> (u32, String, String) {
    if let Some((uwp_pid, uwp_path)) = resolve_uwp_process(hwnd, process_id) {
        let exe = file_name_from_path(&uwp_path);
        return (uwp_pid, uwp_path, exe);
    }

    match query_process_path(process_id) {
        Some(path) => {
            let exe = file_name_from_path(&path);
            (process_id, path, exe)
        }
        // A window class describes a UI surface, not the application that owns it.
        // Without a process identity this tick cannot safely enter history or rules.
        None => (process_id, String::new(), String::new()),
    }
}

fn resolve_uwp_process(hwnd: HWND, host_pid: u32) -> Option<(u32, String)> {
    let host_path = query_process_path(host_pid)?;
    let host_exe = file_name_from_path(&host_path);
    if !host_exe.eq_ignore_ascii_case("ApplicationFrameHost.exe") {
        return None;
    }

    if let Some(child_hwnd) = find_core_window_child(hwnd) {
        let child_pid = window_process_id(child_hwnd);
        if child_pid != 0 {
            if let Some(path) = query_process_path(child_pid) {
                return Some((child_pid, path));
            }
        }
    }

    let mut info = GUITHREADINFO {
        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetGUIThreadInfo(0, &mut info) }.ok()?;

    let target = if info.hwndFocus.0.is_null() {
        info.hwndActive
    } else {
        info.hwndFocus
    };
    if target.0.is_null() {
        return None;
    }

    let gui_pid = window_process_id(target);
    if gui_pid == 0 || gui_pid == host_pid {
        return None;
    }

    query_process_path(gui_pid).map(|path| (gui_pid, path))
}

unsafe extern "system" fn enum_core_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let found = unsafe { &mut *(lparam.0 as *mut Option<HWND>) };
    if class_name(hwnd).eq_ignore_ascii_case("Windows.UI.Core.CoreWindow") {
        *found = Some(hwnd);
        return false.into();
    }
    true.into()
}

fn find_core_window_child(parent: HWND) -> Option<HWND> {
    let mut found: Option<HWND> = None;
    // The return value only reports whether enumeration stopped early.
    let _ = unsafe {
        EnumChildWindows(
            Some(parent),
            Some(enum_core_window),
            LPARAM(&mut found as *mut Option<HWND> as isize),
        )
    };
    found
}

fn belongs_to_this_process(hwnd: HWND) -> bool {
    // WebView popups (menus, dropdowns) are a different process, owned by our window.
    let self_pid = std::process::id();
    let mut current = hwnd;
    for _ in 0..8 {
        if current.0.is_null() {
            return false;
        }
        if window_process_id(current) == self_pid {
            return true;
        }
        let owner = unsafe { GetWindow(current, GW_OWNER) }.unwrap_or_default();
        let parent = unsafe { GetAncestor(current, GA_PARENT) };
        let next = if !owner.0.is_null() && owner != current {
            owner
        } else if !parent.0.is_null() && parent != current {
            parent
        } else {
            return false;
        };
        current = next;
    }
    false
}

fn window_process_id(hwnd: HWND) -> u32 {
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut process_id)) };
    process_id
}

fn query_process_path(process_id: u32) -> Option<String> {
    let handle =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let path = read_process_image_path(handle);
    let _ = unsafe { CloseHandle(handle) };
    path
}

fn read_process_image_path(handle: HANDLE) -> Option<String> {
    let mut buffer = vec![0u16; PROCESS_IMAGE_PATH_CHARS];
    let mut size = buffer.len() as u32;
    unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &mut size,
        )
    }
    .ok()?;
    Some(String::from_utf16_lossy(&buffer[..size as usize]))
}

fn read_bounds(hwnd: HWND) -> Bounds {
    let mut rect = RECT::default();
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut RECT).cast(),
            std::mem::size_of::<RECT>() as u32,
        )
    };
    if dwm.is_err() {
        let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    }

    Bounds {
        x: rect.left as f64,
        y: rect.top as f64,
        width: (rect.right - rect.left).max(0) as f64,
        height: (rect.bottom - rect.top).max(0) as f64,
    }
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let result = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut u32).cast(),
            std::mem::size_of::<u32>() as u32,
        )
    };
    result.is_ok() && cloaked != 0
}

fn file_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-window")
        .to_owned()
}

fn class_name(hwnd: HWND) -> String {
    read_wide_string(|buffer| unsafe { GetClassNameW(hwnd, buffer) })
}

fn read_wide_string(read: impl FnOnce(&mut [u16]) -> i32) -> String {
    let mut buffer = vec![0u16; 512];
    let length = read(&mut buffer).max(0) as usize;
    String::from_utf16_lossy(&buffer[..length.min(buffer.len())])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Set SUPERVISOR_PROBE_PID to a running packaged app's PID, then run this ignored test.
    #[test]
    #[ignore = "manual: checks the OS display name of a running packaged app"]
    fn manual_packaged_app_name() {
        let pid: u32 = std::env::var("SUPERVISOR_PROBE_PID").expect("pid").parse().expect("numeric pid");
        let name = package_display_name(pid).expect("package display name");
        assert!(!identifier_like(&name), "{name}");
        println!("{name}");
    }

    #[test]
    fn system_apps_dir_is_a_real_folder_holding_shell_programs() {
        let dir = system_apps_dir().expect("Windows folder");
        let has_program = std::fs::read_dir(dir)
            .expect("read SystemApps")
            .filter_map(Result::ok)
            .filter_map(|package| std::fs::read_dir(package.path()).ok())
            .flatten()
            .filter_map(Result::ok)
            .any(|file| {
                let path = file.path().to_string_lossy().into_owned();
                path.to_ascii_lowercase().ends_with(".exe") && is_inside_dir(&path, dir)
            });
        assert!(has_program, "no programs found under {dir}");
    }
}
