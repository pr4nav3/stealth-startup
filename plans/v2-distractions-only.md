# V2: distractions only

Branch `app-v2-simplified-rules`, cut from `main` at c7eb971. Last updated: 2026-10-22.
Status: implemented on the branch (backend, frontend, mock, docs); 192 backend tests pass;
every screen rendered through the mock harness. Not yet run in the built app.

One sentence for the user: **tell Supervisor what distracts you; everything else
counts as work.** Rules are a single list of distractions. Relevant, Neutral and
Unclassified are gone, in the data, the engine, the reports and the screens.
Rules apply from the next sample onward and never rewrite recorded time.

This file is the change inventory. Every item was traced to its code before
writing. `plans/single-rule-set.md` (plan A, three buckets with a session
layer) is superseded by this.

## The model

| | V1 (main) | V2 |
| --- | --- | --- |
| Rule | target + value + classification (focused / neutral / distracting) | target + value; a rule *is* a distraction |
| Time with no rule | Unclassified; does not count; nags to classify | Work; counts |
| Focus states | focused, neutral, suspicious, distracted, idle | focused, suspicious, distracted, idle |
| Score | Relevant time ÷ duration | time not on a distraction ÷ duration |
| Rule change | rescores all past unclassified time | forward only; history never changes |
| Per-session | a copy of the whole rule set | an allow-list of exceptions ("not today") |
| Marks during a session | Relevant / Distracting, both permanent | "This is a distraction" (permanent), "Not today" (session), "Never a distraction" (removes the rule) |
| After a session | classify queue, 1/2/3 keys | nothing asked; the report is read-only |

Stretches, recovery, drifts, phone time, nudges, trends, streak and the day
card keep their meaning; "on task" simply becomes everything that is not a
distraction, which the day card already draws that way.

## Backend

### `focus.rs`
- `RuleClassification` removed. `AppRule { target, value }`. Constructors
  `app(value)`, `website(key)`, `from_input(text)`, `from_input_with_apps(text, names)`.
- `FocusState::Neutral` removed. `record_sample`: a matching rule gives
  Suspicious (inside the grace) or Distracted; no rule gives Focused.
- `FocusSnapshot.classification` becomes `distracting: bool`;
  `matched_rule` stays (`Some` means a distraction rule matched).
- `FocusEngine` gets `exceptions: Vec<AppRule>`; `matching_rule` skips a rule an
  exception names. `set_exceptions`, `replace_rules` kept.
- `starter_rules` unchanged in content (five sites), now plain distractions.
- Tests: rewrite the ones that build Focused/Neutral rules.

### `session.rs`
- `Session.rules` removed. Engine rules come from `persisted_rules`;
  `Session.exceptions: Vec<AppRule>` composes with them via `rebuild_engine`.
- `SessionMetrics`: `focused_seconds`, `distracted_seconds` (suspicious and
  phone included), `idle_seconds`, `phone_seconds`, `nudges`. `neutral_seconds`
  and `suspicious_seconds` removed; `metric_mut` maps Suspicious to distracted.
- `SessionSummary`: `neutral_seconds`, `unrated_seconds` removed.
- `StartSessionInput.rules` becomes `exceptions: Option<Vec<AppRule>>`.
- `update_rules` saves and rebuilds the running session's engine, so a Settings
  edit mid-session applies at the next sample. No rescore; `RulesSaved` is just
  the rules; `RescoreNotice` and `SessionSnapshot.rescore` removed.
- `mark_current(classification)` becomes three operations, all needing a
  markable activity:
  - `flag_current` (permanent): prepend a rule, save, rebuild, resync the open
    span to Distracted as `mark_current` does today, evidence "Marked distraction",
    event `marked_distracting`.
  - `allow_current_for_session`: push an exception, rebuild, resync the span to
    Focused, evidence "Allowed this session", event `allowed_for_session`.
  - `allow_current_always`: remove the matching rule, save, rebuild, resync,
    evidence "Rule removed", event `rule_removed`.
- `update_current_rules`, `replace_rules`, `marked_focus_state`'s Neutral arm,
  `state_label`'s Neutral arm removed. `keep_current_rules` / `clean_rules` stay.
- `CurrentSession.rules` becomes `CurrentSession.exceptions`.
- Tests: `mark_current_relevant_wins_over_starter_rules` and the unrated
  assertions are rewritten; new tests for the three operations, for a
  mid-session permanent save reaching the engine, and for exceptions not
  persisting.

### `storage.rs`
- Schema v12: `DELETE FROM rule WHERE classification != 'distracting'`;
  `UPDATE span SET state = 'focused' WHERE state = 'neutral'`;
  `ALTER TABLE session DROP COLUMN neutral_seconds` and `unrated_seconds`;
  recompute every session once; `user_version = 12`. The `classification`
  column stays and is written as `'distracting'`, so older binaries can still
  read the table.
- `rated_state` removed: spans read back exactly as recorded.
  `load_spans_for_session` loses its `rules` parameter. Supervisor's own window
  is on task like any other.
- `recompute_session_metrics` sums focused / distracted (suspicious, distracted,
  phone) / idle (idle, unknown) / phone. `on_task(state)` is `state == "focused"`.
- `replace_rules` writes rules and nothing else. `rescore_history` stays as a
  private helper for migrations and `recover_orphans`.
- `RecapApp`: `group` is `"Distractions"`, `"Work"` or `"Away"` from span
  states; `rating`, `flagged` and `ratable` are gone. A row reports time and
  nothing else; the distractions list is edited in Settings or mid-session.
