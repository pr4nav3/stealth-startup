# Known issues (accepted for now)

Problems we know about and decided not to fix yet. Each entry says what goes wrong, where,
and what a fix would be.

## Report categories use loose keyword matching

Where: `src-tauri/src/storage.rs`, `span_category` and the `is_media_signal`, `is_comms_app`,
`is_work_app`, `is_browser_app` keyword lists.

What goes wrong: a span's category (work, browse, comms, media) is decided by checking if the
app name (and, for media, the window title) *contains* a short keyword. Examples:

- Media keywords are checked against window titles, so a document named
  "YouTube strategy.docx" or a Reddit programming thread counts as media.
- Work keyword "code" (for VS Code) also matches "Barcode Scanner"; "cursor" matches any app
  with "cursor" in its name.
- Comms keyword "zoom" also matches "ZoomIt" (a screen magnifier); "mail" matches any app
  with "mail" in its name.
- The browser list ("arc", "edge", ...) matches loosely too ("arc" is inside "Search"), but it
  has no effect today: "browse" is also the fallback when nothing else matches.

Effect: some time lands in the wrong category in session reports and trends. Focus scoring is
not affected (that uses the focus rules, not these categories).

Possible fix: match on exact program names (`Code.exe`, `Arc.app`) and known site domains
instead of substrings, and let the user recategorize an app from the report.
