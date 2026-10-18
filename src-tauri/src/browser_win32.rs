//! Browser address read on Windows through UI Automation: read the address bar's value,
//! never the page. The address bar is found once per window and reused, so a normal tick is
//! two cheap property reads.

use crate::activity::BrowserRead;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};
use windows::core::Interface;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::System::Variant::{VARIANT, VT_I4};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation, IUIAutomation2, IUIAutomationCondition, IUIAutomationElement,
    IUIAutomationTreeWalker, IUIAutomationValuePattern, TreeScope_Descendants,
    UIA_ControlTypePropertyId, UIA_DocumentControlTypeId, UIA_EditControlTypeId,
    UIA_ValuePatternId,
};

/// A hung browser must not stall the 1 s session tick.
const TIMEOUT_MS: u32 = 300;
/// After a failed search, leave that window alone for a while instead of re-walking it.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// People switch between a handful of browser windows; forget the rest.
const MAX_WINDOWS: usize = 8;

struct Reader {
    automation: IUIAutomation,
    edit: IUIAutomationCondition,
    walker: IUIAutomationTreeWalker,
    bars: HashMap<isize, IUIAutomationElement>,
    failed: HashMap<isize, Instant>,
}

thread_local! {
    // `None` inside means setup failed once; don't retry COM setup every tick.
    // Never dropped: releasing UI Automation objects while the thread exits can hang.
    static READER: ManuallyDrop<RefCell<Option<Option<Reader>>>> =
        const { ManuallyDrop::new(RefCell::new(None)) };
}

pub fn read_browser(hwnd: HWND) -> BrowserRead {
    READER.with(|cell| {
        let mut cell = cell.borrow_mut();
        match cell.get_or_insert_with(|| unsafe { Reader::new() }.ok()) {
            Some(reader) => unsafe { reader.read(hwnd) },
            None => BrowserRead::default(),
        }
    })
}

impl Reader {
    unsafe fn new() -> windows::core::Result<Self> {
        // S_FALSE or RPC_E_CHANGED_MODE still leave COM usable on this thread.
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let automation: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER)?;
        if let Ok(timeouts) = automation.cast::<IUIAutomation2>() {
            let _ = timeouts.SetConnectionTimeout(TIMEOUT_MS);
            let _ = timeouts.SetTransactionTimeout(TIMEOUT_MS);
        }
        let edit = automation
            .CreatePropertyCondition(UIA_ControlTypePropertyId, &variant_i32(UIA_EditControlTypeId.0))?;
        let walker = automation.ControlViewWalker()?;
        Ok(Self {
            automation,
            edit,
            walker,
            bars: HashMap::new(),
            failed: HashMap::new(),
        })
    }

    unsafe fn read(&mut self, hwnd: HWND) -> BrowserRead {
        let id = hwnd.0 as isize;
        if self.failed.get(&id).is_some_and(|at| at.elapsed() < RETRY_AFTER) {
            return BrowserRead::default();
        }
        let bar = match self.bars.get(&id) {
            Some(bar) => bar.clone(),
            None => match self.find_address_bar(hwnd) {
                Some(bar) => {
                    remember(&mut self.bars, id, bar.clone());
                    bar
                }
                None => {
                    remember(&mut self.failed, id, Instant::now());
                    return BrowserRead::default();
                }
            },
        };
        match read_bar(&bar) {
            Some(read) => read,
            None => {
                // Stale after a toolbar rebuild or a closed window: search again next tick.
                self.bars.remove(&id);
                BrowserRead::default()
            }
        }
    }

    /// The first Edit in the window that is not inside the page. Toolbars come before page
    /// content in the tree, so this stops early on a normal browser window.
    unsafe fn find_address_bar(&self, hwnd: HWND) -> Option<IUIAutomationElement> {
        let root = self.automation.ElementFromHandle(hwnd).ok()?;
        let bar = root.FindFirst(TreeScope_Descendants, &self.edit).ok()?;
        let mut parent = self.walker.GetParentElement(&bar).ok();
        for _ in 0..32 {
            let Some(element) = parent else { break };
            if element
                .CurrentControlType()
                .is_ok_and(|control| control == UIA_DocumentControlTypeId)
            {
                return None;
            }
            parent = self.walker.GetParentElement(&element).ok();
        }
        Some(bar)
    }
}

unsafe fn read_bar(bar: &IUIAutomationElement) -> Option<BrowserRead> {
    let value: IUIAutomationValuePattern = bar.GetCurrentPatternAs(UIA_ValuePatternId).ok()?;
    let address = value.CurrentValue().ok()?.to_string();
    let editing = bar.CurrentHasKeyboardFocus().ok()?.as_bool();
    Some(BrowserRead {
        address: Some(address),
        editing,
    })
}

fn remember<V>(map: &mut HashMap<isize, V>, id: isize, value: V) {
    if map.len() >= MAX_WINDOWS {
        map.clear();
    }
    map.insert(id, value);
}

fn variant_i32(value: i32) -> VARIANT {
    let mut variant = VARIANT::default();
    unsafe {
        let inner = &mut *variant.Anonymous.Anonymous;
        inner.vt = VT_I4;
        inner.Anonymous.lVal = value;
    }
    variant
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::BOOL;
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClassNameW, IsWindowVisible};

    /// `cargo test manual_read_open_browser_windows -- --ignored --nocapture`
    #[test]
    #[ignore = "manual: reads every visible Chromium/Firefox window, in front or not"]
    fn manual_read_open_browser_windows() {
        unsafe extern "system" fn collect(hwnd: HWND, list: LPARAM) -> BOOL {
            let list = unsafe { &mut *(list.0 as *mut Vec<HWND>) };
            let mut class = [0u16; 64];
            let length = unsafe { GetClassNameW(hwnd, &mut class) } as usize;
            let class = String::from_utf16_lossy(&class[..length]);
            if unsafe { IsWindowVisible(hwnd) }.as_bool()
                && (class == "Chrome_WidgetWin_1" || class == "MozillaWindowClass")
            {
                list.push(hwnd);
            }
            true.into()
        }
        let mut windows: Vec<HWND> = Vec::new();
        let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut windows as *mut _ as isize)) };
        for hwnd in windows {
            for pass in ["find", "cached"] {
                let started = Instant::now();
                let read = read_browser(hwnd);
                println!("{:?} {pass:>6} {:>4}ms {read:?}", hwnd.0, started.elapsed().as_millis());
            }
        }
    }
}
