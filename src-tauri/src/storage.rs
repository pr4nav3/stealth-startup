use crate::focus::{starter_rules, AppRule, RuleClassification, RuleTarget};
use crate::session::{utc_now, SessionSettings, SessionSummary, Strictness, HISTORY_CACHE_LIMIT};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i32 = 2;
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
pub struct TrendsData {
    pub sessions: Vec<TrendSession>,
    pub daily_focus: Vec<DailyFocus>,
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
            let top_hints = self.top_hints_for_session(id as u64)?;
            let duration_seconds = self.session_duration_seconds(id as u64)?;
            summaries.push(SessionSummary {
                id: id as u64,
                goal,
                started_at,
                duration_seconds,
                focus_seconds: focused_seconds as u64,
                distracted_seconds: distracted_seconds as u64,
                idle_seconds: idle_seconds as u64,
                nudges: nudges as u64,
                top_hints,
            });
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
            .prepare(
                &format!(
                    "SELECT substr(started_at, 1, 10) AS day,
                            SUM(focused_seconds) AS focus_sum,
                            SUM({RUN_SECONDS_SQL}) AS run_seconds,
                            COUNT(*) AS session_count
                     FROM session
                     WHERE ended_at IS NOT NULL
                     GROUP BY day
                     ORDER BY day DESC
                     LIMIT 14"
                ),
            )
            .map_err(|error| error.to_string())?;
        let daily_rows = daily_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut daily_focus = Vec::new();
        for row in daily_rows {
            let (date, focus_sum, run_seconds, session_count) =
                row.map_err(|error| error.to_string())?;
            let focus_percent = if run_seconds == 0 {
                0
            } else {
                (focus_sum as u64 * 100) / run_seconds as u64
            };
            daily_focus.push(DailyFocus {
                date,
                focus_percent,
                focus_seconds: focus_sum as u64,
                session_count: session_count as u64,
            });
        }
        daily_focus.reverse();

        Ok(TrendsData {
            sessions,
            daily_focus,
        })
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
