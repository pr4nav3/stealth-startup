use super::activity_normalize::{
    app_name_from_process_path, classify_foreground_surface, is_neutral_window_class,
    normalize_title_for_browser, normalize_title_text, ForegroundDisposition,
    RawForegroundIdentity,
};
use crate::activity::{ActivitySample, Bounds, CapturePermissions};
use std::ffi::c_void;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};

type Hwnd = isize;

const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
const DWMWA_CLOAKED: u32 = 14;
const DWMWA_EXTENDED_FRAME_BOUNDS: u32 = 9;
const PROCESS_IMAGE_PATH_CHARS: usize = 32768;

#[repr(C)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct GuiThreadInfo {
    cb_size: u32,
    flags: u32,
    hwnd_active: Hwnd,
    hwnd_focus: Hwnd,
    hwnd_capture: Hwnd,
    hwnd_menu_owner: Hwnd,
    hwnd_move_size: Hwnd,
    hwnd_caret: Hwnd,
    rc_caret: Rect,
}

#[link(name = "User32")]
unsafe extern "system" {
    fn GetForegroundWindow() -> Hwnd;
    fn GetWindowTextW(hwnd: Hwnd, buffer: *mut u16, size: i32) -> i32;
    fn GetClassNameW(hwnd: Hwnd, buffer: *mut u16, size: i32) -> i32;
    fn GetWindowThreadProcessId(hwnd: Hwnd, process_id: *mut u32) -> u32;
    fn IsWindowVisible(hwnd: Hwnd) -> i32;
    fn IsIconic(hwnd: Hwnd) -> i32;
    fn GetWindowRect(hwnd: Hwnd, rect: *mut Rect) -> i32;
    fn GetGUIThreadInfo(id_thread: u32, info: *mut GuiThreadInfo) -> i32;
    fn EnumChildWindows(
        hwnd_parent: Hwnd,
        callback: unsafe extern "system" fn(Hwnd, isize) -> i32,
        LPARAM: isize,
    ) -> i32;
}

#[link(name = "Kernel32")]
unsafe extern "system" {
    fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> *mut c_void;
    fn CloseHandle(handle: *mut c_void) -> i32;
    fn QueryFullProcessImageNameW(
        handle: *mut c_void,
        flags: u32,
        buffer: *mut u16,
        size: *mut u32,
    ) -> i32;
}

#[link(name = "Dwmapi")]
unsafe extern "system" {
    fn DwmGetWindowAttribute(hwnd: Hwnd, attribute: u32, value: *mut c_void, size: u32) -> i32;
}

pub fn capture_current(
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
) -> Result<ActivitySample, String> {
    let captured_at = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|error| error.to_string())?;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        return Err("no foreground window".to_string());
    }

    if is_cloaked(hwnd) || unsafe { IsIconic(hwnd) != 0 } {
        return Err("foreground window is not usable".to_string());
    }

    if unsafe { IsWindowVisible(hwnd) == 0 } {
        return Err("foreground window is not visible".to_string());
    }

    let window_class = read_wide_string(hwnd, read_class_name);
    let raw_title = read_wide_string(hwnd, read_window_title);
    let mut process_id = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    if process_id == 0 {
        return Err("foreground window has no process".to_string());
    }

    let (process_id, process_path, exe_name) =
        resolve_process_identity(hwnd, process_id, &window_class);

    let disposition = classify_foreground_surface(&RawForegroundIdentity {
        exe_name: exe_name.clone(),
        window_class: window_class.clone(),
        window_title: raw_title.clone(),
    });

    match disposition {
        ForegroundDisposition::NoSample => {
            return Err("foreground surface is not classifiable".to_string());
        }
        ForegroundDisposition::Neutral => {
            return Ok(build_sample(
                captured_at,
                permissions,
                idle_seconds,
                hwnd,
                process_id,
                process_path,
                exe_name,
                window_class,
                raw_title,
                true,
            ));
        }
        ForegroundDisposition::Sample => {}
    }

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
        false,
    ))
}

fn build_sample(
    captured_at: String,
    permissions: CapturePermissions,
    idle_seconds: Option<f64>,
    hwnd: Hwnd,
    process_id: u32,
    process_path: String,
    exe_name: String,
    window_class: String,
    raw_title: String,
    neutral_surface: bool,
) -> ActivitySample {
    let mut app_name = app_name_from_process_path(&process_path, &window_class);
    if app_name.is_empty() {
        app_name = exe_name.clone();
    }
    if neutral_surface && is_neutral_window_class(&window_class) {
        app_name = window_class.clone();
    }
    let window_title = normalize_title_for_browser(&exe_name, &raw_title)
        .or_else(|| normalize_title_text(&raw_title));

    ActivitySample {
        captured_at,
        platform: "windows",
        app_name,
        process_path,
        process_id: process_id as u64,
        window_title,
        window_id: hwnd.to_string(),
        bounds: read_bounds(hwnd),
        idle_seconds,
        permissions,
    }
}

