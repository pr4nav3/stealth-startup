//! Browser address read on macOS through the Accessibility API: the focused window's web
//! area reports the page address (`AXURL`), never the page contents. Safari, Chrome, Edge,
//! Brave, Arc, and Firefox all expose it. The address is the loaded page, not what is being
//! typed, so `editing` is always false here.
//!
//! The web area's place in the window (a path of child indexes) is found once per browser and
//! walked again each tick, so a normal read is a handful of cheap attribute reads, always on
//! the tab in front. Needs the Accessibility permission (`accessibility_granted`).

use crate::activity::BrowserRead;
use objc2_core_foundation::{
    kCFBooleanTrue, CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType, Type, CFURL,
};
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A hung browser must not stall the 1 s session tick.
const TIMEOUT_SECONDS: f32 = 0.3;
/// After a failed search, leave that browser alone for a while instead of re-walking it.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// A Chromium browser builds its accessibility tree over a few seconds after it is first
/// asked (longer on a cold launch); keep searching every tick until then.
const TREE_BUILD_GRACE: Duration = Duration::from_secs(15);
/// Toolbars come before page content; a browser window reaches its web area well within this.
const MAX_VISITED: usize = 400;
const MAX_DEPTH: usize = 12;

type AXError = i32;
const AX_SUCCESS: AXError = 0;

// AXUIElementRef is an opaque CFType; every Copy/Create call hands back a +1 reference.
#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> u8;
    fn AXIsProcessTrustedWithOptions(options: *const CFDictionary) -> u8;
    fn AXUIElementCreateApplication(pid: i32) -> *mut CFType;
    fn AXUIElementCopyAttributeValue(
        element: *const CFType,
        attribute: *const CFString,
        value: *mut *mut CFType,
    ) -> AXError;
    fn AXUIElementSetAttributeValue(
        element: *const CFType,
        attribute: *const CFString,
        value: *const CFType,
    ) -> AXError;
    fn AXUIElementSetMessagingTimeout(element: *const CFType, timeout_seconds: f32) -> AXError;
    static kAXTrustedCheckOptionPrompt: *const CFString;
}

pub fn accessibility_granted() -> bool {
    unsafe { AXIsProcessTrusted() != 0 }
}

/// Show the system's Accessibility prompt once per run (it also lists Supervisor under
/// Privacy & Security > Accessibility). Later calls only report the answer.
pub fn request_accessibility() -> bool {
    static ASKED: OnceLock<()> = OnceLock::new();
    if ASKED.set(()).is_err() {
        return accessibility_granted();
    }
    let prompt = unsafe { kAXTrustedCheckOptionPrompt.as_ref() };
    let (Some(prompt), Some(yes)) = (prompt, unsafe { kCFBooleanTrue }) else {
        return accessibility_granted();
    };
    let yes: &CFType = yes;
    let options = CFDictionary::from_slices(&[prompt], &[yes]);
    let options: *const CFDictionary = (&*options as *const CFDictionary<CFString, CFType>).cast();
    unsafe { AXIsProcessTrustedWithOptions(options) != 0 }
}

struct Reader {
    /// Child-index path from a browser's window to its web area, by process id.
    paths: HashMap<u64, Vec<usize>>,
    failed: HashMap<u64, Instant>,
    /// Browsers already told to build their accessibility tree (Chromium needs asking).
    enabled: HashMap<u64, Instant>,
}

thread_local! {
    static READER: RefCell<Reader> = RefCell::new(Reader {
        paths: HashMap::new(),
        failed: HashMap::new(),
        enabled: HashMap::new(),
    });
}

pub fn read_browser(process_id: u64) -> BrowserRead {
    if !accessibility_granted() {
        return BrowserRead::default();
    }
    READER.with(|reader| reader.borrow_mut().read(process_id))
}

/// The focused window's title through Accessibility, which needs no Screen Recording permission.
pub fn focused_window_title(process_id: u64) -> Option<String> {
    if !accessibility_granted() {
        return None;
    }
    let app = application(process_id)?;
    let window = attr(&app, "AXFocusedWindow")?;
    string_attr(&window, "AXTitle")
}

impl Reader {
    fn read(&mut self, pid: u64) -> BrowserRead {
        if self.failed.get(&pid).is_some_and(|at| at.elapsed() < RETRY_AFTER) {
            return BrowserRead::default();
        }
        let Some(app) = application(pid) else {
            return BrowserRead::default();
        };
        // Chromium exposes its web area only once a client asks, and builds it over the next
        // second or two; Safari and Firefox ignore the switch. Measured: YouTube in Chrome reads
        // on the fourth tick, Safari on the first.
        let just_enabled = !self.enabled.contains_key(&pid);
        if just_enabled {
            self.enabled.insert(pid, Instant::now());
            enable_chromium_accessibility(&app);
        }
        let Some(window) = attr(&app, "AXFocusedWindow") else {
            return BrowserRead::default();
        };
        let web_area = self
            .paths
            .get(&pid)
            .and_then(|path| walk(&window, path))
            .or_else(|| {
                // Stale after a toolbar change, or never found: search this window again.
                let (web_area, path) = find_web_area(&window)?;
                self.paths.insert(pid, path);
                Some(web_area)
            });
        let Some(web_area) = web_area else {
            self.paths.remove(&pid);
            // A tree still being built is not a failure; search again next tick.
            let building = self.enabled.get(&pid).is_some_and(|at| at.elapsed() < TREE_BUILD_GRACE);
            if !building {
                self.failed.insert(pid, Instant::now());
            }
            return BrowserRead::default();
        };
        BrowserRead { address: string_attr(&web_area, "AXURL"), editing: false }
    }
}

