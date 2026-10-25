//! macOS system calls, all through maintained bindings (objc2 family, libc, mach2).
//! Type-checked from Windows with `cargo check --target aarch64-apple-darwin` in a scratch
//! crate; see plans/mac-handoff.md for what still needs a real Mac.

use std::ffi::CString;
use std::sync::OnceLock;

pub fn screen_recording_granted() -> bool {
    objc2_core_graphics::CGPreflightScreenCaptureAccess()
}

pub fn idle_seconds() -> Option<f64> {
    use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};

    // kCGAnyInputEventType is a C macro, `(CGEventType)(~0)`, so the bindings cannot export it.
    let any_input = CGEventType(!0);
    let seconds =
        CGEventSource::seconds_since_last_event_type(CGEventSourceStateID::HIDSystemState, any_input);
    seconds.is_finite().then_some(seconds)
}

/// Whether process `pid`, or a helper acting for it, holds a "keep the display awake" power
/// assertion. Browsers take one while a video plays on screen (not for audio, a paused video,
/// or a hidden tab), and call apps during calls. Public IOKit; no permission prompt.
pub fn keeps_display_awake(pid: u64) -> bool {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{ns_string, NSArray, NSDictionary, NSNumber, NSString};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        /// A `CFDictionaryRef` (toll-free bridged to `NSDictionary`) from each pid to its
        /// assertions. Returns `kIOReturnSuccess` (0) on success.
        fn IOPMCopyAssertionsByProcess(assertions_by_pid: *mut *mut NSDictionary) -> i32;
    }
    const DISPLAY_AWAKE: [&str; 2] = ["PreventUserIdleDisplaySleep", "NoDisplaySleepAssertion"];

    let mut raw = std::ptr::null_mut();
    if unsafe { IOPMCopyAssertionsByProcess(&mut raw) } != 0 {
        return false;
    }
    // A Copy function hands over ownership; `Retained` releases the dictionary when dropped.
    let Some(by_pid) = (unsafe { Retained::from_raw(raw) }) else {
        return false;
    };
    let number = |value: Option<Retained<AnyObject>>| {
        value?.downcast::<NSNumber>().ok().map(|number| number.integerValue())
    };
    let pid = pid as isize;

    by_pid.keys().any(|owner| {
        let owner_pid = number(Some(owner.clone()));
        let Some(assertions) = by_pid.objectForKey(&owner).and_then(|value| value.downcast::<NSArray>().ok()) else {
            return false;
        };
        assertions.iter().any(|assertion| {
            let Ok(assertion) = assertion.downcast::<NSDictionary>() else {
                return false;
            };
            let keeps_display = assertion
                .objectForKey(ns_string!("AssertType"))
                .and_then(|kind| kind.downcast::<NSString>().ok())
                .is_some_and(|kind| DISPLAY_AWAKE.contains(&kind.to_string().as_str()));
            // WebKit and other multi-process apps assert from a helper on the app's behalf.
            let on_behalf_of = number(assertion.objectForKey(ns_string!("AssertionOnBehalfOfPID")));
            keeps_display && (owner_pid == Some(pid) || on_behalf_of == Some(pid))
        })
    })
}

/// The bundle identifier ("com.microsoft.VSCode") of the app bundle at `path`. It survives
/// renames and updates, unlike the app's display name. Read once per path: it runs every tick.
pub fn bundle_identifier(path: &str) -> Option<String> {
    use objc2_foundation::{NSBundle, NSString};
    use std::collections::HashMap;
    use std::sync::Mutex;

    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    if path.is_empty() {
        return None;
    }
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(id) = cache.get(path) {
        return id.clone();
    }
    let id = NSBundle::bundleWithPath(&NSString::from_str(path))
        .and_then(|bundle| bundle.bundleIdentifier())
        .map(|id| id.to_string().trim().to_string())
        .filter(|id| !id.is_empty());
    if cache.len() >= 256 {
        cache.clear();
    }
    cache.insert(path.to_string(), id.clone());
    id
}

/// The app macOS considers in front, even when it has no window on screen (its last window
/// closed, all minimized, or the desktop clicked): pid, name, bundle path.
pub fn frontmost_app() -> Option<(u64, String, std::path::PathBuf)> {
    use objc2_app_kit::NSWorkspace;

    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let pid = u64::try_from(app.processIdentifier()).ok()?;
    let name = app.localizedName().map(|name| name.to_string()).unwrap_or_default();
    let path = app
        .bundleURL()
        .and_then(|url| url.path())
        .map(|path| std::path::PathBuf::from(path.to_string()))
        .unwrap_or_default();
    Some((pid, name, path))
}

/// The login screen or the lock screen is up. Like the Windows lock screen, it records nothing.
pub fn screen_locked() -> bool {
    use objc2_core_foundation::{CFBoolean, CFString};

    let Some(session) = objc2_core_graphics::CGSessionCopyCurrentDictionary() else {
        return false;
    };
    let key = CFString::from_static_str("CGSSessionScreenIsLocked");
    // SAFETY: a CFDictionary from CoreGraphics; the key and value are CF types.
    let session: &objc2_core_foundation::CFDictionary<CFString, objc2_core_foundation::CFType> =
        unsafe { &*(&*session as *const objc2_core_foundation::CFDictionary).cast() };
    session
        .get(&key)
        .and_then(|value| value.downcast_ref::<CFBoolean>().map(CFBoolean::value))
        .unwrap_or(false)
}

