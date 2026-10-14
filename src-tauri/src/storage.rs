use crate::focus::{starter_rules, AppRule, RuleClassification, RuleTarget};
use crate::session::{utc_now, SessionSettings, SessionSummary, Strictness, HISTORY_CACHE_LIMIT};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i32 = 2;
const STRETCH_WEEK_COUNT: usize = 5;
const PHRASE_LIBRARY_KEY: &str = "voice.phrase_library";

/// How long a `session` row's timer actually ran. Sessions that never closed cleanly have no
/// timer value; their tracked span time is the best record of how long they ran.
const RUN_SECONDS_SQL: &str = "COALESCE(session.elapsed_seconds,
    (SELECT COALESCE(SUM(span.seconds), 0) FROM span WHERE span.session_id = session.id))";

const WEBCAM_SETTING_KEYS: &[&str] = &[
    "webcam.enabled",
    "webcam.phone_nudge",
    "webcam.away_after_seconds",
];

pub struct SessionStorage {
    conn: Connection,
    failure: RefCell<Option<String>>,
}

/// What the user must be told about history storage for this run.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StorageNotice {
    /// The existing database could not be opened; it was moved aside and a fresh one started.
    Reset { reason: String, moved_to: String },
    /// No database could be opened or created; history is not saved this run.
    Off { reason: String },
    /// A save or read failed during this run.
    Failed { reason: String },
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StoredPhraseLibrary {
    pub custom_tones: Vec<String>,
    pub custom_phrases: Vec<StoredPhrase>,
    pub phrase_overrides: Vec<StoredPhrase>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StoredPhrase {
    pub id: String,
    pub tone: String,
    pub text: String,
}

pub struct StoredState {
    pub settings: SessionSettings,
    pub rules: Vec<AppRule>,
    pub history: Vec<SessionSummary>,
}

pub fn open_or_reset(
    path: Result<PathBuf, String>,
) -> (Option<(SessionStorage, StoredState)>, Option<StorageNotice>) {
    let path = match path {
        Ok(path) => path,
        Err(reason) => return (None, Some(StorageNotice::Off { reason })),
    };
    let reason = match SessionStorage::open(&path) {
        Ok(opened) => return (Some(opened), None),
        Err(reason) if path.exists() => reason,
        Err(reason) => return (None, Some(StorageNotice::Off { reason })),
    };
    let moved_to = match move_aside(&path) {
        Ok(moved_to) => moved_to,
        Err(error) => {
            let reason = format!("{reason}; could not move the damaged file aside: {error}");
            return (None, Some(StorageNotice::Off { reason }));
        }
    };
    match SessionStorage::open(&path) {
        Ok(opened) => (
            Some(opened),
            Some(StorageNotice::Reset {
                reason,
                moved_to: moved_to.display().to_string(),
            }),
        ),
        Err(reason) => (None, Some(StorageNotice::Off { reason })),
    }
}

fn move_aside(path: &Path) -> std::io::Result<PathBuf> {
    let stamp = time::OffsetDateTime::now_utc().unix_timestamp();
    let moved = path.with_file_name(format!("sessions.broken-{stamp}.db"));
    std::fs::rename(path, &moved)?;
    // A leftover -wal/-shm next to a fresh database would be replayed into it.
    for suffix in ["-wal", "-shm"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            std::fs::rename(&sidecar, format!("{}{suffix}", moved.display()))
                .or_else(|_| std::fs::remove_file(&sidecar))?;
        }
    }
    Ok(moved)
}

