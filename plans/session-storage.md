# Session Storage and Reports

**Dated.** Written 2026-10-15 against an earlier schema. Treat `feat/window-supervision` as the source of truth. Window targeting, attribution, and history storage are being rewritten on that branch. Use this file for background, not as the spec to implement.

Last updated: 2026-10-15 (body). Banner: 2026-10-16.

2026-10-17 implementation update: open spans are now checkpointed in place every
15 seconds during normal polling, then finalized on close. Schema v6 repairs
cached stretch/drift metrics so gaps break continuous focus. Current audit and
future retention design: `plans/window-history-audit.md`. The older statements
below about writing only on close and needing no future aggregate tables are
historical design notes, not constraints on that retention work.

SQLite persistence, the report, trends, and week/month recap screens are already in the app. PDF export is unimplemented; its empty control was removed. Live tracker: `plans/STATUS.md`.

## Scope

**Session-only.** The app watches while a session runs and not otherwise. No
always-on tracking, no work-hour schedules, no categorizing every window the
user has ever opened.

That keeps the privacy line short ("we only watch when you ask") and keeps the
question sharp: did you do the thing you said you would? The time breakdown we
show is the time the user *intended* to be productive, which is the useful
slice anyway.

## Store spans, not samples

The ticker samples once a second. Those samples are never rows. While the
active window and focus state are unchanged, extend the open span; when either
changes, close the span and write it. A two-hour session with forty switches is
about eighty rows.

Three rules that are not negotiable:

- **Spans use an offset from session start, not wall clock.** `session.rs`
  already works in `Duration` since app start. Offsets keep it that way and
  sidestep DST and clock changes. Session start is the only wall clock stored.
- **Write each span as it closes, never batched at session end.** A crash or a
  sleep must not lose the session.
- **Rollups on `session` are a cache.** They are written on close for fast
  history reads, and they are fully derivable from `span`.

```sql
CREATE TABLE session (
  id INTEGER PRIMARY KEY,
  goal TEXT NOT NULL,
  strictness TEXT NOT NULL,
  started_at TEXT NOT NULL,          -- RFC3339 with offset
  ended_at TEXT,                     -- NULL while running
  planned_seconds INTEGER NOT NULL,
  ended_reason TEXT,                 -- completed | stopped | abandoned
  focused_seconds INTEGER NOT NULL DEFAULT 0,
  distracted_seconds INTEGER NOT NULL DEFAULT 0,
  idle_seconds INTEGER NOT NULL DEFAULT 0,
  away_seconds INTEGER NOT NULL DEFAULT 0,
  phone_seconds INTEGER NOT NULL DEFAULT 0,
  drift_count INTEGER NOT NULL DEFAULT 0,
  nudge_count INTEGER NOT NULL DEFAULT 0,
  longest_stretch_seconds INTEGER NOT NULL DEFAULT 0,
  median_stretch_seconds INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE span (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  app TEXT NOT NULL,                 -- program in front (the browser for website rows)
  kind TEXT NOT NULL,                -- app | website | browser_pages | browser_local | other_websites
  activity_key TEXT NOT NULL,        -- app id, website key (youtube.com), or the browser id
  label TEXT NOT NULL,               -- display name at write time; website labels are re-derived on read
  source TEXT NOT NULL,              -- app | address | grace | title | none
  category TEXT NOT NULL,            -- retired; written empty
  state TEXT NOT NULL,               -- focused | neutral | suspicious | distracted | idle | paused | unknown
                                     -- (read back, neutral becomes today's rating or `unrated`)
  started_offset_seconds INTEGER NOT NULL,
  seconds INTEGER NOT NULL
);
CREATE INDEX span_session ON span(session_id, started_offset_seconds);

CREATE TABLE event (
  id INTEGER PRIMARY KEY,
  session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
  at_offset_seconds INTEGER NOT NULL,
  kind TEXT NOT NULL,                -- nudge | snooze | pause | resume | marked_relevant | marked_distracting | phone | away | back
  detail TEXT
);

CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE rule (
  id INTEGER PRIMARY KEY,
  target TEXT NOT NULL,              -- app | website
  value TEXT NOT NULL,               -- app key or legacy name, or website key
  classification TEXT NOT NULL,
  source TEXT NOT NULL               -- starter | user
);

CREATE TABLE site_name (
  key TEXT PRIMARY KEY,              -- website key (nytimes.com)
  name TEXT NOT NULL                 -- what the user calls it (NYT)
);

CREATE TABLE app_name (
  key TEXT PRIMARY KEY,              -- current app key (Windows exe stem, macOS app name)
  name TEXT NOT NULL                 -- user's display name
);
```

