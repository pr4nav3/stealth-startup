use crate::activity;
use crate::app_identity::{self, AppKeys};
use crate::attribution::{Activity, PAUSED_KEY, PHONE_KEY, SUPERVISOR_KEY, UNKNOWN_KEY, UNKNOWN_LABEL};
use crate::focus::{matching_rule, starter_rules, AppRule, RuleClassification, RuleTarget};
use crate::session::{utc_now, SessionSettings, SessionSummary, Strictness, HISTORY_CACHE_LIMIT};
use crate::website;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i32 = 11;
/// Weeks of history the Trends page and the attention-span chart can draw.
const TREND_WEEK_COUNT: usize = 8;
const PHRASE_LIBRARY_KEY: &str = "voice.phrase_library";

/// How long a `session` row's timer actually ran. Sessions that never closed cleanly have no
/// timer value; their tracked span time is the best record of how long they ran.
const RUN_SECONDS_SQL: &str = "COALESCE(session.elapsed_seconds,
    (SELECT COALESCE(SUM(span.seconds), 0) FROM span
     WHERE span.session_id = session.id AND span.state != 'paused'))";

const WEBCAM_SETTING_KEYS: &[&str] = &[
    "webcam.enabled",
    "webcam.phone_nudge",
    "webcam.away_after_seconds",
];

pub struct SessionStorage {
    conn: Connection,
    failure: RefCell<Option<String>>,
    site_names: RefCell<HashMap<String, String>>,
    app_names: RefCell<HashMap<String, String>>,
    native_app_names: RefCell<HashMap<String, String>>,
    /// Which app key each native app identity was given (see `app_identity`).
    app_keys: RefCell<AppKeys>,
    /// Each identity's key and the latest label seen for it.
    identity_labels: RefCell<HashMap<String, (String, String)>>,
    /// Seconds of timeline holes filled at close. The recorder should keep this at zero.
    repaired_seconds: std::cell::Cell<u64>,
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
    /// Bundled phrase ids the user deleted. Older libraries have none.
    #[serde(default)]
    pub hidden_phrases: Vec<String>,
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
        Err(reason) if path.exists() && matches!(reason.as_str(),
            "file is not a database" | "database disk image is malformed") => reason,
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

#[derive(Clone, Debug, Serialize)]
pub struct ReportSpan {
    pub started_offset_seconds: u64,
    pub seconds: u64,
    /// The program that was in front (a browser for website rows).
    pub app: String,
    pub kind: String,
    pub activity_key: String,
    pub label: String,
    pub source: String,
    pub state: String,
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
    /// Time on apps and sites with no rule; it counts once the user rates them.
    pub unrated_seconds: u64,
    pub distracted_seconds: u64,
    pub idle_seconds: u64,
    pub away_seconds: u64,
    pub phone_seconds: u64,
    pub drift_count: u64,
    pub nudge_count: u64,
    pub longest_stretch_seconds: u64,
    pub median_stretch_seconds: u64,
    pub spans: Vec<ReportSpan>,
    /// Time per app/site, grouped like the timeline (see `rated_state`).
    pub apps: Vec<RecapApp>,
    pub events: Vec<ReportEvent>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DailyFocus {
    pub date: String,
    pub focus_percent: u64,
    pub focus_seconds: u64,
    pub session_count: u64,
}

/// One calendar week, from every span recorded in it (not averages of session averages).
#[derive(Clone, Debug, Serialize)]
pub struct TrendWeek {
    pub week_start: String,
    pub session_count: u64,
    pub focus_seconds: u64,
    pub median_stretch_seconds: u64,
    pub recovery_median_seconds: Option<u64>,
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
    pub daily_focus: Vec<DailyFocus>,
    /// Oldest first, ending with the current week; starts at the first week with data.
    pub weeks: Vec<TrendWeek>,
    /// Completed weeks (not this one) that have at least one session.
    pub full_weeks: u64,
    pub week_recap: FocusRecap,
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
    pub kind: String,
    pub key: String,
    pub label: String,
    pub seconds: u64,
    /// Whether a rule can name exactly this row (not Supervisor, Away, or browser catch-alls).
    pub ratable: bool,
    /// The current rule's decision; `None` means unrated, which does not count as focus.
    pub rating: Option<RuleClassification>,
}

#[derive(Clone, Debug, Serialize)]
pub struct WeekRecap {
    pub start: String,
    pub end: String,
    pub focus_seconds: u64,
    pub session_count: u64,
    pub buckets: Vec<RecapBucket>,
    pub median_stretch_seconds: u64,
    pub longest_stretch_seconds: u64,
    /// Typical time from leaving focus to being back on task; `None` when nothing drifted.
    pub recovery_median_seconds: Option<u64>,
    pub recovery_count: u64,
    /// Drifts still going when their session ended.
    pub unrecovered_count: u64,
    /// The same figure for the week before, so the page can say faster or slower.
    pub previous_recovery_median_seconds: Option<u64>,
    pub apps: Vec<RecapApp>,
}

/// Attention figures pooled from the spans of every session in a period.
#[derive(Default)]
struct SpanStats {
    stretches: Vec<u64>,
    recoveries: Vec<u64>,
    unrecovered: u64,
}

impl SpanStats {
    fn add(&mut self, spans: &[ReportSpan]) {
        self.stretches.extend(focused_stretches(spans));
        let (recoveries, unrecovered) = drift_recoveries(spans);
        self.recoveries.extend(recoveries);
        self.unrecovered += unrecovered;
    }

    fn recovery_median(&self) -> Option<u64> {
        (!self.recoveries.is_empty()).then(|| median(&self.recoveries))
    }
}

struct RecapSession {
    id: u64,
    day: String,
    focused_seconds: u64,
    longest_stretch_seconds: u64,
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
            site_names: RefCell::default(),
            app_names: RefCell::default(),
            native_app_names: RefCell::default(),
            app_keys: RefCell::default(),
            identity_labels: RefCell::default(),
            repaired_seconds: std::cell::Cell::new(0),
        };
        storage.apply_pragmas()?;
        storage.migrate()?;
        storage.seed_webcam_setting_keys()?;
        storage.load_site_names()?;
        storage.load_app_names()?;
        storage.load_app_identities()?;
        Ok(storage)
    }

    fn load_app_identities(&self) -> Result<(), String> {
        let mut stmt = self.conn.prepare("SELECT identity, key, label FROM app_identity ORDER BY rowid")
            .map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut keys = AppKeys::default();
        let mut labels = HashMap::new();
        for (identity, key, label) in rows {
            if keys.insert(&identity, &key) {
                labels.insert(identity, (key, label));
            }
        }
        *self.app_keys.borrow_mut() = keys;
        *self.identity_labels.borrow_mut() = labels;
        Ok(())
    }

    /// The app key for a native identity, assigned on first sight and kept from then on. The
    /// first identity whose name fits the history already kept under its program name takes
    /// that name as its key, so old spans and rules carry on; any other gets `name#2`...
    pub fn app_key(&self, identity: &str, stem: &str, label: &str) -> String {
        let existing = self.app_keys.borrow().get(identity).map(str::to_string);
        if let Some(key) = existing {
            let stale = self.identity_labels.borrow().get(identity).is_none_or(|(_, seen)| seen != label);
            if stale && !label.is_empty() {
                if let Err(error) = self.conn.execute(
                    "UPDATE app_identity SET label = ?2 WHERE identity = ?1", params![identity, label],
                ) {
                    self.record_failure("save an app's name", error.to_string());
                }
                self.identity_labels.borrow_mut().insert(identity.to_string(), (key.clone(), label.to_string()));
            }
            return key;
        }
        let stem = app_identity::clean_stem(stem);
        let fits = match self.latest_app_label(&stem) {
            Ok(None) => true,
            Ok(Some(seen)) => seen.eq_ignore_ascii_case(label) || seen.eq_ignore_ascii_case(&stem),
            Err(error) => {
                self.record_failure("read app history", error);
                false
            }
        };
        let key = self.app_keys.borrow_mut().claim(identity, &stem, fits);
        if let Err(error) = self.conn.execute(
            "INSERT INTO app_identity (identity, key, label) VALUES (?1, ?2, ?3)",
            params![identity, key, label],
        ) {
            self.record_failure("save an app's identity", error.to_string());
        }
        self.identity_labels.borrow_mut().insert(identity.to_string(), (key.clone(), label.to_string()));
        key
    }

    /// The label most recently recorded under an app key.
    fn latest_app_label(&self, key: &str) -> Result<Option<String>, String> {
        let label = self.conn.query_row(
            "SELECT label FROM span WHERE kind = 'app' AND source != 'none' AND activity_key = ?1
             ORDER BY id DESC LIMIT 1",
            [key],
            |row| row.get::<_, String>(0),
        );
        match label {
            Ok(label) => Ok(Some(label)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    fn load_site_names(&self) -> Result<(), String> {
        let mut stmt = self
            .conn
            .prepare("SELECT key, name FROM site_name")
            .map_err(|error| error.to_string())?;
        let names = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| error.to_string())?
            .collect::<Result<HashMap<String, String>, _>>()
            .map_err(|error| error.to_string())?;
        *self.site_names.borrow_mut() = names;
        Ok(())
    }

    fn load_app_names(&self) -> Result<(), String> {
        let mut stmt = self.conn.prepare("SELECT key, name FROM app_name")
            .map_err(|error| error.to_string())?;
        let names = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|error| error.to_string())?
            .collect::<Result<HashMap<String, String>, _>>()
            .map_err(|error| error.to_string())?;
        *self.app_names.borrow_mut() = names;
        Ok(())
    }

    /// Website labels are looked up at read time, so a rename reaches old history too.
    fn display_label(&self, kind: &str, key: &str, stored: String) -> String {
        match kind {
            "website" => self.site_label(key),
            "app" => self.app_label(key, self.native_app_names.borrow().get(key).map(String::as_str).unwrap_or(&stored)),
            _ => stored,
        }
    }

    fn refresh_native_app_names(&self) -> Result<(), String> {
        let mut stmt = self.conn.prepare(
            "SELECT activity_key, label FROM span WHERE id IN (
                 SELECT MAX(id) FROM span
                 WHERE kind = 'app' AND source != 'none' AND activity_key != ''
                 GROUP BY activity_key)",
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())?;
        let mut names: HashMap<String, String> = rows.into_iter()
            .filter(|(key, _)| !legacy_unidentified_window(key))
            .collect();
        let identified: HashMap<String, String> = self.identity_labels.borrow().values()
            .filter(|(_, label)| !label.is_empty())
            .cloned()
            .collect();
        // A running process is found by program name, which names one app only while no
        // other identity shares it.
        let keys: Vec<String> = names.keys()
            .filter(|key| !identified.contains_key(*key) && !self.app_keys.borrow().stem_is_shared(key))
            .cloned()
            .collect();
        names.extend(activity::running_app_labels(&keys));
        names.extend(identified);
        *self.native_app_names.borrow_mut() = names;
        Ok(())
    }