#[derive(Clone, Debug)]
pub struct StoredRule {
    pub rule: AppRule,
    pub source: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportSpan {
    pub started_offset_seconds: u64,
    pub seconds: u64,
    pub app: String,
    pub context: Option<String>,
    pub category: String,
    pub state: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct CategoryTotal {
    pub category: String,
    pub seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DriftEntry {
    pub at_offset_seconds: u64,
    pub label: String,
    pub seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReportEvent {
    pub at_offset_seconds: u64,
    pub kind: String,
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct SessionReport {
    pub id: u64,
    pub goal: String,
    pub strictness: String,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub ended_reason: Option<String>,
    pub planned_seconds: u64,
    pub duration_seconds: u64,
    pub focus_percent: u64,
    pub focused_seconds: u64,
    pub distracted_seconds: u64,
    pub idle_seconds: u64,
    pub away_seconds: u64,
    pub phone_seconds: u64,
    pub drift_count: u64,
    pub nudge_count: u64,
    pub longest_stretch_seconds: u64,
    pub median_stretch_seconds: u64,
    pub spans: Vec<ReportSpan>,
    pub category_totals: Vec<CategoryTotal>,
    pub top_apps: Vec<String>,
    pub top_drifts: Vec<DriftEntry>,
    pub events: Vec<ReportEvent>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrendSession {
    pub id: u64,
    pub started_at: String,
    pub median_stretch_seconds: u64,
    pub focus_percent: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct DailyFocus {
    pub date: String,
    pub focus_percent: u64,
    pub focus_seconds: u64,
    pub session_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct WeeklyStretch {
    pub week_start: String,
    pub median_stretch_seconds: u64,
    pub session_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct FocusRecap {
    pub start: String,
    pub end: String,
    pub focus_seconds: u64,
    pub session_count: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct TrendsData {
    pub sessions: Vec<TrendSession>,
    pub daily_focus: Vec<DailyFocus>,
    pub weekly_stretch: Vec<WeeklyStretch>,
    pub week_recap: FocusRecap,
    pub month_recap: FocusRecap,
}

#[derive(Clone, Debug, Serialize)]
pub struct HistoryWeek {
    pub week_start: String,
    pub sessions: Vec<SessionSummary>,
    pub oldest_day: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecapBucket {
    pub label: String,
    pub focus_seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct RecapApp {
    pub group: String,
    pub label: String,
    pub seconds: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct PeriodRecap {
    pub kind: String,
    pub start: String,
    pub end: String,
    pub focus_seconds: u64,
    pub session_count: u64,
    pub buckets: Vec<RecapBucket>,
    pub median_stretch_seconds: u64,
    pub longest_stretch_seconds: u64,
    pub stretch_range_low: Option<u64>,
    pub stretch_range_high: Option<u64>,
    pub first_half_focus_percent: Option<u64>,
    pub second_half_focus_percent: Option<u64>,
    pub apps: Vec<RecapApp>,
}

struct RecapSession {
    id: u64,
    day: String,
    focused_seconds: u64,
    longest_stretch_seconds: u64,
    median_stretch_seconds: u64,
    duration_seconds: u64,
}

impl SessionStorage {
    pub fn open(path: &Path) -> Result<(Self, StoredState), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let conn = Connection::open(path).map_err(|error| error.to_string())?;
        let storage = Self::from_connection(conn)?;
        storage.recover_orphans()?;
        let state = StoredState {
            settings: storage.load_settings()?,
            rules: storage.load_rules()?,
            history: storage.list_history(HISTORY_CACHE_LIMIT)?,
        };
        Ok((storage, state))
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        Self::from_connection(Connection::open_in_memory().map_err(|error| error.to_string())?)
    }

    fn from_connection(conn: Connection) -> Result<Self, String> {
        let storage = Self {
            conn,
            failure: RefCell::new(None),
        };
        storage.apply_pragmas()?;
        storage.migrate()?;
        storage.seed_webcam_setting_keys()?;
        Ok(storage)
    }

    fn apply_pragmas(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                PRAGMA journal_mode = WAL;
                ",
            )
            .map_err(|error| error.to_string())?;
        let check: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if check != "ok" {
            return Err(format!("database is damaged: {check}"));
        }
        Ok(())
    }

    /// Remembers the first failed save or read so the UI can say history isn't saving.
    pub fn record_failure(&self, what: &str, error: String) {
        eprintln!("supervisor storage: {what}: {error}");
        self.failure
            .borrow_mut()
            .get_or_insert_with(|| format!("{what}: {error}"));
    }

    pub fn failure(&self) -> Option<String> {
        self.failure.borrow().clone()
    }

    fn migrate(&self) -> Result<(), String> {
        let version: i32 = self
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        if version > SCHEMA_VERSION {
            return Err(format!("unsupported database schema version {version}"));
        }
        // Each step is one transaction, so a crash mid-migration leaves the previous version intact.
        if version < 1 {
            self.conn
                .execute_batch(
                    "
                    BEGIN;
                    CREATE TABLE session (
                      id INTEGER PRIMARY KEY,
                      goal TEXT NOT NULL,
                      strictness TEXT NOT NULL,
                      started_at TEXT NOT NULL,
                      ended_at TEXT,
                      planned_seconds INTEGER NOT NULL,
                      ended_reason TEXT,
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
                      context TEXT,
                      category TEXT NOT NULL,
                      state TEXT NOT NULL,
                      started_offset_seconds INTEGER NOT NULL,
                      seconds INTEGER NOT NULL
                    );
                    CREATE INDEX span_session ON span(session_id, started_offset_seconds);

                    CREATE TABLE event (
                      id INTEGER PRIMARY KEY,
                      session_id INTEGER NOT NULL REFERENCES session(id) ON DELETE CASCADE,
                      at_offset_seconds INTEGER NOT NULL,
                      kind TEXT NOT NULL,
                      detail TEXT
                    );

                    CREATE TABLE setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);

                    CREATE TABLE rule (
                      id INTEGER PRIMARY KEY,
                      target TEXT NOT NULL,
                      value TEXT NOT NULL,
                      classification TEXT NOT NULL,
                      source TEXT NOT NULL
                    );

                    PRAGMA user_version = 1;
                    COMMIT;
                    ",
                )
                .map_err(|error| error.to_string())?;
        }
        if version < 2 {
            // Timer seconds at close (pauses excluded). NULL for sessions that never closed cleanly.
            self.conn
                .execute_batch(
                    "
                    BEGIN;
                    ALTER TABLE session ADD COLUMN elapsed_seconds INTEGER;
                    PRAGMA user_version = 2;
                    COMMIT;
                    ",
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    fn seed_webcam_setting_keys(&self) -> Result<(), String> {
        for key in WEBCAM_SETTING_KEYS {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO setting (key, value) VALUES (?1, ?2)",
                    params![key, ""],
                )
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub fn load_settings(&self) -> Result<SessionSettings, String> {
        let mut settings = SessionSettings::default();
        let mut stmt = self
            .conn
            .prepare("SELECT key, value FROM setting")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (key, value) = row.map_err(|error| error.to_string())?;
            match key.as_str() {
                "default_strictness" => {
                    settings.default_strictness = parse_strictness(&value);
                }
                "voice_profile" => {
                    if !value.trim().is_empty() {
                        settings.voice_profile = value;
                    }
                }
                "status_window_enabled" => {
                    settings.status_window_enabled = value == "true";
                }
                "status_window_on_top" => {
                    settings.status_window_on_top = value == "true";
                }
                "webcam_gpu" => {
                    settings.webcam_gpu = value == "true";
                }
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &SessionSettings) -> Result<(), String> {
        self.set_setting("default_strictness", strictness_key(settings.default_strictness))?;
        self.set_setting("voice_profile", &settings.voice_profile)?;
        self.set_setting(
            "status_window_enabled",
            if settings.status_window_enabled {
                "true"
            } else {
                "false"
            },
        )?;
        self.set_setting(
            "status_window_on_top",
            if settings.status_window_on_top {
                "true"
            } else {
                "false"
            },
        )?;
        self.set_setting(
            "webcam_gpu",
            if settings.webcam_gpu { "true" } else { "false" },
        )?;
        Ok(())
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO setting (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![key, value],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn load_phrase_library(&self) -> Result<StoredPhraseLibrary, String> {
        let raw = self.conn.query_row(
            "SELECT value FROM setting WHERE key = ?1",
            [PHRASE_LIBRARY_KEY],
            |row| row.get::<_, String>(0),
        );
        let library = match raw {
            Ok(value) => {
                serde_json::from_str::<StoredPhraseLibrary>(&value).map_err(|error| error.to_string())?
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => StoredPhraseLibrary::default(),
            Err(error) => return Err(error.to_string()),
        };
        Ok(clean_phrase_library(library))
    }

    pub fn save_phrase_library(
        &self,
        library: StoredPhraseLibrary,
    ) -> Result<StoredPhraseLibrary, String> {
        let library = clean_phrase_library(library);
        let value = serde_json::to_string(&library).map_err(|error| error.to_string())?;
        self.set_setting(PHRASE_LIBRARY_KEY, &value)?;
        Ok(library)
    }

    pub fn load_rules(&self) -> Result<Vec<AppRule>, String> {
        let stored = self.load_rules_with_source()?;
        if stored.is_empty() {
            let starter = starter_rules();
            self.replace_rules(&starter)?;
            return Ok(starter);
        }
        Ok(stored.into_iter().map(|entry| entry.rule).collect())
    }

    fn load_rules_with_source(&self) -> Result<Vec<StoredRule>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT target, value, classification, source FROM rule ORDER BY id ASC")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(StoredRule {
                    rule: AppRule {
                        target: parse_rule_target(row.get(0)?),
                        value: row.get(1)?,
                        classification: parse_classification(row.get(2)?),
                    },
                    source: row.get(3)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn replace_rules(&self, rules: &[AppRule]) -> Result<(), String> {
        let tx = self.conn.unchecked_transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM rule", [])
            .map_err(|error| error.to_string())?;
        for rule in rules {
            let source = rule_source_label(rule);
            tx.execute(
                "INSERT INTO rule (target, value, classification, source) VALUES (?1, ?2, ?3, ?4)",
                params![
                    rule_target_key(&rule.target),
                    rule.value,
                    classification_key(rule.classification),
                    source
                ],
            )
            .map_err(|error| error.to_string())?;
        }
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn insert_session(
        &self,
        goal: &str,
        strictness: Strictness,
        started_at: &str,
        planned_seconds: u64,
    ) -> Result<u64, String> {
        self.conn
            .execute(
                "INSERT INTO session (goal, strictness, started_at, planned_seconds)
                 VALUES (?1, ?2, ?3, ?4)",
                params![goal, strictness_key(strictness), started_at, planned_seconds],
            )
            .map_err(|error| error.to_string())?;
        Ok(self.conn.last_insert_rowid() as u64)
    }

    pub fn close_session(
        &self,
        id: u64,
        ended_at: &str,
        ended_reason: &str,
        elapsed_seconds: u64,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE session SET ended_at = ?1, ended_reason = ?2, elapsed_seconds = ?3
                 WHERE id = ?4",
                params![ended_at, ended_reason, elapsed_seconds, id],
            )
            .map_err(|error| error.to_string())?;
        self.recompute_session_metrics(id)?;
        Ok(())
    }

    pub fn insert_span(
        &self,
        session_id: u64,
        app: &str,
        context: Option<&str>,
        category: &str,
        state: &str,
        started_offset_seconds: u64,
        seconds: u64,
    ) -> Result<(), String> {
        if seconds == 0 {
            return Ok(());
        }
        self.conn
            .execute(
                "INSERT INTO span (session_id, app, context, category, state, started_offset_seconds, seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    session_id,
                    app,
                    context,
                    category,
                    state,
                    started_offset_seconds,
                    seconds
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn insert_event(
        &self,
        session_id: u64,
        at_offset_seconds: u64,
        kind: &str,
        detail: Option<&str>,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO event (session_id, at_offset_seconds, kind, detail)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id, at_offset_seconds, kind, detail],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn recover_orphans(&self) -> Result<(), String> {
        let ids: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM session WHERE ended_at IS NULL")
                .map_err(|error| error.to_string())?;
            let rows = stmt
                .query_map([], |row| row.get(0))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|error| error.to_string())?
        };
        for id in ids {
            let ended_at = utc_now();
            self.conn
                .execute(
                    "UPDATE session SET ended_at = ?1, ended_reason = 'abandoned' WHERE id = ?2",
                    params![ended_at, id],
                )
                .map_err(|error| error.to_string())?;
            self.recompute_session_metrics(id as u64)?;
        }
        Ok(())
    }

    pub fn recompute_session_metrics(&self, session_id: u64) -> Result<(), String> {
        let spans = self.load_spans_for_session(session_id)?;
        let events = self.load_events_for_session(session_id)?;

        let mut focused_seconds = 0u64;
        let mut distracted_seconds = 0u64;
        let mut idle_seconds = 0u64;

        for span in &spans {
            match span.state.as_str() {
                "focused" | "neutral" => focused_seconds += span.seconds,
                "suspicious" | "distracted" => distracted_seconds += span.seconds,
                "idle" => idle_seconds += span.seconds,
                _ => {}
            }
        }

        let nudge_count = events
            .iter()
            .filter(|event| event.kind == "nudge")
            .count() as u64;

        let drift_count = count_drifts(&spans);
        let stretches = focused_stretches(&spans);
        let longest_stretch_seconds = stretches.iter().copied().max().unwrap_or(0);
        let median_stretch_seconds = median(&stretches);

        self.conn
            .execute(
                "UPDATE session SET
                    focused_seconds = ?1,
                    distracted_seconds = ?2,
                    idle_seconds = ?3,
                    drift_count = ?4,
                    nudge_count = ?5,
                    longest_stretch_seconds = ?6,
                    median_stretch_seconds = ?7
                 WHERE id = ?8",
                params![
                    focused_seconds,
                    distracted_seconds,
                    idle_seconds,
                    drift_count,
                    nudge_count,
                    longest_stretch_seconds,
                    median_stretch_seconds,
                    session_id
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn list_history(&self, limit: usize) -> Result<Vec<SessionSummary>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, goal, started_at, focused_seconds, distracted_seconds,
                        idle_seconds, nudge_count
                 FROM session
                 WHERE ended_at IS NOT NULL
                 ORDER BY started_at DESC
                 LIMIT ?1",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64], |row| {
                let id: i64 = row.get(0)?;
                let goal: String = row.get(1)?;
                let started_at: String = row.get(2)?;
                let focused_seconds: i64 = row.get(3)?;
                let distracted_seconds: i64 = row.get(4)?;
                let idle_seconds: i64 = row.get(5)?;
                let nudges: i64 = row.get(6)?;
                Ok((id, goal, started_at, focused_seconds, distracted_seconds, idle_seconds, nudges))
            })
            .map_err(|error| error.to_string())?;

        let mut summaries = Vec::new();
        for row in rows {
            let (id, goal, started_at, focused_seconds, distracted_seconds, idle_seconds, nudges) =
                row.map_err(|error| error.to_string())?;
            summaries.push(self.summary_from_row(
                id,
                goal,
                started_at,
                focused_seconds,
                distracted_seconds,
                idle_seconds,
                nudges,
            )?);
        }
        Ok(summaries)
    }

    fn session_duration_seconds(&self, session_id: u64) -> Result<u64, String> {
        let seconds: i64 = self
            .conn
            .query_row(
                &format!("SELECT {RUN_SECONDS_SQL} FROM session WHERE id = ?1"),
                [session_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        Ok(seconds as u64)
    }

    fn top_hints_for_session(&self, session_id: u64) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT app, context, SUM(seconds) AS total
                 FROM span
                 WHERE session_id = ?1
                 GROUP BY app, context
                 ORDER BY total DESC, app ASC
                 LIMIT 3",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([session_id], |row| {
                let app: String = row.get(0)?;
                let context: Option<String> = row.get(1)?;
                let total: i64 = row.get(2)?;
                Ok((hint_label(&app, context.as_deref()), total))
            })
            .map_err(|error| error.to_string())?;
        let mut hints = Vec::new();
        for row in rows {
            let (hint, _) = row.map_err(|error| error.to_string())?;
            hints.push(hint);
        }
        Ok(hints)
    }

    pub fn get_session_report(&self, id: u64) -> Result<SessionReport, String> {
        let row = self
            .conn
            .query_row(
                "SELECT goal, strictness, started_at, ended_at, ended_reason, planned_seconds,
                        focused_seconds, distracted_seconds, idle_seconds, away_seconds, phone_seconds,
                        drift_count, nudge_count, longest_stretch_seconds, median_stretch_seconds
                 FROM session WHERE id = ?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, i64>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, i64>(8)?,
                        row.get::<_, i64>(9)?,
                        row.get::<_, i64>(10)?,
                        row.get::<_, i64>(11)?,
                        row.get::<_, i64>(12)?,
                        row.get::<_, i64>(13)?,
                        row.get::<_, i64>(14)?,
                    ))
                },
            )
            .map_err(|error| error.to_string())?;

        let (
            goal,
            strictness,
            started_at,
            ended_at,
            ended_reason,
            planned_seconds,
            focused_seconds,
            distracted_seconds,
            idle_seconds,
            away_seconds,
            phone_seconds,
            drift_count,
            nudge_count,
            longest_stretch_seconds,
            median_stretch_seconds,
        ) = row;

        let duration_seconds = self.session_duration_seconds(id)?;
        let focus_percent = if duration_seconds == 0 {
            0
        } else {
            (focused_seconds as u64 * 100) / duration_seconds
        };

        let spans = self.load_spans_for_session(id)?;
        let category_totals = category_totals_from_spans(&spans);
        let top_apps = top_apps_from_spans(&spans, 3);
        let top_drifts = top_drifts_from_spans(&spans, 5);
        let events = self.load_events_for_session(id)?;

        Ok(SessionReport {
            id,
            goal,
            strictness,
            started_at,
            ended_at,
            ended_reason,
            planned_seconds: planned_seconds as u64,
            duration_seconds,
            focus_percent,
            focused_seconds: focused_seconds as u64,
            distracted_seconds: distracted_seconds as u64,
            idle_seconds: idle_seconds as u64,
            away_seconds: away_seconds as u64,
            phone_seconds: phone_seconds as u64,
            drift_count: drift_count as u64,
            nudge_count: nudge_count as u64,
            longest_stretch_seconds: longest_stretch_seconds as u64,
            median_stretch_seconds: median_stretch_seconds as u64,
            spans,
            category_totals,
            top_apps,
            top_drifts,
            events,
        })
    }

    pub fn get_trends(&self) -> Result<TrendsData, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, median_stretch_seconds, focused_seconds
                 FROM session
                 WHERE ended_at IS NOT NULL
                 ORDER BY started_at DESC
                 LIMIT 14",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, started_at, median_stretch_seconds, focused_seconds) =
                row.map_err(|error| error.to_string())?;
            let duration = self.session_duration_seconds(id as u64)?;
            let focus_percent = if duration == 0 {
                0
            } else {
                (focused_seconds as u64 * 100) / duration
            };
            sessions.push(TrendSession {
                id: id as u64,
                started_at,
                median_stretch_seconds: median_stretch_seconds as u64,
                focus_percent,
            });
        }

        let mut daily_stmt = self
            .conn
            .prepare(&format!(
                "SELECT started_at, focused_seconds, {RUN_SECONDS_SQL}
                 FROM session
                 WHERE ended_at IS NOT NULL"
            ))
            .map_err(|error| error.to_string())?;
        let daily_rows = daily_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut by_day = std::collections::HashMap::<String, (u64, u64, u64)>::new();
        for row in daily_rows {
            let (started_at, focus_seconds, run_seconds) = row.map_err(|error| error.to_string())?;
            let entry = by_day.entry(local_day(&started_at)).or_insert((0, 0, 0));
            entry.0 += focus_seconds as u64;
            entry.1 += run_seconds.max(0) as u64;
            entry.2 += 1;
        }
        let mut days: Vec<_> = by_day.into_iter().collect();
        days.sort_by(|left, right| left.0.cmp(&right.0));
        if days.len() > 14 {
            days = days.split_off(days.len() - 14);
        }
        let daily_focus = days
            .into_iter()
            .map(|(date, (focus_seconds, run_seconds, session_count))| {
                let focus_percent = if run_seconds == 0 {
                    0
                } else {
                    (focus_seconds * 100) / run_seconds
                };
                DailyFocus {
                    date,
                    focus_percent,
                    focus_seconds,
                    session_count,
                }
            })
            .collect();

        let monday = this_monday();
        let today = local_today();
        Ok(TrendsData {
            sessions,
            daily_focus,
            weekly_stretch: self.weekly_stretch(monday)?,
            week_recap: self.focus_recap(
                monday,
                monday.saturating_add(time::Duration::days(7)),
            )?,
            month_recap: self.focus_recap(first_of_month(today), next_month_start(today))?,
        })
    }

    pub fn get_history_week(&self, week_start: &str) -> Result<HistoryWeek, String> {
        let start = monday_on_or_before(parse_day(week_start)?);
        let end = start.saturating_add(time::Duration::days(7));
        Ok(HistoryWeek {
            week_start: format_day(start),
            sessions: self.list_history_between(&format_day(start), &format_day(end))?,
            oldest_day: self.oldest_session_day()?,
        })
    }

    pub fn get_period_recap(&self, period: &str, start: &str) -> Result<PeriodRecap, String> {
        let day = parse_day(start)?;
        match period {
            "week" => {
                let monday = monday_on_or_before(day);
                self.build_period_recap(
                    "week",
                    monday,
                    monday.saturating_add(time::Duration::days(7)),
                )
            }
            "month" => {
                let month_start = first_of_month(day);
                self.build_period_recap("month", month_start, next_month_start(month_start))
            }
            _ => Err("period must be week or month".into()),
        }
    }

    fn build_period_recap(
        &self,
        kind: &str,
        start: time::Date,
        end_exclusive: time::Date,
    ) -> Result<PeriodRecap, String> {
        let sessions = self.recap_sessions(&format_day(start), &format_day(end_exclusive))?;
        let mut by_day = std::collections::HashMap::<String, u64>::new();
        let mut stretch_samples = Vec::new();
        let mut longest_stretch_seconds = 0u64;
        let mut focus_seconds = 0u64;
        let mut first_focus = 0u64;
        let mut first_total = 0u64;
        let mut second_focus = 0u64;
        let mut second_total = 0u64;
        let mut app_totals = std::collections::HashMap::<(String, String), u64>::new();
        let rules = self.load_rules().unwrap_or_default();

        for session in &sessions {
            focus_seconds += session.focused_seconds;
            longest_stretch_seconds = longest_stretch_seconds.max(session.longest_stretch_seconds);
            if session.median_stretch_seconds > 0 {
                stretch_samples.push(session.median_stretch_seconds);
            }
            *by_day.entry(session.day.clone()).or_insert(0) += session.focused_seconds;
            let spans = self.load_spans_for_session(session.id)?;
            let halves = half_focus(&spans, session.duration_seconds);
            first_focus += halves.0;
            first_total += halves.1;
            second_focus += halves.2;
            second_total += halves.3;
            for span in spans {
                let group = recap_app_group(&rules, &span.app, span.context.as_deref());
                let label = recap_activity_label(&span.app, span.context.as_deref());
                *app_totals
                    .entry((group.to_string(), label))
                    .or_insert(0) += span.seconds;
            }
        }

        const GROUP_ORDER: [&str; 4] = ["Focus", "Neutral", "Distracting", "Miscellaneous"];
        let mut apps: Vec<RecapApp> = app_totals
            .into_iter()
            .map(|((group, label), seconds)| RecapApp {
                group,
                label,
                seconds,
            })
            .collect();
        apps.sort_by(|a, b| {
            let left = GROUP_ORDER.iter().position(|group| *group == a.group).unwrap_or(9);
            let right = GROUP_ORDER.iter().position(|group| *group == b.group).unwrap_or(9);
            left.cmp(&right)
                .then_with(|| b.seconds.cmp(&a.seconds))
                .then_with(|| a.label.cmp(&b.label))
        });

        let (stretch_range_low, stretch_range_high) = if kind == "month" {
            full_week_stretch_range(start, end_exclusive, &sessions)
        } else {
            (None, None)
        };

        Ok(PeriodRecap {
            kind: kind.to_string(),
            start: format_day(start),
            end: format_day(end_exclusive.saturating_sub(time::Duration::days(1))),
            focus_seconds,
            session_count: sessions.len() as u64,
            buckets: recap_buckets(kind, start, end_exclusive, &by_day),
            median_stretch_seconds: median(&stretch_samples),
            longest_stretch_seconds,
            stretch_range_low,
            stretch_range_high,
            first_half_focus_percent: percent(first_focus, first_total),
            second_half_focus_percent: percent(second_focus, second_total),
            apps,
        })
    }

    fn recap_sessions(
        &self,
        start_day: &str,
        end_day: &str,
    ) -> Result<Vec<RecapSession>, String> {
        let (from, to) = padded_day_bounds(start_day, end_day);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, started_at, focused_seconds, longest_stretch_seconds, median_stretch_seconds
                 FROM session
                 WHERE ended_at IS NOT NULL
                   AND substr(started_at, 1, 10) >= ?1
                   AND substr(started_at, 1, 10) < ?2
                 ORDER BY started_at ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![from, to], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, started_at, focused_seconds, longest, median_stretch) =
                row.map_err(|error| error.to_string())?;
            let Some(day) = local_day_in(&started_at, start_day, end_day) else {
                continue;
            };
            let duration_seconds = self.session_duration_seconds(id as u64)?;
            sessions.push(RecapSession {
                id: id as u64,
                day,
                focused_seconds: focused_seconds as u64,
                longest_stretch_seconds: longest as u64,
                median_stretch_seconds: median_stretch as u64,
                duration_seconds,
            });
        }
        Ok(sessions)
    }

    fn list_history_between(&self, start_day: &str, end_day: &str) -> Result<Vec<SessionSummary>, String> {
        let (from, to) = padded_day_bounds(start_day, end_day);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, goal, started_at, focused_seconds, distracted_seconds,
                        idle_seconds, nudge_count
                 FROM session
                 WHERE ended_at IS NOT NULL
                   AND substr(started_at, 1, 10) >= ?1
                   AND substr(started_at, 1, 10) < ?2
                 ORDER BY started_at DESC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![from, to], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut summaries = Vec::new();
        for row in rows {
            let (id, goal, started_at, focused_seconds, distracted_seconds, idle_seconds, nudges) =
                row.map_err(|error| error.to_string())?;
            if local_day_in(&started_at, start_day, end_day).is_none() {
                continue;
            }
            summaries.push(self.summary_from_row(
                id,
                goal,
                started_at,
                focused_seconds,
                distracted_seconds,
                idle_seconds,
                nudges,
            )?);
        }
        Ok(summaries)
    }

    fn summary_from_row(
        &self,
        id: i64,
        goal: String,
        started_at: String,
        focused_seconds: i64,
        distracted_seconds: i64,
        idle_seconds: i64,
        nudges: i64,
    ) -> Result<SessionSummary, String> {
        let top_hints = self.top_hints_for_session(id as u64)?;
        let duration_seconds = self.session_duration_seconds(id as u64)?;
        Ok(SessionSummary {
            id: id as u64,
            goal,
            started_at,
            duration_seconds,
            focus_seconds: focused_seconds as u64,
            distracted_seconds: distracted_seconds as u64,
            idle_seconds: idle_seconds as u64,
            nudges: nudges as u64,
            top_hints,
        })
    }

    fn oldest_session_day(&self) -> Result<Option<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT started_at FROM session WHERE ended_at IS NOT NULL")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| error.to_string())?;
        let mut oldest: Option<String> = None;
        for row in rows {
            let day = local_day(&row.map_err(|error| error.to_string())?);
            if oldest.as_ref().is_none_or(|current| day < *current) {
                oldest = Some(day);
            }
        }
        Ok(oldest)
    }

    fn weekly_stretch(&self, monday: time::Date) -> Result<Vec<WeeklyStretch>, String> {
        let oldest = monday.saturating_sub(time::Duration::days(
            ((STRETCH_WEEK_COUNT - 1) * 7) as i64,
        ));
        let range_start = format_day(oldest);
        let range_end = format_day(monday.saturating_add(time::Duration::days(7)));
        let (from, to) = padded_day_bounds(&range_start, &range_end);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT started_at, median_stretch_seconds
                 FROM session
                 WHERE ended_at IS NOT NULL
                   AND substr(started_at, 1, 10) >= ?1
                   AND substr(started_at, 1, 10) < ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([from, to], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?;

        let mut buckets = vec![Vec::<u64>::new(); STRETCH_WEEK_COUNT];
        for row in rows {
            let (started_at, stretch) = row.map_err(|error| error.to_string())?;
            if stretch <= 0 {
                continue;
            }
            let Some(day) = local_day_in(&started_at, &range_start, &range_end) else {
                continue;
            };
            let date = parse_day(&day)?;
            let days_from_oldest = (date - oldest).whole_days();
            if days_from_oldest < 0 {
                continue;
            }
            let index = (days_from_oldest / 7) as usize;
            if let Some(bucket) = buckets.get_mut(index) {
                bucket.push(stretch as u64);
            }
        }

        Ok((0..STRETCH_WEEK_COUNT)
            .map(|index| {
                let week_start = oldest.saturating_add(time::Duration::days(index as i64 * 7));
                WeeklyStretch {
                    week_start: format_day(week_start),
                    median_stretch_seconds: median(&buckets[index]),
                    session_count: buckets[index].len() as u64,
                }
            })
            .collect())
    }

    fn focus_recap(
        &self,
        start: time::Date,
        end_exclusive: time::Date,
    ) -> Result<FocusRecap, String> {
        let (focus_seconds, session_count) =
            self.sum_focus(&format_day(start), &format_day(end_exclusive))?;
        Ok(FocusRecap {
            start: format_day(start),
            end: format_day(end_exclusive.saturating_sub(time::Duration::days(1))),
            focus_seconds,
            session_count,
        })
    }

    fn sum_focus(&self, start_day: &str, end_day_exclusive: &str) -> Result<(u64, u64), String> {
        let (from, to) = padded_day_bounds(start_day, end_day_exclusive);
        let mut stmt = self
            .conn
            .prepare(
                "SELECT started_at, focused_seconds
                 FROM session
                 WHERE ended_at IS NOT NULL
                   AND substr(started_at, 1, 10) >= ?1
                   AND substr(started_at, 1, 10) < ?2",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![from, to], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|error| error.to_string())?;
        let mut seconds = 0u64;
        let mut count = 0u64;
        for row in rows {
            let (started_at, focused_seconds) = row.map_err(|error| error.to_string())?;
            if local_day_in(&started_at, start_day, end_day_exclusive).is_none() {
                continue;
            }
            seconds += focused_seconds as u64;
            count += 1;
        }
        Ok((seconds, count))
    }

    pub fn delete_session(&self, id: u64) -> Result<(), String> {
        let ended_at = match self.conn.query_row(
            "SELECT ended_at FROM session WHERE id = ?1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(ended_at) => Some(ended_at),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(error.to_string()),
        };
        match ended_at {
            Some(None) => Err("Stop the session before deleting it.".to_string()),
            Some(Some(_)) => {
                self.conn
                    .execute("DELETE FROM session WHERE id = ?1", [id])
                    .map_err(|error| error.to_string())?;
                Ok(())
            }
            None => Ok(()),
        }
    }

    pub fn delete_all_history(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM session WHERE ended_at IS NOT NULL", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn scrub_span_context(&self) -> Result<(), String> {
        self.conn
            .execute("UPDATE span SET context = NULL", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    fn load_spans_for_session(&self, session_id: u64) -> Result<Vec<ReportSpan>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT started_offset_seconds, seconds, app, context, category, state
                 FROM span
                 WHERE session_id = ?1
                 ORDER BY started_offset_seconds ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([session_id], |row| {
                Ok(ReportSpan {
                    started_offset_seconds: row.get::<_, i64>(0)? as u64,
                    seconds: row.get::<_, i64>(1)? as u64,
                    app: row.get(2)?,
                    context: row.get(3)?,
                    category: row.get(4)?,
                    state: row.get(5)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    fn load_events_for_session(&self, session_id: u64) -> Result<Vec<ReportEvent>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT at_offset_seconds, kind, detail
                 FROM event
                 WHERE session_id = ?1
                 ORDER BY at_offset_seconds ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([session_id], |row| {
                Ok(ReportEvent {
                    at_offset_seconds: row.get::<_, i64>(0)? as u64,
                    kind: row.get(1)?,
                    detail: row.get(2)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }
}

pub fn span_category(
    app: &str,
    context: Option<&str>,
    state: &str,
    classification: RuleClassification,
) -> &'static str {
    if state == "idle" {
        return "offscreen";
    }

    let app_lower = app.to_lowercase();
    let context_lower = context.map(|value| value.to_lowercase()).unwrap_or_default();
    let combined = format!("{app_lower} {context_lower}");

    if is_media_signal(&combined) {
        return "media";
    }
    if is_comms_app(&app_lower) {
        return "comms";
    }
    if state == "focused" || classification == RuleClassification::Focused {
        return "work";
    }
    if state == "distracted" || classification == RuleClassification::Distracting {
        return if is_media_signal(&combined) {
            "media"
        } else {
            "browse"
        };
    }
    if is_work_app(&app_lower) {
        return "work";
    }
    if is_browser_app(&app_lower) {
        return "browse";
    }
    "browse"
}

pub fn focus_state_key(state: crate::focus::FocusState) -> &'static str {
    use crate::focus::FocusState;
    match state {
        FocusState::Focused => "focused",
        FocusState::Neutral => "neutral",
        FocusState::Suspicious => "suspicious",
        FocusState::Distracted => "distracted",
        FocusState::Idle => "idle",
    }
}

fn is_media_signal(combined: &str) -> bool {
    const MARKERS: &[&str] = &[
        "youtube",
        "netflix",
        "tiktok",
        "instagram",
        "reddit",
        "spotify",
        "twitch",
        "hulu",
        "disney+",
        "prime video",
    ];
    MARKERS.iter().any(|marker| combined.contains(marker))
}

fn is_comms_app(app_lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "slack",
        "discord",
        "teams",
        "zoom",
        "outlook",
        "mail",
        "messages",
        "telegram",
        "whatsapp",
    ];
    MARKERS.iter().any(|marker| app_lower.contains(marker))
}

fn is_work_app(app_lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "code",
        "cursor",
        "visual studio",
        "devenv",
        "xcode",
        "intellij",
        "webstorm",
        "pycharm",
        "terminal",
        "iterm",
        "windows terminal",
        "notion",
        "obsidian",
        "figma",
    ];
    MARKERS.iter().any(|marker| app_lower.contains(marker))
}

fn is_browser_app(app_lower: &str) -> bool {
    const MARKERS: &[&str] = &[
        "chrome",
        "firefox",
        "edge",
        "brave",
        "safari",
        "opera",
        "arc",
    ];
    MARKERS.iter().any(|marker| app_lower.contains(marker))
}

fn hint_label(app: &str, context: Option<&str>) -> String {
    context
        .filter(|title| !title.trim().is_empty())
        .map(|title| title.to_string())
        .unwrap_or_else(|| app.to_string())
}

fn recap_activity_label(app: &str, context: Option<&str>) -> String {
    if is_browser_app(&app.to_lowercase()) {
        context
            .map(str::trim)
            .filter(|title| !title.is_empty())
            .map(|title| title.to_string())
            .unwrap_or_else(|| "Browser · site unknown".to_string())
    } else {
        app.to_string()
    }
}

fn recap_app_group(rules: &[AppRule], app: &str, context: Option<&str>) -> &'static str {
    match rules
        .iter()
        .find(|rule| rule.matches_app(app, context))
        .map(|rule| rule.classification)
    {
        Some(RuleClassification::Focused) => "Focus",
        Some(RuleClassification::Neutral) => "Neutral",
        Some(RuleClassification::Distracting) => "Distracting",
        None => "Miscellaneous",
    }
}

fn percent(part: u64, total: u64) -> Option<u64> {
    if total == 0 {
        None
    } else {
        Some((part * 100) / total)
    }
}

fn overlap(start: u64, end: u64, window_start: u64, window_end: u64) -> u64 {
    end.min(window_end).saturating_sub(start.max(window_start))
}

fn half_focus(spans: &[ReportSpan], duration: u64) -> (u64, u64, u64, u64) {
    if duration == 0 {
        return (0, 0, 0, 0);
    }
    let mid = duration / 2;
    let mut first_focus = 0u64;
    let mut first_total = 0u64;
    let mut second_focus = 0u64;
    let mut second_total = 0u64;
    for span in spans {
        let start = span.started_offset_seconds;
        let end = start + span.seconds;
        let first = overlap(start, end, 0, mid);
        let second = overlap(start, end, mid, duration);
        first_total += first;
        second_total += second;
        if matches!(span.state.as_str(), "focused" | "neutral") {
            first_focus += first;
            second_focus += second;
        }
    }
    (first_focus, first_total, second_focus, second_total)
}

fn recap_buckets(
    kind: &str,
    start: time::Date,
    end_exclusive: time::Date,
    by_day: &std::collections::HashMap<String, u64>,
) -> Vec<RecapBucket> {
    if kind == "week" {
        const LETTERS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];
        return (0i64..7)
            .map(|index| {
                let day = start.saturating_add(time::Duration::days(index));
                RecapBucket {
                    label: LETTERS[index as usize].to_string(),
                    focus_seconds: *by_day.get(&format_day(day)).unwrap_or(&0),
                }
            })
            .collect();
    }

    let last_day = end_exclusive
        .saturating_sub(time::Duration::days(1))
        .day();
    let ranges = [
        (1u8, 7u8),
        (8, 14),
        (15, 21),
        (22, 28),
        (29, last_day.max(29)),
    ];
    ranges
        .into_iter()
        .filter(|(from, _)| *from <= last_day)
        .map(|(from, to)| {
            let to = to.min(last_day);
            let mut focus_seconds = 0u64;
            let mut cursor = start.replace_day(from).unwrap_or(start);
            let stop = start.replace_day(to).unwrap_or(start);
            loop {
                focus_seconds += by_day.get(&format_day(cursor)).copied().unwrap_or(0);
                if cursor >= stop {
                    break;
                }
                cursor = cursor.saturating_add(time::Duration::days(1));
            }
            RecapBucket {
                label: format!("{from}–{to}"),
                focus_seconds,
            }
        })
        .collect()
}

fn full_week_stretch_range(
    start: time::Date,
    end_exclusive: time::Date,
    sessions: &[RecapSession],
) -> (Option<u64>, Option<u64>) {
    let mut cursor = start;
    while cursor < end_exclusive && cursor.weekday() != time::Weekday::Monday {
        cursor = cursor.saturating_add(time::Duration::days(1));
    }
    let mut week_medians = Vec::new();
    while cursor.saturating_add(time::Duration::days(7)) <= end_exclusive {
        let week_end = format_day(cursor.saturating_add(time::Duration::days(7)));
        let week_start = format_day(cursor);
        let samples: Vec<u64> = sessions
            .iter()
            .filter(|session| session.day >= week_start && session.day < week_end)
            .map(|session| session.median_stretch_seconds)
            .filter(|value| *value > 0)
            .collect();
        if !samples.is_empty() {
            week_medians.push(median(&samples));
        }
        cursor = cursor.saturating_add(time::Duration::days(7));
    }
    let low = week_medians.iter().copied().min();
    let high = week_medians.iter().copied().max();
    if low == high {
        (None, None)
    } else {
        (low, high)
    }
}

fn monday_on_or_before(date: time::Date) -> time::Date {
    date.saturating_sub(time::Duration::days(i64::from(
        date.weekday().number_days_from_monday(),
    )))
}

fn count_drifts(spans: &[ReportSpan]) -> u64 {
    let mut count = 0u64;
    let mut in_focus_track = false;
    for span in spans {
        let focus_like = matches!(span.state.as_str(), "focused" | "neutral");
        let drift_like = matches!(span.state.as_str(), "distracted" | "idle" | "suspicious");
        if focus_like {
            in_focus_track = true;
        } else if drift_like && in_focus_track {
            count += 1;
            in_focus_track = false;
        }
    }
    count
}

fn focused_stretches(spans: &[ReportSpan]) -> Vec<u64> {
    let mut stretches = Vec::new();
    let mut current = 0u64;
    for span in spans {
        match span.state.as_str() {
            "focused" | "neutral" => current += span.seconds,
            "distracted" | "idle" | "suspicious" => {
                if current > 0 {
                    stretches.push(current);
                    current = 0;
                }
            }
            _ => {}
        }
    }
    if current > 0 {
        stretches.push(current);
    }
    stretches
}

fn parse_day(value: &str) -> Result<time::Date, String> {
    if value.len() != 10 || value.as_bytes().get(4) != Some(&b'-') || value.as_bytes().get(7) != Some(&b'-') {
        return Err("date must be YYYY-MM-DD".into());
    }
    let year: i32 = value[0..4]
        .parse()
        .map_err(|_| "date must be YYYY-MM-DD".to_string())?;
    let month: u8 = value[5..7]
        .parse()
        .map_err(|_| "date must be YYYY-MM-DD".to_string())?;
    let day: u8 = value[8..10]
        .parse()
        .map_err(|_| "date must be YYYY-MM-DD".to_string())?;
    let month = time::Month::try_from(month).map_err(|error| error.to_string())?;
    time::Date::from_calendar_date(year, month, day).map_err(|error| error.to_string())
}

fn format_day(date: time::Date) -> String {
    format!("{:04}-{:02}-{:02}", date.year(), u8::from(date.month()), date.day())
}

fn local_today() -> time::Date {
    time::OffsetDateTime::now_local()
        .map(|now| now.date())
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc().date())
}

fn this_monday() -> time::Date {
    monday_on_or_before(local_today())
}

fn calendar_day(instant: time::OffsetDateTime, offset: time::UtcOffset) -> String {
    format_day(instant.to_offset(offset).date())
}

fn local_day(started_at: &str) -> String {
    let prefix = started_at.get(..10).unwrap_or(started_at).to_string();
    let Ok(instant) = time::OffsetDateTime::parse(
        started_at,
        &time::format_description::well_known::Rfc3339,
    ) else {
        return prefix;
    };
    let offset = time::UtcOffset::local_offset_at(instant).unwrap_or(time::UtcOffset::UTC);
    calendar_day(instant, offset)
}

fn local_day_in(started_at: &str, start: &str, end_exclusive: &str) -> Option<String> {
    let day = local_day(started_at);
    if day.as_str() >= start && day.as_str() < end_exclusive {
        Some(day)
    } else {
        None
    }
}

// ponytail: ±1 UTC day covers every real zone. Widen this if a zone ever exceeds 24 hours.
fn padded_day_bounds(start: &str, end_exclusive: &str) -> (String, String) {
    let from = parse_day(start)
        .map(|day| format_day(day.saturating_sub(time::Duration::days(1))))
        .unwrap_or_else(|_| start.to_string());
    let to = parse_day(end_exclusive)
        .map(|day| format_day(day.saturating_add(time::Duration::days(1))))
        .unwrap_or_else(|_| end_exclusive.to_string());
    (from, to)
}

fn first_of_month(date: time::Date) -> time::Date {
    date.replace_day(1).expect("month has a first day")
}

fn next_month_start(date: time::Date) -> time::Date {
    let start = first_of_month(date);
    if start.month() == time::Month::December {
        time::Date::from_calendar_date(start.year() + 1, time::Month::January, 1)
            .expect("january 1")
    } else {
        let month = time::Month::try_from(u8::from(start.month()) + 1).expect("next month");
        time::Date::from_calendar_date(start.year(), month, 1).expect("month start")
    }
}

fn median(values: &[u64]) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 0 {
        (sorted[mid - 1] + sorted[mid]) / 2
    } else {
        sorted[mid]
    }
}

fn category_totals_from_spans(spans: &[ReportSpan]) -> Vec<CategoryTotal> {
    let mut totals = std::collections::HashMap::new();
    for span in spans {
        *totals.entry(span.category.clone()).or_insert(0u64) += span.seconds;
    }
    let mut rows: Vec<CategoryTotal> = totals
        .into_iter()
        .map(|(category, seconds)| CategoryTotal { category, seconds })
        .collect();
    rows.sort_by(|a, b| b.seconds.cmp(&a.seconds).then_with(|| a.category.cmp(&b.category)));
    rows
}

fn top_apps_from_spans(spans: &[ReportSpan], limit: usize) -> Vec<String> {
    let mut totals = std::collections::HashMap::new();
    for span in spans {
        *totals.entry(span.app.clone()).or_insert(0u64) += span.seconds;
    }
    let mut pairs: Vec<_> = totals.into_iter().collect();
    pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    pairs.into_iter().take(limit).map(|(app, _)| app).collect()
}

fn top_drifts_from_spans(spans: &[ReportSpan], limit: usize) -> Vec<DriftEntry> {
    let mut drifts = Vec::new();
    for span in spans {
        if matches!(span.state.as_str(), "distracted" | "suspicious") {
            drifts.push(DriftEntry {
                at_offset_seconds: span.started_offset_seconds,
                label: hint_label(&span.app, span.context.as_deref()),
                seconds: span.seconds,
            });
        }
    }
    drifts.sort_by(|a, b| b.seconds.cmp(&a.seconds));
    drifts.truncate(limit);
    drifts
}

fn strictness_key(strictness: Strictness) -> &'static str {
    match strictness {
        Strictness::Gentle => "gentle",
        Strictness::Normal => "normal",
        Strictness::Strict => "strict",
    }
}

fn parse_strictness(value: &str) -> Strictness {
    match value {
        "gentle" => Strictness::Gentle,
        "strict" => Strictness::Strict,
        _ => Strictness::Normal,
    }
}

fn rule_target_key(target: &RuleTarget) -> &'static str {
    match target {
        RuleTarget::AppName => "app_name",
        RuleTarget::ProcessPath => "process_path",
        RuleTarget::WindowTitle => "window_title",
    }
}

fn parse_rule_target(value: String) -> RuleTarget {
    match value.as_str() {
        "app_name" => RuleTarget::AppName,
        "process_path" => RuleTarget::ProcessPath,
        _ => RuleTarget::WindowTitle,
    }
}

fn classification_key(classification: RuleClassification) -> &'static str {
    match classification {
        RuleClassification::Focused => "focused",
        RuleClassification::Neutral => "neutral",
        RuleClassification::Distracting => "distracting",
    }
}

fn parse_classification(value: String) -> RuleClassification {
    match value.as_str() {
        "focused" => RuleClassification::Focused,
        "distracting" => RuleClassification::Distracting,
        _ => RuleClassification::Neutral,
    }
}

fn rule_source_label(rule: &AppRule) -> &'static str {
    let starter = starter_rules();
    if starter.iter().any(|starter_rule| starter_rule == rule) {
        "starter"
    } else {
        "user"
    }
}

fn clean_block_suggestions(suggestions: &[String]) -> Vec<String> {
    let mut cleaned = Vec::new();
    for suggestion in suggestions {
        let value: String = suggestion.trim().chars().take(160).collect();
        if !value.is_empty()
            && !cleaned
                .iter()
                .any(|item: &String| item.eq_ignore_ascii_case(&value))
        {
            cleaned.push(value);
        }
        if cleaned.len() == 100 {
            break;
        }
    }
    cleaned
}

fn clean_phrase_library(library: StoredPhraseLibrary) -> StoredPhraseLibrary {
    StoredPhraseLibrary {
        custom_tones: clean_block_suggestions(&library.custom_tones),
        custom_phrases: clean_stored_phrases(library.custom_phrases),
        phrase_overrides: clean_stored_phrases(library.phrase_overrides),
    }
}

fn clean_stored_phrases(phrases: Vec<StoredPhrase>) -> Vec<StoredPhrase> {
    let mut cleaned = Vec::new();
    for phrase in phrases {
        let id: String = phrase
            .id
            .trim()
            .chars()
            .filter(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
            .take(80)
            .collect();
        let tone: String = phrase.tone.trim().chars().take(80).collect();
        let text: String = phrase.text.trim().chars().take(140).collect();
        if !id.is_empty()
            && !tone.is_empty()
            && !text.is_empty()
            && !cleaned
                .iter()
                .any(|item: &StoredPhrase| item.id.eq_ignore_ascii_case(&id))
        {
            cleaned.push(StoredPhrase { id, tone, text });
        }
        if cleaned.len() == 100 {
            break;
        }
    }
    cleaned
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::focus::FocusState;

    fn fixture_spans() -> Vec<ReportSpan> {
        vec![
            ReportSpan {
                started_offset_seconds: 0,
                seconds: 300,
                app: "Code".to_string(),
                context: Some("session.rs".to_string()),
                category: "work".to_string(),
                state: "focused".to_string(),
            },
            ReportSpan {
                started_offset_seconds: 300,
                seconds: 120,
                app: "Chrome".to_string(),
                context: Some("YouTube".to_string()),
                category: "media".to_string(),
                state: "distracted".to_string(),
            },
            ReportSpan {
                started_offset_seconds: 420,
                seconds: 180,
                app: "Code".to_string(),
                context: Some("storage.rs".to_string()),
                category: "work".to_string(),
                state: "neutral".to_string(),
            },
        ]
    }

    #[test]
    fn recompute_session_metrics_matches_fixture() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Ship storage", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        for span in fixture_spans() {
            storage
                .insert_span(
                    id,
                    &span.app,
                    span.context.as_deref(),
                    &span.category,
                    &span.state,
                    span.started_offset_seconds,
                    span.seconds,
                )
                .expect("insert span");
        }
        storage
            .insert_event(id, 310, "nudge", Some("voice"))
            .expect("insert event");
        storage
            .close_session(id, "2026-09-22T13:00:00Z", "completed", 600)
            .expect("close session");

        let report = storage.get_session_report(id).expect("report");
        assert_eq!(report.focused_seconds, 480);
        assert_eq!(report.distracted_seconds, 120);
        assert_eq!(report.drift_count, 1);
        assert_eq!(report.nudge_count, 1);
        assert_eq!(report.longest_stretch_seconds, 300);
        assert_eq!(report.median_stretch_seconds, 240);
        assert_eq!(report.focus_percent, 80);
    }

    #[test]
    fn metrics_survive_close_and_reopen() {
        let dir = std::env::temp_dir().join(format!(
            "supervisor-storage-test-{}",
            std::process::id()
        ));
        let path = dir.join("sessions.db");
        let _ = std::fs::remove_dir_all(&dir);

        let id = {
            let (storage, _) = SessionStorage::open(&path).expect("open db");
            let id = storage
                .insert_session("Persist", Strictness::Normal, "2026-09-22T12:00:00Z", 600)
                .expect("insert session");
            for span in fixture_spans() {
                storage
                    .insert_span(
                        id,
                        &span.app,
                        span.context.as_deref(),
                        &span.category,
                        &span.state,
                        span.started_offset_seconds,
                        span.seconds,
                    )
                    .expect("insert span");
            }
            storage
                .close_session(id, "2026-09-22T12:30:00Z", "stopped", 1800)
                .expect("close session");
            id
        };

        let (storage, state) = SessionStorage::open(&path).expect("reopen db");
        let report = storage.get_session_report(id).expect("report");
        assert_eq!(report.focused_seconds, 480);
        assert_eq!(report.distracted_seconds, 120);
        assert_eq!(state.history.len(), 1);
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn daily_score_uses_time_actually_run() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Hour plan", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        storage
            .insert_span(id, "Code", None, "work", "focused", 0, 1500)
            .expect("insert span");
        storage
            .insert_span(id, "Chrome", Some("YouTube"), "media", "distracted", 1500, 300)
            .expect("insert span");
        storage
            .close_session(id, "2026-09-22T12:30:00Z", "stopped", 1800)
            .expect("close session");

        let trends = storage.get_trends().expect("trends");
        assert_eq!(trends.daily_focus.len(), 1);
        assert_eq!(trends.daily_focus[0].focus_percent, 83);
        assert_eq!(trends.daily_focus[0].focus_seconds, 1500);
        assert_eq!(trends.sessions[0].focus_percent, 83);
        assert_eq!(trends.weekly_stretch.len(), 5);
        let (seconds, count) = storage
            .sum_focus("2026-09-22", "2026-09-23")
            .expect("sum");
        assert_eq!(count, 1);
        assert_eq!(seconds, 1500);
        let (empty_seconds, empty_count) = storage
            .sum_focus("2026-09-23", "2026-09-24")
            .expect("empty sum");
        assert_eq!(empty_count, 0);
        assert_eq!(empty_seconds, 0);
    }

    #[test]
    fn recap_uses_inclusive_start_and_exclusive_end() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let first = storage
            .insert_session("First", Strictness::Normal, "2026-09-01T08:00:00Z", 1800)
            .expect("insert first");
        storage
            .insert_span(first, "Code", None, "work", "focused", 0, 600)
            .expect("span");
        storage
            .close_session(first, "2026-09-01T08:10:00Z", "stopped", 600)
            .expect("close");
        let second = storage
            .insert_session("Second", Strictness::Normal, "2026-09-30T08:00:00Z", 1800)
            .expect("insert second");
        storage
            .insert_span(second, "Code", None, "work", "focused", 0, 900)
            .expect("span");
        storage
            .close_session(second, "2026-09-30T08:15:00Z", "stopped", 900)
            .expect("close");
        let october = storage
            .insert_session("October", Strictness::Normal, "2026-10-01T08:00:00Z", 1800)
            .expect("insert october");
        storage
            .insert_span(october, "Code", None, "work", "focused", 0, 120)
            .expect("span");
        storage
            .close_session(october, "2026-10-01T08:02:00Z", "stopped", 120)
            .expect("close");

        let september = storage
            .focus_recap(
                parse_day("2026-09-01").expect("sep 1"),
                parse_day("2026-10-01").expect("oct 1"),
            )
            .expect("september recap");
        assert_eq!(september.start, "2026-09-01");
        assert_eq!(september.end, "2026-09-30");
        assert_eq!(september.session_count, 2);
        assert_eq!(september.focus_seconds, 1500);

        let week = storage
            .focus_recap(
                parse_day("2026-09-28").expect("week start"),
                parse_day("2026-10-05").expect("week end"),
            )
            .expect("week recap");
        assert_eq!(week.session_count, 2);
        assert_eq!(week.focus_seconds, 1020);
    }

    #[test]
    fn period_recap_splits_week_days_and_session_halves() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Split", Strictness::Normal, "2026-09-23T12:00:00Z", 1200)
            .expect("insert");
        storage
            .insert_span(id, "Cursor", None, "work", "focused", 0, 600)
            .expect("first half");
        storage
            .insert_span(id, "Chrome", None, "browse", "distracted", 600, 600)
            .expect("second half");
        storage
            .close_session(id, "2026-09-23T12:20:00Z", "stopped", 1200)
            .expect("close");

        let recap = storage
            .get_period_recap("week", "2026-09-23")
            .expect("week recap");
        assert_eq!(recap.kind, "week");
        assert_eq!(recap.start, "2026-09-21");
        assert_eq!(recap.end, "2026-09-27");
        assert_eq!(recap.session_count, 1);
        assert_eq!(recap.focus_seconds, 600);
        assert_eq!(recap.buckets.len(), 7);
        assert_eq!(recap.buckets[2].label, "W");
        assert_eq!(recap.buckets[2].focus_seconds, 600);
        assert_eq!(recap.buckets[0].focus_seconds, 0);
        assert_eq!(recap.first_half_focus_percent, Some(100));
        assert_eq!(recap.second_half_focus_percent, Some(0));
        assert_eq!(
            recap
                .apps
                .iter()
                .map(|app| (app.group.as_str(), app.label.as_str(), app.seconds))
                .collect::<Vec<_>>(),
            vec![
                ("Miscellaneous", "Browser · site unknown", 600),
                ("Miscellaneous", "Cursor", 600),
            ]
        );

        let month = storage
            .get_period_recap("month", "2026-09-23")
            .expect("month recap");
        assert_eq!(month.start, "2026-09-01");
        assert_eq!(month.end, "2026-09-30");
        assert_eq!(month.buckets.len(), 5);
        assert_eq!(month.buckets[3].label, "22–28");
        assert_eq!(month.buckets[3].focus_seconds, 600);
        assert!(storage.get_period_recap("year", "2026-09-01").is_err());
    }

    #[test]
    fn period_recap_groups_apps_by_current_rules() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        storage
            .replace_rules(&[
                AppRule::app_name("Cursor", RuleClassification::Focused),
                AppRule::app_name("Slack", RuleClassification::Neutral),
                AppRule::title_contains("YouTube", RuleClassification::Distracting),
            ])
            .expect("rules");
        let id = storage
            .insert_session("Grouped", Strictness::Normal, "2026-09-23T12:00:00Z", 250)
            .expect("insert");
        storage
            .insert_span(id, "Cursor", None, "work", "focused", 0, 100)
            .expect("focus");
        storage
            .insert_span(id, "Slack", None, "comms", "neutral", 100, 50)
            .expect("neutral");
        storage
            .insert_span(
                id,
                "Chrome",
                Some("YouTube - Home"),
                "browse",
                "distracted",
                150,
                80,
            )
            .expect("distract");
        storage
            .insert_span(id, "Notepad", None, "browse", "neutral", 230, 20)
            .expect("misc");
        storage
            .close_session(id, "2026-09-23T12:04:10Z", "stopped", 250)
            .expect("close");

        let recap = storage
            .get_period_recap("week", "2026-09-23")
            .expect("week recap");
        assert_eq!(
            recap
                .apps
                .iter()
                .map(|app| (app.group.as_str(), app.label.as_str(), app.seconds))
                .collect::<Vec<_>>(),
            vec![
                ("Focus", "Cursor", 100),
                ("Neutral", "Slack", 50),
                ("Distracting", "YouTube - Home", 80),
                ("Miscellaneous", "Notepad", 20),
            ]
        );
    }

    #[test]
    fn month_bounds_roll_to_january() {
        let december = parse_day("2026-12-09").expect("december");
        assert_eq!(format_day(first_of_month(december)), "2026-12-01");
        assert_eq!(format_day(next_month_start(december)), "2027-01-01");
        let march = parse_day("2026-03-31").expect("march");
        assert_eq!(format_day(next_month_start(march)), "2026-04-01");
    }

    #[test]
    fn history_week_keeps_sessions_on_that_monday_week() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let in_week = storage
            .insert_session("In week", Strictness::Normal, "2026-09-22T12:00:00Z", 1800)
            .expect("insert in week");
        storage
            .insert_span(in_week, "Code", None, "work", "focused", 0, 600)
            .expect("span");
        storage
            .close_session(in_week, "2026-09-22T12:10:00Z", "stopped", 600)
            .expect("close");
        let later = storage
            .insert_session("Later", Strictness::Normal, "2026-10-06T12:00:00Z", 1800)
            .expect("insert later");
        storage
            .insert_span(later, "Code", None, "work", "focused", 0, 300)
            .expect("span");
        storage
            .close_session(later, "2026-10-06T12:05:00Z", "stopped", 300)
            .expect("close");

        let week = storage
            .get_history_week("2026-09-21")
            .expect("history week");
        assert_eq!(week.week_start, "2026-09-21");
        assert_eq!(week.sessions.len(), 1);
        assert_eq!(week.sessions[0].goal, "In week");
        assert_eq!(week.oldest_day.as_deref(), Some("2026-09-22"));

        let empty = storage
            .get_history_week("2026-09-07")
            .expect("empty week");
        assert!(empty.sessions.is_empty());
        assert!(storage.get_history_week("09-21-2026").is_err());
    }

    #[test]
    fn sunday_stays_in_the_week_already_underway() {
        let sunday = parse_day("2026-10-11").expect("sunday");
        assert_eq!(format_day(monday_on_or_before(sunday)), "2026-10-05");

        let instant = time::OffsetDateTime::parse(
            "2026-10-12T03:26:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .expect("parse");
        let central = time::UtcOffset::from_hms(-5, 0, 0).expect("offset");
        assert_eq!(calendar_day(instant, central), "2026-10-11");

        let today = local_today();
        let monday = this_monday();
        assert_eq!(monday.weekday(), time::Weekday::Monday);
        assert!(today >= monday);
        assert!(today < monday.saturating_add(time::Duration::days(7)));

        let storage = SessionStorage::open_in_memory().expect("open db");
        let recap = storage
            .get_period_recap("week", "2026-10-11")
            .expect("week of sunday");
        assert_eq!(recap.start, "2026-10-05");
        assert_eq!(recap.end, "2026-10-11");
        let history = storage
            .get_history_week("2026-10-11")
            .expect("history week");
        assert_eq!(history.week_start, "2026-10-05");
    }

    #[test]
    fn version_1_database_migrates_and_keeps_history() {
        let dir = std::env::temp_dir().join(format!(
            "supervisor-storage-migrate-{}",
            std::process::id()
        ));
        let path = dir.join("sessions.db");
        let _ = std::fs::remove_dir_all(&dir);
        {
            let (storage, _) = SessionStorage::open(&path).expect("open db");
            let id = storage
                .insert_session("Old", Strictness::Normal, "2026-09-01T12:00:00Z", 1500)
                .expect("insert session");
            storage
                .insert_span(id, "Code", None, "work", "focused", 0, 420)
                .expect("insert span");
            storage
                .close_session(id, "2026-09-01T12:30:00Z", "stopped", 999)
                .expect("close session");
            storage
                .conn
                .execute_batch(
                    "ALTER TABLE session DROP COLUMN elapsed_seconds; PRAGMA user_version = 1;",
                )
                .expect("downgrade to v1");
        }

        let (storage, state) = SessionStorage::open(&path).expect("migrate db");
        let version: i32 = storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].duration_seconds, 420);
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_database_is_moved_aside_and_replaced() {
        let dir = std::env::temp_dir().join(format!(
            "supervisor-storage-corrupt-{}",
            std::process::id()
        ));
        let path = dir.join("sessions.db");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        std::fs::write(&path, b"this is not a sqlite database, just garbage bytes").expect("write");

        let (opened, notice) = open_or_reset(Ok(path.clone()));

        let Some(StorageNotice::Reset { moved_to, .. }) = notice else {
            panic!("expected a reset notice, got {notice:?}");
        };
        assert!(std::path::Path::new(&moved_to).exists());
        let (storage, state) = opened.expect("fresh database");
        assert!(state.history.is_empty());
        storage
            .insert_session("After reset", Strictness::Normal, "2026-09-28T00:00:00Z", 600)
            .expect("fresh database accepts writes");
        drop(storage);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unusable_location_turns_history_off() {
        let (opened, notice) = open_or_reset(Err("no app data folder".to_string()));
        assert!(opened.is_none());
        assert!(matches!(notice, Some(StorageNotice::Off { .. })));

        let dir = std::env::temp_dir().join(format!(
            "supervisor-storage-blocked-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create dir");
        let blocker = dir.join("not-a-folder");
        std::fs::write(&blocker, b"file where a folder should be").expect("write");

        let (opened, notice) = open_or_reset(Ok(blocker.join("sessions.db")));
        assert!(opened.is_none());
        assert!(matches!(notice, Some(StorageNotice::Off { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphan_session_recovers_as_abandoned() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Crash", Strictness::Strict, "2026-09-22T08:00:00Z", 1500)
            .expect("insert session");
        storage
            .insert_span(
                id,
                "Code",
                Some("lib.rs"),
                "work",
                "focused",
                0,
                90,
            )
            .expect("insert span");

        storage.recover_orphans().expect("recover");

        let report = storage.get_session_report(id).expect("report");
        assert_eq!(report.ended_reason.as_deref(), Some("abandoned"));
        assert!(report.ended_at.is_some());
        assert_eq!(report.focused_seconds, 90);
    }

    #[test]
    fn span_category_mapping_is_deterministic() {
        assert_eq!(
            span_category("Code", Some("main.rs"), "focused", RuleClassification::Focused),
            "work"
        );
        assert_eq!(
            span_category("Slack", None, "neutral", RuleClassification::Neutral),
            "comms"
        );
        assert_eq!(
            span_category(
                "Chrome",
                Some("YouTube - watch"),
                "distracted",
                RuleClassification::Distracting
            ),
            "media"
        );
        assert_eq!(
            span_category("Chrome", Some("docs"), "idle", RuleClassification::Neutral),
            "offscreen"
        );
    }

    #[test]
    fn settings_and_rules_round_trip() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let settings = SessionSettings {
            default_strictness: Strictness::Strict,
            voice_profile: "Custom".to_string(),
            status_window_enabled: false,
            status_window_on_top: false,
            webcam_gpu: true,
        };
        storage.save_settings(&settings).expect("save settings");
        let loaded = storage.load_settings().expect("load settings");
        assert_eq!(loaded, settings);

        let rules = vec![AppRule::app_name("Code", RuleClassification::Focused)];
        storage.replace_rules(&rules).expect("save rules");
        let loaded_rules = storage.load_rules().expect("load rules");
        assert_eq!(loaded_rules, rules);
    }

    #[test]
    fn focus_state_key_matches_schema() {
        assert_eq!(focus_state_key(FocusState::Suspicious), "suspicious");
    }
}