No window title, URL, or file path is ever stored. Titles are read only to pick
the activity (see `attribution.rs`).

A website row is shown as the user's name from `site_name`, else the built-in
name (`SITES` in `website.rs`), else the domain. Names live apart from rules,
so deleting a rule keeps the name, and a rename reaches old history because
website labels are looked up when read. Schema v4 added `site_name`.
Schema v5 added `app_name` with the same read-time behavior for app rows.
Naming changes display only; new Mark current rules use the app key. Legacy
label-valued app rules still match the OS label.

Schema v3 (`PRAGMA user_version = 3`) moved old databases to this shape:
non-browser spans became `app` rows, browser spans became `other_websites`
(their titles cannot be trusted to name a site), and `context` was dropped.
Rules: `app_name` became `app`; title rules became `website` when they were an
address or a verified site name, `app` when they were a plain name, and were
deleted otherwise; `process_path` rules were deleted.

`away_seconds` and `phone_seconds` stay zero until webcam signals reach
`session.rs` (webcam-session leftover in `plans/STATUS.md`). The columns exist
now so the report does not need a migration later.

On launch, every session with `ended_at IS NULL` is closed as `abandoned` and
its rollups are recomputed from its spans. That is the whole crash-recovery
story; it needs no journal and no extra table.

Do not add daily rollup tables. Three sessions a day for a year is roughly
110k spans, which SQLite scans in milliseconds.

## Metrics

A **focused stretch** is an unbroken run of focused, neutral, or unrated time
until the first distracted, idle, or away state. Unrated time is not drift. Three numbers come out of it, and
each has to be explainable in one sentence.

| Number | Definition | Why |
| --- | --- | --- |
| Focus % | Relevant seconds over session length; Neutral and unrated don't count | The honest headline |
| Typical stretch | **median** stretch length | The trend line; a median keeps one heroic hour from hiding a choppy day |
| Longest stretch | longest stretch length | The record to beat |

No composite score out of 100. Rize blends "20+ attributes" nobody can inspect;
users stop trusting that the first time it disagrees with how the day felt.

## Session report

One page, four blocks, top to bottom. Opens when a session ends, and is
reachable again from History.

1. **One headline line.** `52 min · 78% focused · typical stretch 9 min`. No
   cards, no chrome.
2. **One full-width timeline bar**, colored by state over time, ticked where
   nudges fired. Highest information density in the design: solid for twenty
   minutes, fell apart in the middle, recovered. Hover gives app and duration.
   CSS flexbox over the span rows.
3. **Where it went** — time per app/site, grouped Focus / Neutral /
   Distracting / Unrated / Away, matching the timeline colors and summing to
   the scores. Same rows as the weekly recap.

**Rating past time.** Time recorded as neutral had no rule then (or a Neutral
one), so it follows today's rules: rating an app rescores its past unrated time
and every cached session total. Relevant and distracting decisions made during
a session keep, since they may come from that session's own rules.

## Trends

Two elements above the existing History list.

1. **Attention-span sparkline**, last 14 sessions, captioned with the change
   against the previous seven. This is the "is it improving" answer.
2. **Focus % per day**, 14 small bars. Catches focusing well but rarely, which
   a session average hides.

The session list keeps its rows and gains the attention-span number and a
miniature timeline bar.

## Dependencies

`rusqlite` with `bundled` (no system SQLite on Windows) is the only addition.
Charts are hand-rolled SVG and CSS; no chart library.

## Deletion

- Delete one session (cascades to spans and events)
- Delete all history

## Out of scope

Always-on tracking, work-hour schedules, pie charts, contribution heatmaps,
composite scores, streaks and badges, PDF export, weekly digests, cloud sync of
any kind. macOS website reads (`browser_macos.rs` is a stub until a Mac is on
hand).

## Check

`recompute_session_metrics(id)` is the seam to test: build spans in a fixture,
assert the rollups, then assert they survive a close-and-reload and that an
orphaned session recovers as `abandoned`.

## Order

Settings and rules persistence first — smallest visible win, and it proves the
migration path. Then the span writer and orphan recovery, then rollups and the
report command, then the report screen, then trends, then deletion.

The report and trend screens **are** the UI overhaul for two of three tabs.
Build them in the new styling once rather than against the old styling twice.
