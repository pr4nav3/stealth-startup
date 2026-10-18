# Window supervision and history audit

Reviewed 2026-10-17 on the dirty `feat/window-supervision` working tree.

## Assessment

The foundation is useful, but it is not ready to promise reliable Windows and
macOS focus tracking. Storing contiguous activity spans instead of samples,
keeping website identity separate from its label, and keeping titles and full
URLs out of the database are sound choices. The main remaining risks are
observation quality, stable app identity, and what the focus score actually means.

## Corrected in this audit

- An intentionally empty rule list no longer reloads starter rules. Initialization
  is recorded in the settings table when rules are saved.
- Known app keys containing dots resolve as apps before website parsing. App and
  website rules with identical text no longer delete each other during deduplication.
- The app-rule preview now states the existing browser exception: recognized
  websites use site rules, even when the browser has an app rule.
- A duplicate Start command preserves the running session instead of orphaning it.
- An address read cannot rewrite a title guess from a different page title in the
  same browser window.
- Open spans are checkpointed every 15 seconds during normal polling, updating
  the same database row. Closing a span finalizes that row. A crash no longer
  loses an entire long uninterrupted activity; unsaved time since the checkpoint
  can still be lost, and write failures remain reported to the user.
- Fractional polling intervals no longer repeatedly discard fractional time from
  live counters. Capture-gap recovery accounts the missing interval as untracked.
- Mark current changes the stored state at the time of the action.
- Delayed expiry cannot credit time beyond the session timer.
- Gaps split focus stretches and drift sequences. Schema v6 recomputes existing
  cached metrics transactionally so old reports receive this correction too.
- Reports and recap halves use the span offset range, including pauses, so later
  activity is not clipped. Timeline bars and nudge markers use the same scale;
  blank space represents time without a recorded span.
- App display renames do not change recap rule matching. The recap explains that
  its groups use current rules while scores describe recorded session decisions.
- Removed the work/comms/browse/media category guesses. They were a second,
  hard-coded taxonomy beside the user's rules and only fed one report card. The
  session report now lists time per app/site grouped by the recorded decision,
  using the same rows as the weekly recap. The `span.category` column is
  retired (written empty) so older databases still open.
- Mute nudges no longer stops recording. Muted time is recorded and judged like
  active time; only nudges and the drift popup are held back. If the user is
  still drifting when the 5-minute mute ends, the held nudge arrives. Pause is
  the only untracked break; muting while paused is disabled.
- Positive durations below a minute display as `<1 min`.
- Removed the nonfunctional PDF button and its empty handler until export exists.
- A newer or temporarily unavailable database is preserved. Automatic reset is
  limited to SQLite's explicit corruption/not-a-database errors.
- Session completion and its summary update are one transaction. Removed the
  unused `StoredRule.source` wrapper; the old database column remains compatible.
- Malformed non-ASCII date input returns an error rather than risking a slice panic.

- **Gap-free timeline.** A session's spans now tile it from Start to Stop. Every
  span starts where the previous one ended (`recorded_until` in `session.rs`),
  so late samples, rule edits, and resumes no longer leave slivers. Supervisor's
  own windows are recorded as a focused "Supervisor" app row (never judged, no
  nudges). Pauses are `paused` spans: drawn on the timeline, excluded from every
  score, and they end a focus stretch. A silent poll loop (>15 s: sleep or a
  freeze) and failed captures become "Device asleep / unknown" (`unknown`,
  counted with idle) instead of crediting the last app. Close and orphan
  recovery fill any remaining hole as lost time and log it; the
  `every_second_of_a_session_is_recorded` property test fails if that repair
  ever runs. Only pre-fix sessions can still show hatched "Not recorded" time.

- **Recovery replaces first/second half; Trends replaces the monthly recap.**
  Recovery is the median time from leaving focus (into Slipping/Distracted) to
  being back on a focused or neutral app; away and paused time is not counted,
  and drifts still open at session end are reported separately. Trends shows
  8 weeks of focused hours, attention span, and recovery, unlocked after 2
  completed weeks. Weekly figures now pool every stretch and drift in the week
  instead of taking medians of session medians (gap 7 below is resolved).

## Remaining release gaps

1. **macOS browser addresses are not implemented.** `browser_macos.rs` always
   returns unavailable. Title fallback covers only a small predefined set of
   sites. Mac permissions, browser behavior, and packaged app behavior need real
   Mac acceptance testing. Passing Windows tests cannot establish this.