fn resolve_process_identity(
    hwnd: Hwnd,
    process_id: u32,
    window_class: &str,
) -> (u32, String, String) {
    if let Some((uwp_pid, uwp_path)) = resolve_uwp_process(hwnd, process_id) {
        let exe = file_name_from_path(&uwp_path);
        return (uwp_pid, uwp_path, exe);
    }

    match query_process_path(process_id) {
        Ok(path) => {
            let exe = file_name_from_path(&path);
            (process_id, path, exe)
        }
        Err(_) => {
            let fallback_name = window_class.to_owned();
            (process_id, String::new(), fallback_name)
        }
    }
}

fn resolve_uwp_process(hwnd: Hwnd, host_pid: u32) -> Option<(u32, String)> {
    let host_path = query_process_path(host_pid).ok()?;
    let host_exe = file_name_from_path(&host_path);
    if !host_exe.eq_ignore_ascii_case("ApplicationFrameHost.exe") {
        return None;
    }

    if let Some(child_hwnd) = find_core_window_child(hwnd) {
        let mut child_pid = 0u32;
        unsafe { GetWindowThreadProcessId(child_hwnd, &mut child_pid) };
        if child_pid != 0 {
            if let Ok(path) = query_process_path(child_pid) {
                return Some((child_pid, path));
            }
        }
    }

    let mut info = GuiThreadInfo {
        cb_size: std::mem::size_of::<GuiThreadInfo>() as u32,
        flags: 0,
        hwnd_active: 0,
        hwnd_focus: 0,
        hwnd_capture: 0,
        hwnd_menu_owner: 0,
        hwnd_move_size: 0,
        hwnd_caret: 0,
        rc_caret: Rect {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        },
    };
    if unsafe { GetGUIThreadInfo(0, &mut info) == 0 } {
        return None;
    }

    let target = if info.hwnd_focus != 0 {
        info.hwnd_focus
    } else {
        info.hwnd_active
    };
    if target == 0 {
        return None;
    }

    let mut gui_pid = 0u32;
    unsafe { GetWindowThreadProcessId(target, &mut gui_pid) };
    if gui_pid == 0 || gui_pid == host_pid {
        return None;
    }

    query_process_path(gui_pid).ok().map(|path| (gui_pid, path))
}

struct CoreWindowSearch {
    found: Hwnd,
}

unsafe extern "system" fn enum_core_window(hwnd: Hwnd, lparam: isize) -> i32 {
    let search = &mut *(lparam as *mut CoreWindowSearch);
    let class = read_wide_string(hwnd, read_class_name);
    if class.eq_ignore_ascii_case("Windows.UI.Core.CoreWindow") {
        search.found = hwnd;
        return 0;
    }
    1
}

fn find_core_window_child(parent: Hwnd) -> Option<Hwnd> {
    let mut search = CoreWindowSearch { found: 0 };
    unsafe {
        EnumChildWindows(
            parent,
            enum_core_window,
            (&mut search as *mut CoreWindowSearch) as isize,
        );
    }
    (search.found != 0).then_some(search.found)
}

fn query_process_path(process_id: u32) -> Result<String, ()> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
    if handle.is_null() {
        return Err(());
    }

    let result = read_process_image_path(handle);
    unsafe { CloseHandle(handle) };
    result
}

fn read_process_image_path(handle: *mut c_void) -> Result<String, ()> {
    let mut buffer = vec![0u16; PROCESS_IMAGE_PATH_CHARS];
    let mut size = buffer.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size as *mut u32)
    };
    if ok == 0 {
        return Err(());
    }
    Ok(String::from_utf16_lossy(&buffer[..size as usize]))
}

fn read_bounds(hwnd: Hwnd) -> Bounds {
    let mut rect = Rect {
        left: 0,
        top: 0,
        right: 0,
        bottom: 0,
    };
    let dwm_ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut rect as *mut Rect).cast(),
            std::mem::size_of::<Rect>() as u32,
        )
    };
    if dwm_ok != 0 {
        let _ = unsafe { GetWindowRect(hwnd, &mut rect) };
    }

    Bounds {
        x: rect.left as f64,
        y: rect.top as f64,
        width: (rect.right - rect.left).max(0) as f64,
        height: (rect.bottom - rect.top).max(0) as f64,
    }
}

fn is_cloaked(hwnd: Hwnd) -> bool {
    let mut cloaked = 0i32;
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            (&mut cloaked as *mut i32).cast(),
            std::mem::size_of::<i32>() as u32,
        )
    };
    ok == 0 && cloaked != 0
}

fn file_name_from_path(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("unknown-window")
        .to_owned()
}

fn read_window_title(hwnd: Hwnd, buffer: &mut [u16]) -> i32 {
    unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) }
}

fn read_class_name(hwnd: Hwnd, buffer: &mut [u16]) -> i32 {
    unsafe { GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) }
}

fn read_wide_string(hwnd: Hwnd, read: fn(Hwnd, &mut [u16]) -> i32) -> String {
    let mut buffer = vec![0u16; 512];
    let length = read(hwnd, &mut buffer);
    let end = length.max(0) as usize;
    String::from_utf16_lossy(&buffer[..end.min(buffer.len())])
}