- `SessionReport.unrated_seconds` removed; `SUMMARY_SELECT` loses the columns.
- `week_recap` and trends unchanged apart from the fields above.
- Tests: the v7 and rating-rescore tests become a v12 migration test (neutral
  spans read as focused, non-distraction rules dropped, columns gone) and a
  forward-only test (adding a rule leaves an old report untouched).

### `lib.rs`
- Commands removed: `mark_current_relevant`, `update_session_rules`.
- Commands added: `flag_current_distracting` (rename of
  `mark_current_distracting`), `allow_current_for_session`, `allow_current_always`.
- `resolve_rule_input(text)` loses the classification argument.
- `update_rules` unchanged in name; returns `RulesSaved { rules }`.

## Frontend (`src/main.ts`, `src/styles.css`)

Removed outright:
- `RuleClassification`, `RuleScope`, `preflightRules`, `rulesForScope`,
  `renderRuleGroup`, the Mode select in the rule editor, the rule drag and drop
  and its CSS, the drag hint, `apply-rules-permanently` and its button,
  `persistSessionRuleChanges`, `applyRulesPermanently`.
- The classify flow: `rating` state, `renderRatingModal`, `startRating`,
  `advanceRating`, `finishRating`, `chooseRating`, `saveRating`,
  `renderRatePrompt`, `unratedApps`, `scoreAwaitsClassifying`,
  `renderFocusStat`'s classify branch, the 1/2/3 keyboard handler, the
  `rate-*` actions and `.rate-*` / `.unrated-dot` / `.rating-changed` CSS.
- `SCORING_CAPTION`, `noteRescore`, `freshRescoreNote`, rescore toasts.
- `state-neutral`, `state-unrated` and "Slipping" from the legend, mini
  timeline and timeline CSS; the "Mark what counts" onboarding step.

Changed:
- **Rule manager** (`renderRuleManager`): one chip list under "Distractions",
  "+ Add", chips open the editor (website or app, name, preview, Save, Delete).
  Same component in Settings and in the live dialog, saving through
  `persistPermanentRuleChanges`.
- **Preflight**: "Distractions" disclosure shows the chips as toggles; a tapped
  chip is struck through and listed as "not today"; "+ Add" opens the editor
  (permanent). `startSession` sends `exceptions`.
- **Live session card**: "Distractions" chips, allowed-today chips struck, a
  line "Not today: reddit.com", button "Edit rules" opens the dialog.
- **Nudge popup**: "Return to task", "Not today", "Never a distraction".
- **Overlay More settings**: one mark, "This is a distraction".
- **Report / recap apps**: `APP_GROUPS = ["Distractions", "Work", "Away"]`;
  rows are read-only (name, bar, time). The week's pane shows Work beside
  Distractions, each column saying so when empty, with Away as a footer line.
- **Report stats**: Duration, Focused %, Nudges, Phone when present. The
  summary line keeps attention span.
- **Legend**: Work, Distraction, Away, Paused (phone and suspicious draw red).
- **State copy**: `stateTitle` loses "neutral"; default stays "Watching".
- **Voice page** (mockup "Voice v2", 920×760, no scrolling): Voices as before;
  a "?" on the title row opens the how-to; Voice model and Memory are two tiles
  that turn red when they block and open popups (`›`); Clips for one voice
  shows its tones (left) and the viewed tone's lines (right). A voice speaks
  one tone (`set_voice_tone`, stored in profile.json); it is selectable once
  every line of that tone has a clip, and nudges draw only from that tone.
  "Generate N missing" makes the lines one after another in place (about 90 s
  each), with Stop after this line and per-line Redo. Edit lines adds, edits
  and deletes lines; custom tones can be renamed or deleted.
- **Onboarding**: clone a voice, webcam (optional), first session. The "Add
  rules" step is replaced by a line in the intro: "Five common distractions are
  already on the list."
- **History** (mockup "History v4"): a toolbar panel (week ‹ › with "This
  week" / "Last week" tag, search across every session by goal via
  `search_sessions`), a yellow week hero (focused hours, longest stretch,
  recovery, daily bars with today drawn light), a Trends door (change against
  last week through the same weekday, sparkline of recent weeks), then a
  centered Sessions / Apps & sites toggle over one panel. The week label opens
  a month picker (`get_focus_days(start, end)`: focused time per local day),
  days shaded by focused time, the viewed week outlined; any past day picks
  its week. Bars in the hero and the Trends card scale so the biggest reaches
  the top of its track whatever the week held. Apps & sites is three
  columns (Work, Distractions, Away) with totals; rows still toggle the rule.
  Day card: `SessionSummary` fields; `stripOnTask` is `state === "focused"`.

## Mock, landing page, docs
- `mock.html`: rules without classification; `flag_current_distracting`,
  `allow_current_for_session`, `allow_current_always`; a `live` session carries
  `exceptions`; no rescore field.
- `docs/index.html`: feature card 03 "Your own rules" → "Your distractions:
  five common ones are on the list; add yours, and tell it 'not today' when a
  site is the work." Mock chips "Relevant / Distracting" → "Not today / Never".
- `plans/STATUS.md`: a V2 section describing the model; the "Focused-only
  score" and "browsers are Neutral" lines rewritten (browsers are still never
  rules; only sites are).
- `plans/known-issues.md`: drop the Neutral and unclassified notes; keep
  "decisions during a session keep" reworded as "history never changes".

## Order
1. Backend `focus.rs` → `session.rs` → `storage.rs` → `lib.rs`; `cargo test`
   green at each step.
2. Frontend deletions, then the rule manager, preflight, session card, popup.
3. Report, recap, history, onboarding, landing page, docs.
4. Mock flags; render Settings, preflight, live card, dialog, popup, report,
   recap, History, Home, in both themes, with many rules.