fn application(pid: u64) -> Option<CFRetained<CFType>> {
    let pid = i32::try_from(pid).ok()?;
    let app = NonNull::new(unsafe { AXUIElementCreateApplication(pid) })?;
    let app = unsafe { CFRetained::from_raw(app) };
    unsafe { AXUIElementSetMessagingTimeout(&*app, TIMEOUT_SECONDS) };
    Some(app)
}

/// Chromium's own switch for assistive clients that are not VoiceOver. Older builds only know
/// the VoiceOver one, so set both.
fn enable_chromium_accessibility(app: &CFType) {
    if let Some(yes) = unsafe { kCFBooleanTrue } {
        let yes: &CFType = yes;
        for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
            let name = CFString::from_static_str(name);
            unsafe { AXUIElementSetAttributeValue(app, &*name, yes) };
        }
    }
}

/// Roles along the first levels under the focused window, for the probe: shows why a
/// browser's web area was not found.
pub fn describe_focused_window(process_id: u64, depth: usize) -> String {
    let Some(app) = application(process_id) else {
        return "no app element".to_string();
    };
    let Some(window) = attr(&app, "AXFocusedWindow") else {
        return "no focused window".to_string();
    };
    let mut lines = Vec::new();
    let mut queue = VecDeque::from([(window, 0usize)]);
    while let Some((element, level)) = queue.pop_front() {
        if lines.len() >= 60 {
            break;
        }
        let role = string_attr(&element, "AXRole").unwrap_or_default();
        let kids = children(&element);
        lines.push(format!("{}{role} ({} children)", "  ".repeat(level), kids.len()));
        if level < depth {
            queue.extend(kids.into_iter().map(|child| (child, level + 1)));
        }
    }
    lines.join("\n")
}

/// Breadth-first, so the web area nearest the window wins: the page in front, not a
/// find-in-page or extension popup.
fn find_web_area(window: &CFType) -> Option<(CFRetained<CFType>, Vec<usize>)> {
    let mut queue: VecDeque<(CFRetained<CFType>, Vec<usize>)> =
        VecDeque::from([(window.retain(), Vec::new())]);
    let mut visited = 0;
    while let Some((element, path)) = queue.pop_front() {
        visited += 1;
        if visited > MAX_VISITED {
            return None;
        }
        if string_attr(&element, "AXRole").as_deref() == Some("AXWebArea") {
            return Some((element, path));
        }
        if path.len() >= MAX_DEPTH {
            continue;
        }
        for (index, child) in children(&element).into_iter().enumerate() {
            let mut child_path = path.clone();
            child_path.push(index);
            queue.push_back((child, child_path));
        }
    }
    None
}

/// Follow a remembered path; `None` once anything along it moved.
fn walk(window: &CFType, path: &[usize]) -> Option<CFRetained<CFType>> {
    let mut element = window.retain();
    for &index in path {
        element = children(&element).into_iter().nth(index)?;
    }
    (string_attr(&element, "AXRole").as_deref() == Some("AXWebArea")).then_some(element)
}

fn children(element: &CFType) -> Vec<CFRetained<CFType>> {
    let Some(value) = attr(element, "AXChildren") else {
        return Vec::new();
    };
    if value.downcast_ref::<CFArray>().is_none() {
        return Vec::new();
    }
    // AXChildren is always an array of AXUIElements; the type id check above covers the cast.
    let array: &CFArray<CFType> = unsafe { &*(&*value as *const CFType).cast() };
    (0..array.len()).filter_map(|index| array.get(index)).collect()
}

fn attr(element: &CFType, name: &'static str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_static_str(name);
    let mut value = std::ptr::null_mut();
    let status = unsafe { AXUIElementCopyAttributeValue(element, &*name, &mut value) };
    if status != AX_SUCCESS {
        return None;
    }
    NonNull::new(value).map(|value| unsafe { CFRetained::from_raw(value) })
}

/// A string or URL attribute as text. `AXURL` is a CFURL in Safari and Firefox.
fn string_attr(element: &CFType, name: &'static str) -> Option<String> {
    let value = attr(element, name)?;
    if let Some(text) = value.downcast_ref::<CFString>() {
        return Some(text.to_string());
    }
    value.downcast_ref::<CFURL>().map(|url| url.string().to_string())
}

#[allow(dead_code)]
fn bool_attr(element: &CFType, name: &'static str) -> Option<bool> {
    attr(element, name)?.downcast_ref::<CFBoolean>().map(CFBoolean::value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With Safari, Chrome, or Firefox open:
    /// `cargo test manual_read_browser_windows_macos -- --ignored --nocapture`
    #[test]
    #[ignore = "manual: reads the front window of every running browser"]
    fn manual_read_browser_windows_macos() {
        println!("accessibility granted: {}", accessibility_granted());
        for name in ["Safari", "Google Chrome", "firefox", "Brave Browser", "Microsoft Edge", "Arc"] {
            let output = std::process::Command::new("pgrep").arg("-x").arg(name).output().unwrap();
            for pid in String::from_utf8_lossy(&output.stdout).lines().filter_map(|line| line.parse::<u64>().ok()) {
                for pass in ["find", "cached"] {
                    let started = Instant::now();
                    let read = read_browser(pid);
                    let title = focused_window_title(pid);
                    println!("{name:<16} {pid:>6} {pass:>6} {:>4}ms {read:?} title={title:?}", started.elapsed().as_millis());
                }
            }
        }
    }
}