/// `/System/Library/CoreServices` as macOS reports it: home of the Dock, Spotlight,
/// Notification Center, Control Center, Finder, and the login window.
pub fn core_services_dir() -> Option<&'static str> {
    static DIR: OnceLock<Option<String>> = OnceLock::new();
    DIR.get_or_init(|| {
        use objc2_foundation::{
            NSSearchPathDirectory, NSSearchPathDomainMask, NSSearchPathForDirectoriesInDomains,
        };

        NSSearchPathForDirectoriesInDomains(
            NSSearchPathDirectory::CoreServiceDirectory,
            NSSearchPathDomainMask::SystemDomainMask,
            true,
        )
        .firstObject()
        .map(|path| path.to_string())
    })
    .as_deref()
}

pub fn total_ram_bytes() -> Option<u64> {
    sysctl_u64("hw.memsize")
}

/// The kernel's dispatch levels: 1 normal, 2 warning, 4 critical. None if it does not report it.
pub fn memory_pressure_level() -> Option<u32> {
    sysctl_u32("kern.memorystatus_vm_pressure_level")
}

/// Memory macOS can hand out without swapping: free plus inactive (reclaimable cache) pages,
/// minus speculative pages, which are already counted as free. This is not Windows ullAvailPhys.
pub fn available_ram_bytes() -> Option<u64> {
    let stats = vm_statistics()?;
    let page_size = sysctl_u64("hw.pagesize")?;
    let pages = u64::from(stats.free_count) + u64::from(stats.inactive_count)
        - u64::from(stats.speculative_count).min(u64::from(stats.free_count));
    Some(pages.saturating_mul(page_size))
}

fn vm_statistics() -> Option<libc::vm_statistics64> {
    #[allow(deprecated)]
    let host = unsafe { libc::mach_host_self() };
    let mut stats: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
    let mut count = libc::HOST_VM_INFO64_COUNT;
    let status = unsafe {
        libc::host_statistics64(
            host,
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        )
    };
    unsafe { mach2::mach_port::mach_port_deallocate(mach2::traps::mach_task_self(), host) };
    (status == libc::KERN_SUCCESS).then_some(stats)
}

/// The memory Activity Monitor shows for a process ("physical footprint").
pub fn process_ram_bytes(pid: u32) -> Option<u64> {
    let mut info: libc::rusage_info_v2 = unsafe { std::mem::zeroed() };
    let status = unsafe {
        libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V2,
            (&mut info as *mut libc::rusage_info_v2).cast(),
        )
    };
    (status == 0).then_some(info.ri_phys_footprint)
}

pub fn available_disk_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;

    let path = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    let status = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    (status == 0).then(|| u64::from(stat.f_bavail).saturating_mul(stat.f_frsize))
}

fn sysctl_u32(name: &str) -> Option<u32> {
    let name = CString::new(name).ok()?;
    let mut value = 0u32;
    let mut len = std::mem::size_of::<u32>();
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u32).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && len == std::mem::size_of::<u32>()).then_some(value)
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let name = CString::new(name).ok()?;
    let mut value = 0u64;
    let mut len = std::mem::size_of::<u64>();
    let status = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut u64).cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    (status == 0 && len == std::mem::size_of::<u64>()).then_some(value)
}

/// macOS asks once per app. The webcam worker is our child process, so the answer given to
/// Supervisor covers it. Asking here makes the prompt name Supervisor, and lets us wait for it.
pub fn camera_access() -> Result<(), String> {
    let video = unsafe { objc2_av_foundation::AVMediaTypeVideo }
        .ok_or("macOS did not provide the video media type")?;
    media_access(
        video,
        "Camera access is off for Supervisor. Turn it on in System Settings > Privacy & Security > Camera, then start the preview again.",
    )
}

/// The same for the microphone, before a voice sample is recorded. Without it a denied
/// microphone gives silence, not an error.
pub fn microphone_access() -> Result<(), String> {
    let audio = unsafe { objc2_av_foundation::AVMediaTypeAudio }
        .ok_or("macOS did not provide the audio media type")?;
    media_access(
        audio,
        "Microphone access is off for Supervisor. Turn it on in System Settings > Privacy & Security > Microphone, then record again.",
    )
}

fn media_access(kind: &objc2_av_foundation::AVMediaType, denied: &str) -> Result<(), String> {
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice};

    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(kind) };
    if status == AVAuthorizationStatus::Authorized {
        return Ok(());
    }
    if status != AVAuthorizationStatus::NotDetermined {
        return Err(denied.to_string());
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool| {
        let _ = sender.send(granted.as_bool());
    });
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(kind, &handler) };
    // A prompt left unanswered must not hang the command forever.
    match receiver.recv_timeout(std::time::Duration::from_secs(120)) {
        Ok(true) => Ok(()),
        _ => Err(denied.to_string()),
    }
}
