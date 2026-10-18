//! Browser address read on macOS. Not built yet: every read is "unavailable", so browsers
//! show as Other websites (or a verified title match) until this lands.
//!
//! To build it: read the focused window's `AXURL` (Safari, Chrome, Edge, Brave, Arc expose
//! it on the web area) through the Accessibility API for `process_id`, and report
//! `editing` when the address field has keyboard focus. That needs the Accessibility
//! permission; check it next to the Screen Recording check in `activity.rs`.

use crate::activity::BrowserRead;

pub fn read_browser(_process_id: u64) -> BrowserRead {
    BrowserRead::default()
}