    /// The user's name for an app, else `fallback`, marked when another app shares it.
    pub fn app_label(&self, key: &str, fallback: &str) -> String {
        self.app_names.borrow().get(key).cloned()
            .unwrap_or_else(|| app_identity::display_label(key, fallback))
    }

    /// Observed app labels plus the user's names, for rule chips and name editing.
    pub fn app_names(&self) -> Result<HashMap<String, String>, String> {
        self.refresh_native_app_names()?;
        let mut names = HashMap::new();
        let mut stmt = self.conn.prepare(
            "SELECT activity_key, label FROM span
             WHERE kind = 'app' AND source != 'none' AND activity_key != ''
             ORDER BY id DESC",
        ).map_err(|error| error.to_string())?;
        let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
            .map_err(|error| error.to_string())?;
        for row in rows {
            let (key, label) = row.map_err(|error| error.to_string())?;
            if !legacy_unidentified_window(&key) {
                names.entry(key).or_insert(label);
            }
        }
        names.extend(self.native_app_names.borrow().clone());
        for (key, label) in names.iter_mut() {
            *label = app_identity::display_label(key, label);
        }
        names.extend(self.app_names.borrow().iter()
            .filter(|(key, _)| !legacy_unidentified_window(key))
            .map(|(key, name)| (key.clone(), name.clone())));
        Ok(names)
    }

    pub fn set_app_name(&self, key: &str, name: &str) -> Result<(), String> {
        let key = key.trim().to_lowercase();
        let name: String = name.trim().chars().take(60).collect();
        if key.is_empty() {
            return Err("No app to name".to_string());
        }
        if name.is_empty() {
            self.conn.execute("DELETE FROM app_name WHERE key = ?1", [&key])
                .map_err(|error| error.to_string())?;
            self.app_names.borrow_mut().remove(&key);
        } else {
            self.conn.execute(
                "INSERT INTO app_name (key, name) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET name = excluded.name",
                [&key, &name],
            ).map_err(|error| error.to_string())?;
            self.app_names.borrow_mut().insert(key, name);
        }
        Ok(())
    }

    /// The user's name for a site, then the built-in name, then the domain.
    pub fn site_label(&self, key: &str) -> String {
        self.site_names
            .borrow()
            .get(key)
            .cloned()
            .unwrap_or_else(|| website::site_label(key))
    }

    /// Every site with a name other than its domain: built-in names overlaid by the user's.
    pub fn site_names(&self) -> HashMap<String, String> {
        let mut names: HashMap<String, String> = website::preset_names()
            .map(|(key, label)| (key.to_string(), label.to_string()))
            .collect();
        names.extend(self.site_names.borrow().clone());
        names
    }

    /// An empty name, or the built-in name, clears the user's name.
    pub fn set_site_name(&self, key: &str, name: &str) -> Result<(), String> {
        let key = key.trim().to_lowercase();
        let name: String = name.trim().chars().take(60).collect();
        if key.is_empty() {
            return Err("No website to name".to_string());
        }
        if name.is_empty() || name == website::site_label(&key) {
            self.conn
                .execute("DELETE FROM site_name WHERE key = ?1", [&key])
                .map_err(|error| error.to_string())?;
            self.site_names.borrow_mut().remove(&key);
        } else {
            self.conn
                .execute(
                    "INSERT INTO site_name (key, name) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET name = excluded.name",
                    [&key, &name],
                )
                .map_err(|error| error.to_string())?;
            self.site_names.borrow_mut().insert(key, name);
        }
        Ok(())
    }

    fn apply_pragmas(&self) -> Result<(), String> {
        self.conn
            .execute_batch(
                "
                PRAGMA foreign_keys = ON;
                PRAGMA journal_mode = WAL;
                PRAGMA synchronous = NORMAL;
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
        if version < 3 {
            self.migrate_to_activity_identity()
                .map_err(|error| error.to_string())?;
        }
        if version < 4 {
            // Names the user gave sites. Kept apart from rules so deleting a rule keeps the name.
            self.conn
                .execute_batch(
                    "
                    BEGIN;
                    CREATE TABLE site_name (key TEXT PRIMARY KEY, name TEXT NOT NULL);
                    PRAGMA user_version = 4;
                    COMMIT;
                    ",
                )
                .map_err(|error| error.to_string())?;
        }
        if version < 5 {
            self.conn.execute_batch(
                "BEGIN;
                 CREATE TABLE app_name (key TEXT PRIMARY KEY, name TEXT NOT NULL);
                 PRAGMA user_version = 5;
                 COMMIT;",
            ).map_err(|error| error.to_string())?;
        }
        if version < 6 {
            // v6 recomputed stretches; v7 below recomputes every metric, so only the version moves.
            self.conn.execute_batch("PRAGMA user_version = 6;").map_err(|error| error.to_string())?;
        }
        if version < 7 {
            // The score becomes Relevant time only, and unrated time gets its own total.
            let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
            tx.execute_batch(
                "ALTER TABLE session ADD COLUMN neutral_seconds INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE session ADD COLUMN unrated_seconds INTEGER NOT NULL DEFAULT 0;",
            ).map_err(|error| error.to_string())?;
            self.rescore_history(&self.scoring_rules()?)?;
            tx.execute_batch("PRAGMA user_version = 7;").map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
        }
        if version < 8 {
            // Supervisor's own window stopped counting as focus (see `rated_state`).
            let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
            self.rescore_history(&self.scoring_rules()?)?;
            tx.execute_batch("PRAGMA user_version = 8;").map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
        }
        if version < 9 {
            // A browser's own pages ("Google Chrome pages") are the browser app now: same row,
            // same rules. Their key and app were already the browser's app id.
            let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
            tx.execute_batch(
                "UPDATE span SET kind = 'app', label = CASE
                     WHEN label LIKE '% local pages' THEN substr(label, 1, length(label) - 12)
                     WHEN label LIKE '% pages' THEN substr(label, 1, length(label) - 6)
                     ELSE label END
                 WHERE kind IN ('browser_pages', 'browser_local');",
            ).map_err(|error| error.to_string())?;
            self.rescore_history(&self.scoring_rules()?)?;
            tx.execute_batch("PRAGMA user_version = 9;").map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
        }
        if version < 10 {
            // Browsers became always Neutral: their unrated time is Neutral now (see `ratable`).
            let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
            self.rescore_history(&self.scoring_rules()?)?;
            tx.execute_batch("PRAGMA user_version = 10;").map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
        }
        if version < 11 {
            // Apps are known by native identity (see `app_identity`). Existing keys stay as they
            // are: the first identity that fits one takes it over when it is next seen.
            let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS app_identity (
                   identity TEXT PRIMARY KEY,
                   key TEXT NOT NULL UNIQUE,
                   label TEXT NOT NULL DEFAULT ''
                 );",
            ).map_err(|error| error.to_string())?;
            self.map_app_rules_to_keys()?;
            self.rescore_history(&self.scoring_rules()?)?;
            tx.execute_batch("PRAGMA user_version = 11;").map_err(|error| error.to_string())?;
            tx.commit().map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    /// v3: spans name an app or website key instead of keeping window titles, and rules
    /// target an app or a website. Old browser time can't be split by site, so it becomes
    /// Other websites; title rules that don't name a site or an app are dropped.
    fn migrate_to_activity_identity(&self) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        let browsers = website::BROWSERS
            .iter()
            .map(|name| format!("'{name}'"))
            .collect::<Vec<_>>()
            .join(", ");
        tx.execute_batch(&format!(
            "
            ALTER TABLE span ADD COLUMN kind TEXT NOT NULL DEFAULT 'app';
            ALTER TABLE span ADD COLUMN activity_key TEXT NOT NULL DEFAULT '';
            ALTER TABLE span ADD COLUMN label TEXT NOT NULL DEFAULT '';
            ALTER TABLE span ADD COLUMN source TEXT NOT NULL DEFAULT 'app';
            UPDATE span SET activity_key = lower(app), label = app;
            UPDATE span SET source = 'none' WHERE app = 'Untracked';
            UPDATE span SET kind = 'other_websites', activity_key = 'other-websites',
                            label = 'Other websites', source = 'none'
              WHERE lower(app) IN ({browsers});
            ALTER TABLE span DROP COLUMN context;
            "
        ))?;

        let legacy = {
            let mut stmt = tx.prepare("SELECT target, value, classification, source FROM rule ORDER BY id")?;
            let rows = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.execute("DELETE FROM rule", [])?;
        let mut seen = std::collections::HashSet::new();
        for (target, value, classification, source) in legacy {
            let classification = parse_classification(classification);
            let rule = match target.as_str() {
                "app_name" => Some(AppRule::app(&value, classification)),
                "window_title" => AppRule::from_input(&value, classification),
                _ => None,
            };
            let Some(rule) = rule.filter(|rule| !rule.value.is_empty()) else {
                continue;
            };
            if seen.insert(rule.value.to_lowercase()) {
                tx.execute(
                    "INSERT INTO rule (target, value, classification, source) VALUES (?1, ?2, ?3, ?4)",
                    params![
                        rule_target_key(&rule.target),
                        rule.value,
                        classification_key(rule.classification),
                        source
                    ],
                )?;
            }
        }
        tx.pragma_update(None, "user_version", 3)?;
        tx.commit()
    }

    /// v11: app rules written as a name ("Visual Studio Code") become a rule on that app's key
    /// when exactly one recorded app goes by the name, so the rule can't reach another app that
    /// later takes the same name. A name two apps share, or no recorded app has, stays as it is.
    /// Callers hold a transaction.
    fn map_app_rules_to_keys(&self) -> Result<(), String> {
        let latest: Vec<(String, String)> = {
            let mut stmt = self.conn.prepare(
                "SELECT activity_key, label FROM span WHERE id IN (
                     SELECT MAX(id) FROM span
                     WHERE kind = 'app' AND source != 'none' AND activity_key != ''
                     GROUP BY activity_key)",
            ).map_err(|error| error.to_string())?;
            let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
                .into_iter()
                .filter(|(key, _)| ratable("app", key) && !legacy_unidentified_window(key))
                .collect()
        };
        let rules: Vec<(i64, String)> = {
            let mut stmt = self.conn.prepare("SELECT id, value FROM rule WHERE target = 'app' ORDER BY id")
                .map_err(|error| error.to_string())?;
            let rows = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
        };
        let mut seen = std::collections::HashSet::new();
        for (id, value) in rules {
            let lower = value.trim().to_lowercase();
            let mapped = if latest.iter().any(|(key, _)| *key == lower) {
                Some(lower)
            } else {
                let named: Vec<&String> = latest.iter()
                    .filter(|(_, label)| label.eq_ignore_ascii_case(value.trim()))
                    .map(|(key, _)| key)
                    .collect();
                match named.as_slice() {
                    [key] => Some((*key).clone()),
                    _ => None,
                }
            };
            let value_now = mapped.unwrap_or_else(|| value.clone());
            // The earlier rule already decided this app; a later duplicate never applied.
            if !seen.insert(value_now.to_lowercase()) {
                self.conn.execute("DELETE FROM rule WHERE id = ?1", [id]).map_err(|error| error.to_string())?;
            } else if value_now != value {
                self.conn.execute("UPDATE rule SET value = ?2 WHERE id = ?1", params![id, value_now])
                    .map_err(|error| error.to_string())?;
            }
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
                "webcam_enabled" => {
                    settings.webcam_enabled = value == "true";
                }
                _ => {}
            }
        }
        Ok(settings)
    }

