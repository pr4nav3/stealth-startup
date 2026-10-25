# Single rule set

Last updated: 2026-10-22. Status: **superseded** on 2026-10-22 by
`plans/v2-distractions-only.md`, which took the exceptions idea from here and dropped the
three buckets entirely. Kept for the reasoning; do not implement from it.

One set of rules, applied to every session, editable wherever the rule manager
already appears (Settings, preflight, the live dialog). The per-session copy and
"Apply permanently" go away. What replaces the copy is a one-click exception
during a session: "Allow X for this session", which expires with the session and
never touches the rules.

## Why

- Two editable rule sets is the largest source of duplicated UI in the app:
  a second scope through the rule editor and the drag and drop, a second save
  path, "Apply permanently", and the paragraph under every report explaining
  how decisions made during a session relate to today's rules.
- The two sets already leak into each other. `SessionManager::mark_current`
  saves the running session's whole rule list as the permanent list
  (`session.rs`, `save_rules(session.rules)`), so marking one window during a
  session with temporary edits silently promotes every temporary edit.
- The case temporary rules protect against ("this session is different, and I
  will forget to flip the rule back") is better served by an exception scoped
  to one app or site than by a copy of the whole set.

## Trade-offs accepted

- Relevant becomes the union of everything the user works in, so time in any
  relevant app counts as focus whatever the session's goal. The copy allowed
  per-goal tailoring; the app cannot judge goal semantics anyway.
- The exception is reactive: a site the user knows they need today still draws
  one nudge before "Allow for this session". A preflight allow-list would fix
  that and is a small per-session list again; not built until asked for.

## Backend (`src-tauri/src/session.rs`, `lib.rs`)

Today a session owns `rules: Vec<AppRule>` (a cleaned copy of the preflight
list) and its engine is built from them. Saving permanent rules during a
session does not reach the running engine.

1. **Session rules are the persisted rules plus exceptions.** Add
   `exceptions: Vec<AppRule>` to `Session`. The engine's list is
   `exceptions ++ persisted_rules` (first match wins, so an exception beats the
   rule it overrides). Drop `Session.rules`; `rebuild_rules(&mut self)` composes
   the list and calls `engine.replace_rules`, closing the open span the way
   `replace_rules` does now.
2. **Permanent saves reach the running session.** `update_rules` and
   `mark_current` call `rebuild_rules` on `current` after `save_rules`. This is
   the behaviour the single set needs: edit a rule in Settings mid-session and
   the next sample uses it.
3. **Exception command.** `allow_current_for_session` → `Session::allow_current(now)`:
   same preconditions as `mark_current` (`latest_focus.markable`), pushes an
   `AppRule` for the current activity with `Focused` onto `exceptions`,
   rebuilds, resyncs the open span like `mark_current` does, pushes the
   evidence line "Allowed this session", persists an event
   `allowed_for_session` with the activity key as detail.
4. **`mark_current` stays** as the permanent mark (overlay "More settings"), but
   saves `persisted_rules` with the new rule prepended, not the session list.
5. **Remove** `StartSessionInput.rules`, `update_session_rules`,
   `update_current_rules`, `Session::replace_rules` (folded into
   `rebuild_rules`). `start` always uses `persisted_rules`.
6. **Snapshot.** `CurrentSession.rules` becomes `CurrentSession.exceptions`.
   The UI already holds the permanent list.
7. **Report.** No change. The span state recorded during the session is the
   decision, as now; an allowed app shows under Focus with "now Distracting",
   which is accurate. The `allowed_for_session` event is recorded for the
   timeline only.
8. **Tests.** Two tests call `update_current_rules` and at least one passes
   `rules:` to `StartSessionInput`; rewrite them around `update_rules` plus
   `allow_current`. Add: exception beats a permanent Distracting rule; exception
   is gone in the next session; a permanent save mid-session changes the next
   sample; `mark_current` no longer persists exceptions.

## Frontend (`src/main.ts`, `src/styles.css`)

The rule manager stays in all three places; every instance edits
`permanentRules` through `persistPermanentRuleChanges`. Only one instance is on
screen at a time, so the editor and the drag and drop no longer need a scope.

Remove:

- `RuleScope`, `preflightRules`, `rulesForScope`, the scope parameter and
  data attributes on the rule manager, editor, chips, groups and drag and drop.
- `persistSessionRuleChanges`; the live dialog saves like Settings does.
- `apply-rules-permanently`, `applyRulesPermanently`, `.apply-permanent-button`.
- `rules` from the `start_session` payload.

Add or change:

- **Preflight** keeps the "Block distractions" disclosure and the manager; the
  helper line becomes "Changes apply to every session."
- **Live session card** keeps "What counts" and the dialog (retitled
  "Edit rules"). A second line lists exceptions: "Allowed this session:
  reddit.com".
- **Nudge popup** gets "Allow for this session" in place of "Mark as relevant",
  wired to `allow_current_for_session` and dismissing the popup like the
  current button. The permanent marks stay in the overlay's "More settings".
- **Report and recap** are untouched; caption and legend changes belong to
  the report simplification pass.
- `viewKey` and `editorIsOpen` keep their dialog entries; only the scope goes.

## Mock and docs

- `mock.html`: drop `update_session_rules`, add `allow_current_for_session`,
  give the live session `exceptions: []`.
- `plans/STATUS.md`: note the single set under Window supervision; remove the
  "Session rules" wording from the test gate if present.
- `plans/known-issues.md`: nothing to add; the "decisions made during a
  session keep" note still holds.

## Order

1. Backend: exceptions, rebuild on save, new command, removals, tests green.
2. Frontend removals (the diff is mostly deletions); type-check.
3. Preflight summary, "Edit rules", popup button, session card line.
4. Mock flags, render all three managers plus the popup, both themes.

Roughly a day. Backend and frontend can be done in either order since the
snapshot field rename is the only contract change; do the backend first so the
mock mirrors a real payload.
