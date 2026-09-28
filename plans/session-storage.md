# Session Storage and Reports

Last updated: 2026-09-22

Local SQLite for session history, settings, and rules, plus the report and
trend screens that make the data worth keeping. Tracker item:
`plans/STATUS.md` §4 (and the two-thirds of §5 that these screens replace).

Nothing persists today. `SessionManager` keeps history in a `Vec`, settings and
rules are in-memory, and a restart loses all of it. Only voice files survive,
on disk.

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
  app TEXT NOT NULL,
  context TEXT,                      -- tab or document title; nullable so it can be scrubbed
  category TEXT NOT NULL,            -- work | comms | browse | media | offscreen
  state TEXT NOT NULL,               -- focused | neutral | suspicious | distracted | idle
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
  target TEXT NOT NULL,
  value TEXT NOT NULL,
  classification TEXT NOT NULL,
  source TEXT NOT NULL               -- starter | user
);
```

`away_seconds` and `phone_seconds` stay zero until webcam signals reach
`session.rs` (`plans/STATUS.md` §7). The columns exist now so the report does
not need a migration later.

On launch, every session with `ended_at IS NULL` is closed as `abandoned` and
its rollups are recomputed from its spans. That is the whole crash-recovery
story; it needs no journal and no extra table.

Do not add daily rollup tables. Three sessions a day for a year is roughly
110k spans, which SQLite scans in milliseconds.

## Metrics

A **focused stretch** is an unbroken run from entering focused or neutral until
the first distracted, idle, or away state. Three numbers come out of it, and
each has to be explainable in one sentence.

| Number | Definition | Why |
| --- | --- | --- |
| Focus % | focused seconds over session length | The honest headline |
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
3. **Where it went** — five category bars, with the top three apps as plain
   text underneath. Bars, not a pie: they stay readable small and sort.
4. **Biggest drifts** — at most five lines (`12:40 YouTube 4 min`). Full log
   behind a disclosure triangle.

Five fixed categories: work, comms, browse, media, offscreen (idle plus away
plus phone). Re-tagging an app from the report writes a `rule` row, reusing the
existing mark-relevant / mark-distracting mechanic after the fact.

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
- Scrub `span.context` while keeping categories and metrics, so the shape of a
  day survives without the titles

## Out of scope

Always-on tracking, work-hour schedules, pie charts, contribution heatmaps,
composite scores, streaks and badges, PDF export, per-website breakdown before
the Windows URL path is real, weekly digests, cloud sync of any kind.

## Category mapping

Span categories are assigned deterministically from focus state, rule
classification, app name, and window title (lowercased). Order matters:

1. **offscreen** — focus state is `idle` (future `away` / `phone` webcam spans
   will also land here).
2. **media** — app/title contains a media marker (`youtube`, `netflix`, `tiktok`,
   `instagram`, `reddit`, `spotify`, `twitch`, `hulu`, `disney+`, `prime video`).
3. **comms** — app name contains `slack`, `discord`, `teams`, `zoom`, `outlook`,
   `mail`, `messages`, `telegram`, or `whatsapp`.
4. **work** — state is `focused`, or classification is `focused`.
5. **media** or **browse** — classification/state is distracting: media markers
   win, otherwise `browse`.
6. **work** — app matches a work tool (`code`, `cursor`, `visual studio`, IDE
   names, `terminal`, `notion`, `obsidian`, `figma`, …).
7. **browse** — browser apps (`chrome`, `firefox`, `edge`, `brave`, `safari`,
   `opera`, `arc`) with neutral/suspicious focus.
8. Default **browse** for anything else.

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