    pub fn save_settings(&self, settings: &SessionSettings) -> Result<(), String> {
        // One transaction: six separate commits were six fsyncs per toggle.
        let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
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
        self.set_setting(
            "webcam_enabled",
            if settings.webcam_enabled { "true" } else { "false" },
        )?;
        tx.commit().map_err(|error| error.to_string())
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
        match self.saved_rules()? {
            Some(rules) => Ok(rules),
            None => {
                let starter = starter_rules();
                self.replace_rules(&starter)?;
                Ok(starter)
            }
        }
    }

    /// `None` until rules are first saved, so an emptied rule list stays empty.
    fn saved_rules(&self) -> Result<Option<Vec<AppRule>>, String> {
        let stored = self.read_rules()?;
        let initialized: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM setting WHERE key = 'rules.initialized')", [], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        Ok((initialized || !stored.is_empty()).then_some(stored))
    }

    /// The rules history is scored with. Never writes, so it is safe inside a transaction.
    fn scoring_rules(&self) -> Result<Vec<AppRule>, String> {
        Ok(self.saved_rules()?.unwrap_or_else(starter_rules))
    }

    fn read_rules(&self) -> Result<Vec<AppRule>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT target, value, classification FROM rule ORDER BY id ASC")
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(AppRule {
                        target: parse_rule_target(row.get(0)?),
                        value: row.get(1)?,
                        classification: parse_classification(row.get(2)?),
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    /// Returns the finished sessions whose scores the new rules changed.
    pub fn replace_rules(&self, rules: &[AppRule]) -> Result<Vec<u64>, String> {
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
        tx.execute("INSERT OR REPLACE INTO setting (key, value) VALUES ('rules.initialized', 'true')", [])
            .map_err(|error| error.to_string())?;
        // A rating reaches past unrated time, so every saved score is redone with the new rules.
        let changed = self.rescore_history(rules)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(changed)
    }

    /// Recomputes the cached metrics of every finished session and returns the ones that
    /// changed. Callers hold a transaction.
    fn rescore_history(&self, rules: &[AppRule]) -> Result<Vec<u64>, String> {
        let ids = {
            let mut stmt = self.conn.prepare("SELECT id FROM session WHERE ended_at IS NOT NULL")
                .map_err(|error| error.to_string())?;
            let rows = stmt.query_map([], |row| row.get::<_, u64>(0))
                .map_err(|error| error.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?
        };
        let mut changed = Vec::new();
        for id in ids {
            if self.recompute_session_metrics(id, rules)? {
                changed.push(id);
            }
        }
        Ok(changed)
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
        let tx = self.conn.unchecked_transaction().map_err(|error| error.to_string())?;
        tx
            .execute(
                "UPDATE session SET ended_at = ?1, ended_reason = ?2, elapsed_seconds = ?3
                 WHERE id = ?4",
                params![ended_at, ended_reason, elapsed_seconds, id],
            )
            .map_err(|error| error.to_string())?;
        self.fill_span_holes(id)?;
        self.recompute_session_metrics(id, &self.scoring_rules()?)?;
        tx.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn insert_span(
        &self,
        session_id: u64,
        activity: &Activity,
        state: &str,
        started_offset_seconds: u64,
        seconds: u64,
    ) -> Result<(), String> {
        if seconds == 0 {
            return Ok(());
        }
        // `category` is a retired column kept so older databases stay readable.
        self.conn
            .execute(
                "INSERT INTO span (session_id, app, kind, activity_key, label, source, category, state,
                                   started_offset_seconds, seconds)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7, ?8, ?9)",
                params![
                    session_id,
                    activity.app_id,
                    activity.kind.key(),
                    activity.key,
                    activity.label,
                    activity.source.key(),
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

    /// Refresh the unfinished span in place; repeated checkpoints do not add history rows.
    pub fn checkpoint_span(
        &self,
        session_id: u64,
        activity: &Activity,
        state: &str,
        started_offset_seconds: u64,
        seconds: u64,
    ) -> Result<(), String> {
        if seconds == 0 {
            return Ok(());
        }
        let changed = self.conn.execute(
            "UPDATE span SET app = ?2, kind = ?3, activity_key = ?4, label = ?5,
                 source = ?6, state = ?7, seconds = ?9
             WHERE session_id = ?1 AND started_offset_seconds = ?8",
            params![session_id, activity.app_id, activity.kind.key(), activity.key,
                activity.label, activity.source.key(), state, started_offset_seconds, seconds],
        ).map_err(|error| error.to_string())?;
        if changed == 0 {
            self.insert_span(session_id, activity, state, started_offset_seconds, seconds)?;
        }
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
            self.fill_span_holes(id as u64)?;
            self.recompute_session_metrics(id as u64, &self.scoring_rules()?)?;
        }
        Ok(())
    }

    /// Safety net: a session's spans must tile it. Any hole left by a recorder bug becomes
    /// lost time instead of a blank in the report, and is logged so it gets fixed.
    fn fill_span_holes(&self, session_id: u64) -> Result<(), String> {
        let mut cursor = 0u64;
        for span in self.load_spans_for_session(session_id, &[])? {
            if span.started_offset_seconds > cursor {
                let gap = span.started_offset_seconds - cursor;
                eprintln!("supervisor: session {session_id} had a {gap}s timeline hole at {cursor}s");
                self.repaired_seconds.set(self.repaired_seconds.get() + gap);
                self.insert_span(session_id, &Activity::unknown(), "unknown", cursor, gap)?;
            }
            cursor = cursor.max(span.started_offset_seconds + span.seconds);
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn repaired_seconds(&self) -> u64 {
        self.repaired_seconds.get()
    }

    fn attach_ratings(&self, apps: &mut [RecapApp], rules: &[AppRule]) {
        for app in apps {
            app.ratable = ratable(&app.kind, &app.key);
            if !app.ratable {
                continue;
            }
            // Display overrides must never become rule-matching identities.
            let rule_label = self.native_app_names.borrow().get(&app.key).cloned()
                .unwrap_or_else(|| app.key.clone());
            let activity = Activity::from_stored(&app.kind, &app.key, &rule_label, &app.key);
            app.rating = matching_rule(rules, &activity).map(|rule| rule.classification);
        }
    }

    /// Returns whether any saved figure changed; unchanged sessions are not rewritten.
    pub fn recompute_session_metrics(&self, session_id: u64, rules: &[AppRule]) -> Result<bool, String> {
        let spans = self.load_spans_for_session(session_id, rules)?;
        let events = self.load_events_for_session(session_id)?;

        let mut focused_seconds = 0u64;
        let mut neutral_seconds = 0u64;
        let mut unrated_seconds = 0u64;
        let mut distracted_seconds = 0u64;
        let mut idle_seconds = 0u64;
        let mut phone_seconds = 0u64;

        for span in &spans {
            match span.state.as_str() {
                "focused" => focused_seconds += span.seconds,
                "neutral" => neutral_seconds += span.seconds,
                "unrated" => unrated_seconds += span.seconds,
                "suspicious" | "distracted" => distracted_seconds += span.seconds,
                // Phone time is distracted time, also shown on its own.
                "phone" => {
                    distracted_seconds += span.seconds;
                    phone_seconds += span.seconds;
                }
                "idle" | "unknown" => idle_seconds += span.seconds,
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

        let metrics = [
            focused_seconds,
            distracted_seconds,
            idle_seconds,
            drift_count,
            nudge_count,
            longest_stretch_seconds,
            median_stretch_seconds,
            neutral_seconds,
            unrated_seconds,
            phone_seconds,
        ];
        let saved = self.conn.query_row(
            "SELECT focused_seconds, distracted_seconds, idle_seconds, drift_count, nudge_count,
                    longest_stretch_seconds, median_stretch_seconds, neutral_seconds, unrated_seconds,
                    phone_seconds
             FROM session WHERE id = ?1",
            [session_id],
            |row| (0..metrics.len()).map(|index| row.get::<_, i64>(index).map(|value| value as u64))
                .collect::<rusqlite::Result<Vec<u64>>>(),
        ).map_err(|error| error.to_string())?;
        if saved == metrics {
            return Ok(false);
        }

        self.conn
            .execute(
                "UPDATE session SET
                    focused_seconds = ?1,
                    distracted_seconds = ?2,
                    idle_seconds = ?3,
                    drift_count = ?4,
                    nudge_count = ?5,
                    longest_stretch_seconds = ?6,
                    median_stretch_seconds = ?7,
                    neutral_seconds = ?9,
                    unrated_seconds = ?10,
                    phone_seconds = ?11
                 WHERE id = ?8",
                params![
                    focused_seconds,
                    distracted_seconds,
                    idle_seconds,
                    drift_count,
                    nudge_count,
                    longest_stretch_seconds,
                    median_stretch_seconds,
                    session_id,
                    neutral_seconds,
                    unrated_seconds,
                    phone_seconds
                ],
            )
            .map_err(|error| error.to_string())?;
        Ok(true)
    }

    pub fn list_history(&self, limit: usize) -> Result<Vec<SessionSummary>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "{SUMMARY_SELECT}
                 WHERE ended_at IS NOT NULL
                 ORDER BY started_at DESC
                 LIMIT ?1"
            ))
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([limit as i64], summary_from_row)
            .map_err(|error| error.to_string())?;
        let mut summaries = Vec::new();
        for row in rows {
            summaries.push(self.complete_summary(row.map_err(|error| error.to_string())?)?);
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
                "SELECT kind, activity_key, MAX(label), SUM(seconds) AS total
                 FROM span
                 WHERE session_id = ?1 AND state NOT IN ('paused', 'unknown')
                   AND NOT (kind = 'app' AND activity_key IN ('untracked', 'supervisor'))
                   AND state != 'idle'
                 GROUP BY kind, activity_key
                 ORDER BY total DESC, activity_key ASC
                 LIMIT 3",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([session_id], |row| {
                let kind: String = row.get(0)?;
                let key: String = row.get(1)?;
                let label: String = row.get(2)?;
                let total: i64 = row.get(3)?;
                Ok((self.display_label(&kind, &key, label), total))
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
        self.refresh_native_app_names()?;
        let row = self
            .conn
            .query_row(
                "SELECT goal, strictness, started_at, ended_at, ended_reason, planned_seconds,
                        focused_seconds, distracted_seconds, idle_seconds, away_seconds, phone_seconds,
                        drift_count, nudge_count, longest_stretch_seconds, median_stretch_seconds,
                        unrated_seconds
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
                        row.get::<_, i64>(15)?,
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
            unrated_seconds,
        ) = row;

        let duration_seconds = self.session_duration_seconds(id)?;
        let focus_percent = if duration_seconds == 0 {
            0
        } else {
            rounded_percent(focused_seconds as u64, duration_seconds)
        };

        let rules = self.scoring_rules()?;
        let spans = self.load_spans_for_session(id, &rules)?;
        let mut apps = apps_from_spans(&spans);
        self.attach_ratings(&mut apps, &rules);
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
            unrated_seconds: unrated_seconds as u64,
            distracted_seconds: distracted_seconds as u64,
            idle_seconds: idle_seconds as u64,
            away_seconds: away_seconds as u64,
            phone_seconds: phone_seconds as u64,
            drift_count: drift_count as u64,
            nudge_count: nudge_count as u64,
            longest_stretch_seconds: longest_stretch_seconds as u64,
            median_stretch_seconds: median_stretch_seconds as u64,
            spans,
            apps,
            events,
        })
    }

    pub fn get_trends(&self) -> Result<TrendsData, String> {
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
        let weeks = self.trend_weeks(monday)?;
        let full_weeks = weeks
            .iter()
            .filter(|week| week.session_count > 0 && week.week_start < format_day(monday))
            .count() as u64;
        Ok(TrendsData {
            daily_focus,
            weeks,
            full_weeks,
            week_recap: self.focus_recap(
                monday,
                monday.saturating_add(time::Duration::days(7)),
            )?,
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

    /// The Monday-to-Sunday week containing `day`.
    pub fn get_week_recap(&self, day: &str) -> Result<WeekRecap, String> {
        self.refresh_native_app_names()?;
        let start = monday_on_or_before(parse_day(day)?);
        let end_exclusive = start.saturating_add(time::Duration::days(7));
        let sessions = self.recap_sessions(&format_day(start), &format_day(end_exclusive))?;
        let mut by_day = std::collections::HashMap::<String, u64>::new();
        let mut stats = SpanStats::default();
        let mut longest_stretch_seconds = 0u64;
        let mut focus_seconds = 0u64;
        let mut all_spans = Vec::new();
        let rules = self.scoring_rules()?;

        for session in &sessions {
            focus_seconds += session.focused_seconds;
            longest_stretch_seconds = longest_stretch_seconds.max(session.longest_stretch_seconds);
            *by_day.entry(session.day.clone()).or_insert(0) += session.focused_seconds;
            let spans = self.load_spans_for_session(session.id, &rules)?;
            stats.add(&spans);
            all_spans.extend(spans);
        }
        let mut apps = apps_from_spans(&all_spans);
        self.attach_ratings(&mut apps, &rules);

        let previous = self.span_stats(start.saturating_sub(time::Duration::days(7)), start, &rules)?;

        Ok(WeekRecap {
            start: format_day(start),
            end: format_day(end_exclusive.saturating_sub(time::Duration::days(1))),
            focus_seconds,
            session_count: sessions.len() as u64,
            buckets: day_buckets(start, &by_day),
            median_stretch_seconds: median(&stats.stretches),
            longest_stretch_seconds,
            recovery_median_seconds: stats.recovery_median(),
            recovery_count: stats.recoveries.len() as u64,
            unrecovered_count: stats.unrecovered,
            previous_recovery_median_seconds: previous.recovery_median(),
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
                "SELECT id, started_at, focused_seconds, longest_stretch_seconds
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
                ))
            })
            .map_err(|error| error.to_string())?;

        let mut sessions = Vec::new();
        for row in rows {
            let (id, started_at, focused_seconds, longest) = row.map_err(|error| error.to_string())?;
            let Some(day) = local_day_in(&started_at, start_day, end_day) else {
                continue;
            };
            sessions.push(RecapSession {
                id: id as u64,
                day,
                focused_seconds: focused_seconds as u64,
                longest_stretch_seconds: longest as u64,
            });
        }
        Ok(sessions)
    }

    fn list_history_between(&self, start_day: &str, end_day: &str) -> Result<Vec<SessionSummary>, String> {
        let (from, to) = padded_day_bounds(start_day, end_day);
        let mut stmt = self
            .conn
            .prepare(&format!(
                "{SUMMARY_SELECT}
                 WHERE ended_at IS NOT NULL
                   AND substr(started_at, 1, 10) >= ?1
                   AND substr(started_at, 1, 10) < ?2
                 ORDER BY started_at DESC"
            ))
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map(params![from, to], summary_from_row)
            .map_err(|error| error.to_string())?;

        let mut summaries = Vec::new();
        for row in rows {
            let summary = row.map_err(|error| error.to_string())?;
            if local_day_in(&summary.started_at, start_day, end_day).is_none() {
                continue;
            }
            summaries.push(self.complete_summary(summary)?);
        }
        Ok(summaries)
    }

    fn complete_summary(&self, mut summary: SessionSummary) -> Result<SessionSummary, String> {
        summary.top_hints = self.top_hints_for_session(summary.id)?;
        summary.duration_seconds = self.session_duration_seconds(summary.id)?;
        Ok(summary)
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

    fn span_stats(
        &self,
        start: time::Date,
        end_exclusive: time::Date,
        rules: &[AppRule],
    ) -> Result<SpanStats, String> {
        let mut stats = SpanStats::default();
        for session in self.recap_sessions(&format_day(start), &format_day(end_exclusive))? {
            stats.add(&self.load_spans_for_session(session.id, rules)?);
        }
        Ok(stats)
    }

    fn trend_weeks(&self, monday: time::Date) -> Result<Vec<TrendWeek>, String> {
        let oldest = monday.saturating_sub(time::Duration::days(((TREND_WEEK_COUNT - 1) * 7) as i64));
        let mut weeks = Vec::with_capacity(TREND_WEEK_COUNT);
        let rules = self.scoring_rules()?;
        for index in 0..TREND_WEEK_COUNT {
            let start = oldest.saturating_add(time::Duration::days(index as i64 * 7));
            let end = start.saturating_add(time::Duration::days(7));
            let sessions = self.recap_sessions(&format_day(start), &format_day(end))?;
            let mut stats = SpanStats::default();
            for session in &sessions {
                stats.add(&self.load_spans_for_session(session.id, &rules)?);
            }
            weeks.push(TrendWeek {
                week_start: format_day(start),
                session_count: sessions.len() as u64,
                focus_seconds: sessions.iter().map(|session| session.focused_seconds).sum(),
                median_stretch_seconds: median(&stats.stretches),
                recovery_median_seconds: stats.recovery_median(),
            });
        }
        let first = weeks.iter().position(|week| week.session_count > 0);
        Ok(first.map(|index| weeks.split_off(index)).unwrap_or_default())
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

    /// Bytes the database uses; pages freed by deletes don't count, so this drops at once.
    pub fn history_bytes(&self) -> Result<u64, String> {
        self.conn
            .query_row(
                "SELECT (page_count - freelist_count) * page_size
                 FROM pragma_page_count(), pragma_freelist_count(), pragma_page_size()",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|bytes| bytes.max(0) as u64)
            .map_err(|error| error.to_string())
    }

    pub fn delete_all_history(&self) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM session WHERE ended_at IS NOT NULL", [])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    /// Spans with `rated_state` applied. Pass no rules when only the offsets matter.
    fn load_spans_for_session(&self, session_id: u64, rules: &[AppRule]) -> Result<Vec<ReportSpan>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT started_offset_seconds, seconds, app, kind, activity_key, label, source, state
                 FROM span
                 WHERE session_id = ?1
                 ORDER BY started_offset_seconds ASC",
            )
            .map_err(|error| error.to_string())?;
        let rows = stmt
            .query_map([session_id], |row| {
                let kind: String = row.get(3)?;
                let mut activity_key: String = row.get(4)?;
                let stored: String = row.get(5)?;
                let legacy_shell = kind == "app" && legacy_unidentified_window(&activity_key);
                if legacy_shell {
                    activity_key = "untracked".to_string();
                }
                let lost_time = kind == "app" && activity_key == UNKNOWN_KEY;
                let app: String = if legacy_shell { "untracked".to_string() } else { row.get(2)? };
                // Rules match the stored capture, never a display name the user chose.
                let state = if lost_time {
                    "unknown".to_string()
                } else {
                    rated_state(rules, &kind, &activity_key, &stored, &app, row.get(7)?)
                };
                let label = if lost_time {
                    UNKNOWN_LABEL.to_string()
                } else {
                    self.display_label(&kind, &activity_key, stored)
                };
                Ok(ReportSpan {
                    started_offset_seconds: row.get::<_, i64>(0)? as u64,
                    seconds: row.get::<_, i64>(1)? as u64,
                    app,
                    kind,
                    activity_key,
                    label,
                    source: if legacy_shell { "none".to_string() } else { row.get(6)? },
                    state,
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

/// The inverse of `focus_state_key`. `None` for markers that are not focus states
/// ("paused", "unknown") and for read-time states ("unrated").
pub fn focus_state_from_key(key: &str) -> Option<crate::focus::FocusState> {
    use crate::focus::FocusState;
    match key {
        "focused" => Some(FocusState::Focused),
        "neutral" => Some(FocusState::Neutral),
        "suspicious" => Some(FocusState::Suspicious),
        "distracted" => Some(FocusState::Distracted),
        "idle" => Some(FocusState::Idle),
        _ => None,
    }
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

/// Time recorded as neutral had no rule then (or a Neutral one), so today's rating decides it:
/// rating an app reaches its past unrated time. Relevant and distracting decisions made during a
/// session keep, since they may come from that session's own rules and it may have nudged.
fn rated_state(rules: &[AppRule], kind: &str, key: &str, label: &str, app: &str, state: String) -> String {
    if key == SUPERVISOR_KEY {
        // Supervisor's own window is never rated. Older sessions recorded it as focus.
        return if state == "focused" { "neutral".to_string() } else { state };
    }
    if state != "neutral" {
        return state;
    }
    let activity = Activity::from_stored(kind, key, label, app);
    match matching_rule(rules, &activity).map(|rule| rule.classification) {
        Some(RuleClassification::Focused) => "focused",
        Some(RuleClassification::Distracting) => "distracted",
        Some(RuleClassification::Neutral) => "neutral",
        None if ratable(kind, key) => "unrated",
        // Browsers and Other websites are always Neutral.
        None => "neutral",
    }
    .to_string()
}

/// Whether a rule can name exactly this row: a website, or an app that isn't a browser or one
/// of Supervisor's own markers.
fn ratable(kind: &str, key: &str) -> bool {
    match kind {
        "website" => true,
        "app" => ![SUPERVISOR_KEY, UNKNOWN_KEY, PAUSED_KEY, PHONE_KEY].contains(&key) && !website::is_browser(key),
        _ => false,
    }
}

/// Spans that are not drift: they extend a focus stretch and end a drift.
fn on_task(state: &str) -> bool {
    matches!(state, "focused" | "neutral" | "unrated")
}

/// Rows written by the old window-class fallback have no trustworthy app identity.
fn legacy_unidentified_window(key: &str) -> bool {
    [
        "xamlexplorerhostislandwindow",
        "shell_traywnd",
        "windows.ui.core.corewindow",
        "shellhost",
    ]
    .contains(&key.to_ascii_lowercase().as_str())
}

fn rounded_percent(part: u64, total: u64) -> u64 {
    if total == 0 {
        0
    } else {
        (part * 100 + total / 2) / total
    }
}

fn day_buckets(monday: time::Date, by_day: &std::collections::HashMap<String, u64>) -> Vec<RecapBucket> {
    const LETTERS: [&str; 7] = ["M", "T", "W", "T", "F", "S", "S"];
    (0i64..7)
        .map(|index| {
            let day = monday.saturating_add(time::Duration::days(index));
            RecapBucket {
                label: LETTERS[index as usize].to_string(),
                focus_seconds: *by_day.get(&format_day(day)).unwrap_or(&0),
            }
        })
        .collect()
}

fn monday_on_or_before(date: time::Date) -> time::Date {
    date.saturating_sub(time::Duration::days(i64::from(
        date.weekday().number_days_from_monday(),
    )))
}

fn count_drifts(spans: &[ReportSpan]) -> u64 {
    let mut count = 0u64;
    let mut in_focus_track = false;
    let mut previous_end = 0;
    for span in spans {
        if span.started_offset_seconds > previous_end {
            in_focus_track = false;
        }
        previous_end = span.started_offset_seconds.saturating_add(span.seconds);
        let focus_like = on_task(&span.state);
        let drift_like = matches!(span.state.as_str(), "distracted" | "idle" | "suspicious" | "phone");
        if matches!(span.state.as_str(), "paused" | "unknown") {
            in_focus_track = false;
        } else if focus_like {
            in_focus_track = true;
        } else if drift_like && in_focus_track {
            count += 1;
            in_focus_track = false;
        }
    }
    count
}

/// Seconds from leaving focus to being back on a focused or neutral app, one entry per drift,
/// plus whether a drift was still going at the end. Away and paused time neither starts nor
/// ends a drift and is not counted in it; a legacy hole forgets the drift.
fn drift_recoveries(spans: &[ReportSpan]) -> (Vec<u64>, u64) {
    let mut recoveries = Vec::new();
    let mut drifting: Option<u64> = None;
    let mut was_focused = false;
    let mut previous_end = 0;
    for span in spans {
        if span.started_offset_seconds > previous_end {
            drifting = None;
            was_focused = false;
        }
        previous_end = span.started_offset_seconds.saturating_add(span.seconds);
        match span.state.as_str() {
            state if on_task(state) => {
                if let Some(seconds) = drifting.take() {
                    recoveries.push(seconds);
                }
                was_focused = true;
            }
            "suspicious" | "distracted" | "phone" => match drifting.as_mut() {
                Some(seconds) => *seconds += span.seconds,
                None if was_focused => drifting = Some(span.seconds),
                None => {}
            },
            _ => {}
        }
    }
    (recoveries, u64::from(drifting.is_some()))
}

fn focused_stretches(spans: &[ReportSpan]) -> Vec<u64> {
    let mut stretches = Vec::new();
    let mut current = 0u64;
    let mut previous_end = 0;
    for span in spans {
        if span.started_offset_seconds > previous_end && current > 0 {
            stretches.push(current);
            current = 0;
        }
        previous_end = span.started_offset_seconds.saturating_add(span.seconds);
        match span.state.as_str() {
            state if on_task(state) => current += span.seconds,
            "distracted" | "idle" | "suspicious" | "phone" | "paused" | "unknown" => {
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
    if !value.is_ascii() || value.len() != 10 || value.as_bytes().get(4) != Some(&b'-') || value.as_bytes().get(7) != Some(&b'-') {
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

static STARTUP_OFFSET: std::sync::OnceLock<time::UtcOffset> = std::sync::OnceLock::new();

/// Call before any thread starts. On macOS/Linux `time` refuses to read the local offset from
/// a multi-threaded process, and history days would silently fall back to UTC.
pub fn capture_local_offset() {
    if let Ok(offset) = time::UtcOffset::current_local_offset() {
        let _ = STARTUP_OFFSET.set(offset);
    }
}

/// Per-instant offset where the platform allows it (DST-correct); otherwise the startup one.
fn local_offset_at(instant: time::OffsetDateTime) -> time::UtcOffset {
    time::UtcOffset::local_offset_at(instant)
        .ok()
        .or_else(|| STARTUP_OFFSET.get().copied())
        .unwrap_or(time::UtcOffset::UTC)
}

fn local_today() -> time::Date {
    let now = time::OffsetDateTime::now_utc();
    now.to_offset(local_offset_at(now)).date()
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
    calendar_day(instant, local_offset_at(instant))
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

/// Time per app/site and group. Groups follow span states, so they sum to the scores.
fn apps_from_spans(spans: &[ReportSpan]) -> Vec<RecapApp> {
    const GROUP_ORDER: [&str; 5] = ["Focus", "Neutral", "Distracting", "Unclassified", "Away"];
    let mut totals = std::collections::HashMap::<(&'static str, &str, &str), (&str, u64)>::new();
    for span in spans.iter().filter(|span| span.state != "paused") {
        let group = match span.state.as_str() {
            "focused" => "Focus",
            "neutral" => "Neutral",
            "unrated" => "Unclassified",
            "suspicious" | "distracted" | "phone" => "Distracting",
            _ => "Away",
        };
        totals
            .entry((group, &span.kind, &span.activity_key))
            .or_insert((&span.label, 0))
            .1 += span.seconds;
    }
    let mut apps: Vec<RecapApp> = totals
        .into_iter()
        .map(|((group, kind, key), (label, seconds))| RecapApp {
            group: group.to_string(),
            kind: kind.to_string(),
            key: key.to_string(),
            label: label.to_string(),
            seconds,
            ratable: false,
            rating: None,
        })
        .collect();
    let rank = |group: &str| GROUP_ORDER.iter().position(|g| *g == group).unwrap_or(9);
    apps.sort_by(|a, b| {
        rank(&a.group)
            .cmp(&rank(&b.group))
            .then_with(|| b.seconds.cmp(&a.seconds))
            .then_with(|| a.label.cmp(&b.label))
    });
    apps
}

const SUMMARY_SELECT: &str = "SELECT id, goal, started_at, focused_seconds, neutral_seconds,
    unrated_seconds, distracted_seconds, idle_seconds, nudge_count FROM session";

/// Reads a `SUMMARY_SELECT` row; `complete_summary` adds hints and run time.
fn summary_from_row(row: &rusqlite::Row) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        id: row.get::<_, i64>(0)? as u64,
        goal: row.get(1)?,
        started_at: row.get(2)?,
        duration_seconds: 0,
        focus_seconds: row.get::<_, i64>(3)? as u64,
        neutral_seconds: row.get::<_, i64>(4)? as u64,
        unrated_seconds: row.get::<_, i64>(5)? as u64,
        distracted_seconds: row.get::<_, i64>(6)? as u64,
        idle_seconds: row.get::<_, i64>(7)? as u64,
        nudges: row.get::<_, i64>(8)? as u64,
        top_hints: Vec::new(),
    })
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
        RuleTarget::App => "app",
        RuleTarget::Website => "website",
    }
}

fn parse_rule_target(value: String) -> RuleTarget {
    match value.as_str() {
        "website" => RuleTarget::Website,
        _ => RuleTarget::App,
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
        hidden_phrases: clean_block_suggestions(&library.hidden_phrases),
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

    fn app(name: &str) -> Activity {
        Activity::app(name)
    }

    #[test]
    fn rule_saves_report_only_sessions_whose_scores_changed() {
        let storage = SessionStorage::open_in_memory().unwrap();
        storage.replace_rules(&[]).unwrap();
        let notion = storage.insert_session("A", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(notion, &app("Notion"), "neutral", 0, 60).unwrap();
        storage.close_session(notion, "2026-09-22T12:01:00Z", "stopped", 60).unwrap();
        let code = storage.insert_session("B", Strictness::Normal, "2026-09-22T13:00:00Z", 600).unwrap();
        storage.insert_span(code, &app("Code"), "focused", 0, 60).unwrap();
        storage.close_session(code, "2026-09-22T13:01:00Z", "stopped", 60).unwrap();

        let rules = [AppRule::app("notion", RuleClassification::Focused)];
        assert_eq!(storage.replace_rules(&rules).unwrap(), vec![notion]);
        assert_eq!(storage.replace_rules(&rules).unwrap(), Vec::<u64>::new());
    }

    #[test]
    fn supervisor_time_recorded_as_focus_reads_as_neutral() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Old", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(id, &app("Code"), "focused", 0, 60).unwrap();
        storage.insert_span(id, &Activity::supervisor(), "focused", 60, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:02:00Z", "stopped", 120).unwrap();

        let report = storage.get_session_report(id).unwrap();
        assert_eq!(report.focused_seconds, 60);
        assert_eq!(report.focus_percent, 50);
        assert_eq!(report.spans[1].state, "neutral");
    }

    #[test]
    fn history_size_drops_when_history_is_deleted() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Size", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        for i in 0..2000 {
            storage.insert_span(id, &app("Code"), "focused", i, 1).unwrap();
        }
        storage.close_session(id, "2026-09-22T12:40:00Z", "stopped", 2000).unwrap();
        let full = storage.history_bytes().unwrap();
        storage.delete_all_history().unwrap();
        assert!(storage.history_bytes().unwrap() < full);
    }

    #[test]
    fn version_nine_folds_browser_pages_into_the_browser() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Pages", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(id, &app("chrome"), "neutral", 0, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:01:00Z", "stopped", 60).unwrap();
        storage.conn.execute_batch(
            "UPDATE span SET kind = 'browser_pages', label = 'Google Chrome pages';
             INSERT INTO span (session_id, app, kind, activity_key, label, source, category, state,
                               started_offset_seconds, seconds)
               SELECT session_id, app, 'browser_local', activity_key, 'Google Chrome local pages',
                      source, '', state, 60, 30 FROM span;
             PRAGMA user_version = 8;",
        ).unwrap();
        let reopened = SessionStorage::from_connection(storage.conn).unwrap();
        let spans = reopened.load_spans_for_session(id, &[]).unwrap();
        assert!(spans.iter().all(|span| span.kind == "app" && span.activity_key == "chrome"));
        assert!(spans.iter().all(|span| span.label == "Google Chrome"));
    }

    #[test]
    fn browsers_are_neutral_and_no_rule_reaches_them() {
        let storage = SessionStorage::open_in_memory().unwrap();
        storage.replace_rules(&[AppRule::app("chrome", RuleClassification::Distracting)]).unwrap();
        let id = storage.insert_session("Browse", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(id, &app("chrome"), "neutral", 0, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:01:00Z", "stopped", 60).unwrap();

        let report = storage.get_session_report(id).unwrap();
        assert_eq!(report.spans[0].state, "neutral");
        assert_eq!(report.unrated_seconds, 0);
        assert!(!report.apps[0].ratable);
    }

    #[test]
    fn deleting_all_rules_does_not_restore_defaults() {
        let storage = SessionStorage::open_in_memory().unwrap();
        assert!(!storage.load_rules().unwrap().is_empty());
        storage.replace_rules(&[]).unwrap();
        assert!(storage.load_rules().unwrap().is_empty());
    }

    #[test]
    fn breaks_split_stretches_and_later_activity_stays_in_recap_halves() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Break", Strictness::Normal, "2026-09-22T12:00:00Z", 300).unwrap();
        storage.insert_span(id, &app("Editor"), "focused", 0, 60).unwrap();
        storage.insert_span(id, &app("Editor"), "focused", 180, 60).unwrap();
        let mut spans = storage.load_spans_for_session(id, &[]).unwrap();
        assert_eq!(focused_stretches(&spans), vec![60, 60]);
        spans[1].state = "distracted".into();
        assert_eq!(count_drifts(&spans), 0);
        // A legacy hole forgets the drift instead of guessing how it ended.
        assert_eq!(drift_recoveries(&spans), (vec![], 0));
    }

    #[test]
    fn recovery_counts_only_drifting_time_until_back_on_task() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Drift", Strictness::Normal, "2026-09-22T12:00:00Z", 900).unwrap();
        for (activity, state, start, seconds) in [
            (site("youtube.com"), "distracted", 0, 60), // before any focus: not a drift
            (app("Code"), "focused", 60, 120),
            (site("youtube.com"), "suspicious", 180, 20),
            (site("youtube.com"), "distracted", 200, 40),
            (Activity::paused(), "paused", 240, 300), // a pause is not counted
            (site("youtube.com"), "distracted", 540, 30),
            (app("Notes"), "neutral", 570, 60),
            (site("reddit.com"), "distracted", 630, 90), // still drifting at the end
        ] {
            storage.insert_span(id, &activity, state, start, seconds).unwrap();
        }
        let spans = storage.load_spans_for_session(id, &[]).unwrap();
        assert_eq!(drift_recoveries(&spans), (vec![90], 1));
    }

    #[test]
    fn version_seven_rescores_existing_history() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Break", Strictness::Normal, "2026-09-22T12:00:00Z", 300).unwrap();
        storage.insert_span(id, &app("Editor"), "focused", 0, 60).unwrap();
        storage.insert_span(id, &app("Editor"), "focused", 180, 60).unwrap();
        storage.insert_span(id, &app("Notes"), "neutral", 240, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:05:00Z", "stopped", 180).unwrap();
        // A v6 database: neutral counted as focus, and stretches ran across pauses.
        storage.conn.execute_batch(
            "UPDATE session SET focused_seconds = 180, longest_stretch_seconds = 120, median_stretch_seconds = 120;
             ALTER TABLE session DROP COLUMN neutral_seconds;
             ALTER TABLE session DROP COLUMN unrated_seconds;
             PRAGMA user_version = 6;",
        ).unwrap();
        let reopened = SessionStorage::from_connection(storage.conn).unwrap();
        let report = reopened.get_session_report(id).unwrap();
        assert_eq!(report.focused_seconds, 120);
        assert_eq!(report.unrated_seconds, 60);
        assert_eq!(report.longest_stretch_seconds, 120);
        assert_eq!(report.median_stretch_seconds, 90);
    }

    #[test]
    fn rating_an_app_rescores_only_its_unrated_time() {
        let storage = SessionStorage::open_in_memory().unwrap();
        storage.replace_rules(&[]).unwrap();
        let id = storage.insert_session("Rate", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(id, &app("Notion"), "neutral", 0, 120).unwrap();
        // Decided during the session (say by a session rule): the decision keeps.
        storage.insert_span(id, &app("Notion"), "distracted", 120, 60).unwrap();
        storage.insert_span(id, &app("Code"), "focused", 180, 120).unwrap();
        storage.insert_span(id, &Activity::website("chrome", "example.com"), "neutral", 300, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:06:00Z", "stopped", 360).unwrap();

        let before = storage.get_session_report(id).unwrap();
        assert_eq!((before.focused_seconds, before.unrated_seconds, before.focus_percent), (120, 180, 33));

        storage.replace_rules(&[AppRule::app("notion", RuleClassification::Focused)]).unwrap();
        let after = storage.get_session_report(id).unwrap();
        assert_eq!((after.focused_seconds, after.unrated_seconds, after.focus_percent), (240, 60, 67));
        assert_eq!(after.distracted_seconds, 60);
        assert_eq!(storage.list_history(10).unwrap()[0].focus_seconds, 240);

        storage.replace_rules(&[AppRule::website("example.com", RuleClassification::Neutral)]).unwrap();
        let neutral = storage.get_session_report(id).unwrap();
        // Notion's rule is gone, so its untouched past time is unrated again.
        assert_eq!((neutral.focused_seconds, neutral.unrated_seconds), (120, 120));
        assert_eq!(storage.list_history(10).unwrap()[0].neutral_seconds, 60);
    }

    #[test]
    fn unrated_time_does_not_break_a_stretch_but_a_distracting_rating_does() {
        let storage = SessionStorage::open_in_memory().unwrap();
        storage.replace_rules(&[]).unwrap();
        let id = storage.insert_session("Stretch", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(id, &app("Code"), "focused", 0, 120).unwrap();
        storage.insert_span(id, &app("Slack"), "neutral", 120, 30).unwrap();
        storage.insert_span(id, &app("Code"), "focused", 150, 120).unwrap();
        storage.close_session(id, "2026-09-22T12:04:30Z", "stopped", 270).unwrap();
        let unrated = storage.get_session_report(id).unwrap();
        assert_eq!((unrated.longest_stretch_seconds, unrated.drift_count), (270, 0));

        storage.replace_rules(&[AppRule::app("slack", RuleClassification::Distracting)]).unwrap();
        let rated = storage.get_session_report(id).unwrap();
        assert_eq!((rated.longest_stretch_seconds, rated.drift_count), (120, 1));
    }

    fn app_span(key: &str, label: &str) -> Activity {
        let mut activity = Activity::app(label);
        activity.key = key.to_string();
        activity.app_id = key.to_string();
        activity
    }

    #[test]
    fn version_eleven_maps_name_rules_to_keys_only_when_unambiguous() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let id = storage.insert_session("Old", Strictness::Normal, "2026-09-22T12:00:00Z", 600).unwrap();
        storage.insert_span(id, &app_span("code", "Visual Studio Code"), "neutral", 0, 60).unwrap();
        storage.insert_span(id, &app_span("gl", "Game Launcher"), "neutral", 60, 60).unwrap();
        storage.insert_span(id, &app_span("gamelauncher", "Game Launcher"), "neutral", 120, 60).unwrap();
        storage.insert_span(id, &app_span("notes", "Notes"), "neutral", 180, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:04:00Z", "stopped", 240).unwrap();
        storage.replace_rules(&[
            AppRule::app("Visual Studio Code", RuleClassification::Focused),
            AppRule::app("Game Launcher", RuleClassification::Distracting),
            AppRule::app("Notes", RuleClassification::Focused),
            AppRule::app("Spotify", RuleClassification::Distracting),
            // Mapped onto the same app as the first rule, which always won.
            AppRule::app("code", RuleClassification::Distracting),
        ]).unwrap();
        storage.conn.execute_batch("DROP TABLE app_identity; PRAGMA user_version = 10;").unwrap();

        let reopened = SessionStorage::from_connection(storage.conn).unwrap();
        let rules: Vec<(String, RuleClassification)> = reopened.read_rules().unwrap()
            .into_iter().map(|rule| (rule.value, rule.classification)).collect();
        assert_eq!(rules, vec![
            ("code".to_string(), RuleClassification::Focused),
            // Two recorded apps carry this name: neither is chosen for it.
            ("Game Launcher".to_string(), RuleClassification::Distracting),
            ("notes".to_string(), RuleClassification::Focused),
            // No recorded app has this name yet; it still waits for one.
            ("Spotify".to_string(), RuleClassification::Distracting),
        ]);
        // History is untouched and still rated by the same rules.
        let report = reopened.get_session_report(id).unwrap();
        let keys: Vec<&str> = report.spans.iter().map(|span| span.activity_key.as_str()).collect();
        assert_eq!(keys, ["code", "gl", "gamelauncher", "notes"]);
        assert_eq!((report.focused_seconds, report.distracted_seconds), (120, 120));
    }

    #[test]
    fn app_identities_never_share_a_key_and_keep_it_across_restarts() {
        let storage = SessionStorage::open_in_memory().unwrap();
        let alpha = "exe:app|alpha ltd|alpha";
        let beta = "exe:app|beta inc|beta";
        assert_eq!(storage.app_key(alpha, "App", "Alpha"), "app");
        assert_eq!(storage.app_key(beta, "App", "Beta"), "app#2");
        assert_eq!(storage.app_key(alpha, "App", "Alpha 2"), "app");

        let reopened = SessionStorage::from_connection(storage.conn).unwrap();
        assert_eq!(reopened.app_key(beta, "App", "Beta"), "app#2");
        assert_eq!(reopened.app_key(alpha, "App", "Alpha 2"), "app");
        assert_eq!(reopened.app_key("path:c:\\gamma\\app.exe", "app", "App"), "app#3");
        let names = reopened.app_names().unwrap();
        assert_eq!((names["app"].as_str(), names["app#2"].as_str()), ("Alpha 2", "Beta (2)"));
        assert_eq!(reopened.app_label("app#3", "App"), "App (3)");
    }

    #[test]
    fn an_identity_takes_over_old_history_only_when_its_name_fits() {
        let storage = SessionStorage::open_in_memory().unwrap();
        storage.replace_rules(&[AppRule::app("tool", RuleClassification::Focused)]).unwrap();
        let id = storage.insert_session("Old", Strictness::Normal, "2026-09-22T12:00:00Z", 300).unwrap();
        storage.insert_span(id, &app_span("tool", "Old Tool"), "neutral", 0, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:01:00Z", "stopped", 60).unwrap();

        // A different program that happens to share the file name does not inherit it.
        assert_eq!(storage.app_key("exe:tool|new co|new tool", "tool", "New Tool"), "tool#2");
        // The program the history belongs to does, along with its rule.
        assert_eq!(storage.app_key("exe:tool|old co|old tool", "tool", "Old Tool"), "tool");
        assert_eq!(storage.get_session_report(id).unwrap().focused_seconds, 60);
        // Shared hosts never inherit: their history may be several apps.
        storage.insert_span(id, &app_span("javaw", "Java(TM) Platform SE binary"), "neutral", 60, 60).unwrap();
        assert_eq!(storage.app_key("exe:javaw|oracle|java|jar:a", "javaw", "Java(TM) Platform SE binary"), "javaw#2");
    }

    #[test]
    fn renaming_an_app_does_not_change_its_recap_rule_group() {
        let storage = SessionStorage::open_in_memory().unwrap();
        storage.replace_rules(&[
            AppRule::app("Draft Pad", RuleClassification::Focused),
            AppRule::app("Games", RuleClassification::Distracting),
        ]).unwrap();
        let id = storage.insert_session("Names", Strictness::Normal, "2026-09-22T12:00:00Z", 300).unwrap();
        let mut activity = app("Draft Pad");
        activity.key = "draftpadtest".into();
        activity.app_id = activity.key.clone();
        storage.insert_span(id, &activity, "focused", 0, 60).unwrap();
        storage.close_session(id, "2026-09-22T12:01:00Z", "stopped", 60).unwrap();
        storage.set_app_name(&activity.key, "Games").unwrap();
        let recap = storage.get_week_recap("2026-09-22").unwrap();
        assert_eq!(recap.apps[0].label, "Games");
        assert_eq!(recap.apps[0].group, "Focus");
    }

    #[test]
    fn invalid_unicode_dates_return_an_error() {
        assert!(parse_day("abcé-1-23").is_err());
    }

    fn site(key: &str) -> Activity {
        Activity::website("chrome", key)
    }

    fn insert_fixture_spans(storage: &SessionStorage, id: u64) {
        for (activity, state, start, seconds) in [
            (app("Code"), "focused", 0, 300),
            (site("youtube.com"), "distracted", 300, 120),
            (app("Code"), "neutral", 420, 180),
        ] {
            storage
                .insert_span(id, &activity, state, start, seconds)
                .expect("insert span");
        }
    }

    #[test]
    fn recompute_session_metrics_matches_fixture() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Ship storage", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        insert_fixture_spans(&storage, id);
        storage
            .insert_event(id, 310, "nudge", Some("voice"))
            .expect("insert event");
        storage
            .close_session(id, "2026-09-22T13:00:00Z", "completed", 600)
            .expect("close session");

        let report = storage.get_session_report(id).expect("report");
        assert_eq!(report.focused_seconds, 300);
        assert_eq!(report.unrated_seconds, 180);
        assert_eq!(report.distracted_seconds, 120);
        assert_eq!(report.drift_count, 1);
        assert_eq!(report.nudge_count, 1);
        assert_eq!(report.longest_stretch_seconds, 300);
        assert_eq!(report.median_stretch_seconds, 240);
        assert_eq!(report.focus_percent, 50);
    }

    #[test]
    fn site_names_rename_history_and_outlive_rules() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Names", Strictness::Normal, "2026-09-22T12:00:00Z", 600)
            .expect("insert session");
        storage
            .insert_span(id, &site("nytimes.com"), "neutral", 0, 60)
            .expect("insert span");
        storage
            .insert_span(id, &site("youtube.com"), "distracted", 60, 60)
            .expect("insert span");
        let labels = |storage: &SessionStorage| -> Vec<String> {
            storage
                .load_spans_for_session(id, &[])
                .expect("spans")
                .into_iter()
                .map(|span| span.label)
                .collect()
        };
        assert_eq!(labels(&storage), ["nytimes.com", "YouTube"]);

        storage.set_site_name("nytimes.com", "  NYT ").expect("name");
        storage.set_site_name("youtube.com", "Videos").expect("name");
        storage.replace_rules(&[]).expect("clear rules");
        assert_eq!(labels(&storage), ["NYT", "Videos"]);
        assert_eq!(storage.site_names().get("youtube.com").map(String::as_str), Some("Videos"));

        storage.set_site_name("youtube.com", "").expect("clear");
        assert_eq!(labels(&storage), ["NYT", "YouTube"]);
    }

    #[test]
    fn app_names_change_old_history_without_changing_identity() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage.insert_session("Names", Strictness::Normal, "2026-09-22T12:00:00Z", 600)
            .expect("insert session");
        storage.insert_span(id, &app("WhatsApp.Root"), "neutral", 0, 60)
            .expect("insert span");
        storage.set_app_name("whatsapp.root", "WhatsApp").expect("rename");
        let span = storage.load_spans_for_session(id, &[]).expect("spans").remove(0);
        assert_eq!(span.activity_key, "whatsapp.root");
        assert_eq!(span.label, "WhatsApp");
        storage.set_app_name("whatsapp.root", "").expect("reset");
        assert_eq!(storage.load_spans_for_session(id, &[]).expect("spans")[0].label, "WhatsApp.Root");
    }

    #[test]
    fn native_names_refresh_old_history_and_legacy_shell_rows_become_untracked() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage.insert_session("Names", Strictness::Normal, "2026-09-22T12:00:00Z", 600)
            .expect("insert session");
        storage.insert_span(id, &app("WhatsApp.Root"), "neutral", 0, 60)
            .expect("app span");
        storage.insert_span(id, &app("XamlExplorerHostIslandWindow"), "neutral", 60, 60)
            .expect("shell span");
        storage.native_app_names.borrow_mut().insert("whatsapp.root".into(), "WhatsApp".into());
        let spans = storage.load_spans_for_session(id, &[]).expect("spans");
        assert_eq!(spans[0].label, "WhatsApp");
        assert_eq!(spans[1].label, "Device asleep / unknown");
        assert_eq!(spans[1].activity_key, "untracked");
        assert_eq!(spans[1].source, "none");
        storage.set_app_name("whatsapp.root", "Messages").expect("custom name");
        assert_eq!(storage.load_spans_for_session(id, &[]).expect("spans")[0].label, "Messages");
    }

    #[test]
    fn newest_observed_app_name_relabels_older_spans() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage.insert_session("Names", Strictness::Normal, "2026-09-22T12:00:00Z", 600)
            .expect("insert session");
        storage.insert_span(id, &app("Draftpadtest"), "neutral", 0, 30)
            .expect("old span");
        let mut newer = app("Draftpadtest");
        newer.app_label = "Draft Pad".into();
        newer.label = "Draft Pad".into();
        storage.insert_span(id, &newer, "neutral", 30, 30)
            .expect("new span");
        storage.refresh_native_app_names().expect("refresh names");
        let spans = storage.load_spans_for_session(id, &[]).expect("spans");
        assert_eq!(spans[0].label, "Draft Pad");
        assert_eq!(spans[1].label, "Draft Pad");
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
            insert_fixture_spans(&storage, id);
            storage
                .close_session(id, "2026-09-22T12:30:00Z", "stopped", 1800)
                .expect("close session");
            id
        };

        let (storage, state) = SessionStorage::open(&path).expect("reopen db");
        let report = storage.get_session_report(id).expect("report");
        assert_eq!(report.focused_seconds, 300);
        assert_eq!(report.unrated_seconds, 180);
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
            .insert_span(id, &app("Code"), "focused", 0, 1500)
            .expect("insert span");
        storage
            .insert_span(id, &site("youtube.com"), "distracted", 1500, 300)
            .expect("insert span");
        storage
            .close_session(id, "2026-09-22T12:30:00Z", "stopped", 1800)
            .expect("close session");

        let trends = storage.get_trends().expect("trends");
        assert_eq!(trends.daily_focus.len(), 1);
        assert_eq!(trends.daily_focus[0].focus_percent, 83);
        assert_eq!(trends.daily_focus[0].focus_seconds, 1500);
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
    fn trend_weeks_start_at_first_data_week() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let empty = storage.get_trends().expect("empty trends");
        assert!(empty.weeks.is_empty());
        assert_eq!(empty.full_weeks, 0);

        let started = utc_now();
        let id = storage
            .insert_session("Now", Strictness::Normal, &started, 3600)
            .expect("insert session");
        storage
            .insert_span(id, &app("Code"), "focused", 0, 600)
            .expect("insert span");
        storage
            .close_session(id, &utc_now(), "stopped", 600)
            .expect("close session");

        let trends = storage.get_trends().expect("trends");
        assert_eq!(trends.weeks.len(), 1);
        assert_eq!(trends.weeks[0].session_count, 1);
        assert_eq!(trends.weeks[0].median_stretch_seconds, 600);
        // This week is not a full week yet.
        assert_eq!(trends.full_weeks, 0);
    }

    #[test]
    fn recap_uses_inclusive_start_and_exclusive_end() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let first = storage
            .insert_session("First", Strictness::Normal, "2026-09-01T08:00:00Z", 1800)
            .expect("insert first");
        storage
            .insert_span(first, &app("Code"), "focused", 0, 600)
            .expect("span");
        storage
            .close_session(first, "2026-09-01T08:10:00Z", "stopped", 600)
            .expect("close");
        let second = storage
            .insert_session("Second", Strictness::Normal, "2026-09-30T08:00:00Z", 1800)
            .expect("insert second");
        storage
            .insert_span(second, &app("Code"), "focused", 0, 900)
            .expect("span");
        storage
            .close_session(second, "2026-09-30T08:15:00Z", "stopped", 900)
            .expect("close");
        let october = storage
            .insert_session("October", Strictness::Normal, "2026-10-01T08:00:00Z", 1800)
            .expect("insert october");
        storage
            .insert_span(october, &app("Code"), "focused", 0, 120)
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
    fn week_recap_splits_days_and_reports_recovery() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Split", Strictness::Normal, "2026-09-23T12:00:00Z", 1200)
            .expect("insert");
        storage
            .insert_span(id, &app("Cursor"), "focused", 0, 600)
            .expect("first half");
        storage
            .insert_span(id, &site("example.com"), "distracted", 600, 600)
            .expect("second half");
        storage
            .close_session(id, "2026-09-23T12:20:00Z", "stopped", 1200)
            .expect("close");

        let recap = storage
            .get_week_recap("2026-09-23")
            .expect("week recap");
        assert_eq!(recap.start, "2026-09-21");
        assert_eq!(recap.end, "2026-09-27");
        assert_eq!(recap.session_count, 1);
        assert_eq!(recap.focus_seconds, 600);
        assert_eq!(recap.buckets.len(), 7);
        assert_eq!(recap.buckets[2].label, "W");
        assert_eq!(recap.buckets[2].focus_seconds, 600);
        assert_eq!(recap.buckets[0].focus_seconds, 0);
        assert_eq!(recap.recovery_median_seconds, None);
        assert_eq!(recap.unrecovered_count, 1);
        assert_eq!(
            recap
                .apps
                .iter()
                .map(|app| (app.group.as_str(), app.label.as_str(), app.seconds))
                .collect::<Vec<_>>(),
            vec![
                ("Focus", "Cursor", 600),
                ("Distracting", "example.com", 600),
            ]
        );
    }

    #[test]
    fn week_recap_groups_apps_like_their_scores() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        storage
            .replace_rules(&[
                AppRule::app("Cursor", RuleClassification::Focused),
                AppRule::app("Slack", RuleClassification::Neutral),
                AppRule::website("youtube.com", RuleClassification::Distracting),
            ])
            .expect("rules");
        let id = storage
            .insert_session("Grouped", Strictness::Normal, "2026-09-23T12:00:00Z", 250)
            .expect("insert");
        storage
            .insert_span(id, &app("Cursor"), "focused", 0, 100)
            .expect("focus");
        storage
            .insert_span(id, &app("Slack"), "neutral", 100, 50)
            .expect("neutral");
        storage
            .insert_span(id, &site("youtube.com"), "distracted", 150, 80)
            .expect("distract");
        storage
            .insert_span(id, &app("Notepad"), "neutral", 230, 20)
            .expect("misc");
        storage
            .close_session(id, "2026-09-23T12:04:10Z", "stopped", 250)
            .expect("close");

        let recap = storage
            .get_week_recap("2026-09-23")
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
                ("Distracting", "YouTube", 80),
                ("Unclassified", "Notepad", 20),
            ]
        );
    }

    #[test]
    fn history_week_keeps_sessions_on_that_monday_week() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let in_week = storage
            .insert_session("In week", Strictness::Normal, "2026-09-22T12:00:00Z", 1800)
            .expect("insert in week");
        storage
            .insert_span(in_week, &app("Code"), "focused", 0, 600)
            .expect("span");
        storage
            .close_session(in_week, "2026-09-22T12:10:00Z", "stopped", 600)
            .expect("close");
        let later = storage
            .insert_session("Later", Strictness::Normal, "2026-10-06T12:00:00Z", 1800)
            .expect("insert later");
        storage
            .insert_span(later, &app("Code"), "focused", 0, 300)
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
            .get_week_recap("2026-10-11")
            .expect("week of sunday");
        assert_eq!(recap.start, "2026-10-05");
        assert_eq!(recap.end, "2026-10-11");
        let history = storage
            .get_history_week("2026-10-11")
            .expect("history week");
        assert_eq!(history.week_start, "2026-10-05");
    }

    #[test]
    fn version_1_database_migrates_titles_out_and_keeps_history() {
        let dir = std::env::temp_dir().join(format!(
            "supervisor-storage-migrate-{}",
            std::process::id()
        ));
        let path = dir.join("sessions.db");
        let _ = std::fs::remove_dir_all(&dir);
        let id = {
            let (storage, _) = SessionStorage::open(&path).expect("open db");
            let id = storage
                .insert_session("Old", Strictness::Normal, "2026-09-01T12:00:00Z", 1500)
                .expect("insert session");
            storage
                .close_session(id, "2026-09-01T12:30:00Z", "stopped", 999)
                .expect("close session");
            // Rebuild the v1 layout: titles in `span.context`, title and path rules.
            storage
                .conn
                .execute_batch(&format!(
                    r"
                    ALTER TABLE span DROP COLUMN kind;
                    ALTER TABLE span DROP COLUMN activity_key;
                    ALTER TABLE span DROP COLUMN label;
                    ALTER TABLE span DROP COLUMN source;
                    ALTER TABLE span ADD COLUMN context TEXT;
                    ALTER TABLE session DROP COLUMN elapsed_seconds;
                    ALTER TABLE session DROP COLUMN neutral_seconds;
                    ALTER TABLE session DROP COLUMN unrated_seconds;
                    DROP TABLE site_name;
                    DROP TABLE app_name;
                    INSERT INTO span (session_id, app, context, category, state, started_offset_seconds, seconds)
                      VALUES ({id}, 'Code', 'main.rs', 'work', 'focused', 0, 420),
                             ({id}, 'chrome', 'Inbox - Gmail', 'browse', 'neutral', 420, 60);
                    DELETE FROM rule;
                    INSERT INTO rule (target, value, classification, source) VALUES
                      ('window_title', 'YouTube', 'distracting', 'starter'),
                      ('app_name', 'Slack', 'neutral', 'user'),
                      ('window_title', 'Watch Later - YouTube', 'distracting', 'user'),
                      ('process_path', 'C:\x.exe', 'focused', 'user'),
                      ('window_title', 'Notes', 'focused', 'user');
                    PRAGMA user_version = 1;
                    "
                ))
                .expect("downgrade to v1");
            id
        };

        let (storage, state) = SessionStorage::open(&path).expect("migrate db");
        let version: i32 = storage
            .conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .expect("version");
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].duration_seconds, 480);

        let spans: Vec<(String, String)> = storage
            .get_session_report(id)
            .expect("report")
            .spans
            .into_iter()
            .map(|span| (span.kind, span.label))
            .collect();
        assert_eq!(
            spans,
            vec![
                ("app".to_string(), "Code".to_string()),
                ("other_websites".to_string(), "Other websites".to_string()),
            ]
        );
        assert_eq!(
            storage.load_rules().expect("rules"),
            vec![
                AppRule::website("youtube.com", RuleClassification::Distracting),
                AppRule::app("Slack", RuleClassification::Neutral),
                AppRule::app("Notes", RuleClassification::Focused),
            ]
        );
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
    fn a_newer_database_is_preserved_instead_of_reset() {
        let path = std::env::temp_dir().join(format!("supervisor-newer-schema-{}.db", std::process::id()));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 999; CREATE TABLE IF NOT EXISTS keep_me (value TEXT);").unwrap();
        drop(conn);
        let (opened, notice) = open_or_reset(Ok(path.clone()));
        assert!(opened.is_none());
        assert!(matches!(notice, Some(StorageNotice::Off { .. })));
        let conn = Connection::open(&path).unwrap();
        assert_eq!(conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0)).unwrap(), 999);
        assert_eq!(conn.query_row("SELECT COUNT(*) FROM keep_me", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
        drop(conn);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn orphan_session_recovers_as_abandoned() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Crash", Strictness::Strict, "2026-09-22T08:00:00Z", 1500)
            .expect("insert session");
        storage
            .insert_span(id, &app("Code"), "focused", 0, 90)
            .expect("insert span");

        storage.recover_orphans().expect("recover");

        let report = storage.get_session_report(id).expect("report");
        assert_eq!(report.ended_reason.as_deref(), Some("abandoned"));
        assert!(report.ended_at.is_some());
        assert_eq!(report.focused_seconds, 90);
    }

    #[test]
    fn recovered_sessions_do_not_count_pauses_as_run_time() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Crash", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        storage.insert_span(id, &app("Code"), "focused", 0, 600).unwrap();
        storage.insert_span(id, &Activity::paused(), "paused", 600, 600).unwrap();
        storage.recover_orphans().unwrap();
        let report = storage.get_session_report(id).expect("report");

        assert_eq!(report.duration_seconds, 600);
        assert_eq!(report.focus_percent, 100);
    }

    #[test]
    fn recap_puts_away_time_in_away_not_the_app_rule() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        storage.replace_rules(&[AppRule::app("code", RuleClassification::Focused)]).unwrap();
        let id = storage
            .insert_session("Away", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        storage.insert_span(id, &app("Code"), "focused", 0, 300).unwrap();
        storage.insert_span(id, &app("Code"), "idle", 300, 600).unwrap();
        storage.close_session(id, "2026-09-22T12:15:00Z", "stopped", 900).unwrap();
        let apps = storage.get_week_recap("2026-09-22").unwrap().apps;
        let group = |name: &str| apps.iter().filter(|app| app.group == name).map(|app| app.seconds).sum::<u64>();

        assert_eq!(group("Focus"), 300);
        assert_eq!(group("Away"), 600);
    }

    #[test]
    fn report_rows_say_whether_and_how_they_are_rated() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Rate", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        storage.insert_span(id, &app("Snipping Tool"), "neutral", 0, 60).unwrap();
        storage.insert_span(id, &site("youtube.com"), "distracted", 60, 60).unwrap();
        storage.insert_span(id, &Activity::supervisor(), "focused", 120, 60).unwrap();
        let apps = storage.get_session_report(id).expect("report").apps;
        let row = |key: &str| apps.iter().find(|app| app.key == key).expect(key);

        assert!(row("snipping tool").ratable);
        assert_eq!(row("snipping tool").rating, None);
        assert_eq!(row("youtube.com").rating, Some(RuleClassification::Distracting));
        assert!(!row("supervisor").ratable);
    }

    #[test]
    fn session_report_groups_apps_like_the_timeline() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let id = storage
            .insert_session("Ship storage", Strictness::Normal, "2026-09-22T12:00:00Z", 3600)
            .expect("insert session");
        insert_fixture_spans(&storage, id);
        let rows: Vec<(String, String, u64)> = storage
            .get_session_report(id)
            .expect("report")
            .apps
            .into_iter()
            .map(|app| (app.group, app.key, app.seconds))
            .collect();
        assert_eq!(
            rows,
            vec![
                ("Focus".to_string(), "code".to_string(), 300),
                ("Distracting".to_string(), "youtube.com".to_string(), 120),
                ("Unclassified".to_string(), "code".to_string(), 180),
            ]
        );
    }

    #[test]
    fn deleted_bundled_phrases_round_trip_and_old_libraries_still_load() {
        let storage = SessionStorage::open_in_memory().expect("open db");
        let old: StoredPhraseLibrary = serde_json::from_str(
            r#"{"custom_tones":[],"custom_phrases":[],"phrase_overrides":[]}"#,
        ).expect("old shape");
        assert!(old.hidden_phrases.is_empty());
        let saved = storage
            .save_phrase_library(StoredPhraseLibrary {
                hidden_phrases: vec![" mom_again ".into(), "mom_again".into(), "".into()],
                ..old
            })
            .expect("save");
        assert_eq!(saved.hidden_phrases, vec!["mom_again"]);
        assert_eq!(storage.load_phrase_library().expect("load").hidden_phrases, vec!["mom_again"]);
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
            webcam_enabled: true,
        };
        storage.save_settings(&settings).expect("save settings");
        let loaded = storage.load_settings().expect("load settings");
        assert_eq!(loaded, settings);

        let rules = vec![
            AppRule::app("Code", RuleClassification::Focused),
            AppRule::website("github.com", RuleClassification::Focused),
        ];
        storage.replace_rules(&rules).expect("save rules");
        let loaded_rules = storage.load_rules().expect("load rules");
        assert_eq!(loaded_rules, rules);
    }

    #[test]
    fn focus_state_key_matches_schema() {
        assert_eq!(focus_state_key(FocusState::Suspicious), "suspicious");
    }
}
