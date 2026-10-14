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

/// 0 normal, 1 warning, 2 urgent, 3 critical. None if the kernel does not report it.
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
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeVideo};

    const DENIED: &str = "Camera access is off for Supervisor. Turn it on in System Settings > Privacy & Security > Camera, then start the preview again.";

    let video = unsafe { AVMediaTypeVideo }.ok_or("macOS did not provide the video media type")?;
    let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(video) };
    if status == AVAuthorizationStatus::Authorized {
        return Ok(());
    }
    if status != AVAuthorizationStatus::NotDetermined {
        return Err(DENIED.to_string());
    }

    let (sender, receiver) = std::sync::mpsc::channel();
    let handler = RcBlock::new(move |granted: Bool| {
        let _ = sender.send(granted.as_bool());
    });
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(video, &handler) };
    match receiver.recv() {
        Ok(true) => Ok(()),
        _ => Err(DENIED.to_string()),
    }
}