2. **Windows address detection is heuristic.** The UI Automation reader selects
   the first Edit control outside page content. This needs acceptance cases for
   Firefox, browser search/sidebar controls, redirects, toolbar rebuilds, and
   localized browsers. An unreadable address must remain visibly unknown.
3. **App identity (mostly resolved, schema v11).** Each sample carries a native
   identity (`app_identity.rs`): a packaged app's AUMID; else the exe name plus
   version-info CompanyName/ProductName; else the install path with version
   folders ignored; java/python/node/electron and other shared hosts add the
   jar/module/script from the command line; macOS uses the bundle identifier.
   The `app_identity` table maps identities to keys (unique). The first identity
   whose label fits a stem's stored history keeps the old stem as its key, so old
   spans and rules carry on; others get `stem#2`, shown as "Name (2)". Shared
   hosts and Supervisor's marker keys are never inherited. Unresolved
   ApplicationFrameHost frames are a shell surface. v11 rewrites name-valued app
   rules to a key only when exactly one recorded app has that name. Remaining:
   history recorded before v11 under one stem by two different programs cannot be
   split (it stays with whichever identity fits its latest label); a typed name rule
   that two apps share still rates both (new typed names that are ambiguous are
   refused); macOS bundle-id capture needs a real Mac run. Historical shell
   cleanup is still a limited compatibility mapping.
4. **Capture failures still count with idle time in the headline.** The score
   is now Relevant time only (schema v7): Neutral and unrated time no longer
   count as focus, unrated time has its own total, and rating an app rescores
   its past unrated time. `source = none` keeps capture failures distinct in
   detailed spans, but the Away total does not; expose observation coverage.
5. **Sleep/resume needs a live acceptance test.** The expiry cap prevents
   overcounting past the timer, but does not prove that time while suspended
   was productive. (Mute vs pause semantics are now resolved; see above.)
6. **Historical explanation is incomplete.** Spans retain state/category/source,
   but not the matched rule or whether neutral was explicit versus unmatched.
   Add this provenance before depending on historical rule explanations.
   Time recorded as neutral follows today's rules, so a Neutral rating given
   during a session is overridden by a later rating; Relevant and distracting
   decisions keep. This was chosen over a per-span flag (see `rated_state`).
   Browser app rules currently exclude recognized websites entirely. Decide
   whether users should instead be able to set a browser-wide default with site
   exceptions; the preview now describes the behavior actually implemented.
7. **Interaction QA is unfinished.** Live-session polling no longer rebuilds
   under an open editor or focused field; keyboard behavior still needs a pass. PDF export is
   still unimplemented; its nonfunctional control has been removed.
8. **Webcam is not in the session pipeline.** Phone/away totals remain zero and
   strictness/nudge arbitration is not integrated. This audit did not change voice
   generation or webcam behavior.

## Keeping detail now and shrinking later

Keep recent spans and events. A view can show only the most useful rows while
retaining full detail for inspection; storage retention and visual clutter are
separate concerns. The current span model is already much smaller than one row
per second. Checkpointing does not increase the number of rows.

Before adding automatic deletion, measure actual database growth and choose a
user-visible retention policy. A reasonable proposal is full timelines for a
recent period, then per-session/app/site/state/source aggregates for older data.
Preserve goals, timestamps, durations, focus/distraction/idle/unknown totals,
nudges, observation coverage, and stretch distributions needed for trend metrics.
Preserve name overrides independently. Mark old sessions as summarized so the
UI does not imply that their exact timeline is still available.

Create and verify aggregates in a transaction before deleting their source
spans. Reads must understand both detailed and summarized sessions; do not count
both. Only then consider idle-time SQLite space reclamation. Deleting rows alone
normally makes their pages reusable rather than reducing the file immediately.
No automatic retention/deletion policy was introduced in this audit.

## Validation

Regression tests cover empty rules, dotted app keys, rule target deduplication,
duplicate Start, title correction across tabs, crash checkpoints, fractional
timing, capture recovery, Mark current boundaries, delayed expiry, pause gaps,
schema-v6 metric repair, rename-independent grouping, muted-time recording,
per-session app grouping, malformed dates, and preserving newer databases. Run `cargo test` and
`npm run check`. Result: 133 Rust tests passed, 3 manual probes ignored; frontend
typecheck and production build passed. Native macOS and visual end-to-end
acceptance remain outstanding.
