import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type PermissionState = "granted" | "missing" | "not_required";
type RuleTarget = "app" | "website";
/** A browser's own pages (New Tab, settings, files) are "app": the browser itself. */
type ActivityKind = "app" | "website" | "other_websites";
type Strictness = "gentle" | "normal" | "strict";
type SessionStatus = "idle" | "active" | "paused" | "snoozed" | "complete";
type FocusState = "focused" | "suspicious" | "distracted" | "idle";
type Tab = "session" | "settings" | "history";
type AppView = "main" | "overlay";
type SettingsView = "home" | "voice" | "webcam";
type WebcamGaze = "at_pc" | "away_from_pc" | "at_phone";
type WebcamReaction = "alarm" | "voice";
type VoiceTone = string;
type VoiceSource = "none" | "starter" | "custom";
type VoiceBusy = "download-model" | "delete-model" | "generate-clips" | "delete-voice" | null;
/** Lines being made one after another for one voice; `index` is the line in progress. */
type ClipQueue = {
  voiceId: string;
  phraseIds: string[];
  index: number;
  phase: "working" | "error";
  error: string;
  startedAt: number;
  stopAfter: boolean;
};
type VoiceDownloadPhase = "idle" | "checking" | "downloading" | "cancelling" | "complete" | "error";
type RecordingState = "idle" | "recording" | "recorded" | "error";

interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface CapturePermissions {
  screen_recording: PermissionState;
  accessibility: PermissionState;
}

interface ActivitySample {
  captured_at: string;
  platform: string;
  app_name: string;
  app_label: string;
  process_path: string;
  process_id: number;
  window_id: string;
  bounds: Bounds;
  idle_seconds: number | null;
  permissions: CapturePermissions;
}

/** What is being supervised: an app, a website, or one of the browser's own rows. */
interface Activity {
  kind: ActivityKind;
  key: string;
  label: string;
}

/** A distraction: a website key ("youtube.com") or an app. */
interface AppRule {
  target: RuleTarget;
  value: string;
}

interface FocusSnapshot {
  sample: ActivitySample;
  activity: Activity;
  /** A website or a non-browser app: something a rule can name. */
  markable: boolean;
  state: FocusState;
  /** A distraction rule matched, and no exception for this session lifted it. */
  distracting: boolean;
  matched_rule: AppRule | null;
  seconds_on_current_window: number;
  should_nudge: boolean;
  next_nudge_in_seconds: number | null;
}

interface SessionSettings {
  default_strictness: Strictness;
  voice_profile: string;
  status_window_enabled: boolean;
  status_window_on_top: boolean;
  webcam_gpu: boolean;
  webcam_enabled: boolean;
  webcam_camera: number;
}

interface SessionMetrics {
  focused_seconds: number;
  distracted_seconds: number;
  idle_seconds: number;
  /** Part of distracted_seconds. */
  phone_seconds: number;
  nudges: number;
}

/** What the webcam says during a session. Only flags; frames never reach the session. */
interface CameraStatus {
  live: boolean;
  present: boolean;
  phone: boolean;
  phone_seconds: number;
  /** Past the strictness grace: the popup shows and the phone is nudged when due. */
  phone_distracted: boolean;
}

interface EvidenceEvent {
  at_seconds: number;
  label: string;
  value: string;
}

interface CurrentSession {
  id: number;
  goal: string;
  duration_seconds: number;
  elapsed_seconds: number;
  remaining_seconds: number;
  strictness: Strictness;
  status: SessionStatus;
  snoozed_until_seconds: number | null;
  metrics: SessionMetrics;
  evidence: EvidenceEvent[];
  focus: FocusSnapshot | null;
  /** Supervisor's own window is in front; `focus` still holds the window before it. */
  on_supervisor?: boolean;
  /** Distractions allowed for this session. */
  exceptions: AppRule[];
  camera: CameraStatus;
}

interface SessionSummary {
  id: number;
  goal: string;
  started_at: string;
  duration_seconds: number;
  /** Time not on a distraction. */
  focus_seconds: number;
  distracted_seconds: number;
  idle_seconds: number;
  nudges: number;
  top_hints: string[];
  /** The session in order: runs of one state, with gaps where nothing was recorded. */
  segments: SummarySegment[];
}

interface SummarySegment {
  started_offset_seconds: number;
  seconds: number;
  state: string;
}

interface ReportSpan {
  started_offset_seconds: number;
  seconds: number;
  app: string;
  kind: ActivityKind;
  activity_key: string;
  label: string;
  state: string;
}

interface ReportEvent {
  at_offset_seconds: number;
  kind: string;
  detail: string | null;
}

interface SessionReport {
  id: number;
  goal: string;
  strictness: string;
  started_at: string;
  ended_at: string | null;
  ended_reason: string | null;
  planned_seconds: number;
  duration_seconds: number;
  focus_percent: number;
  focused_seconds: number;
  distracted_seconds: number;
  idle_seconds: number;
  away_seconds: number;
  phone_seconds: number;
  drift_count: number;
  nudge_count: number;
  longest_stretch_seconds: number;
  median_stretch_seconds: number;
  spans: ReportSpan[];
  apps: RecapApp[];
  events: ReportEvent[];
}

interface DailyFocus {
  date: string;
  focus_percent: number;
  focus_seconds: number;
  session_count: number;
}

interface TrendWeek {
  week_start: string;
  session_count: number;
  focus_seconds: number;
  median_stretch_seconds: number;
  recovery_median_seconds: number | null;
}

interface FocusRecap {
  start: string;
  end: string;
  focus_seconds: number;
  session_count: number;
}

interface HistoryWeek {
  week_start: string;
  sessions: SessionSummary[];
  oldest_day: string | null;
}

interface TrendsData {
  daily_focus: DailyFocus[];
  /** Oldest first, ending with the current week. */
  weeks: TrendWeek[];
  full_weeks: number;
  week_recap: FocusRecap;
}

type HistoryView = "list" | "report" | "trends";

/** Trends need this many completed weeks before week-over-week lines mean anything. */
const TRENDS_UNLOCK_WEEKS = 2;

interface RecapBucket {
  label: string;
  focus_seconds: number;
}

interface RecapApp {
  group: string;
  kind: ActivityKind;
  key: string;
  label: string;
  seconds: number;
}

interface WeekRecap {
  start: string;
  end: string;
  focus_seconds: number;
  session_count: number;
  buckets: RecapBucket[];
  median_stretch_seconds: number;
  longest_stretch_seconds: number;
  recovery_median_seconds: number | null;
  recovery_count: number;
  unrecovered_count: number;
  previous_recovery_median_seconds: number | null;
  apps: RecapApp[];
}

interface HistoryConfirm {
  kind: "delete-all" | "delete-session";
  id?: number;
  /** History's size on disk, for the delete-all prompt. `null` when it couldn't be read. */
  bytes?: number | null;
}

/** A few seconds of burst photos on a phone. History is a sliver of that. */
const PHOTO_BURST_BYTES = 30 * 1024 ** 2;


interface SessionSnapshot {
  status: SessionStatus;
  settings: SessionSettings;
  current: CurrentSession | null;
  history: SessionSummary[];
}

interface RulesSaved {
  rules: AppRule[];
}

interface RuleEditor {
  index: number | null;
  target: RuleTarget;
  value: string;
  name: string;
  nameTouched: boolean;
  preview: RulePreview;
}

type RulePreview = { target: RuleTarget; value: string } | { error: string } | null;

interface VoiceModelFileStatus {
  path: string;
  bytes: number;
  sha256: string;
  installed: boolean;
  valid: boolean;
}

interface VoiceModelStatus {
  id: string;
  display_name: string;
  repo: string;
  revision: string;
  install_dir: string;
  installed: boolean;
  installed_bytes: number;
  required_bytes: number;
  recommended_free_bytes: number;
  files: VoiceModelFileStatus[];
}

interface VoiceRuntimeStatus {
  target: string;
  install_dir: string;
  installed: boolean;
  executable_path: string | null;
  version: string | null;
  worker_api: number | null;
  source: "release" | "dev_override" | "missing";
  required_bytes: number | null;
  installed_bytes: number;
}

interface VoiceSystemInfo {
  os: string;
  arch: string;
  cpu_threads: number;
  total_ram_bytes: number | null;
  available_ram_bytes: number | null;
  min_required_ram_bytes: number;
  min_free_ram_bytes: number;
  last_peak_ram_bytes: number | null;
  has_enough_ram: boolean | null;
  available_disk_bytes: number | null;
  recommended_free_disk_bytes: number;
  has_enough_disk: boolean | null;
}

interface VoiceClipSummary {
  clip_id: string;
  phrase_id: string;
  file_name: string;
  path: string;
  bytes: number;
}

interface VoiceProfileSummary {
  profile_id: string;
  display_name: string;
  profile_dir: string;
  reference_path: string | null;
  reference_retained: boolean;
  /** The tone this voice speaks in sessions; null until the user picks one. */
  tone: string | null;
  clip_count: number;
  clips: VoiceClipSummary[];
}

interface VoiceBackendStatus {
  model: VoiceModelStatus;
  runtime: VoiceRuntimeStatus;
  system: VoiceSystemInfo;
  profiles: VoiceProfileSummary[];
}

interface VoiceDownloadSnapshot {
  phase: VoiceDownloadPhase;
  downloaded_bytes: number;
  total_bytes: number;
  current_file: string | null;
  elapsed_seconds: number;
  eta_seconds: number | null;
  message: string;
  up_to_date: boolean;
  model: VoiceModelStatus;
  runtime: VoiceRuntimeStatus;
}

interface VoiceOption {
  id: string;
  name: string;
  kind: string;
  detail: string;
  coverage: string;
  source: VoiceSource;
  selectable: boolean;
  profile: VoiceProfileSummary | null;
}

interface PhraseDefinition {
  id: string;
  tone: VoiceTone;
  text: string;
}

interface PhraseEditor {
  phraseId: string | null;
  tone: VoiceTone;
  text: string;
}

interface PhraseLibraryState {
  custom_tones: string[];
  custom_phrases: PhraseDefinition[];
  phrase_overrides: PhraseDefinition[];
  /** Bundled phrases the user deleted. */
  hidden_phrases: string[];
}

interface RecordedVoice {
  path: string;
  url: string;
  bytes: number;
  seconds: number;
  name: string;
}

interface WebcamFrame {
  face_in_frame: boolean;
  gaze: WebcamGaze | null;
  person_in_frame?: boolean;
  present?: boolean;
  phone_in_frame: boolean;
  looking_at_phone: boolean;
  phone_count: number;
  phone_confidence?: number;
  jpeg?: string;
  device?: string;
  provider?: string;
  error?: string | null;
  /** False on a preview picture sent between detections; its labels are the last detection's. */
  detected?: boolean;
}

interface VoiceRecordingSnapshot {
  phase: RecordingState;
  path: string | null;
  bytes: number;
  duration_seconds: number;
  elapsed_seconds: number;
  sample_rate: number | null;
  message: string;
}

const app = getAppRoot();
const appView: AppView = new URLSearchParams(window.location.search).get("view") === "overlay" ? "overlay" : "main";
document.body.classList.toggle("overlay-view", appView === "overlay");

let theme: "light" | "dark" = localStorage.getItem("supervisor-theme") === "dark" ? "dark" : "light";

function applyTheme(): void {
  if (theme === "dark") {
    document.documentElement.dataset.theme = "dark";
  } else {
    delete document.documentElement.dataset.theme;
  }
}

applyTheme();
window.addEventListener("storage", (event) => {
  if (event.key !== "supervisor-theme") {
    return;
  }
  theme = event.newValue === "dark" ? "dark" : "light";
  applyTheme();
});
window.addEventListener("resize", fitQuote);
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    closeTopLayer();
    return;
  }
});

const starterRules: AppRule[] = ["youtube.com", "netflix.com", "tiktok.com", "instagram.com", "reddit.com"].map(
  (value) => ({
    target: "website",
    value,
    classification: "distracting",
  }),
);

const toneOrder: VoiceTone[] = ["gentle", "mom", "genz", "stern"];
/** The editor's "New set…" choice. */

const phraseBank: PhraseDefinition[] = [
  { id: "gentle_back_to_it", tone: "gentle", text: "Hey. Back to it." },
  { id: "gentle_that_can_wait", tone: "gentle", text: "That can wait. This can't." },
  { id: "gentle_you_drifted", tone: "gentle", text: "You drifted. It happens. Come back." },
  { id: "gentle_one_more_push", tone: "gentle", text: "One more push on the real thing." },
  { id: "gentle_pick_up_where", tone: "gentle", text: "Close that, and pick up where you were." },
  { id: "mom_i_can_see", tone: "mom", text: "Beta, I can see the screen." },
  { id: "mom_what_is_this", tone: "mom", text: "Arre, what is this now? Finish your work first." },
  { id: "mom_again", tone: "mom", text: "Again? Close it. Then we'll talk." },
  { id: "mom_did_you_finish", tone: "mom", text: "Did you finish? No? Then why is this open." },
  { id: "mom_two_minutes", tone: "mom", text: "Two minutes becomes two hours. I know you." },
  { id: "genz_bro_the_tab", tone: "genz", text: "Bro. The tab." },
  { id: "genz_not_the_assignment", tone: "genz", text: "That's not the assignment." },
  { id: "genz_lock_in", tone: "genz", text: "Lock in. Like, now." },
  { id: "genz_why_behind", tone: "genz", text: "This is why you're behind." },
  { id: "genz_not_today", tone: "genz", text: "We're not doing this today. Back." },
  { id: "stern_not_the_task", tone: "stern", text: "This isn't the task." },
  { id: "stern_close_it_now", tone: "stern", text: "Close it. Now." },
  { id: "stern_you_picked_this", tone: "stern", text: "You picked this. Finish it." },
  { id: "stern_no_negotiating", tone: "stern", text: "Back to work. No negotiating." },
  { id: "stern_want_it_back", tone: "stern", text: "Every minute here is one you'll want back." },
];

const noneVoice: VoiceOption = {
  id: "none",
  name: "No voice",
  kind: "",
  detail: "",
  coverage: "",
  source: "none",
  selectable: true,
  profile: null,
};

const STARTER_PROFILE_IDS = ["derek", "elise"] as const;

function starterVoiceOptions(): VoiceOption[] {
  return [
    starterVoiceCard("starter-male", "derek", "Starter Male", "Low-friction voice for first-run sessions."),
    starterVoiceCard("starter-female", "elise", "Starter Female", "Warmer starter voice with the same phrase bank."),
  ];
}

function starterVoiceCard(id: string, profileId: string, name: string, detail: string): VoiceOption {
  const profile = profileById(profileId);
  const phrases = effectivePhrases();
  const ready = phrases.filter((phrase) => clipForPhraseId(profile?.clips ?? [], phrase.id)).length;
  return {
    id,
    name,
    kind: "Bundled",
    detail,
    coverage: `${ready}/${phrases.length} clips`,
    source: "starter",
    selectable: true,
    profile,
  };
}

const draft = {
  goal: "",
  duration_minutes: 25,
  strictness: "normal" as Strictness,
  voiceId: "starter-female",
};

let activeTab: Tab = "session";
let settingsView: SettingsView = "home";
let snapshot: SessionSnapshot | null = null;
let settings: SessionSettings = {
  default_strictness: "normal",
  voice_profile: "Starter voice",
  status_window_enabled: true,
  status_window_on_top: true,
  webcam_gpu: false,
  webcam_enabled: false,
  webcam_camera: 0,
};
let history: SessionSummary[] = [];
const EMPTY_RECAP: FocusRecap = { start: "", end: "", focus_seconds: 0, session_count: 0 };
/** Order of the groups `apps_from_spans` produces; they sum to the scores. */
const APP_GROUPS = ["Distractions", "Work", "Away"];
/** How the groups and scores are decided, under every Apps & sites list. */
let trends: TrendsData = {
  daily_focus: [],
  weeks: [],
  full_weeks: 0,
  week_recap: EMPTY_RECAP,
};
let historyWeekStart = "";
/** Bumped per week request, so a slow earlier week cannot land over the one shown now. */
let historyWeekRequest = 0;
let historyWeek: HistoryWeek | null = null;
let sessionReport: SessionReport | null = null;
let weekRecap: WeekRecap | null = null;
let historyView: HistoryView = "list";
/** Which list sits under the week: its sessions or where its time went. */
let historyPane: "sessions" | "apps" = "sessions";
let historySearch = "";
/** Null until the first search of the current text answers. */
let historySearchResults: SessionSummary[] | null = null;
let historySearchTimer: number | null = null;
let historySearchRequest = 0;
/** The week (and data state) whose charts already played their entrance. */
let historyChartsFor = "";
/** Arriving on History opens a short window in which its charts rise, even across the data refresh. */
let historyAnimateUntil = 0;
/** The week picker under the toolbar: open, the month it shows (its first day), and that
 * month's days with sessions. */
let weekPickerOpen = false;
let pickerDismissBound = false;
let pickerMonth = "";
let pickerDays: Record<string, DailyFocus> = {};
let pickerRequest = 0;
let historyConfirm: HistoryConfirm | null = null;
let historyHydrated = false;
const knownHistoryIds = new Set<number>();
let permanentRules = cloneRules(starterRules);
let preflightOpen = false;
/** Distractions the user allowed for the session about to start. */
let preflightExceptions: AppRule[] = [];
let preflightBlocksOpen = false;
let preflightAdvancedOpen = false;
let sessionRulesOpen = false;
let paintedViewKey = "";
let overlayMoreOpen = false;
let distractionDismissedFor = "";
let distractionLatchKey = "";
let distractionLatchLabel = "";
let overlayPlacement = "corner";
let lastOverlayFit = "";
const OVERLAY_CORNER_WIDTH = 228;
const OVERLAY_INTERRUPT_WIDTH = 520;
let ruleEditor: RuleEditor | null = null;
let rulePreviewTimer: number | null = null;
let siteNames: Record<string, string> = {};
let appNames: Record<string, string> = {};
let editingRecapNames = false;
let historyRename: { kind: "app" | "website"; key: string; value: string } | null = null;
let customPhrases: PhraseDefinition[] = [];
let customPhraseSets: string[] = [];
let phraseOverrides: Record<string, PhraseDefinition> = {};
let phraseEditor: PhraseEditor | null = null;
let hiddenPhrases = new Set<string>();
let clipsVoiceId = "starter-female";
let clipsVoiceTouched = false;
/** The tone whose lines the Clips panel shows; null means the voice's own tone. */
let clipsTone: VoiceTone | null = null;
/** The Tones popup: open, a rename in progress, a delete awaiting its confirm, a new name being typed. */
let tonesOpen = false;
let toneRename: { from: VoiceTone; value: string } | null = null;
let toneDeleteArmed: VoiceTone | null = null;
let newToneDraft: string | null = null;
let clipQueue: ClipQueue | null = null;
let clipQueueTicker: number | null = null;
/** A one-line job started from the line popup reopens that line when its clip lands. */
let reopenLineAfterQueue: string | null = null;
let ignoreVoiceWorkerError = false;
const clipAssetRevision = new Map<string, number>();
let voiceHelpOpen = false;
let modelInfoOpen = false;
let memoryInfoOpen = false;
let modelDeleteOpen = false;
let voiceBackend: VoiceBackendStatus | null = null;
/** What the OS lets Supervisor see; null until the backend answers. */
let captureAccess: { platform: string; permissions: CapturePermissions } | null = null;
let voiceDownload: VoiceDownloadSnapshot | null = null;
let voiceBusy: VoiceBusy = null;
let voiceDeleteTargetId: string | null = null;
let voiceInfoId: string | null = null;
let voicePreviewAudio: HTMLAudioElement | null = null;
const waveformPeaks = new Map<string, number[]>();
const waveformPending = new Map<string, Promise<AudioBuffer>>();
let waveformContext: AudioContext | null = null;
let voicePreviewPath = "";
let previewWaveRaf = 0;
let previewBarDragging = false;
let recordingOpen = false;
let recordingState: RecordingState = "idle";
let recordingError = "";
let recordedVoice: RecordedVoice | null = null;
let recordingTickId: number | null = null;
let recordingStartedAtMs: number | null = null;
let lastError = "";
let voiceError = "";
let feedbackMessage = "";
let feedbackTimer: number | null = null;
let pollId: number | null = null;
let snapshotInFlight = false;
let downloadPollId: number | null = null;
let downloadPollInFlight = false;
let nudgeAudio: HTMLAudioElement | null = null;
const webcamSettings = {
  phoneUse: false,
  strict: false,
  reaction: "alarm" as WebcamReaction,
};
let webcamFrame: WebcamFrame | null = null;
let webcamError = "";
let webcamHitStreak = 0;
let webcamAlarmAtMs = 0;
let webcamPreviewStarted = false;
/** Camera names in the order the worker opens them (empty where the OS gives none). */
let webcamCameras: string[] = [];

function getAppRoot(): HTMLDivElement {
  const element = document.querySelector<HTMLDivElement>("#app");

  if (!element) {
    throw new Error("Missing app root");
  }

  return element;
}

async function init(): Promise<void> {
  render();
  try {
    const [loadedSettings, loadedHistory, loadedRules, loadedPhraseLibrary, loadedSiteNames, loadedAppNames] = await Promise.all([
      invoke<SessionSettings>("get_settings"),
      invoke<SessionSummary[]>("get_history"),
      invoke<AppRule[]>("get_rules"),
      invoke<PhraseLibraryState>("get_phrase_library"),
      invoke<Record<string, string>>("get_site_names"),
      invoke<Record<string, string>>("get_app_names"),
    ]);
    settings = loadedSettings;
    webcamSettings.phoneUse = loadedSettings.webcam_enabled;
    siteNames = loadedSiteNames;
    appNames = loadedAppNames;
    history = loadedHistory;
    permanentRules = loadedRules;
    customPhraseSets = loadedPhraseLibrary.custom_tones;
    hiddenPhrases = new Set(loadedPhraseLibrary.hidden_phrases);
    customPhrases = loadedPhraseLibrary.custom_phrases;
    phraseOverrides = Object.fromEntries(
      loadedPhraseLibrary.phrase_overrides.map((phrase) => [phrase.id, phrase]),
    );
    history.forEach((item) => knownHistoryIds.add(item.id));
    void refreshCaptureAccess();
    // Coming back from System Settings is when a permission changes.
    window.addEventListener("focus", () => void refreshCaptureAccess());
    draft.strictness = loadedSettings.default_strictness;
    draft.voiceId = normalizeStoredVoiceId(loadedSettings.voice_profile);
  } catch (error) {
    lastError = errorText(error);
  }

  if (appView === "main") {
    await listen<WebcamFrame>("webcam-frame", (event) => {
      applyWebcamFrame(event.payload);
    });
    // The backend sends this once per nudge, the moment it is due (a webview timer would be
    // throttled while the window is minimized).
    await listen("session-nudge", async () => {
      await refreshSnapshot();
      void playNudge(snapshot?.current ?? null);
    });
  }
  await refreshSnapshot();
  historyHydrated = true;

  if (appView === "main") {
    void refreshTrends();
    // Renders when the voice list arrives, so cards that read it are not stale.
    void refreshVoiceStatus();
    void refreshVoiceDownload(false);
  }
}

function voiceLayoutKey(status: VoiceBackendStatus | null): string {
  if (!status) {
    return "";
  }
  return [
    status.system.has_enough_ram ? 1 : 0,
    status.model.installed ? 1 : 0,
    status.runtime.installed ? 1 : 0,
    status.profiles.map((profile) => `${profile.profile_id}:${profile.clip_count}`).join(","),
  ].join("|");
}

async function refreshVoiceStatus(shouldRender = true): Promise<void> {
  const before = voiceLayoutKey(voiceBackend);
  try {
    voiceBackend = await invoke<VoiceBackendStatus>("get_voice_backend_status");
    voiceError = "";
    ensureSelectedVoice();
  } catch (error) {
    voiceError = errorText(error);
  }

  if (!shouldRender) {
    return;
  }
  if (voiceLayoutKey(voiceBackend) !== before) {
    render();
    return;
  }
  paintVoiceRam();
}

async function refreshVoiceDownload(shouldRender = true): Promise<void> {
  try {
    const download = await invoke<VoiceDownloadSnapshot>("get_voice_model_download");
    applyDownloadSnapshot(download);
    if (download.phase === "checking" || download.phase === "downloading" || download.phase === "cancelling") {
      voiceBusy = "download-model";
      startDownloadPolling();
    } else if (download.phase === "error" && !voiceError) {
      // A download that failed before this page loaded; the modal is gone, so the notice says it.
      voiceError = download.message;
    }
  } catch {
    voiceDownload = null;
  }

  if (shouldRender) {
    render();
  }
}

async function refreshSnapshot(): Promise<void> {
  // A slow tick must not be overtaken by the next one and then land its older snapshot.
  if (snapshotInFlight) {
    return;
  }
  snapshotInFlight = true;
  try {
    await refreshSnapshotNow();
  } finally {
    snapshotInFlight = false;
  }
}

async function refreshSnapshotNow(): Promise<void> {
  try {
    snapshot = await invoke<SessionSnapshot>("get_session_snapshot");
    settings = snapshot.settings;
    history = snapshot.history;
    if (!snapshot.current && sessionRulesOpen) {
      sessionRulesOpen = false;
      ruleEditor = null;
    }
    const newest = noteNewHistory(history);
    if (appView === "main") {
      ensureSelectedVoice();
      // Not while a dialog or editor is open: a recording or clip job would vanish behind History.
      if (historyHydrated && newest && !snapshot.current && !editorIsOpen()) {
        void openHistoryReport(newest);
        return;
      }
      if (historyHydrated && newest && activeTab === "history") {
        void refreshTrends(false);
        void refreshHistoryWeek();
      }
    }
    historyHydrated = true;
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }

  const webcamOpen = appView === "main" && activeTab === "settings" && settingsView === "webcam";
  if (!webcamOpen) {
    const current = snapshot?.current ?? null;
    // While something is being edited or typed into, only paint in place. The rebuild
    // waits for the next tick after the editor closes or the field loses focus.
    if (editorIsOpen() || typingInField() || viewKey() === paintedViewKey) {
      paintLive(current);
    } else {
      render();
    }
  }
  reconcilePolling();
}

function closeTopLayer(): void {
  if (voiceBusy === "download-model" && ["complete", "cancelling", "error"].includes(voiceDownload?.phase ?? "")) {
    dismissDownloadModal();
    return;
  }
  if (voiceHelpOpen || modelInfoOpen || memoryInfoOpen) {
    voiceHelpOpen = false;
    modelInfoOpen = false;
    memoryInfoOpen = false;
    render();
    return;
  }
  if (tonesOpen) {
    closeTonesPopup();
    render();
    return;
  }
  if (voiceBusy) {
    return;
  }
  if (historyRename) {
    historyRename = null;
    render();
    return;
  }
  if (ruleEditor) {
    ruleEditor = null;
    render();
    return;
  }
  if (phraseEditor) {
    phraseEditor = null;
    render();
    return;
  }
  if (historyConfirm) {
    historyConfirm = null;
    render();
    return;
  }
  if (voiceDeleteTargetId) {
    voiceDeleteTargetId = null;
    render();
    return;
  }
  if (voiceInfoId) {
    dismissVoiceInfo();
    render();
    return;
  }
  if (modelDeleteOpen) {
    modelDeleteOpen = false;
    render();
    return;
  }
  if (recordingOpen) {
    void closeRecordingDialog();
    return;
  }
  if (sessionRulesOpen) {
    sessionRulesOpen = false;
    ruleEditor = null;
    render();
    return;
  }
  if (onboardingOpen) {
    onboardingOpen = false;
    render();
    return;
  }
  if (preflightOpen) {
    preflightOpen = false;
    ruleEditor = null;
    animateCalendarSoon();
    render();
    return;
  }
  if (historyView === "report" || historyView === "trends") {
    historyView = "list";
    sessionReport = null;
    historyConfirm = null;
    void refreshHistoryWeek();
    return;
  }
  if (weekPickerOpen) {
    closeWeekPicker();
    render();
    return;
  }
  if (activeTab === "history" && historySearch) {
    historySearch = "";
    scheduleHistorySearch();
    return;
  }
  if (overlayMoreOpen) {
    overlayMoreOpen = false;
    render();
    return;
  }
  if (appView === "main" && activeTab === "settings" && settingsView !== "home") {
    void runAction("back-settings");
  }
}

function editorIsOpen(): boolean {
  return (
    preflightOpen ||
    sessionRulesOpen ||
    ruleEditor !== null ||
    historyRename !== null ||
    phraseEditor !== null ||
    tonesOpen ||
    recordingOpen ||
    historyConfirm !== null ||
    voiceDeleteTargetId !== null ||
    voiceInfoId !== null ||
    clipQueue?.phase === "working" ||
    voiceHelpOpen ||
    modelInfoOpen ||
    memoryInfoOpen ||
    modelDeleteOpen ||
    voiceBusy !== null
  );
}

/** A focused text field or dropdown in the main window; a rebuild would drop the caret or close the list. */
function typingInField(): boolean {
  const el = document.activeElement;
  if (appView !== "main" || !el || !app.contains(el)) {
    return false;
  }
  if (el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
    return true;
  }
  return el instanceof HTMLInputElement && !["checkbox", "radio", "range", "button"].includes(el.type);
}

function sessionViewKey(current: CurrentSession | null): string {
  if (!current) {
    return "";
  }
  const focus = current.focus;
  const sample = focus?.sample;
  const rules = current.exceptions.map((rule) => `${rule.target}\t${rule.value}`).join("\n");
  return [
    current.id,
    current.status,
    current.goal,
    current.duration_seconds,
    current.strictness,
    current.snoozed_until_seconds ?? "",
    focus?.state ?? "",
    focus?.distracting ? 1 : 0,
    focus?.should_nudge ? 1 : 0,
    focus?.matched_rule?.value ?? "",
    sample?.app_name ?? "",
    focus?.activity.key ?? "",
    current.on_supervisor ? 1 : 0,
    current.metrics.nudges,
    current.camera.phone ? 1 : 0,
    current.camera.phone_distracted ? 1 : 0,
    rules,
  ].join("\u001f");
}

function viewKey(): string {
  const current = snapshot?.current ?? null;
  return [
    appView,
    activeTab,
    settingsView,
    historyView,
    weekRecap?.start ?? "",
    // Other tabs show the session only in the status chip (painted in place) and the
    // permission notice, so a focus change doesn't rebuild History or Settings.
    appView === "overlay" || activeTab === "session" ? sessionViewKey(current) : permissionNotice(current),
    settings.status_window_on_top ? 1 : 0,
    settings.status_window_enabled ? 1 : 0,
    draft.voiceId,
    voiceInfoId ?? "",
    overlayPlacement,
    distractionLatchKey,
    distractionDismissedFor,
    lastError,
    voiceError,
    history.length,
    // The Getting started card ticks itself off from rules, voice and settings.
    onboardingSteps().map((step) => (step.done ? "1" : "0")).join("") + (onboardingOpen ? "o" : ""),
    historyWeekStart,
    historyWeek?.sessions.length ?? "",
    sessionReport?.id ?? "",
    dayCardKey,
    trends.daily_focus.length,
  ].join("\u001f");
}

/** The parts of a live session that change between rebuilds, painted in place. */
function paintLive(current: CurrentSession | null): void {
  if (!current) {
    return;
  }
  const chip = document.querySelector<HTMLElement>(".status-chip");
  if (chip) {
    chip.textContent = statusChip(current);
  }
  const slab = document.querySelector<HTMLElement>(".active-state-card");
  if (slab) {
    slab.className = `state-slab active-state-card ${stateClass(current)}`;
    const title = slab.querySelector("h2");
    if (title) {
      title.textContent = stateTitle(current);
    }
  }
  const label = formatDuration(current.remaining_seconds);
  const percent = current.duration_seconds
    ? Math.min(100, Math.round((current.elapsed_seconds / current.duration_seconds) * 100))
    : 0;
  document.querySelectorAll<HTMLElement>(".active-timer strong, .interrupt-time").forEach((node) => {
    node.textContent = label;
  });
  const overlayTime = document.querySelector<HTMLElement>(".overlay-progress > span");
  if (overlayTime) {
    overlayTime.textContent = `${label} left`;
  }
  document
    .querySelectorAll<HTMLElement>(".active-progress > span, .overlay-progress .progress > span, .interrupt-facts .progress > span")
    .forEach((node) => {
      node.style.width = `${percent}%`;
    });
}

const PRESERVE_SCROLL = [".voice-list", ".evidence-panel .rows", ".modal-panel"] as const;

function captureUiState(): {
  winX: number;
  winY: number;
  scrolls: Array<[number, number]>;
  focusId: string;
  start: number | null;
  end: number | null;
} {
  const active = document.activeElement as HTMLInputElement | null;
  return {
    winX: window.scrollX,
    winY: window.scrollY,
    scrolls: PRESERVE_SCROLL.map((sel) => {
      const el = document.querySelector<HTMLElement>(sel);
      return el ? [el.scrollLeft, el.scrollTop] : [0, 0];
    }),
    focusId: active?.id ?? "",
    start: active?.selectionStart ?? null,
    end: active?.selectionEnd ?? null,
  };
}

function restoreUiState(state: ReturnType<typeof captureUiState>): void {
  PRESERVE_SCROLL.forEach((sel, i) => {
    const el = document.querySelector<HTMLElement>(sel);
    if (el) {
      el.scrollLeft = state.scrolls[i][0];
      el.scrollTop = state.scrolls[i][1];
    }
  });
  window.scrollTo(state.winX, state.winY);
  if (!state.focusId) {
    return;
  }
  const el = document.getElementById(state.focusId) as HTMLInputElement | null;
  if (!el) {
    return;
  }
  el.focus();
  if (state.start === null) {
    return;
  }
  try {
    el.setSelectionRange(state.start, state.end ?? state.start);
  } catch {
    // not a text field
  }
}

function render(): void {
  const current = snapshot?.current ?? null;
  const ui = captureUiState();
  if (appView === "overlay") {
    app.innerHTML = renderOverlay(current);
    bindEvents();
    syncOverlayPlacement(current);
    restoreUiState(ui);
    paintedViewKey = viewKey();
    reconcilePolling();
    return;
  }

  app.innerHTML = `
    <main class="app-shell">
      <header class="topbar">
        <div>
          <h1>Supervisor</h1>
        </div>
        <div class="topbar-status">
          <button class="status-chip" type="button" data-tab="session" aria-label="${current ? "Session status" : "Today's focused time; opens the Session tab"}">${escapeHtml(current ? statusChip(current) : todayChip())}</button>
          <button class="theme-toggle" type="button" data-action="toggle-theme">${theme === "dark" ? "Dark" : "Light"}</button>
        </div>
      </header>

      <nav class="tabs" aria-label="Supervisor sections">
        ${tabButton("session", "Session")}
        ${tabButton("settings", "Settings")}
        ${tabButton("history", "History")}
      </nav>

      ${lastError ? `<section class="notice">${escapeHtml(lastError)}</section>` : ""}
      ${voiceError ? `<section class="notice">${escapeHtml(voiceError)}</section>` : ""}
      ${permissionNotice(current)}

      ${activeTab === "session" ? renderSession(current) : ""}
      ${activeTab === "settings" ? renderSettings() : ""}
      ${activeTab === "history" ? renderHistory() : ""}
      <div id="voice-layer">${renderVoiceDialogs()}</div>
      ${renderHistoryConfirm()}
    </main>
  `;

  bindEvents();
  fitVoiceCardNames();
  fitQuote();
  fitShareStats();
  if (document.fonts.status !== "loaded") {
    void document.fonts.ready.then(() => {
      if (document.querySelector(".voice-card")) {
        fitVoiceCardNames();
      }
      // The webfont changes the other cards' heights, and so the room the quote has.
      fitQuote();
      fitShareStats();
    });
  }
  restoreUiState(ui);
  if (activeTab === "settings" && settingsView === "webcam") {
    paintWebcamDom();
  }
  paintedViewKey = viewKey();
  reconcilePolling();
}

function renderSession(current: CurrentSession | null): string {
  if (!current) {
    void ensureDayCard();
    const note = dailyNote();
    const streak = streakInfo();
    return `
      <section class="home-ready">
        <div class="home-grid">
          <article class="panel cal-card">
            <p class="eyebrow">Calendar · ${escapeHtml(monthLabel(new Date()))}</p>
            <div class="streak-line">
              <strong>${streak.days}</strong>
              <span>day streak</span>
            </div>
            <p class="streak-rule">25 min a day keeps it. Weekends don't count or break it.</p>
            ${renderCalendar(streak)}
          </article>
          <div class="home-side">
            ${onboardingVisible() ? renderOnboardingLine() : ""}
            <article class="panel quote-card">
              <p class="eyebrow">Daily note · ${escapeHtml(note.date)}</p>
              <blockquote>“${escapeHtml(note.text)}”</blockquote>
              <p class="quote-by">— ${escapeHtml(note.by)}</p>
            </article>
            ${renderDayCard(streak.days)}
          </div>
        </div>
        ${renderStartAction()}
      </section>
      ${onboardingOpen && onboardingVisible() ? renderOnboardingModal() : ""}
      <div id="preflight-layer">${preflightOpen ? renderPreflightModal() : ""}</div>
    `;
  }

  const percent = current.duration_seconds
    ? Math.min(100, Math.round((current.elapsed_seconds / current.duration_seconds) * 100))
    : 0;
  return `
    <section class="active-session-page">
      <div class="active-session-grid">
        <section class="state-slab active-state-card ${stateClass(current)}" aria-live="polite">
          <p class="eyebrow">Current session</p>
          <h2>${escapeHtml(stateTitle(current))}</h2>
          <div class="active-goal">
            <span>Session goal</span>
            <strong>${escapeHtml(current.goal)}</strong>
          </div>
          <div class="active-timer">
            <strong>${formatDuration(current.remaining_seconds)}</strong>
            <span>Remaining</span>
          </div>
          <div class="active-progress" aria-label="Session progress">
            <span style="width: ${percent}%"></span>
          </div>
        </section>

        <aside class="panel session-rules-card">
          <p class="eyebrow">Distractions</p>
          ${renderSessionDistractions(current.exceptions)}
          ${renderNowLine(current)}
          <button type="button" data-action="open-session-rules">Edit distractions</button>
        </aside>
      </div>

      ${renderActiveSessionControls(current)}
    </section>
    ${sessionRulesOpen ? renderSessionRulesModal() : ""}
  `;
}

/** What is in front right now, named the way History names it, and how it counts. */
function nowReading(current: CurrentSession): { name: string; detail: string } {
  if (current.status === "paused") {
    return { name: "Paused", detail: "Nothing is recorded until you resume" };
  }
  if (current.camera.phone) {
    return { name: "Phone", detail: "In view of the camera" };
  }
  const focus = current.focus;
  if (current.on_supervisor) {
    return { name: "Supervisor", detail: "Work" };
  }
  if (!focus) {
    return { name: "Looking…", detail: "Reading the window in front" };
  }
  if (focus.state === "idle") {
    return { name: "Away", detail: "No input for a few minutes" };
  }
  const { activity, sample } = focus;
  const verdict = focus.distracting ? "a distraction" : "work";
  if (activity.kind === "website") {
    return { name: siteNames[activity.key] ?? activity.label, detail: `In ${sample.app_label} · ${verdict}` };
  }
  if (activity.kind === "other_websites") {
    const unread = sample.platform === "macos" && sample.permissions.accessibility === "missing";
    return { name: sample.app_label, detail: unread ? "Allow Accessibility to see the site" : "Site not read · work" };
  }
  return { name: appNames[activity.key] ?? activity.label, detail: focus.distracting ? "A distraction" : "Work" };
}

function renderNowLine(current: CurrentSession): string {
  const now = nowReading(current);
  return `
    <div class="session-now">
      <span>Now</span>
      <strong>${escapeHtml(now.name)}</strong>
      <small>${escapeHtml(now.detail)}</small>
    </div>
  `;
}

/** The distractions, with the ones allowed for this session struck through. */
function renderSessionDistractions(exceptions: AppRule[]): string {
  const rules = permanentRules.slice().reverse();
  const shown = rules.slice(0, 6);
  const chips = shown.map((rule) => {
    const allowed = exceptions.some((exception) => sameRule(exception, rule));
    return `<span class="${allowed ? "is-allowed" : ""}" title="${attr(allowed ? "Allowed for this session" : "")}">${escapeHtml(ruleName(rule))}</span>`;
  });
  if (rules.length > shown.length && chips.length) {
    const last = chips.pop();
    chips.push(`<span class="session-rule-tail">${last}<span class="session-rule-more">...</span></span>`);
  }
  const body = chips.length ? chips.join("") : `<span class="no-session-rules">None yet. Everything counts as work.</span>`;
  const allowed = exceptions.map((exception) => ruleName(exception));
  return `
    <section class="session-rule-set">
      <div>
        ${body}
      </div>
      ${allowed.length ? `<p class="session-allowed">Not today: ${escapeHtml(allowed.join(", "))}</p>` : ""}
    </section>
  `;
}

function sameRule(a: AppRule, b: AppRule): boolean {
  return a.target === b.target && a.value.toLowerCase() === b.value.toLowerCase();
}

function renderActiveSessionControls(current: CurrentSession): string {
  const held = current.status === "paused";
  return `
    <div class="active-session-controls">
      <button class="${held ? "primary" : "overlay-pause"}" type="button" data-action="${held ? "resume" : "pause"}">${held ? "Resume" : "Pause"}</button>
      <button class="danger" type="button" data-action="stop">End session</button>
    </div>
  `;
}

function renderSessionRulesModal(): string {
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel session-rules-panel" role="dialog" aria-modal="true" aria-labelledby="session-rules-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">CURRENT SESSION</p>
            <h2 id="session-rules-title">Edit distractions</h2>
          </div>
          <button class="small-button" type="button" data-action="close-session-rules">Done</button>
        </div>
        ${renderRuleManager("")}
      </section>
    </div>
  `;
}

function renderOverlay(current: CurrentSession | null): string {
  updateDistractionLatch(current);
  if (distractionIsActive()) {
    return renderDistractionOverlay(current as CurrentSession);
  }
  // Muting keeps recording; only Pause holds the timer.
  const held = current?.status === "paused";
  const nudgesMuted = current?.status === "snoozed";
  const pinned = settings.status_window_on_top;
  return `
    <main class="overlay-shell">
      <section class="state-slab overlay-slab ${stateClass(current)}" aria-live="polite">
        ${nudgesMuted ? `<span class="overlay-muted-tag" title="Nudges muted. Still tracking.">Muted</span>` : ""}
        <h2>${escapeHtml(stateTitle(current))}</h2>
        <p>${escapeHtml(current?.goal || stateCopy(current))}</p>
        ${current ? renderOverlayProgress(current) : ""}
      </section>
      ${
        current
          ? `
            <div class="overlay-actions">
              <button class="${held ? "primary" : "overlay-pause"}" type="button" data-action="${held ? "resume" : "pause"}">${held ? "Resume" : "Pause"}</button>
              <button class="danger" type="button" data-action="stop">Stop</button>
            </div>
            <details class="overlay-more" ${overlayMoreOpen ? "open" : ""}>
              <summary>More settings</summary>
              <div class="overlay-more-body">
                <label class="overlay-mute">
                  <span>Mute nudges for 5 min</span>
                  <span class="toggle-field">
                    <input id="overlay-snooze" class="toggle-input" type="checkbox" ${nudgesMuted ? "checked" : ""} ${held ? "disabled" : ""} aria-label="Mute nudges for 5 minutes" />
                    <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
                  </span>
                </label>
                <p>Is this window pulling you off task?</p>
                <div class="overlay-marks">
                  <button class="mark-distraction" type="button" data-action="mark-distracting">This is a distraction</button>
                </div>
                <button class="overlay-pin" type="button" data-action="${pinned ? "unpin-window" : "pin-window"}">${pinned ? "Unpin window" : "Pin window"}</button>
              </div>
            </details>
          `
          : ""
      }
    </main>
  `;
}

function renderDistractionOverlay(current: CurrentSession): string {
  const percent = current.duration_seconds
    ? Math.min(100, Math.round((current.elapsed_seconds / current.duration_seconds) * 100))
    : 0;
  const phone = distractionLatchKey === PHONE_LATCH;
  const alert = phone
    ? `
        <p class="interrupt-kicker">The webcam sees your phone</p>
        <h2>Phone detected.</h2>
        <p>Put it down. The timer is still running.</p>
      `
    : `
        <p class="interrupt-kicker">You opened ${escapeHtml(distractionLatchLabel || distractionLabel(current))}</p>
        <h2>Distraction detected.</h2>
        <p>${escapeHtml(distractionLatchLabel || distractionLabel(current))} is on your distractions list.</p>
      `;
  return `
    <main class="overlay-shell overlay-interrupt">
      <section class="state-slab interrupt-alert">
        ${alert}
      </section>
      <div class="interrupt-facts">
        <article class="panel">
          <p class="eyebrow">Your task</p>
          <strong>${escapeHtml(current.goal)}</strong>
        </article>
        <article class="panel">
          <p class="eyebrow">Remaining</p>
          <strong class="interrupt-time">${formatDuration(current.remaining_seconds)}</strong>
          <div class="progress" aria-label="Session progress"><span style="width: ${percent}%"></span></div>
        </article>
      </div>
      <div class="interrupt-actions">
        <button class="primary" type="button" data-action="dismiss-distraction">Return to task</button>
        ${phone ? "" : `<button type="button" data-action="allow-session">Not today</button>
        <button type="button" data-action="allow-always">Never a distraction</button>`}
      </div>
    </main>
  `;
}

function distractionLabel(current: CurrentSession): string {
  return current.focus?.activity.label || "This app";
}

/** The phone outranks the window in front, in the popup as in the timeline. */
const PHONE_LATCH = "phone|phone";

function distractionIdentity(current: CurrentSession): string {
  if (current.camera.phone) {
    return PHONE_LATCH;
  }
  const activity = current.focus?.activity;
  return `${activity?.kind ?? ""}|${activity?.key ?? ""}`;
}

function distractionIsActive(): boolean {
  return distractionLatchKey !== "" && distractionDismissedFor !== distractionLatchKey;
}

function updateDistractionLatch(current: CurrentSession | null): void {
  if (!current || current.status !== "active") {
    distractionLatchKey = "";
    distractionLatchLabel = "";
    return;
  }
  const key = distractionIdentity(current);
  if (current.camera.phone) {
    // Shown once past the strictness grace; a dismissal lasts until the phone is put down.
    if (current.camera.phone_distracted && distractionDismissedFor !== key) {
      distractionLatchKey = key;
      distractionLatchLabel = "Your phone";
    }
    return;
  }
  if (distractionLatchKey === PHONE_LATCH || distractionDismissedFor === PHONE_LATCH) {
    distractionLatchKey = "";
    distractionLatchLabel = "";
    distractionDismissedFor = "";
  }
  const focus = current.focus;
  if (!focus) {
    return;
  }
  if (focus.state === "distracted") {
    if (distractionDismissedFor !== key) {
      distractionLatchKey = key;
      distractionLatchLabel = distractionLabel(current);
    }
    return;
  }
  if (focus.state === "focused" || focus.state === "idle") {
    distractionLatchKey = "";
    distractionLatchLabel = "";
    distractionDismissedFor = "";
  }
}

function syncOverlayPlacement(current: CurrentSession | null): void {
  if (appView !== "overlay") {
    return;
  }
  const interrupt = distractionIsActive();
  const next = interrupt ? "interrupt" : "corner";
  if (next !== overlayPlacement) {
    overlayPlacement = next;
    lastOverlayFit = "";
    void invoke("set_status_window_size", {
      width: interrupt ? OVERLAY_INTERRUPT_WIDTH : OVERLAY_CORNER_WIDTH,
      height: interrupt ? 380 : 220,
      center: interrupt,
    }).catch((error) => {
      lastError = errorText(error);
    });
  }
  requestAnimationFrame(() => fitOverlayWindow());
}

function fitOverlayWindow(attempt = 0): void {
  if (appView !== "overlay") {
    return;
  }
  const shell = document.querySelector<HTMLElement>(".overlay-shell");
  if (!shell) {
    return;
  }
  const interrupt = overlayPlacement === "interrupt";
  const width = interrupt ? OVERLAY_INTERRUPT_WIDTH : OVERLAY_CORNER_WIDTH;
  // Measure only once the window has this placement's width: at another width the text
  // wraps differently and the height is wrong. Ask for the width first (the window opens at
  // its config size), then wait for it; WebKit applies a resize a few frames late.
  if (Math.round(window.innerWidth) !== width) {
    if (attempt === 0) {
      lastOverlayFit = "";
      void invoke("set_status_window_size", {
        width,
        height: Math.ceil(shell.getBoundingClientRect().height),
        center: interrupt,
      }).catch((error) => {
        lastError = errorText(error);
      });
    }
    if (attempt < 60) {
      requestAnimationFrame(() => fitOverlayWindow(attempt + 1));
    }
    return;
  }
  const height = Math.ceil(shell.getBoundingClientRect().height);
  const key = `${width}x${height}`;
  if (key === lastOverlayFit) {
    return;
  }
  lastOverlayFit = key;
  void invoke("set_status_window_size", { width, height, center: interrupt }).catch((error) => {
    lastOverlayFit = "";
    lastError = errorText(error);
  });
}

function renderOverlayProgress(current: CurrentSession): string {
  const percent = current.duration_seconds
    ? Math.min(100, Math.round((current.elapsed_seconds / current.duration_seconds) * 100))
    : 0;
  return `
    <div class="overlay-progress">
      <span>${formatDuration(current.remaining_seconds)} left</span>
      <div class="progress" aria-label="Session progress"><span style="width: ${percent}%"></span></div>
    </div>
  `;
}

/** The Getting started steps, open as a popup over Home. */
let onboardingOpen = false;

async function refreshCaptureAccess(): Promise<void> {
  try {
    const access = await invoke<{ platform: string; permissions: CapturePermissions }>("get_capture_access");
    const changed = JSON.stringify(access) !== JSON.stringify(captureAccess);
    captureAccess = access;
    if (changed) {
      render();
    }
  } catch {
    // Without an answer the step is simply not listed.
  }
}

interface OnboardingStep {
  id: string;
  title: string;
  copy: string;
  done: boolean;
  label: string;
  optional?: boolean;
}

/** What a new user has to set up, each read from real state so it ticks itself off. */
function onboardingSteps(): OnboardingStep[] {
  // macOS reads window titles and browser addresses through Accessibility; without it every
  // browser counts as Other websites. Windows needs nothing.
  const accessibility =
    captureAccess?.platform === "macos"
      ? [
          {
            id: "accessibility",
            title: "Allow Accessibility",
            copy: "macOS needs it to tell websites apart. Only the address is read, never the page.",
            done: captureAccess.permissions.accessibility !== "missing",
            label: "Open settings",
          },
        ]
      : [];
  return [
    ...accessibility,
    {
      id: "voice",
      title: "Clone a voice",
      copy: "Record twenty seconds of anyone, and that's who nudges you back.",
      done: (voiceBackend?.profiles ?? []).some(
        (profile) => !(STARTER_PROFILE_IDS as readonly string[]).includes(profile.profile_id) && Boolean(profile.reference_path),
      ),
      label: "Record",
    },
    {
      id: "webcam",
      title: "Turn on the webcam",
      copy: "Catches a phone in your hand. Off until you say so.",
      done: settings.webcam_enabled,
      label: "Open",
      optional: true,
    },
    {
      id: "session",
      title: "Run a first session",
      copy: "Fifteen minutes is enough to see how it works.",
      done: history.length > 0,
      label: "Start 15 min",
    },
  ];
}

/** Shown until the required steps are done. The line stays on Home; the steps open as a popup. */
function onboardingVisible(): boolean {
  return onboardingSteps().some((step) => !step.done && !step.optional);
}

function renderOnboardingLine(): string {
  const required = onboardingSteps().filter((step) => !step.optional);
  const done = required.filter((step) => step.done).length;
  return `
    <article class="panel onboard-card">
      <div class="onboard-head">
        <p class="eyebrow">Getting started · ${done} of ${required.length} done</p>
        <button class="small-button" type="button" data-action="onboard-show">Show</button>
      </div>
    </article>
  `;
}

function renderOnboardingModal(): string {
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel onboard-panel" role="dialog" aria-modal="true" aria-labelledby="onboard-title">
        <div class="modal-head onboard-head">
          <div>
            <p class="eyebrow">Getting started</p>
            <h2 id="onboard-title">Set up Supervisor</h2>
          </div>
          <button class="small-button" type="button" data-action="onboard-hide">Close</button>
        </div>
        <p class="onboard-intro">Supervisor watches what's in front of you and nudges you back. Five common distractions are already on the list; everything else counts as work. Nothing leaves this computer.</p>
        <ol class="onboard-steps">
          ${onboardingSteps()
            .map(
              (step) => `
                <li class="onboard-step ${step.done ? "is-done" : ""}">
                  <i class="onboard-dot" aria-hidden="true"></i>
                  <div>
                    <strong>${escapeHtml(step.title)}${step.optional ? ` <span class="onboard-optional">optional</span>` : ""}</strong>
                    <span>${escapeHtml(step.copy)}</span>
                  </div>
                  ${step.done ? `<span class="onboard-done">Done</span>` : `<button class="small-button" type="button" data-action="onboard-${step.id}">${escapeHtml(step.label)}</button>`}
                </li>
              `,
            )
            .join("")}
        </ol>
      </section>
    </div>
  `;
}

function renderStartAction(): string {
  return `
    <button class="primary start-watch-button" type="button" data-action="open-preflight">Start Focus Session</button>
  `;
}

function renderPreflightModal(): string {
  const ruleCount = permanentRules.length;
  const progress = ((draft.duration_minutes - 15) / 75) * 100;
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel preflight-panel" role="dialog" aria-modal="true" aria-labelledby="preflight-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">SESSION PREFLIGHT</p>
            <h2 id="preflight-title">Set the watch</h2>
          </div>
          <button class="small-button" type="button" data-action="close-preflight">Cancel</button>
        </div>

        <div class="preflight-main">
          <label class="preflight-goal">
            <span>What are you working on?</span>
            <input id="preflight-goal" maxlength="220" value="${attr(draft.goal)}" placeholder="Finish the Supervisor UI overhaul" />
          </label>

          <section class="duration-field">
            <div class="duration-head">
              <span>How long?</span>
              <output id="preflight-duration-output" for="preflight-duration">${draft.duration_minutes} min</output>
            </div>
            <input id="preflight-duration" type="range" min="15" max="90" step="5" value="${draft.duration_minutes}" style="--range-progress: ${progress}%" />
            <div class="duration-ticks" aria-hidden="true">
              <span style="left: 0%">15</span>
              <span style="left: 20%">30</span>
              <span style="left: 60%">60</span>
              <span style="left: 100%">90</span>
            </div>
          </section>

          <details class="preflight-disclosure" data-preflight-section="blocks" ${preflightBlocksOpen ? "open" : ""}>
            <summary>
              <span>Distractions</span>
              <span>${ruleCount} on the list${preflightExceptions.length ? ` · ${preflightExceptions.length} allowed today` : ""}</span>
            </summary>
            <div class="disclosure-body">
              ${renderPreflightDistractions()}
            </div>
          </details>

          <details class="preflight-disclosure" data-preflight-section="advanced" ${preflightAdvancedOpen ? "open" : ""}>
            <summary>
              <span>Advanced settings</span>
            </summary>
            <div class="disclosure-body preflight-advanced">
              <label>
                <span>Strictness</span>
                <select id="preflight-strictness">${strictnessOptions(draft.strictness)}</select>
                <small class="field-help">${STRICTNESS_HELP}</small>
              </label>
              <label>
                <span>Voice</span>
                <select id="preflight-voice">${voiceOptions(draft.voiceId)}</select>
              </label>
            </div>
          </details>
        </div>

        <button class="primary preflight-start" type="button" data-action="confirm-start-session">Start focus session</button>
      </section>
    </div>
  `;
}

function renderSettings(): string {
  if (settingsView === "voice") {
    return renderVoiceSettings();
  }
  if (settingsView === "webcam") {
    return renderWebcamSettings();
  }

  return `
    <section class="settings-page">
      <section class="panel defaults-card">
        <p class="eyebrow">Defaults</p>
        <div class="defaults-grid">
          <label class="default-control">
            <strong>Strictness</strong>
            <select id="default-strictness">
              ${strictnessOptions(settings.default_strictness)}
            </select>
          </label>
          <label class="default-control">
            <strong>Show session timer</strong>
            <span class="toggle-field">
              <input id="status-window-enabled" class="toggle-input" type="checkbox" ${settings.status_window_enabled ? "checked" : ""} aria-label="Show session timer" />
              <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
            </span>
          </label>
        </div>
        <small class="field-help defaults-help">Strictness is ${STRICTNESS_HELP.charAt(0).toLowerCase()}${STRICTNESS_HELP.slice(1)}</small>
      </section>

      <div class="settings-feature-grid">
        <section class="panel settings-feature-card voice-summary-panel">
          <p class="eyebrow">Voice</p>
          <div class="voice-summary">
            <strong>${escapeHtml(selectedVoice().name)}</strong>
            <span class="explain">Clone anyone's voice. Phrases play when you get distracted.</span>
            <button type="button" data-action="open-voice-settings">Customize</button>
          </div>
        </section>

        <section class="panel settings-feature-card">
          <p class="eyebrow">Webcam</p>
          <div class="voice-summary">
            <strong>Live camera check</strong>
            <span class="explain">Local-only models monitor face and phone use.</span>
            <button type="button" data-action="open-webcam-settings">Open</button>
          </div>
        </section>
      </div>

      <section class="panel settings-rules-panel">
        ${renderRuleManager("Distractions")}
      </section>

      <section class="panel settings-data-panel">
        <p class="eyebrow">Your data</p>
        <div class="settings-data-row">
          <span class="explain">Sessions and history stay on this computer. Nothing is uploaded.</span>
          <button class="danger" type="button" data-action="ask-delete-all">Delete all history</button>
        </div>
      </section>
    </section>
  `;
}

function renderWebcamSettings(): string {
  const frame = webcamFrame;
  const alerting = webcamWouldAlert(frame);
  return `
    <section class="webcam-page">
      <div class="subpage-head">
        <button type="button" data-action="back-settings">Back</button>
        <div>
          <p class="eyebrow">WEBCAM MONITORING</p>
          <h2>Live camera check</h2>
        </div>
      </div>

      <section class="webcam-home">
        <section class="state-slab webcam-stage ${webcamStateClass(frame, alerting)}" aria-live="polite">
          <div class="state-head">
            <span id="webcam-chip" class="${webcamSettings.phoneUse ? "" : "is-off"}">${webcamStatusChip()}</span>
          </div>
          <h2 id="webcam-title">${escapeHtml(webcamTitle(frame, alerting))}</h2>
          <p id="webcam-copy">${escapeHtml(webcamCopy(frame, alerting))}</p>
          <div class="webcam-frame-wrap${frame?.jpeg ? " is-live" : ""}" id="webcam-frame-wrap">
            <img id="webcam-preview" alt="" />
            <p id="webcam-waiting" class="webcam-waiting">${escapeHtml(webcamWaitingCopy())}</p>
          </div>
          <p id="webcam-live-error" class="webcam-live-error"${webcamSettings.phoneUse && webcamError ? "" : " hidden"}>${escapeHtml(webcamError)}</p>
        </section>

        <div class="webcam-side">
          <section class="panel">
            <p class="eyebrow">Settings</p>
            <label class="setting-row toggle-row">
              <span>Turn on webcam monitoring</span>
              <span class="toggle-field">
                <input id="webcam-phone-use" class="toggle-input" type="checkbox" ${webcamSettings.phoneUse ? "checked" : ""} aria-label="Turn on webcam monitoring" />
                <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
              </span>
            </label>
            <label class="setting-row toggle-row">
              <span>Run on GPU</span>
              <span class="toggle-field">
                <input id="webcam-gpu" class="toggle-input" type="checkbox" ${settings.webcam_gpu ? "checked" : ""} aria-label="Run on GPU" />
                <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
              </span>
            </label>
            <label class="setting-row">
              <span>Camera</span>
              <select id="webcam-camera" aria-label="Camera">${cameraOptions()}</select>
            </label>
          </section>
          <section class="panel webcam-test-panel">
            <p class="eyebrow">Camera test</p>
            ${renderWebcamMetrics(frame)}
            <p class="quiet">No images saved. Vision models run locally while a session runs. A phone in view is recorded as phone time and nudged like any distraction; leaving your desk is recorded as Away.</p>
          </section>
        </div>
      </section>
    </section>
  `;
}

/** Named cameras where the OS lists them; otherwise the first four indexes. The saved one always shows. */
function cameraOptions(): string {
  const names = webcamCameras.length ? [...webcamCameras] : [0, 1, 2, 3].map((index) => `Camera ${index}`);
  while (names.length <= settings.webcam_camera) {
    names.push(`Camera ${names.length}`);
  }
  return names
    .map((name, index) => `<option value="${index}" ${settings.webcam_camera === index ? "selected" : ""}>${escapeHtml(name)}</option>`)
    .join("");
}

async function refreshCameras(): Promise<void> {
  try {
    webcamCameras = await invoke<string[]>("list_cameras");
  } catch {
    webcamCameras = [];
  }
  const select = document.getElementById("webcam-camera");
  if (select) {
    select.innerHTML = cameraOptions();
  }
}

function renderWebcamMetrics(frame: WebcamFrame | null): string {
  return `
    <div class="metric-grid webcam-metrics">
      ${metric("Face", frame?.face_in_frame ? "IN" : "OUT")}
      ${metric("Phone", frame?.phone_in_frame ? "IN FRAME" : "NO")}
    </div>
  `;
}

/** The one list. Chips open the editor; the same component serves Settings and the live dialog. */
function renderRuleManager(title: string): string {
  return `
    <div class="rule-manager permanent-rule-manager">
      <div class="rule-toolbar">
        <div>
          ${title ? `<p class="eyebrow">${escapeHtml(title)}</p>` : ""}
          <strong>Everything else counts as work.</strong>
        </div>
        <div class="rule-toolbar-actions">
          <button class="primary add-rule-button" type="button" data-rule-action="add">+ Add</button>
        </div>
      </div>
      ${ruleEditor ? renderRuleEditor(ruleEditor) : ""}
      <div class="rule-group">
        <div class="rule-chip-list">
          ${
            permanentRules.length === 0
              ? `<span class="no-session-rules">None yet. Add the sites and apps that pull you off task.</span>`
              : permanentRules
                  .map((rule, index) => ({ rule, index }))
                  .reverse()
                  .map(
                    ({ rule, index }) => `
                      <button class="rule-chip rule-chip-distracting" type="button" data-rule-action="edit" data-rule-index="${index}"${rule.target === "website" ? ` title="${attr(rule.value)}"` : ""}>
                        ${escapeHtml(ruleName(rule))}
                      </button>
                    `,
                  )
                  .join("")
          }
        </div>
      </div>
    </div>
  `;
}

/** Before a session: tap a distraction to allow it today; "+ Add" adds one for good. */
function renderPreflightDistractions(): string {
  const chips = permanentRules
    .map((rule, index) => ({ rule, index }))
    .reverse()
    .map(({ rule, index }) => {
      const allowed = preflightExceptions.some((exception) => sameRule(exception, rule));
      return `
        <button class="rule-chip rule-chip-distracting ${allowed ? "is-allowed" : ""}" type="button" aria-pressed="${allowed}" data-action="toggle-exception" data-rule-index="${index}" title="${attr(allowed ? "Allowed for this session. Tap to flag it again." : "Tap to allow it for this session.")}">
          ${escapeHtml(ruleName(rule))}
        </button>
      `;
    })
    .join("");
  return `
    <div class="rule-manager preflight-rule-manager">
      <p class="explain">Tap one to allow it for this session. Everything else counts as work.</p>
      ${ruleEditor ? renderRuleEditor(ruleEditor) : ""}
      <div class="rule-group">
        <div class="rule-chip-list">
          ${chips || `<span class="no-session-rules">None yet.</span>`}
          <button class="add-target-button" type="button" data-rule-action="add">+ Add</button>
        </div>
      </div>
    </div>
  `;
}

function siteName(key: string): string {
  return siteNames[key] ?? key;
}

function ruleName(rule: AppRule): string {
  return rule.target === "website" ? siteName(rule.value) : appNames[rule.value.toLowerCase()] ?? rule.value;
}

function previewSite(editor: RuleEditor): string | null {
  const preview = editor.preview;
  return preview && "value" in preview && preview.target === "website" ? preview.value : null;
}

function renderRulePreview(editor: RuleEditor): string {
  const preview = editor.preview;
  if (!editor.value.trim() || !preview) {
    return `<span class="rule-preview-note">Supervisor will show what this rule matches before you save.</span>`;
  }
  if ("error" in preview) {
    return `<span class="rule-preview-error">${escapeHtml(preview.error)}</span>`;
  }
  const site = preview.target === "website";
  const value = site ? preview.value : appNames[preview.value.toLowerCase()] ?? preview.value;
  return `
    <span class="rule-kind">${site ? "Site" : "App"}</span>
    <strong class="rule-preview-value">${escapeHtml(value)}</strong>
    <span class="rule-preview-note">${site ? "Every page on this site" : "App windows; browser websites use site rules"}</span>
  `;
}

function renderRuleEditor(editor: RuleEditor): string {
  const site = previewSite(editor);
  return `
    <form class="rule-editor" data-rule-form="rules" autocomplete="off">
      <label>
        <span>Website or app</span>
        <input id="rule-value" name="value" maxlength="300" spellcheck="false" value="${attr(editor.value)}" placeholder="youtube.com or Spotify" />
      </label>
      <label class="rule-name-field" ${site ? "" : "hidden"}>
        <span>Shown as</span>
        <input id="rule-name" name="name" maxlength="60" value="${attr(editor.name)}" placeholder="${attr(site ?? "")}" />
      </label>
      <p class="rule-entry-help">Paste a web address to match every page on that site, or enter an app name to match its windows. Check the match below before saving.</p>
      <p class="rule-preview" id="rule-preview" aria-live="polite">${renderRulePreview(editor)}</p>
      <div class="rule-editor-actions">
        <button class="primary" type="submit">Save</button>
        ${
          editor.index === null
            ? ""
            : `<button class="danger" type="button" data-rule-action="delete" data-rule-index="${editor.index}">Delete</button>`
        }
        <button type="button" data-rule-action="cancel">Cancel</button>
      </div>
    </form>
  `;
}

function renderVoiceSettings(): string {
  const voices = availableVoices();
  const creating = voiceBusy !== null || ["checking", "downloading", "cancelling"].includes(voiceDownload?.phase ?? "");
  return `
    <section class="voice-page">
      <div class="subpage-head voice-head">
        <button type="button" data-action="back-settings">Back</button>
        <div>
          <p class="eyebrow">VOICE SETTINGS</p>
          <h2>Voices</h2>
        </div>
        <button class="voice-help-button" type="button" data-action="open-voice-help" aria-label="How voice cloning works" aria-haspopup="dialog" title="How voice cloning works">?</button>
      </div>

      <section class="voice-grid">
        <section class="panel voice-roster-panel">
          <div class="rule-toolbar">
            <p class="eyebrow">Voices</p>
            <button class="new-voice-button" type="button" data-action="record-voice" ${creating ? "disabled" : ""}>+ New voice</button>
          </div>
          <div class="voice-list">
            ${voices.map(renderVoiceCard).join("")}
          </div>
        </section>

        ${renderVoiceTiles()}
        ${renderVoiceClipsPanel()}
      </section>
    </section>
  `;
}

/** The biggest quote that still fits its card: short quotes grow, long ones shrink. */
/** A stat value never wraps or clips: it steps down (to a floor) until it fits its tile. */
window.addEventListener("resize", () =>
  requestAnimationFrame(() => {
    fitShareStats();
    fitQuote();
  }),
);

/** Stat values never wrap or ellipsize: shrink the font until the value fits its tile. The
 *  report tiles use the system monospace, which is wider on a Mac (SF Mono) than on Windows,
 *  so "40:00 min" overflows there at the stylesheet's size. */
function fitShareStats(): void {
  document.querySelectorAll<HTMLElement>(".share-stats strong").forEach((value) => {
    let size = 1.1;
    value.style.fontSize = `${size}rem`;
    while (size > 0.72 && value.scrollWidth > value.clientWidth) {
      size -= 0.04;
      value.style.fontSize = `${size.toFixed(2)}rem`;
    }
  });
  document.querySelectorAll<HTMLElement>(".report-stat strong").forEach((value) => {
    value.style.fontSize = "";
    let size = parseFloat(getComputedStyle(value).fontSize) / 16;
    while (size > 0.9 && value.scrollWidth > value.clientWidth) {
      size -= 0.04;
      value.style.fontSize = `${size.toFixed(2)}rem`;
    }
  });
}

function fitQuote(): void {
  const card = document.querySelector<HTMLElement>(".quote-card");
  const quote = card?.querySelector<HTMLElement>("blockquote");
  if (!card || !quote) {
    return;
  }
  // The grid stretches the card to the side column, so the smallest size shows the real room.
  // The quote takes at most 60% of it: a card full of text reads like a wall.
  quote.style.fontSize = "18px";
  const room = card.clientHeight * 0.6;
  const fits = (px: number) => {
    quote.style.fontSize = `${px}px`;
    return quote.offsetHeight <= room && quote.scrollWidth <= quote.clientWidth;
  };
  let low = 18;
  let high = 60;
  while (high - low > 1) {
    const mid = Math.floor((low + high) / 2);
    if (fits(mid)) {
      low = mid;
    } else {
      high = mid;
    }
  }
  quote.style.fontSize = `${low}px`;
}

function fitVoiceCardNames(): void {
  document.querySelectorAll<HTMLElement>(".voice-card").forEach((card) => {
    const name = card.querySelector("strong");
    if (!name) {
      return;
    }
    card.style.width = "";
    const extra = name.scrollWidth - name.clientWidth;
    if (extra > 0) {
      card.style.width = `${Math.min(720, card.offsetWidth + extra + 8)}px`;
    }
  });
}

function renderVoiceCard(voice: VoiceOption): string {
  const selected = draft.voiceId === voice.id;
  const profile = voice.profile;
  const needsGeneration = voice.source === "custom" && Boolean(profile?.reference_retained) && !voice.selectable;
  const canUse = voice.selectable && voiceBusy === null;
  const action = needsGeneration ? "set-up-clips" : "select-voice";
  const disabled = needsGeneration ? voiceBusy !== null : !canUse || selected;

  return `
    <article class="voice-card ${selected ? "selected" : ""}">
      <strong>${escapeHtml(voice.name)}</strong>
      ${voiceCardMark(voice)}
      <div class="voice-actions ${voice.source === "none" ? "" : "has-info"}">
        ${
          voice.source === "none"
            ? ""
            : `<button class="voice-info" type="button" data-action="voice-info" data-voice-id="${escapeHtml(voice.id)}">Info</button>`
        }
        <button class="voice-select" type="button" data-action="${action}" data-voice-id="${escapeHtml(voice.id)}" ${disabled ? "disabled" : ""}>${selected ? "Selected" : needsGeneration ? "Set up clips" : "Select"}</button>
      </div>
    </article>
  `;
}

function voiceCardMark(voice: VoiceOption): string {
  if (voice.source === "none") {
    return `<svg class="voice-card-mute" viewBox="0 0 24 24" aria-hidden="true"><path d="M4 9h3.2L12 5v14l-4.8-4H4V9z" fill="currentColor"/><path d="M15.2 9.2l5.6 5.6M20.8 9.2l-5.6 5.6" stroke="currentColor" stroke-width="1.8" fill="none" stroke-linecap="square"/></svg>`;
  }
  const path = voice.profile?.reference_path ?? previewClipForVoice(voice)?.path ?? "";
  return `<canvas class="voice-card-wave" width="88" height="32" data-wave-path="${attr(path)}" data-wave-bars="11"></canvas>`;
}

/** The first clip waits for the worker to load the model and the voice (about 90 s on a typical
 *  laptop); the worker stays loaded, so each later clip takes seconds (3-6 s measured on a Mac
 *  and a Windows laptop). Both rounded up, so the estimate is never the slow surprise. */
const FIRST_CLIP_SECONDS = 90;
const NEXT_CLIP_SECONDS = 15;

/** How long `count` clips take when `started` of the queue are already done. */
function clipQueueSeconds(count: number, started = 0): number {
  if (count <= 0) {
    return 0;
  }
  return started === 0 ? FIRST_CLIP_SECONDS + (count - 1) * NEXT_CLIP_SECONDS : count * NEXT_CLIP_SECONDS;
}

/** Redraws the Clips panel alone. Picking a tone or toggling editing changes nothing outside it,
 * so the voices, the tiles and the page scroll stay put and the swap takes a few milliseconds. */
function paintClipsPanel(): void {
  const panel = document.querySelector(".phrase-panel");
  if (!panel) {
    render();
    return;
  }
  const holder = document.createElement("div");
  holder.innerHTML = renderVoiceClipsPanel();
  const next = holder.firstElementChild;
  if (!next) {
    render();
    return;
  }
  panel.replaceWith(next);
  next.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "", button);
    });
  });
  bindClipsControls(next);
}

/** The voice select and the tone-rename form, inside `root`. */
function bindClipsControls(root: ParentNode): void {
  // A click anywhere on a row that is not on one of its buttons opens the line.
  root.querySelectorAll<HTMLElement>(".line-row.is-openable").forEach((row) => {
    row.addEventListener("click", (event) => {
      if ((event.target as Element).closest("button")) {
        return;
      }
      row.querySelector<HTMLButtonElement>(".line-open")?.click();
    });
  });
  root.querySelector<HTMLSelectElement>("#voice-clips-voice")?.addEventListener("change", (event) => {
    const voiceId = (event.target as HTMLSelectElement).value;
    if (!availableVoices().some((item) => item.id === voiceId)) {
      return;
    }
    clearFailedQueue();
    clipsVoiceId = voiceId;
    clipsVoiceTouched = true;
    clipsTone = null;
    phraseEditor = null;
    paintClipsPanel();
  });
  root.querySelector<HTMLFormElement>("#tone-rename")?.addEventListener("submit", (event) => {
    event.preventDefault();
    void renameTone();
  });
  root.querySelector<HTMLInputElement>("#tone-rename-input")?.addEventListener("input", (event) => {
    if (toneRename) {
      toneRename.value = (event.target as HTMLInputElement).value;
    }
  });
  root.querySelector<HTMLFormElement>("#new-tone")?.addEventListener("submit", (event) => {
    event.preventDefault();
    void createTone();
  });
  root.querySelector<HTMLInputElement>("#new-tone-input")?.addEventListener("input", (event) => {
    newToneDraft = (event.target as HTMLInputElement).value;
  });
}

function bindPhraseEditor(root: ParentNode): void {
  const phraseForm = root.querySelector<HTMLFormElement>("#phrase-editor");
  phraseForm?.addEventListener("submit", (event) => {
    event.preventDefault();
    void saveLine(true);
  });
  const text = phraseForm?.querySelector<HTMLTextAreaElement>('textarea[name="text"]');
  text?.addEventListener("input", () => {
    if (phraseEditor) {
      phraseEditor.text = text.value;
      paintLinePopupButtons();
    }
  });
  // Enter submits, as the old single-line field did; a line is one sentence, never two rows.
  text?.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !event.shiftKey && !composing(event)) {
      event.preventDefault();
      phraseForm?.requestSubmit();
    }
  });
  phraseForm?.querySelector<HTMLSelectElement>('select[name="tone"]')?.addEventListener("change", (event) => {
    if (phraseEditor) {
      phraseEditor.tone = (event.target as HTMLSelectElement).value;
    }
  });
}

/** Typing changes which buttons make sense; redraw just the popup's action row. */
function paintLinePopupButtons(): void {
  const actions = document.querySelector(".line-popup-actions");
  if (!actions) {
    return;
  }
  const holder = document.createElement("div");
  holder.innerHTML = renderLinePopup();
  const next = holder.querySelector(".line-popup-actions");
  if (!next) {
    return;
  }
  actions.replaceWith(next);
  next.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "", button);
    });
  });
}

/** A line or tones popup opening or closing repaints only the dialog layer; the page under it stays put. */
function paintVoiceLayer(): void {
  const layer = document.getElementById("voice-layer");
  if (!layer) {
    render();
    return;
  }
  layer.innerHTML = renderVoiceDialogs();
  layer.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "", button);
    });
  });
  bindClipsControls(layer);
  bindPhraseEditor(layer);
}

/** Model and memory, the two things that can stop a clip being made. Each tile opens its numbers. */
function renderVoiceTiles(): string {
  const busyDownload = ["checking", "downloading", "cancelling"].includes(voiceDownload?.phase ?? "");
  const modelOk = modelInstalled() || busyDownload;
  const modelValue = busyDownload ? "Updating…" : modelInstalled() ? "Installed" : "Not installed";
  return `
    <div class="voice-tiles">
      <div class="voice-tile ${modelOk ? "" : "is-bad"}">
        <span class="voice-tile-dot" aria-hidden="true"></span>
        <div><p class="eyebrow">Voice model</p><strong>${escapeHtml(modelValue)}</strong></div>
        <button type="button" data-action="open-model-info" aria-label="Voice model details" aria-haspopup="dialog">›</button>
      </div>
      <div class="voice-tile ${ramGenerationLocked() ? "is-bad" : ""}" data-memory-tile>
        <span class="voice-tile-dot" aria-hidden="true"></span>
        <div><p class="eyebrow">Memory</p><strong data-memory-value>${escapeHtml(memoryTileValue())}</strong></div>
        <button type="button" data-action="open-memory-info" aria-label="Memory details" aria-haspopup="dialog">›</button>
      </div>
    </div>
  `;
}

function memoryTileValue(): string {
  const system = voiceBackend?.system;
  const free = system?.available_ram_bytes ?? null;
  const need = system?.min_free_ram_bytes ?? 0;
  if (free === null) {
    return "Unknown";
  }
  return ramGenerationLocked() ? `${formatBytes(free)} free, needs ${formatBytes(need)}` : `${formatBytes(free)} free`;
}

/** Clips for one voice: its tones on the left, the viewed tone's lines on the right. */
function renderVoiceClipsPanel(): string {
  const voice = clipsVoice();
  const speaks = voiceTone(voice);
  const tones = phraseToneOrder();
  const viewed = viewedTone(voice);
  const viewedPhrases = tonePhrases(viewed);
  const speaksPhrases = tonePhrases(speaks);
  const summary = `· speaks ${toneLabel(speaks)} · ${readyClipCount(voice, speaksPhrases)} of ${speaksPhrases.length} lines ready`;
  return `
    <section class="panel phrase-panel">
      <div class="clips-strip">
        <div class="clips-for">
          <p class="eyebrow">Clips</p>
          <span class="clips-for-word">for</span>
          <select id="voice-clips-voice" aria-label="Voice to make clips for" ${clipQueue?.phase === "working" ? "disabled" : ""}>${voiceOptions(voice.id, true)}</select>
          <span class="clips-summary">${escapeHtml(summary)}</span>
        </div>
      </div>
      <div class="clips-grid">
        <div class="clips-card">
          <div class="clips-card-head">
            <p class="eyebrow">Tone</p>
            <span class="clips-card-tools"><span>${tones.length === 1 ? "1 tone" : `${tones.length} tones`}</span><button class="small-button" type="button" data-action="open-tones" aria-haspopup="dialog" ${canEditPhrases() ? "" : "disabled"}>Manage</button></span>
          </div>
          <div class="tone-list" aria-label="Tones">
            ${tones.map((tone) => renderToneRow(voice, tone, sameTone(tone, viewed), sameTone(tone, speaks))).join("")}
          </div>
        </div>
        <div class="clips-card clips-lines">${renderLinesCard(voice, viewed)}</div>
      </div>
    </section>
  `;
}

function renderLinesCard(voice: VoiceOption, viewed: VoiceTone): string {
  const viewedPhrases = tonePhrases(viewed);
  return `
    <div class="clips-card-head">
      <p class="eyebrow">${escapeHtml(toneLabel(viewed))} lines</p>
      <span>${readyClipCount(voice, viewedPhrases)} of ${viewedPhrases.length} ready</span>
    </div>
    <div class="line-list">
      ${viewedPhrases.length ? viewedPhrases.map((phrase) => renderLineRow(voice, phrase)).join("") : `<p class="empty">No lines in this tone yet. Add one below.</p>`}
    </div>
    ${renderClipsFooter(voice, viewed, viewedPhrases)}
  `;
}

/** A tone pick: the chosen row lights up and the lines card is refilled. Nothing else moves. */
function paintTonePick(): void {
  const panel = document.querySelector(".phrase-panel");
  const card = panel?.querySelector(".clips-lines");
  if (!panel || !card) {
    paintClipsPanel();
    return;
  }
  const voice = clipsVoice();
  const viewed = viewedTone(voice);
  panel.querySelectorAll<HTMLButtonElement>(".tone-row[data-tone]").forEach((row) => {
    const on = sameTone(row.dataset.tone ?? "", viewed);
    row.classList.toggle("is-on", on);
    row.setAttribute("aria-pressed", String(on));
  });
  card.innerHTML = renderLinesCard(voice, viewed);
  card.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "", button);
    });
  });
  bindClipsControls(card);
}

function renderToneRow(voice: VoiceOption, tone: VoiceTone, viewed: boolean, speaks: boolean): string {
  const phrases = tonePhrases(tone);
  const ready = readyClipCount(voice, phrases);
  const complete = phrases.length > 0 && ready === phrases.length;
  return `
    <button type="button" class="tone-row ${viewed ? "is-on" : ""}" data-action="pick-tone" data-tone="${attr(tone)}" aria-pressed="${viewed}">
      <span class="tone-dot ${complete ? "is-ready" : ""}" aria-hidden="true"></span>
      <span class="tone-name">${escapeHtml(toneLabel(tone))}</span>
      ${speaks ? `<span class="tone-speaks">speaks</span>` : ""}
      <span class="tone-count">${ready}/${phrases.length}</span>
    </button>
  `;
}

function renderLineRow(voice: VoiceOption, phrase: PhraseDefinition): string {
  const clip = clipForPhrase(voice, phrase);
  const job = clipQueue && clipQueue.voiceId === voice.id ? clipQueue : null;
  const position = job ? job.phraseIds.indexOf(phrase.id) : -1;
  const busy = Boolean(job && position === job.index && job.phase === "working");
  const failed = Boolean(job && position === job.index && job.phase === "error");
  const queued = Boolean(job && job.phase === "working" && position > job.index);
  const openable = !busy && canEditPhrases();
  const dot = `<span class="line-dot ${clip ? "is-ready" : ""}" aria-hidden="true"></span>`;
  const text = `<button class="line-open" type="button" data-action="open-phrase-editor" data-phrase-id="${attr(phrase.id)}" ${openable ? "" : "disabled"} title="Edit, regenerate or delete this line">${escapeHtml(phrase.text)}</button>`;
  const hint = openable ? `<span class="line-edit-hint" aria-hidden="true">Edit ›</span>` : "";
  let trail: string;
  if (busy) {
    trail = `<span class="line-state is-busy" data-clip-elapsed>${escapeHtml(clipElapsedText())}</span>`;
  } else if (failed) {
    trail = `<span class="line-actions"><span class="line-state is-bad" title="${attr(job?.error ?? "")}">Failed</span><button class="small-button" type="button" data-action="generate-clip" data-phrase-id="${attr(phrase.id)}">Retry</button></span>`;
  } else if (queued) {
    trail = `<span class="line-actions"><span class="line-state">Queued</span>${hint}</span>`;
  } else if (clip) {
    trail = `
      <span class="line-actions">
        ${hint}
        <button class="voice-play" type="button" data-action="play-clip" data-clip-path="${attr(clip.path)}" aria-label="Play">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.2v9.6L11 6z" fill="currentColor"/></svg>
        </button>
      </span>
    `;
  } else {
    trail = `<span class="line-actions"><span class="line-state">No clip</span>${hint}</span>`;
  }
  return `<div class="line-row ${clip ? "" : "is-missing"} ${busy ? "is-busy" : ""} ${openable ? "is-openable" : ""}">${dot}${text}${trail}</div>`;
}

function clipElapsedText(): string {
  const seconds = clipQueue ? Math.max(0, Math.round((performance.now() - clipQueue.startedAt) / 1000)) : 0;
  const expected = clipQueue && clipQueue.index > 0 ? NEXT_CLIP_SECONDS : FIRST_CLIP_SECONDS;
  return `Making · ${seconds} s of ~${expected} s`;
}

function paintClipQueue(): void {
  const label = document.querySelector("[data-clip-elapsed]");
  if (label && clipQueue?.phase === "working") {
    label.textContent = clipElapsedText();
  }
}

/** "4½ min", "1 min", "45 s". */
function formatQueueTime(totalSeconds: number): string {
  if (totalSeconds < 60) {
    return `${totalSeconds} s`;
  }
  const halves = Math.round(totalSeconds / 30);
  const whole = Math.floor(halves / 2);
  return `${whole || ""}${halves % 2 ? "½" : ""} min`.replace(/^½/, "½").trim();
}

function renderClipsFooter(voice: VoiceOption, viewed: VoiceTone, phrases: PhraseDefinition[]): string {
  const speaks = voiceTone(voice);
  const addLine = `<button class="small-button" type="button" data-action="add-line" data-tone="${attr(viewed)}" ${canEditPhrases() ? "" : "disabled"}>+ Add line</button>`;
  const job = clipQueue && clipQueue.voiceId === voice.id ? clipQueue : null;
  if (job && job.phase === "working") {
    const total = job.phraseIds.length;
    const left = clipQueueSeconds(Math.max(1, total - job.index), job.index);
    return `
      <div class="clips-foot">
        <button class="small-button danger" type="button" data-action="stop-clip-queue" ${job.stopAfter ? "disabled" : ""}>${job.stopAfter ? "Stopping after this line" : "Stop after this line"}</button>
        <span class="clips-hint">${job.index} of ${total} done · about ${escapeHtml(formatQueueTime(left))} left</span>
      </div>
    `;
  }
  const useTone = sameTone(viewed, speaks)
    ? ""
    : `<button class="small-button" type="button" data-action="use-tone" data-tone="${attr(viewed)}" ${voice.profile && voiceBusy === null ? "" : "disabled"}>Use this tone</button>`;
  if (phrases.length === 0) {
    return `
      <div class="clips-foot">
        <div class="clips-foot-actions">${addLine}${useTone}</div>
        <span class="clips-hint">A tone needs at least one line before a voice can speak it.</span>
      </div>
    `;
  }
  const missing = phrases.filter((phrase) => !clipForPhrase(voice, phrase));
  if (missing.length === 0) {
    return `
      <div class="clips-foot">
        <div class="clips-foot-actions"><span class="clips-hint is-ready">All ${phrases.length} lines ready.</span>${useTone}${addLine}</div>
      </div>
    `;
  }
  const lock = clipGenerateLockTitle(voice);
  const tileBad = !modelInstalled() || ramGenerationLocked();
  // A failed line says why here, where the eye already is; the row only says "Failed".
  const failed = job?.phase === "error" ? job.error || "The clip could not be made." : "";
  const hint = tileBad
    ? "Fix the red card above first."
    : failed
      ? `Last line failed: ${failed}`
      : lock || `The first clip takes about ${FIRST_CLIP_SECONDS} s while the model loads, then about ${NEXT_CLIP_SECONDS} s each. Sounds off? Open the line and regenerate it alone.`;
  return `
    <div class="clips-foot">
      <div class="clips-foot-actions">
        <button class="primary small-button clips-generate" type="button" data-action="generate-missing" ${lock || clipQueue?.phase === "working" ? "disabled" : ""}>Generate ${missing.length} missing · ~${escapeHtml(formatQueueTime(clipQueueSeconds(missing.length)))}</button>
        ${useTone}
        ${addLine}
      </div>
      <span class="clips-hint ${failed && !tileBad ? "is-bad" : ""}" role="${failed ? "alert" : "status"}">${escapeHtml(hint)}</span>
    </div>
  `;
}

/** One popup per line: edit the words, move it, make its clip, or delete it. The primary button
 * says what it will do, and one click does all of it. */
function renderLinePopup(): string {
  const editor = phraseEditor;
  if (!editor) {
    return "";
  }
  const voice = clipsVoice();
  const existing = editor.phraseId ? effectivePhrases().find((item) => item.id === editor.phraseId) ?? null : null;
  const clip = existing ? clipForPhrase(voice, existing) : null;
  const text = editor.text.trim();
  const changed = existing ? text !== existing.text : text.length > 0;
  const lock = clipQueue?.phase === "working" ? "Wait for the current line to finish" : clipGenerateLockTitle(voice);
  const primary = !existing ? "Add and generate" : changed ? "Save and regenerate" : clip ? "Regenerate" : "Generate";
  const primaryDisabled = Boolean(lock) || !text || voiceBusy !== null;
  const status = clip
    ? `<span class="line-clip-status is-ready">Clip ready</span>
       <button class="small-button line-play" type="button" data-action="play-clip" data-clip-path="${attr(clip.path)}">
         <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.2v9.6L11 6z" fill="currentColor"/></svg>Play
       </button>`
    : `<span class="line-clip-status">No clip yet</span>`;
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup line-popup" role="dialog" aria-modal="true" aria-labelledby="line-popup-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">${escapeHtml(voice.name)}</p>
            <h2 id="line-popup-title">${existing ? "Line" : "New line"}</h2>
          </div>
          <button class="small-button" type="button" data-action="cancel-phrase-editor">Close</button>
        </div>
        <form class="phrase-editor" id="phrase-editor">
          <label>
            <span>Line</span>
            <textarea id="phrase-editor-text" name="text" rows="3" maxlength="140" placeholder="Back to the task." autocomplete="off">${escapeHtml(editor.text)}</textarea>
          </label>
          <label>
            <span>Tone</span>
            <select id="phrase-editor-tone" name="tone">${phraseToneOptions(editor.tone)}</select>
          </label>
          <div class="line-clip-row">${status}</div>
          <div class="modal-actions line-popup-actions">
            ${existing ? `<button class="danger" type="button" data-action="delete-phrase" data-phrase-id="${attr(existing.id)}" ${canEditPhrases() ? "" : "disabled"}>Delete line</button>` : ""}
            ${changed || !existing ? `<button type="button" data-action="save-line" ${text && canEditPhrases() ? "" : "disabled"}>${existing ? "Save only" : "Add only"}</button>` : ""}
            <button class="primary" type="submit" ${primaryDisabled ? "disabled" : ""} title="${attr(lock)}">${primary}</button>
          </div>
        </form>
        <p class="quiet">${escapeHtml(lock ? lock : `Up to ${FIRST_CLIP_SECONDS} seconds while the model loads, a few seconds once it has. A changed line keeps its old clip until a new one is made.`)}</p>
      </section>
    </div>
  `;
}

/** Every tone in one place: rename or delete your own, add a new one. Built-in tones stay. */
function renderTonesModal(): string {
  const voice = clipsVoice();
  const rows = phraseToneOrder()
    .map((tone) => {
      const count = tonePhrases(tone).length;
      const lines = count === 1 ? "1 line" : `${count} lines`;
      let tools: string;
      if (!isCustomTone(tone)) {
        tools = `<span class="tone-manage-note">Built in</span>`;
      } else if (toneRename && sameTone(toneRename.from, tone)) {
        tools = `
          <form class="tone-rename" id="tone-rename">
            <input id="tone-rename-input" maxlength="80" value="${attr(toneRename.value)}" aria-label="Tone name" autocomplete="off" />
            <button class="primary small-button" type="submit">Save</button>
            <button class="small-button" type="button" data-action="cancel-rename-tone">Cancel</button>
          </form>`;
      } else if (toneDeleteArmed && sameTone(toneDeleteArmed, tone)) {
        tools = `
          <span class="tone-manage-note">Delete ${escapeHtml(toneLabel(tone))} and its ${lines}?</span>
          <button class="small-button danger" type="button" data-action="delete-tone" data-tone="${attr(tone)}">Delete</button>
          <button class="small-button" type="button" data-action="cancel-delete-tone">Cancel</button>`;
      } else {
        tools = `
          <button class="small-button" type="button" data-action="rename-tone" data-tone="${attr(tone)}" ${canEditPhrases() ? "" : "disabled"}>Rename</button>
          <button class="small-button danger" type="button" data-action="arm-delete-tone" data-tone="${attr(tone)}" ${canEditPhrases() ? "" : "disabled"}>Delete</button>`;
      }
      const speaks = sameTone(tone, voiceTone(voice)) ? `<span class="tone-speaks">${escapeHtml(voice.name)} speaks</span>` : "";
      return `
        <div class="tone-manage-row">
          <div class="tone-manage-name"><strong>${escapeHtml(toneLabel(tone))}</strong>${speaks}<span class="tone-count">${lines}</span></div>
          <div class="tone-manage-tools">${tools}</div>
        </div>
      `;
    })
    .join("");
  const adder =
    newToneDraft === null
      ? `<button class="small-button" type="button" data-action="new-tone" ${canEditPhrases() ? "" : "disabled"}>+ New tone</button>`
      : `<form class="tone-rename" id="new-tone">
          <input id="new-tone-input" maxlength="80" value="${attr(newToneDraft)}" placeholder="Coach, dry humor, drill sergeant…" aria-label="New tone name" autocomplete="off" />
          <button class="primary small-button" type="submit">Create</button>
          <button class="small-button" type="button" data-action="cancel-new-tone">Cancel</button>
        </form>`;
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup tones-popup" role="dialog" aria-modal="true" aria-labelledby="tones-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">Clips</p>
            <h2 id="tones-title">Tones</h2>
          </div>
          <button class="small-button" type="button" data-action="close-tones">Close</button>
        </div>
        <p>A tone is a set of lines. A voice speaks one tone in sessions; renaming a tone follows it everywhere, deleting one removes its lines.</p>
        <div class="tone-manage-list">${rows}</div>
        <div class="modal-actions">${adder}</div>
      </section>
    </div>
  `;
}

function closeTonesPopup(): void {
  tonesOpen = false;
  toneRename = null;
  toneDeleteArmed = null;
  newToneDraft = null;
}

function renderVoiceHelpModal(): string {
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup" role="dialog" aria-modal="true" aria-labelledby="voice-help-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">Voices</p>
            <h2 id="voice-help-title">How voice cloning works</h2>
          </div>
          <button class="small-button" type="button" data-action="close-voice-help">Close</button>
        </div>
        <p>Supervisor nudges you in short spoken clips. Record anyone for about twenty seconds, make their clips once, and from then on those clips play instantly, offline, without the model.</p>
        <ol class="voice-help-steps">
          <li><strong>Record a voice.</strong> Press + New voice and read the prompt aloud. The starter voices come ready.</li>
          <li><strong>Make its clips.</strong> Under Clips, pick the voice and a tone, then press Generate. The first takes about ${FIRST_CLIP_SECONDS} seconds while the model loads, the rest about ${NEXT_CLIP_SECONDS} seconds each. It needs the voice model installed and some free memory.</li>
          <li><strong>Select it.</strong> A voice speaks one tone. Once every line in that tone has a clip, Select it under Voices.</li>
        </ol>
        <p class="quiet">Listen to each clip. If a line sounds off, press Redo on that line alone.</p>
      </section>
    </div>
  `;
}

function renderModelInfoModal(): string {
  const model = voiceBackend?.model;
  const runtime = voiceBackend?.runtime;
  const installed = modelInstalled();
  const files = model ? `${model.files.filter((file) => file.valid).length} of ${model.files.length}` : "—";
  const busy = voiceBusy !== null || ["checking", "downloading", "cancelling"].includes(voiceDownload?.phase ?? "");
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup" role="dialog" aria-modal="true" aria-labelledby="model-info-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">Voice model</p>
            <h2 id="model-info-title">${installed ? "Installed" : "Not installed"}</h2>
          </div>
          <button class="small-button" type="button" data-action="close-model-info">Close</button>
        </div>
        <div class="voice-kv">
          <div><span>On disk</span><strong>${escapeHtml(model ? formatBytes(model.installed_bytes) : "—")}</strong></div>
          <div><span>Files</span><strong>${escapeHtml(files)}</strong></div>
          <div><span>Runtime</span><strong>${escapeHtml(runtime?.installed ? runtime.version ?? "installed" : "missing")}</strong></div>
        </div>
        <p>${
          installed && voiceDownload?.phase === "complete" && voiceDownload.up_to_date
            ? "Up to date: every file matches the pack, nothing to download. Loaded only while a clip is being made."
            : installed
            ? "Loaded only while a clip is being made. Check for updates downloads only what is missing from the pack."
            : `A one-time download of about ${escapeHtml(formatBytes(model?.required_bytes ?? 0))}. Saved clips play without it; making new ones needs it.`
        }</p>
        <div class="modal-actions">
          ${installed ? `<button class="danger" type="button" data-action="ask-delete-model" ${busy ? "disabled" : ""}>Delete model</button>` : ""}
          <button class="${installed ? "" : "primary"}" type="button" data-action="download-model-pack" ${busy ? "disabled" : ""}>${installed ? "Check for updates" : "Download model"}</button>
        </div>
      </section>
    </div>
  `;
}

function renderMemoryInfoModal(): string {
  const system = voiceBackend?.system;
  const free = system?.available_ram_bytes ?? null;
  const need = system?.min_free_ram_bytes ?? 0;
  const peak = system?.last_peak_ram_bytes ?? null;
  const locked = ramGenerationLocked();
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup" role="dialog" aria-modal="true" aria-labelledby="memory-info-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">Memory</p>
            <h2 id="memory-info-title">${locked ? "Not enough free memory" : "Enough memory"}</h2>
          </div>
          <button class="small-button" type="button" data-action="close-memory-info">Close</button>
        </div>
        <p>Making clips loads the voice model (about ${FIRST_CLIP_SECONDS} seconds the first time) and needs:</p>
        <div class="voice-kv">
          <div><span>Needs free</span><strong>${escapeHtml(need ? formatBytes(need) : "—")}</strong></div>
          <div><span>Free now</span><strong class="${locked ? "is-bad" : ""}" data-ram-free>${escapeHtml(free === null ? "—" : formatBytes(free))}</strong></div>
          <div><span>In this machine</span><strong data-ram-machine>${escapeHtml(system?.total_ram_bytes ? formatBytes(system.total_ram_bytes) : "—")}</strong></div>
        </div>
        <p data-ram-copy>${escapeHtml(ramLockCopy(locked, peak))}</p>
        <p class="quiet">Clips you already have keep playing. They never need the model or this memory.</p>
        <div class="modal-actions">
          <button class="${locked ? "primary" : ""}" type="button" data-action="check-ram">Check again</button>
        </div>
      </section>
    </div>
  `;
}

function ramLockCopy(locked: boolean, peak: number | null): string {
  if (locked) {
    return "Close other apps, then check again.";
  }
  return peak ? `Enough right now. The last clip peaked at ${formatBytes(peak)}; the memory is freed again after each clip.` : "Enough right now. The memory is freed again after each clip.";
}

/** The memory tile and popup follow the live numbers without a full render. */
function paintVoiceRam(): void {
  if (!voiceBackend) {
    return;
  }
  const system = voiceBackend.system;
  const free = system.available_ram_bytes ?? null;
  const locked = ramGenerationLocked();
  const value = document.querySelector("[data-memory-value]");
  if (value) {
    value.textContent = memoryTileValue();
    value.closest("[data-memory-tile]")?.classList.toggle("is-bad", locked);
  }
  document.querySelectorAll("[data-ram-copy]").forEach((copy) => {
    copy.textContent = ramLockCopy(locked, system.last_peak_ram_bytes ?? null);
  });
  document.querySelectorAll("[data-ram-free]").forEach((node) => {
    node.textContent = free === null ? "—" : formatBytes(free);
    node.classList.toggle("is-bad", locked);
  });
}

function renderVoiceDialogs(): string {
  if (appView !== "main") {
    return "";
  }

  const showVoiceDialog = activeTab === "settings" && settingsView === "voice";
  return `
    ${showVoiceDialog && recordingOpen ? renderRecordingModal() : ""}
    ${showVoiceDialog && voiceInfoId && !voiceDeleteTargetId && !voiceBusy ? renderVoiceInfoModal() : ""}
    ${showVoiceDialog && voiceDeleteTargetId ? renderDeleteVoiceModal() : ""}
    ${showVoiceDialog && modelDeleteOpen ? renderDeleteModelModal() : ""}
    ${showVoiceDialog && modelInfoOpen && !modelDeleteOpen && !voiceBusy ? renderModelInfoModal() : ""}
    ${showVoiceDialog && memoryInfoOpen && !voiceBusy ? renderMemoryInfoModal() : ""}
    ${showVoiceDialog && voiceHelpOpen ? renderVoiceHelpModal() : ""}
    ${showVoiceDialog && phraseEditor && !voiceDeleteTargetId ? renderLinePopup() : ""}
    ${showVoiceDialog && tonesOpen && !phraseEditor ? renderTonesModal() : ""}
    ${voiceBusy && !clipQueue ? renderVoiceBusyModal() : ""}
  `;
}

/** Size first (it's tiny), then what deleting costs. */
function deleteAllCopy(bytes: number | null | undefined): string[] {
  const cost =
    "It's also how Supervisor shows you where your time actually goes. Delete it and your attention span, trends and recaps start over from nothing. This can't be undone.";
  if (!bytes) {
    return [cost];
  }
  const ratio = PHOTO_BURST_BYTES / bytes;
  const comparison =
    ratio >= 2
      ? `so deleting it frees about 1/${Math.round(ratio)} of a single burst`
      : ratio >= 0.5
        ? "so it's about one burst"
        : `so it's about ${Math.round(1 / ratio)} bursts`;
  return [
    `All of it takes up ${formatBytes(bytes)}. A quick burst of phone photos is around 30 MB, ${comparison}.`,
    cost,
  ];
}

function renderHistoryConfirm(): string {
  if (!historyConfirm) {
    return "";
  }

  const copy =
    historyConfirm.kind === "delete-all"
      ? {
          eyebrow: "YOUR DATA",
          title: "Delete all history?",
          body: deleteAllCopy(historyConfirm.bytes),
          cancel: "Keep my history",
          confirm: "Delete everything",
          danger: true,
        }
      : {
          eyebrow: "HISTORY",
          title: "Delete this session?",
          body: ["This session will be removed from this computer. You cannot undo this."],
          cancel: "Cancel",
          confirm: "Delete session",
          danger: true,
        };

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel confirm-panel" role="dialog" aria-modal="true" aria-labelledby="history-confirm-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">${copy.eyebrow}</p>
            <h2 id="history-confirm-title">${escapeHtml(copy.title)}</h2>
          </div>
        </div>
        ${copy.body.map((line) => `<p>${escapeHtml(line)}</p>`).join("")}
        <div class="modal-actions">
          <button type="button" data-action="cancel-history-confirm">${escapeHtml(copy.cancel)}</button>
          <button class="${copy.danger ? "danger" : "primary"}" type="button" data-action="confirm-history">${escapeHtml(copy.confirm)}</button>
        </div>
      </section>
    </div>
  `;
}

function renderRecordingModal(): string {
  const isRecording = recordingState === "recording";
  const canSave = Boolean(recordedVoice?.bytes && recordedVoice.name.trim() && voiceBusy === null);

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel recording-panel" role="dialog" aria-modal="true" aria-labelledby="record-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">LOCAL CLONE</p>
            <h2 id="record-title">Record voice</h2>
          </div>
          <button class="small-button" type="button" data-action="close-recording">Close</button>
        </div>

        <div class="recording-grid">
          <section class="recording-stage">
            <button class="mic-button ${isRecording ? "recording" : ""}" type="button" data-action="${isRecording ? "stop-recording" : "start-recording"}" ${recordedVoice && !isRecording ? "disabled" : ""} aria-label="${isRecording ? "Stop recording" : "Start recording"}">
              <span></span>
            </button>
            <strong>${isRecording ? "Recording" : recordedVoice ? "Clip saved" : "Ready"}</strong>
            <p id="recording-elapsed">${isRecording ? formatDuration(recordingDurationSeconds()) : recordedVoice ? `${formatDuration(recordedVoice.seconds)} captured` : "Use a clear 20-30 second sample with minimal background noise."}</p>
            ${recordingError ? `<p class="recording-error">${escapeHtml(recordingError)}</p>` : ""}
          </section>

          <section class="recording-review">
            ${
              recordedVoice
                ? `
                  <label>
                    <span>Voice name</span>
                    <input id="voice-name" maxlength="80" value="${attr(recordedVoice.name)}" placeholder="Mom voice" />
                  </label>
                  <audio controls preload="metadata" src="${attr(recordedVoice.url)}"></audio>
                  <div class="modal-actions">
                    <button type="button" data-action="rerecord-voice">Re-record</button>
                    <button class="primary" type="button" data-action="save-recorded-voice" ${canSave ? "" : "disabled"}>Save voice</button>
                  </div>
                  <p class="quiet">Saving is instant. You pick which phrase sets to generate next.</p>
                `
                : `<p class="empty">The clip will appear here after recording so you can play it back before generation.</p>`
            }
          </section>
        </div>
      </section>
    </div>
  `;
}

function renderDeleteModelModal(): string {
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel confirm-panel" role="dialog" aria-modal="true" aria-labelledby="delete-model-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">VOICE MODEL</p>
            <h2 id="delete-model-title">Delete the voice model?</h2>
          </div>
        </div>
        <p>Saved voices and clips stay on this device. You will need to download the model again to generate new clips.</p>
        <div class="modal-actions">
          <button type="button" data-action="cancel-delete-model">Cancel</button>
          <button class="danger" type="button" data-action="delete-model-files">Delete model</button>
        </div>
      </section>
    </div>
  `;
}

function renderVoiceInfoModal(): string {
  const voice = availableVoices().find((item) => item.id === voiceInfoId);
  if (!voice) {
    return "";
  }

  const clip = previewClipForVoice(voice);
  const profileId = voice.source === "custom" ? voice.profile?.profile_id ?? "" : "";

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-info-panel" role="dialog" aria-modal="true" aria-labelledby="voice-info-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">Voice preview</p>
            <h2 id="voice-info-title">${escapeHtml(voice.name)}</h2>
          </div>
          <button class="small-button" type="button" data-action="close-voice-info">Close</button>
        </div>
        <div class="voice-info-transport">
          <button class="voice-preview-play" type="button" data-action="toggle-voice-preview" ${clip ? "" : "disabled"} aria-label="Play">
            <span class="voice-preview-icon" aria-hidden="true"></span>
          </button>
          <div class="voice-info-wave">
            <canvas id="voice-preview-wave" width="320" height="56" data-wave-path="${attr(clip?.path ?? "")}" data-wave-bars="40"></canvas>
            <input id="voice-preview-bar" type="range" min="0" max="1" value="0" step="0.01" ${clip ? "" : "disabled"} aria-label="Playback" />
            <div class="voice-info-times">
              <span id="voice-preview-elapsed">0:00</span>
              <span id="voice-preview-total">0:00</span>
            </div>
          </div>
        </div>
        <div class="voice-info-foot">
          <button class="danger" type="button" data-action="confirm-delete-voice" data-profile-id="${attr(profileId)}"${profileId ? "" : ' disabled title="Starter voices stay on this device."'}>Delete voice</button>
        </div>
      </section>
    </div>
  `;
}

function renderDeleteVoiceModal(): string {
  const profile = voiceDeleteTargetId ? profileById(voiceDeleteTargetId) : null;
  if (!profile) {
    return "";
  }

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel confirm-panel" role="dialog" aria-modal="true" aria-labelledby="delete-voice-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">VOICE DELETE</p>
            <h2 id="delete-voice-title">${escapeHtml(profile.display_name)}</h2>
          </div>
          <button class="small-button" type="button" data-action="cancel-delete-voice">Close</button>
        </div>
        <p class="quiet">Clips can be generated again from the recording. Deleting the voice removes the recording too.</p>
        <div class="delete-choice-grid">
          <button type="button" data-action="delete-voice-files" ${profile.clip_count === 0 ? "disabled" : ""}>Delete clips, keep the recording</button>
          <button class="danger" type="button" data-action="delete-voice-profile">Delete the voice</button>
        </div>
      </section>
    </div>
  `;
}

function renderVoiceBusyModal(): string {
  if (voiceBusy === "download-model") {
    return renderDownloadBusyModal();
  }

  return `
    <div class="modal-backdrop busy-backdrop" role="presentation">
      <section class="modal-panel busy-panel" role="dialog" aria-modal="true" aria-live="polite">
        <p class="eyebrow">VOICE WORKER</p>
        <h2>${escapeHtml(voiceBusyTitle())}</h2>
        <p>${escapeHtml(voiceBusyCopy())}</p>
        <div class="loading-bar"><span></span></div>
      </section>
    </div>
  `;
}

function renderDownloadBusyModal(): string {
  const download = voiceDownload;
  const phase = download?.phase ?? "downloading";
  const done = phase === "complete";
  const checking = phase === "checking";
  const total = Math.max(0, download?.total_bytes ?? voiceBackend?.model.required_bytes ?? 0);
  const downloaded = Math.max(0, Math.min(download?.downloaded_bytes ?? 0, total || Number.MAX_SAFE_INTEGER));
  const percent = done ? 100 : total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  const upToDate = done && Boolean(download?.up_to_date);
  const title = upToDate
    ? "Up to date"
    : done
      ? "Model pack ready"
      : phase === "checking"
        ? "Checking installed files"
        : phase === "cancelling"
          ? "Cancelling download"
          : "Downloading model pack";
  const copy = upToDate
    ? "The installed model and voice runtime match the latest pack. Nothing was downloaded."
    : done
      ? "The voice model is installed. Saved clips play without keeping this window open."
      : checking
        ? "Comparing the installed files with the pack. Nothing is downloading."
        : download?.message ?? "Downloading model pack.";
  const eta = done
    ? "Done"
    : phase === "checking"
      ? "A few seconds"
      : phase === "cancelling"
        ? "Deleting partial files"
        : download?.eta_seconds === null || download?.eta_seconds === undefined
          ? "Estimating time left"
          : `${formatDuration(download.eta_seconds)} left`;
  const progressText = checking
    ? "Verifying"
    : total > 0
      ? `${formatBytes(downloaded)} / ${formatBytes(total)}`
      : formatBytes(downloaded);
  const note = done
    ? "You can close this and start cloning a voice."
    : checking
      ? "Nothing is downloaded unless part of the pack is missing."
      : `This is a ${total > 0 ? formatBytes(total) : "large"} download and can take several minutes depending on your connection.`;

  return `
    <div class="modal-backdrop busy-backdrop" role="presentation">
      <section class="modal-panel busy-panel" role="dialog" aria-modal="true" aria-live="polite">
        <p class="eyebrow">VOICE WORKER</p>
        <h2>${escapeHtml(title)}</h2>
        <p>${escapeHtml(copy)}</p>
        ${
          upToDate
            ? ""
            : checking
              ? `<div class="loading-bar" aria-label="Checking installed files"><span></span></div>`
              : `<div class="loading-bar determinate" aria-label="Model download progress"><span style="width: ${percent}%"></span></div>`
        }
        ${
          upToDate
            ? ""
            : `<div class="download-meta">
          <span>${escapeHtml(progressText)}</span>
          <span>${escapeHtml(eta)}</span>
        </div>`
        }
        <p class="quiet busy-note">${escapeHtml(note)}</p>
        <div class="modal-actions">
          ${
            done || phase === "cancelling" || phase === "error"
              ? `<button type="button" data-action="close-model-download">Close</button>`
              : `<button class="danger" type="button" data-action="cancel-model-download">${checking ? "Cancel" : "Cancel download"}</button>`
          }
        </div>
      </section>
    </div>
  `;
}

function renderHistory(): string {
  if (historyView === "report" && sessionReport) {
    return renderSessionReport(sessionReport);
  }
  if (historyView === "trends") {
    return renderTrends();
  }

  const weekStart = historyWeek?.week_start || historyWeekStart || thisMondayKey();
  const sessions = historyWeek?.sessions ?? [];
  const recap = weekRecap && weekRecap.start === weekStart ? weekRecap : null;
  const searching = historySearch.trim().length > 0;
  // Both charts rise together on the first draw that has the week's data, and on arriving at the
  // tab. The refresh that lands a moment after arrival redraws inside the window, so it restarts
  // the rise a few ms in rather than cutting it short; a pane switch or a poll later leaves the
  // bars standing.
  const animate = recap !== null && (historyChartsFor !== weekStart || performance.now() < historyAnimateUntil);
  if (recap) {
    historyChartsFor = weekStart;
  }
  const emptyWeek = historyWeek?.oldest_day
    ? "No sessions this week."
    : "Stop a session and the local report lands here. Nothing leaves this machine.";
  return `
    <section class="history-page">
      ${renderHistoryToolbar(weekStart)}
      ${
        searching
          ? renderSearchResults()
          : `
        <div class="history-hero">
          ${renderWeekHero(weekStart, recap, animate)}
          ${renderTrendsDoor(weekStart, recap, animate)}
        </div>
        <div class="history-seg-row">
          <div class="history-seg" role="group" aria-label="Show">
            <button type="button" class="${historyPane === "sessions" ? "is-on" : ""}" aria-pressed="${historyPane === "sessions"}" data-action="history-pane" data-pane="sessions">Sessions · ${sessions.length}</button>
            <button type="button" class="${historyPane === "apps" ? "is-on" : ""}" aria-pressed="${historyPane === "apps"}" data-action="history-pane" data-pane="apps">Apps &amp; sites · ${recap?.apps.length ?? 0}</button>
          </div>
        </div>
        ${historyPane === "apps" ? renderWeekAppsPane(recap) : renderSessionsPane(sessions, emptyWeek)}
      `
      }
    </section>
  `;
}

function renderHistoryToolbar(weekStart: string): string {
  const oldestMonday = historyWeek?.oldest_day ? mondayKey(historyWeek.oldest_day) : null;
  const current = thisMondayKey();
  const prevDisabled = !oldestMonday || weekStart <= oldestMonday;
  const nextDisabled = weekStart >= current;
  const tag = weekStart === current ? "This week" : weekStart === addDaysKey(current, -7) ? "Last week" : "";
  return `
    <div class="panel history-toolbar">
      <div class="history-week-nav">
        <button type="button" class="history-week-arrow" data-action="history-week-prev" ${prevDisabled ? "disabled" : ""} aria-label="Previous week">‹</button>
        <button type="button" class="history-week-label ${weekPickerOpen ? "is-open" : ""}" data-action="toggle-week-picker" aria-expanded="${weekPickerOpen}" aria-haspopup="dialog" title="Pick a week">
          <h2>${escapeHtml(formatWeekRange(weekStart))}, ${parseDayKey(weekStart).getFullYear()}${tag ? `<span>${tag}</span>` : ""}</h2>
        </button>
        <button type="button" class="history-week-arrow" data-action="history-week-next" ${nextDisabled ? "disabled" : ""} aria-label="Next week">›</button>
        ${weekStart === current ? "" : `<button type="button" class="history-week-today" data-action="history-week-today">This week</button>`}
      </div>
      ${weekPickerOpen ? renderWeekPicker(weekStart, oldestMonday) : ""}
      <label class="history-search">
        <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-4-4"/></svg>
        <input id="history-search" type="search" value="${attr(historySearch)}" placeholder="Search all sessions" aria-label="Search sessions by goal" autocomplete="off" spellcheck="false" />
      </label>
    </div>
  `;
}

const WEEKDAY_LABELS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/** First day of the month `key` falls in. */
function monthKey(key: string): string {
  return `${key.slice(0, 7)}-01`;
}

function addMonthsKey(monthStart: string, months: number): string {
  const date = parseDayKey(monthStart);
  date.setMonth(date.getMonth() + months);
  return monthKey(dayKey(date));
}

/** The Monday on or before the 1st; six rows of seven cover any month. */
function pickerGridStart(monthStart: string): string {
  return mondayKey(monthStart);
}

const PICKER_GRID_DAYS = 42;

/** A month of days, shaded by focused time, with the viewed week outlined; a click picks that
 * day's week. */
function renderWeekPicker(weekStart: string, oldestMonday: string | null): string {
  const month = pickerMonth || monthKey(weekStart);
  const today = dayKey(new Date());
  const currentMonth = monthKey(today);
  const weekEnd = addDaysKey(weekStart, 6);
  const gridStart = pickerGridStart(month);
  const prevDisabled = !oldestMonday || month <= monthKey(oldestMonday);
  const nextDisabled = month >= currentMonth;
  const title = parseDayKey(month).toLocaleDateString(undefined, { month: "long", year: "numeric" });
  const cells = Array.from({ length: PICKER_GRID_DAYS }, (_, index) => {
    const day = addDaysKey(gridStart, index);
    const inMonth = day.slice(0, 7) === month.slice(0, 7);
    // Nothing before the first recorded week, same bound as the ‹ arrow.
    const future = day > today || (oldestMonday !== null && mondayKey(day) < oldestMonday);
    const focus = pickerDays[day]?.focus_seconds ?? 0;
    const level = focus <= 0 ? "" : focus >= 90 * 60 ? "l3" : focus >= 25 * 60 ? "l2" : "l1";
    const classes = [
      "wp-day",
      inMonth ? "" : "is-out",
      future ? "is-future" : "",
      level,
      day >= weekStart && day <= weekEnd ? "is-selected" : "",
      day === today ? "is-today" : "",
    ]
      .filter(Boolean)
      .join(" ");
    const info = pickerDays[day];
    const label = info
      ? `${parseDayKey(day).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" })}, ${formatHours(info.focus_seconds)} focused`
      : parseDayKey(day).toLocaleDateString(undefined, { weekday: "short", month: "short", day: "numeric" });
    return `<button type="button" class="${classes}" data-action="pick-week" data-day="${day}" ${future ? "disabled" : ""} aria-label="${attr(label)}" title="${attr(label)}">${Number(day.slice(8))}</button>`;
  }).join("");
  return `
    <div class="panel week-picker" role="dialog" aria-label="Pick a week">
      <div class="week-picker-head">
        <button type="button" data-action="picker-month-prev" ${prevDisabled ? "disabled" : ""} aria-label="Previous month">‹</button>
        <strong>${escapeHtml(title)}</strong>
        <button type="button" data-action="picker-month-next" ${nextDisabled ? "disabled" : ""} aria-label="Next month">›</button>
      </div>
      <div class="week-picker-grid">
        ${["M", "T", "W", "T", "F", "S", "S"].map((d) => `<span class="wp-dow" aria-hidden="true">${d}</span>`).join("")}
        ${cells}
      </div>
      <p class="week-picker-note">Click any day to see its week. Greener days had more focused time.</p>
    </div>
  `;
}

/** Loads the days with sessions for the month the picker shows. */
async function loadPickerDays(): Promise<void> {
  const month = pickerMonth || monthKey(historyWeekStart || thisMondayKey());
  const start = pickerGridStart(month);
  const request = ++pickerRequest;
  try {
    const days = await invoke<DailyFocus[]>("get_focus_days", { start, end: addDaysKey(start, PICKER_GRID_DAYS) });
    if (request !== pickerRequest) {
      return;
    }
    pickerDays = Object.fromEntries(days.map((day) => [day.date, day]));
  } catch (error) {
    if (request !== pickerRequest) {
      return;
    }
    lastError = errorText(error);
  }
  render();
}

function closeWeekPicker(): void {
  weekPickerOpen = false;
  pickerMonth = "";
  pickerDays = {};
  pickerRequest += 1;
}

/** Focused hours, longest stretch and recovery for the week, over its daily bars. */
function renderWeekHero(weekStart: string, recap: WeekRecap | null, animate: boolean): string {
  const current = weekStart === thisMondayKey();
  const empty = recap !== null && recap.session_count === 0;
  const note = !recap
    ? ""
    : empty
      ? current
        ? "No sessions yet this week. Finish one\nand today's bar fills in."
        : "No sessions that week."
      : current && recap.session_count === 1
        ? "1 session so far"
        : `${recap.session_count} sessions${current ? " so far" : ""}`;
  const buckets = recap?.buckets ?? WEEKDAY_LABELS.map((label) => ({ label, focus_seconds: 0 }));
  const max = Math.max(...buckets.map((bucket) => bucket.focus_seconds), 1);
  const todayIndex = current ? (new Date().getDay() + 6) % 7 : -1;
  const longest = recap?.longest_stretch_seconds ? formatStretchChip(recap.longest_stretch_seconds) : "—";
  const recovery =
    recap?.recovery_median_seconds != null
      ? formatRecovery(recap.recovery_median_seconds)
      : recap && recap.session_count > 0
        ? "None"
        : "—";
  return `
    <section class="panel week-hero" aria-label="${current ? "This week" : "That week"}">
      <p class="eyebrow">${current ? "This week" : "Focused time"}</p>
      <div class="week-hero-top">
        <div class="week-hero-figure">
          <strong class="week-hero-value">${escapeHtml(formatHours(recap?.focus_seconds ?? 0))}</strong>
          ${note ? `<p class="week-hero-note">${escapeHtml(note)}</p>` : ""}
        </div>
        <div class="week-hero-stats">
          <div class="week-hero-stat" title="${empty ? "Longest unbroken run of work, once there is one" : "Longest unbroken run of work"}"><span class="eyebrow">Longest stretch</span><strong>${escapeHtml(longest)}</strong></div>
          <div class="week-hero-stat" title="${recovery === "None" ? "No distraction pulled you off work this week, so there was nothing to recover from" : "Typical time back to work after a distraction"}"><span class="eyebrow">Recovery</span><strong>${escapeHtml(recovery)}</strong></div>
        </div>
      </div>
      <div class="recap-bars week-hero-bars ${animate ? "is-animated" : ""}" style="grid-template-columns: repeat(${buckets.length}, minmax(0, 1fr))">
        ${buckets
          .map((bucket, index) => {
            const on = bucket.focus_seconds > 0;
            const focused = formatHours(bucket.focus_seconds);
            const classes = ["recap-bar", on ? "is-on" : "", index === todayIndex ? "is-today" : ""].filter(Boolean).join(" ");
            return `
              <button type="button" class="recap-bar-col" aria-label="${attr(bucket.label)}, ${attr(focused)} focused">
                <span class="recap-bar-track">
                  <span class="${classes}" style="--i: ${index}; ${on ? `height: ${smartBarHeight(bucket.focus_seconds, max, 14)}%` : ""}">
                    <span class="week-tip">${escapeHtml(focused)}</span>
                  </span>
                </span>
                <span class="recap-bar-label">${escapeHtml(bucket.label)}</span>
              </button>
            `;
          })
          .join("")}
      </div>
    </section>
  `;
}

/** The week against the one before it, a sparkline of recent weeks, and the way into Trends. */
function renderTrendsDoor(weekStart: string, recap: WeekRecap | null, animate: boolean): string {
  const weeks = trends.weeks;
  const previousStart = addDaysKey(weekStart, -7);
  const previous = weeks.find((week) => week.week_start === previousStart);
  const current = weekStart === thisMondayKey();
  const todayIndex = (new Date().getDay() + 6) % 7;
  let value = "—";
  let caption = "Nothing to compare";
  if (!recap) {
    caption = "";
  } else if (weeks.length === 0) {
    value = "No weeks yet";
    caption = "Finish a session and this week starts the chart.";
  } else if (!previous) {
    value = weeks[0].week_start === weekStart ? "First week" : "Off the chart";
    caption = weeks[0].week_start === weekStart ? "Nothing before it to compare with." : "Trends keep the last eight weeks.";
  } else if (previous.session_count === 0 && recap.session_count === 0) {
    value = "Quiet";
    caption = current ? "No sessions this week or last." : "No sessions this week or the one before.";
  } else if (recap && previous && (previous.session_count > 0 || recap.session_count > 0)) {
    const before = current && todayIndex < 6 ? focusThrough(previousStart, todayIndex) : previous.focus_seconds;
    // A running week is measured against last week up to the same weekday; "so far" says so.
    const against = current ? "last week" : "the week before";
    const soFar = current && todayIndex < 6 ? " so far" : "";
    const change = recap.focus_seconds - before;
    if (Math.abs(change) < 60) {
      value = "Steady";
      caption = `Same focused time${soFar} as ${against}.`;
    } else {
      value = `${change > 0 ? "↑" : "↓"} ${formatHours(Math.abs(change))}`;
      caption = `${change > 0 ? "More" : "Less"} focused time${soFar} than ${against}.`;
    }
  }
  const shown = weeks.slice(-DOOR_WEEKS);
  return `
    <section class="panel trend-door" aria-label="Trends">
      <p class="eyebrow">Trends</p>
      <strong class="trend-door-value">${escapeHtml(value)}</strong>
      <span class="trend-door-caption">${escapeHtml(caption)}</span>
      ${shown.length ? `<span class="trend-door-chart-title">Focused hours, last ${shown.length === 1 ? "week" : `${shown.length} weeks`}</span>${renderDoorChart(shown, weekStart, animate)}` : ""}
      <button type="button" class="trend-door-go" data-action="open-trends">View trends ›</button>
    </section>
  `;
}

/** Weeks on the door's chart; Trends itself keeps eight. */
const DOOR_WEEKS = 4;

/** Plain bars, newest week at the right, no axis: the figure above says what they mean. */
function renderDoorChart(shown: TrendWeek[], weekStart: string, animate: boolean): string {
  const maxWeek = Math.max(...shown.map((week) => week.focus_seconds), 1);
  const pad = DOOR_WEEKS - shown.length;
  const running = thisMondayKey();
  return `
    <div class="trend-spark ${animate ? "is-animated" : ""}" role="img" aria-label="Focused hours per week, last ${shown.length} weeks">
      ${'<span class="trend-spark-track"><span class="trend-spark-bar is-empty"></span></span>'.repeat(pad)}
      ${shown
        .map((week, index) => {
          const classes = ["trend-spark-bar", week.week_start === weekStart ? "is-viewed" : "", week.week_start === running ? "is-running" : "", week.focus_seconds === 0 ? "is-empty" : ""].filter(Boolean).join(" ");
          const sessions = week.session_count === 1 ? "1 session" : `${week.session_count} sessions`;
          const tip = `${formatWeekStart(week.week_start)} · ${formatHours(week.focus_seconds)}${week.week_start === running ? " so far" : ""}`;
          return `
            <span class="trend-spark-track" aria-label="${attr(`${tip} · ${sessions}`)}">
              <span class="${classes}" style="--i: ${index}; ${week.focus_seconds ? `height: ${smartBarHeight(week.focus_seconds, maxWeek, 14)}%` : ""}">
                <span class="week-tip">${escapeHtml(tip)}</span>
              </span>
            </span>
          `;
        })
        .join("")}
    </div>
  `;
}

/** Focused seconds from `weekStart` through its `weekdayIndex`th day (0 = Monday). */
function focusThrough(weekStart: string, weekdayIndex: number): number {
  const end = addDaysKey(weekStart, weekdayIndex);
  return trends.daily_focus
    .filter((day) => day.date >= weekStart && day.date <= end)
    .reduce((sum, day) => sum + day.focus_seconds, 0);
}

function renderSessionsPane(sessions: SessionSummary[], emptyText: string): string {
  return `
    <section class="panel history-list-panel">
      <div class="history-list">
        ${
          sessions.length === 0
            ? `<p class="empty">${escapeHtml(emptyText)}</p>`
            : groupSessionsByDay(sessions)
                .map(
                  (group) => `
            <section class="history-day">
              <div class="history-day-head">
                <p class="eyebrow">${escapeHtml(group.label)}</p>
                <p class="history-day-total">${escapeHtml(dayTotalLine(group.items))}</p>
              </div>
              ${group.items.map(renderHistoryItem).join("")}
            </section>
          `,
                )
                .join("")
        }
      </div>
    </section>
  `;
}

function renderSearchResults(): string {
  const query = historySearch.trim();
  const results = historySearchResults;
  if (results === null) {
    return `<section class="panel history-list-panel"><p class="empty">Searching…</p></section>`;
  }
  const count = results.length === 1 ? "1 session" : `${results.length} sessions`;
  return `
    <div class="history-search-head">
      <p class="eyebrow">${count} matching “${escapeHtml(query)}”</p>
      ${results.length >= SEARCH_LIMIT ? `<p>Showing the newest ${SEARCH_LIMIT}</p>` : ""}
    </div>
    ${renderSessionsPane(results, "No session goal contains that.")}
  `;
}

/** `search_sessions` stops here; past it the list says so. */
const SEARCH_LIMIT = 100;

function renderWeekAppsPane(recap: WeekRecap | null): string {
  if (!recap || recap.session_count === 0) {
    return `<section class="panel history-list-panel"><p class="empty">No sessions this week, so there are no apps and sites to show yet.</p></section>`;
  }
  const work = recap.apps.filter((app) => app.group === "Work");
  const distractions = recap.apps.filter((app) => app.group === "Distractions");
  const away = recap.apps.filter((app) => app.group === "Away");
  if (work.length + distractions.length === 0) {
    return `<section class="panel history-list-panel"><p class="empty">Sessions ran, but no app or site could be named. Time at the desktop or in an unknown window lands here as nothing.</p></section>`;
  }
  const maxApp = Math.max(...[...work, ...distractions].map((app) => app.seconds), 1);
  const nameable = recap.apps.some((item) => item.kind === "app" || item.kind === "website");
  const column = (title: string, items: RecapApp[], emptyText: string) => {
    const total = items.reduce((sum, app) => sum + app.seconds, 0);
    return `
      <div class="week-app-col">
        <p class="eyebrow">${escapeHtml(title)}</p>
        ${items.length ? `<p class="week-app-total">${escapeHtml(formatHours(total))}</p>` : ""}
        ${items.length ? `<div class="week-app-rows">${renderAppRows(items, maxApp)}</div>` : `<p class="week-app-empty">${escapeHtml(emptyText)}</p>`}
      </div>
    `;
  };
  const awayLine = away.map((row) => `${row.label} ${formatHours(row.seconds)}`).join(" · ");
  return `
    <section class="panel week-apps">
      <div class="week-app-cols">
        ${column("Work", work, "Every minute was on something from the distractions list.")}
        ${column("Distractions", distractions, "Nothing from the distractions list came up this week.")}
      </div>
      <div class="week-app-foot">
        <span>${awayLine ? escapeHtml(awayLine) : "Never away from the keyboard during a session."}</span>
        ${nameable ? `<button type="button" data-action="toggle-recap-names">${editingRecapNames ? "Done naming" : "Edit names"}</button>` : ""}
      </div>
    </section>
  `;
}

function renderHistoryItem(item: SessionSummary): string {
  const time = new Date(item.started_at).toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  const hints = item.top_hints.join(" / ");
  return `
    <button class="history-item" type="button" data-action="open-report" data-session-id="${item.id}">
      <div>
        <h3>${escapeHtml(item.goal)}</h3>
        <p>${escapeHtml(hints ? `${time} · ${hints}` : time)}</p>
      </div>
      <div class="history-item-stats">
        <dl>
          <div title="Time not spent on a distraction"><dt>Focused</dt><dd>${escapeHtml(formatHours(item.focus_seconds))}</dd></div>
          <div title="How long the session ran"><dt>Duration</dt><dd>${formatDuration(item.duration_seconds)}</dd></div>
          <div title="Times Supervisor spoke up about a distraction"><dt>Nudges</dt><dd>${item.nudges}</dd></div>
        </dl>
        ${renderMiniTimeline(item)}
      </div>
    </button>
  `;
}

/** The two most recent completed weeks (oldest first); the current week is still running. */
function completedWeeks(): [TrendWeek | undefined, TrendWeek | undefined] {
  const done = trends.weeks.slice(0, -1);
  return [done[done.length - 2], done[done.length - 1]];
}

function renderAppGroups(apps: RecapApp[], groups: string[]): string {
  const maxApp = Math.max(...apps.map((app) => app.seconds), 1);
  return `<div class="recap-app-list">
    ${groups
      .map((group) => {
        const items = apps.filter((app) => app.group === group);
        if (items.length === 0) {
          return "";
        }
        return `
          <div class="recap-app-block">
            <p class="eyebrow">${escapeHtml(group)}</p>
            <div class="recap-app-group">${renderAppRows(items, maxApp)}</div>
          </div>
        `;
      })
      .join("")}
  </div>`;
}

/** One line per app or site: name, a bar against the busiest row, the time. Read-only; the
 * distractions list is edited in Settings or during a session, never from a past week. */
function renderAppRows(items: RecapApp[], maxApp: number): string {
  return items
    .map((app) => {
      const width = Math.round(app.seconds / 60) > 0 ? Math.max(4, Math.round((app.seconds / maxApp) * 100)) : 0;
      return `
        <div class="recap-app-row ${app.group === "Distractions" ? "is-distraction" : ""}">
          ${renderRecapAppName(app)}
          <span class="recap-app-track">${width ? `<span style="width: ${width}%"></span>` : ""}</span>
          <strong>${escapeHtml(formatHours(app.seconds))}</strong>
        </div>
      `;
    })
    .join("");
}

function renderRecapAppName(app: RecapApp): string {
  if ((app.kind !== "website" && app.kind !== "app") || ["untracked", "supervisor", "phone"].includes(app.key)) {
    return `<span class="recap-app-name">${escapeHtml(app.label)}</span>`;
  }
  if (historyRename?.key === app.key && historyRename.kind === app.kind) {
    return `
      <span class="recap-app-name recap-rename">
        <input id="history-rename" maxlength="60" value="${attr(historyRename.value)}" placeholder="${attr(app.label)}" aria-label="Name for ${attr(app.key)}" />
      </span>
    `;
  }
  if (!editingRecapNames) {
    return `<span class="recap-app-name">${escapeHtml(app.label)}</span>`;
  }
  return `
    <span class="recap-app-name recap-site-name">
      <span>${escapeHtml(app.label)}</span>
      <button class="recap-rename-button" type="button" data-history-rename="${attr(app.key)}" data-rename-kind="${app.kind}" aria-label="Rename ${attr(app.label)}" title="Rename · ${attr(app.key)}">Rename</button>
    </span>
  `;
}

function formatRecovery(totalSeconds: number): string {
  const seconds = Math.max(0, Math.round(totalSeconds));
  if (seconds < 60) {
    return `${seconds}s`;
  }
  if (seconds >= 3600) {
    return formatHours(seconds);
  }
  const rest = seconds % 60;
  return rest ? `${Math.floor(seconds / 60)}m ${rest}s` : `${seconds / 60}m`;
}

function renderTrends(): string {
  const locked = trends.full_weeks < TRENDS_UNLOCK_WEEKS;
  return `
    <section class="history-page recap-page">
      <div class="subpage-head recap-head">
        <button type="button" data-action="close-recap">Back</button>
        <div>
          <p class="eyebrow">History</p>
          <h2>Trends</h2>
        </div>
      </div>
      ${locked ? renderTrendsLocked() : renderTrendRows()}
    </section>
  `;
}

function renderTrendsLocked(): string {
  const have = Math.min(trends.full_weeks, TRENDS_UNLOCK_WEEKS);
  return `
    <section class="panel span-card trends-slab trends-locked">
      <p class="eyebrow">Trends</p>
      <p class="span-value">Unlocks after two full weeks</p>
      <p class="span-caption">Week-over-week lines for focused time, attention span, and recovery need a couple of complete weeks before they mean anything.</p>
      <div class="trend-pips" role="img" aria-label="${have} of ${TRENDS_UNLOCK_WEEKS} full weeks">
        ${Array.from({ length: TRENDS_UNLOCK_WEEKS }, (_, index) => `<span class="${index < have ? "is-on" : ""}"></span>`).join("")}
        <strong>${have} of ${TRENDS_UNLOCK_WEEKS} full weeks</strong>
      </div>
    </section>
  `;
}

function renderTrendRows(): string {
  const weeks = trends.weeks;
  const [previous, last] = completedWeeks();
  const has = (week: TrendWeek | undefined): week is TrendWeek => Boolean(week && week.session_count > 0);
  const lastRecovery = has(last) ? last.recovery_median_seconds : null;
  const previousRecovery = has(previous) ? previous.recovery_median_seconds : null;
  const noLast = has(last) ? "" : "No sessions last week";

  const focus = trendCompare(
    has(last) ? last.focus_seconds : null,
    has(previous) ? previous.focus_seconds : null,
    60,
    formatHours,
    false,
  );
  const span = trendCompare(
    has(last) ? last.median_stretch_seconds : null,
    has(previous) ? previous.median_stretch_seconds : null,
    30,
    formatMinutes,
    false,
  );
  const recovery = trendCompare(lastRecovery, previousRecovery, 5, formatRecovery, true);

  return `
    <div class="trends-grid">
      ${renderTrendCard({
        tone: "focus",
        title: "Focused time",
        value: has(last) ? formatHours(last.focus_seconds) : "—",
        compare: noLast || focus,
        plot: renderWeekPlot(
          weeks.map((week) => ({
            weekStart: week.week_start,
            value: week.session_count > 0 ? week.focus_seconds / 3600 : null,
            chip: formatHours(week.focus_seconds),
          })),
          "Focused hours per week",
          { unit: "HRS", width: 1040, height: 230, bars: true, partialLast: true },
        ),
      })}
      ${renderTrendCard({
        tone: "span",
        title: "Attention span",
        hint: "typical unbroken stretch of work",
        value: has(last) ? formatMinutes(last.median_stretch_seconds) : "—",
        compare: noLast || span,
        plot: renderWeekPlot(
          weeks.map((week) => ({
            weekStart: week.week_start,
            value: week.session_count > 0 ? week.median_stretch_seconds / 60 : null,
            chip: formatStretchChip(week.median_stretch_seconds),
          })),
          "Attention span per week: the typical uninterrupted stretch",
          { unit: "MIN", width: 440, height: 250, partialLast: true, emptyNote: "No stretch of work recorded yet" },
        ),
      })}
      ${renderTrendCard({
        tone: "recovery",
        title: "Recovery",
        hint: "time back to work after a distraction · lower is better",
        value: lastRecovery == null ? "—" : formatRecovery(lastRecovery),
        compare: noLast || (lastRecovery == null ? { line: "No distractions last week" } : recovery),
        plot: renderWeekPlot(
          weeks.map((week) => ({
            weekStart: week.week_start,
            value: week.recovery_median_seconds == null ? null : week.recovery_median_seconds / 60,
            chip: week.recovery_median_seconds == null ? "" : formatRecovery(week.recovery_median_seconds),
          })),
          "Typical time back to work after a distraction, per week",
          { unit: "MIN", width: 440, height: 250, partialLast: true, emptyNote: "No distractions yet, so nothing to recover from" },
        ),
      })}
    </div>
  `;
}

interface TrendComparison {
  /** Short change for the pill, e.g. "↑ 42m"; absent when there is nothing to compare. */
  pill?: string;
  good?: boolean;
  line: string;
}

function trendCompare(
  now: number | null,
  before: number | null,
  threshold: number,
  format: (seconds: number) => string,
  lowerIsBetter: boolean,
): TrendComparison {
  if (now == null) {
    return { line: "No data last week" };
  }
  if (before == null) {
    return { line: "First full week on record" };
  }
  // The pill carries the comparison; the line stays empty so the card reads once.
  const change = now - before;
  if (Math.abs(change) < threshold) {
    return { pill: "Steady", good: true, line: "" };
  }
  const up = change > 0;
  const amount = format(Math.abs(change));
  const pill = lowerIsBetter ? `${amount} ${up ? "slower" : "faster"}` : `${up ? "↑" : "↓"} ${amount}`;
  return { pill, good: lowerIsBetter ? !up : up, line: "" };
}

function renderTrendCard(card: {
  tone: "focus" | "span" | "recovery";
  title: string;
  hint?: string;
  value: string;
  compare: TrendComparison | string;
  plot: string;
}): string {
  const compare = typeof card.compare === "string" ? { line: card.compare } : card.compare;
  return `
    <section class="panel trend-card trend-${card.tone}">
      <div class="trend-head">
        <p class="eyebrow">${escapeHtml(card.title)}</p>
        ${card.hint ? `<p class="trend-hint">${escapeHtml(card.hint)}</p>` : ""}
        <div class="trend-figure">
          <p class="trend-value">${escapeHtml(card.value)}</p>
          ${compare.pill ? `<span class="trend-pill ${compare.good ? "is-good" : "is-bad"}">${escapeHtml(compare.pill)}</span>` : ""}
        </div>
        <p class="trend-line">Last full week${compare.line ? ` · ${escapeHtml(compare.line)}` : ""}</p>
      </div>
      ${card.plot}
    </section>
  `;
}

interface WeekPoint {
  weekStart: string;
  /** In axis units (minutes, hours); null when the week has nothing to plot. */
  value: number | null;
  chip: string;
}

interface WeekPlotOptions {
  unit?: string;
  height?: number;
  width?: number;
  /** Volumes (hours) read better as bars; rates and durations as a line. */
  bars?: boolean;
  /** Draw the running week as a hollow, unconnected dot so a partial week never looks like a drop. */
  partialLast?: boolean;
  /** Shown over the grid when no week has a value. */
  emptyNote?: string;
}

function renderWeekPlot(weeks: WeekPoint[], ariaLabel: string, options: WeekPlotOptions = {}): string {
  const { unit = "", height = 268, width = 680, bars = false, partialLast = false, emptyNote = "Nothing to plot yet" } = options;
  const left = 64;
  const right = 22;
  const top = 36;
  const bottom = 56;
  const plotW = width - left - right;
  const plotH = height - top - bottom;
  const { max: axisMax, step } = niceAxis(Math.max(0, ...weeks.map((week) => week.value ?? 0)));
  const ticks: number[] = [];
  for (let tick = 0; tick <= axisMax + step / 1000; tick += step) {
    ticks.push(tick);
  }
  const last = weeks.length - 1;
  const xAt = (index: number) => left + ((index + 0.5) / weeks.length) * plotW;
  const yAt = (value: number) => top + plotH - (value / axisMax) * plotH;
  const points = weeks.map((week, index) => ({
    x: xAt(index),
    y: yAt(week.value ?? 0),
    hasData: week.value != null,
    current: index === last,
    label: week.chip,
    weekLabel: formatWeekStart(week.weekStart),
  }));
  const line = bars
    ? ""
    : spanLineRibbon(
        points.map((point) => ({ ...point, hasData: point.hasData && !(partialLast && point.current) })),
        8,
      );
  const baseline = yAt(0);
  const barWidth = Math.min(46, (plotW / weeks.length) * 0.58);
  // Eight labels crowd the axis; keep every other one, always ending on this week.
  const showLabel = (index: number) => weeks.length <= 5 || (last - index) % 2 === 0;
  const tickLabel = (tick: number) => (step < 1 ? tick.toFixed(1) : String(Math.round(tick)));
  const grid = ticks
    .map((tick) => {
      const y = yAt(tick);
      return `
        <line class="span-grid" x1="${left}" y1="${y.toFixed(1)}" x2="${width - right}" y2="${y.toFixed(1)}" />
        <text class="span-axis" x="${left - 10}" y="${y.toFixed(1)}" text-anchor="end" dominant-baseline="middle">${tickLabel(tick)}</text>
      `;
    })
    .join("");
  const unitLabel = unit
    ? `<text class="span-axis-sub" x="${left - 10}" y="${top - 22}" text-anchor="end">${escapeHtml(unit)}</text>`
    : "";
  const xLabels = points
    .map((point, index) => {
      if (!showLabel(index)) {
        return "";
      }
      const currentNote = point.current
        ? `<text class="span-axis-sub" x="${point.x.toFixed(1)}" y="${height - 8}" text-anchor="middle">so far</text>`
        : "";
      return `
        <text class="span-axis" x="${point.x.toFixed(1)}" y="${height - 26}" text-anchor="middle">${escapeHtml(point.weekLabel)}</text>
        ${currentNote}
      `;
    })
    .join("");
  const hits = points
    .filter((point) => point.hasData)
    .map((point) => {
      const tipY = point.y - 28;
      const tipWidth = Math.max(44, point.label.length * 9 + 16);
      const partial = partialLast && point.current ? "is-partial" : "";
      const mark = bars
        ? `<rect class="span-bar ${partial}" x="${(point.x - barWidth / 2).toFixed(1)}" y="${Math.min(point.y, baseline - 3).toFixed(1)}" width="${barWidth.toFixed(1)}" height="${Math.max(3, baseline - point.y).toFixed(1)}" rx="3" />`
        : `<rect class="span-dot ${partial}" x="${(point.x - 7).toFixed(1)}" y="${(point.y - 7).toFixed(1)}" width="14" height="14" />`;
      return `
        <g class="span-hit" tabindex="0">
          <circle class="span-hotspot" cx="${point.x.toFixed(1)}" cy="${point.y.toFixed(1)}" r="22" />
          ${mark}
          <g class="span-tip" transform="translate(${point.x.toFixed(1)} ${tipY.toFixed(1)})">
            <rect x="${-tipWidth / 2}" y="-16" width="${tipWidth}" height="24" rx="6" />
            <text y="1" text-anchor="middle" dominant-baseline="middle">${escapeHtml(point.label)}</text>
          </g>
        </g>
      `;
    })
    .join("");

  return `
    <svg class="span-plot" viewBox="0 0 ${width} ${height}" role="img" aria-label="${attr(ariaLabel)}">
      ${grid}
      ${unitLabel}
      ${line ? `<path class="span-line" d="${line}" />` : ""}
      ${xLabels}
      ${hits}
      ${points.some((point) => point.hasData) ? "" : `<text class="span-empty" x="${(left + plotW / 2).toFixed(1)}" y="${(top + plotH / 2).toFixed(1)}" text-anchor="middle" dominant-baseline="middle">${escapeHtml(emptyNote)}</text>`}
    </svg>
  `;
}

function spanLineRibbon(points: { x: number; y: number; hasData: boolean }[], width: number): string {
  const r = width / 2;
  const p = (x: number, y: number) => `${x.toFixed(2)} ${y.toFixed(2)}`;
  const parts: string[] = [];
  for (let i = 0; i < points.length - 1; i += 1) {
    const a = points[i];
    const b = points[i + 1];
    if (!a.hasData || !b.hasData) {
      continue;
    }
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy);
    if (len < 0.01) {
      continue;
    }
    const nx = (-dy / len) * r;
    const ny = (dx / len) * r;
    parts.push(
      `M${p(a.x + nx, a.y + ny)}L${p(b.x + nx, b.y + ny)}A${r} ${r} 0 0 1 ${p(b.x - nx, b.y - ny)}L${p(a.x - nx, a.y - ny)}A${r} ${r} 0 0 1 ${p(a.x + nx, a.y + ny)}Z`,
    );
  }
  return parts.join("");
}

/** Three even steps that cover `max`, on round numbers (0.25, 0.5, 1, 2, 5, 10…). */
function niceAxis(max: number): { max: number; step: number } {
  const raw = Math.max(max, 0.0001) / 3;
  const steps = [0.25, 0.5, 1, 2, 5, 10, 15, 20, 30, 60, 120];
  const step = steps.find((candidate) => candidate >= raw) ?? Math.ceil(raw / 60) * 60;
  return { max: step * 3, step };
}

/** The report's bar in miniature: each run of a state at its real place and length, gaps left bare. */
function renderMiniTimeline(item: SessionSummary): string {
  const segments = item.segments ?? [];
  const end = segments.reduce((max, segment) => Math.max(max, segment.started_offset_seconds + segment.seconds), 0);
  const total = Math.max(item.duration_seconds, end, 1);
  return `
    <div class="mini-timeline" aria-hidden="true">
      ${segments
        .map(
          (segment) =>
            `<span class="state-${escapeHtml(segment.state)}" style="left: ${((segment.started_offset_seconds / total) * 100).toFixed(2)}%; width: ${((segment.seconds / total) * 100).toFixed(2)}%"></span>`,
        )
        .join("")}
    </div>
  `;
}

function renderSessionReport(report: SessionReport): string {
  const recorded = report.spans.reduce((sum, span) => sum + span.seconds, 0);
  const unrecorded = report.spans.length > 0 && recorded + 1 < timelineSeconds(report);
  const legend = renderTimelineLegend(report, unrecorded);
  return `
    <section class="history-page">
      <div class="report-toolbar">
        <button class="small-button" type="button" data-action="close-report">Back</button>
        <button class="small-button danger" type="button" data-action="ask-delete-session" data-session-id="${report.id}">Delete</button>
      </div>
      <div class="report-stage">
        <section class="state-slab report-slab">
          <h2>${escapeHtml(report.goal)}</h2>
          <p class="report-meta">${escapeHtml(reportSummaryLine(report))}</p>
          <p class="report-meta report-hint ${report.spans.length ? "" : "is-placeholder"}" ${report.spans.length ? "" : `aria-hidden="true"`}>Hover over a part of the bar to see the app and how long it lasted.</p>
          ${renderReportTimeline(report)}
          ${legend ? `<div class="report-inset">${legend}</div>` : ""}
        </section>
        <aside class="report-stats" aria-label="Session stats">
          ${reportStat("Duration", formatClockWithUnit(report.duration_seconds), "How long the session ran, pauses included")}
          ${reportStat("Focused", `${report.focus_percent}%`, "Share of the session not spent on a distraction")}
          ${reportStat("Longest stretch", formatMinutes(report.longest_stretch_seconds), "Longest unbroken run of work")}
          ${reportStat("Nudges", String(report.nudge_count), "Times Supervisor spoke up about a distraction")}
          ${report.phone_seconds > 0 ? reportStat("Phone", formatClockWithUnit(report.phone_seconds), "Time the webcam saw a phone") : ""}
        </aside>
      </div>
      <section class="panel recap-apps">
        <p class="eyebrow">WHERE IT WENT</p>
        <h2>Apps & sites</h2>
        ${
          report.apps.length === 0
            ? `<p class="empty">No app time recorded.</p>`
            : renderAppGroups(report.apps, APP_GROUPS)
        }
      </section>
    </section>
  `;
}

function reportStat(labelText: string, value: string, help: string): string {
  return `
    <div class="report-stat" title="${attr(help)}">
      <span>${escapeHtml(labelText)}</span>
      <strong>${escapeHtml(value)}</strong>
    </div>
  `;
}

/** Span offsets include pauses, so the bar can be longer than the timer duration. */
function timelineSeconds(report: SessionReport): number {
  const spanEnd = report.spans.reduce((end, span) => Math.max(end, span.started_offset_seconds + span.seconds), 0);
  const eventEnd = report.events.reduce((end, event) => Math.max(end, event.at_offset_seconds), 0);
  return Math.max(report.duration_seconds, 1, spanEnd, eventEnd);
}

function renderReportTimeline(report: SessionReport): string {
  const duration = timelineSeconds(report);
  const nudges = report.events.filter((event) => event.kind === "nudge");
  if (report.spans.length === 0) {
    // An empty bar where the timeline would be, so the card keeps its layout.
    return `
      <div class="timeline-wrap">
        <div class="timeline-bar timeline-empty" role="img" aria-label="No timeline for this session">
          <span>No timeline for this session</span>
        </div>
      </div>
    `;
  }
  return `
    <div class="timeline-wrap">
      <div class="timeline-bar" role="img" aria-label="Session timeline">
        ${report.spans
          .map((span) => {
            const title = `${span.label} · ${formatDuration(span.seconds)}`;
            const left = (span.started_offset_seconds / duration) * 100;
            const width = (span.seconds / duration) * 100;
            return `<span class="timeline-span state-${escapeHtml(span.state)}" style="left: ${left.toFixed(3)}%; width: ${width.toFixed(3)}%" title="${attr(title)}"></span>`;
          })
          .join("")}
        ${nudges
          .map((event) => {
            const left = Math.min(100, (event.at_offset_seconds / duration) * 100);
            return `<i class="timeline-tick" style="left: ${left.toFixed(2)}%" title="${attr(`Nudge at ${formatDuration(event.at_offset_seconds)}`)}"></i>`;
          })
          .join("")}
      </div>
    </div>
  `;
}

/** Only the states this session's bar actually shows. */
function renderTimelineLegend(report: SessionReport, unrecorded: boolean): string {
  const present = new Set(report.spans.map((span) => span.state));
  const items = [
    [["focused"], "state-focused", "Work"],
    [["suspicious", "distracted"], "state-distracted", "Distraction"],
    [["phone"], "state-phone", "Phone"],
    // Idle (no input) and device asleep / unknown are one "Away"; hovering a segment says which.
    [["idle", "unknown"], "state-idle", "Away"],
    [["paused"], "state-paused", "Paused"],
  ]
    .filter(([states]) => (states as string[]).some((state) => present.has(state)))
    .map(([, cls, name]) => [cls as string, name as string]);
  // Only sessions recorded before the gap-free recorder can have these.
  if (unrecorded) {
    items.push(["state-unrecorded", "Not recorded"]);
  }
  if (items.length === 0) {
    return "";
  }
  return `
    <ul class="legend-swatches">
      ${items
        .map(
          ([cls, name]) => `
            <li class="legend-swatch">
              <i class="legend-key ${cls}" aria-hidden="true"></i>
              <span>${escapeHtml(name)}</span>
            </li>
          `,
        )
        .join("")}
    </ul>
  `;
}

function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const next = button.dataset.tab as Tab;
      if (next === activeTab) {
        return;
      }
      if (next === "session") {
        animateCalendarSoon();
      }
      const leavingWebcam = settingsView === "webcam";
      if (voiceInfoId) {
        dismissVoiceInfo();
      }
      activeTab = next;
      if (next !== "settings") {
        if (settingsView === "voice") {
          leaveVoiceScreen();
        }
        settingsView = "home";
        ruleEditor = null;
      }
      if (activeTab === "history") {
        historyAnimateUntil = performance.now() + 500;
      }
      closeWeekPicker();
      render();
      if (activeTab === "history") {
        void refreshTrends(false).then(() => refreshHistoryWeek());
      }
      if (leavingWebcam) {
        void stopWebcamPreview();
      }
    });
  });

  document.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "", button);
    });
  });

  document.querySelectorAll<HTMLButtonElement>("[data-rule-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void handleRuleAction(button);
    });
  });

  document.querySelectorAll<HTMLFormElement>("[data-rule-form]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      void saveRuleForm(form);
    });
  });

  document.getElementById("rule-value")?.addEventListener("input", (event) => {
    if (ruleEditor) {
      ruleEditor.value = (event.currentTarget as HTMLInputElement).value;
      paintRuleEditorLive();
      scheduleRulePreview();
    }
  });

  document.getElementById("rule-name")?.addEventListener("input", (event) => {
    if (ruleEditor) {
      ruleEditor.name = (event.currentTarget as HTMLInputElement).value;
      ruleEditor.nameTouched = true;
    }
  });

  document.querySelectorAll<HTMLButtonElement>("[data-history-rename]").forEach((button) => {
    button.addEventListener("click", () => {
      const key = button.dataset.historyRename ?? "";
      const kind = button.dataset.renameKind as "app" | "website";
      historyRename = { kind, key, value: kind === "website" ? siteNames[key] ?? "" : appNames[key] ?? "" };
      render();
      (document.getElementById("history-rename") as HTMLInputElement | null)?.select();
    });
  });

  if (!pickerDismissBound) {
    pickerDismissBound = true;
    document.addEventListener("click", (event) => {
      if (!weekPickerOpen) {
        return;
      }
      const target = event.target as Element | null;
      if (target?.closest(".week-picker, .history-week-label")) {
        return;
      }
      closeWeekPicker();
      render();
    });
  }
  const searchInput = document.getElementById("history-search") as HTMLInputElement | null;
  searchInput?.addEventListener("input", () => {
    historySearch = searchInput.value;
    scheduleHistorySearch();
  });
  const renameInput = document.getElementById("history-rename") as HTMLInputElement | null;
  renameInput?.addEventListener("input", () => {
    if (historyRename) {
      historyRename.value = renameInput.value;
    }
  });
  renameInput?.addEventListener("keydown", (event) => {
    if (event.key === "Enter" && !composing(event)) {
      event.preventDefault();
      void saveHistoryRename();
    }
  });
  renameInput?.addEventListener("blur", () => {
    void saveHistoryRename();
  });

  bindPhraseEditor(document);

  document.querySelector("details.overlay-more")?.addEventListener("toggle", (event) => {
    const open = (event.currentTarget as HTMLDetailsElement).open;
    if (open === overlayMoreOpen) {
      return;
    }
    overlayMoreOpen = open;
    lastOverlayFit = "";
    requestAnimationFrame(() => fitOverlayWindow());
  });

  document.getElementById("overlay-snooze")?.addEventListener("change", (event) => {
    const mute = (event.target as HTMLInputElement).checked;
    void invokeSnapshot(mute ? "snooze_session" : "resume_session");
  });

  bindPreflight(document);
  bindClipsControls(document);

  document.getElementById("default-strictness")?.addEventListener("change", (event) => {
    void updateDefaultStrictness((event.target as HTMLSelectElement).value as Strictness);
  });

  document.getElementById("status-window-enabled")?.addEventListener("change", (event) => {
    void updateStatusWindowEnabled((event.target as HTMLInputElement).checked);
  });

  document.getElementById("webcam-phone-use")?.addEventListener("change", (event) => {
    void setWebcamEnabled((event.target as HTMLInputElement).checked);
  });

  document.getElementById("webcam-gpu")?.addEventListener("change", (event) => {
    void setWebcamGpu((event.target as HTMLInputElement).checked);
  });

  document.getElementById("webcam-camera")?.addEventListener("change", (event) => {
    void setWebcamCamera(Number((event.target as HTMLSelectElement).value));
  });

  document.getElementById("voice-name")?.addEventListener("input", (event) => {
    if (recordedVoice) {
      recordedVoice.name = (event.target as HTMLInputElement).value;
    }
  });

  bindVoicePreview();
  bindVoiceWaveforms();
}

/** The preflight's own controls, inside `root`: the whole document after a rebuild, or just the layer. */
function bindPreflight(root: ParentNode): void {
  root.querySelector<HTMLTextAreaElement>("#preflight-goal")?.addEventListener("input", (event) => {
    draft.goal = (event.target as HTMLTextAreaElement).value;
  });

  root.querySelectorAll<HTMLDetailsElement>("[data-preflight-section]").forEach((details) => {
    details.addEventListener("toggle", () => {
      if (details.dataset.preflightSection === "blocks") {
        preflightBlocksOpen = details.open;
      } else {
        preflightAdvancedOpen = details.open;
      }
    });
  });

  root.querySelector<HTMLInputElement>("#preflight-duration")?.addEventListener("input", (event) => {
    const slider = event.target as HTMLInputElement;
    draft.duration_minutes = Number(slider.value || 25);
    slider.style.setProperty("--range-progress", `${((draft.duration_minutes - 15) / 75) * 100}%`);
    const output = root.querySelector("#preflight-duration-output");
    if (output) {
      output.textContent = `${draft.duration_minutes} min`;
    }
  });

  root.querySelector<HTMLSelectElement>("#preflight-strictness")?.addEventListener("change", (event) => {
    draft.strictness = (event.target as HTMLSelectElement).value as Strictness;
  });

  root.querySelector<HTMLSelectElement>("#preflight-voice")?.addEventListener("change", (event) => {
    draft.voiceId = (event.target as HTMLSelectElement).value;
    void updateSelectedVoice(draft.voiceId);
  });
}

/**
 * Opening or closing the preflight repaints only its layer. Home underneath keeps its calendar,
 * quote and scroll position instead of being rebuilt behind the modal.
 */
function paintPreflightLayer(): void {
  const layer = document.getElementById("preflight-layer");
  if (!layer) {
    render();
    return;
  }
  layer.innerHTML = preflightOpen ? renderPreflightModal() : "";
  layer.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "", button);
    });
  });
  layer.querySelectorAll<HTMLButtonElement>("[data-rule-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void handleRuleAction(button);
    });
  });
  bindPreflight(layer);
  if (preflightOpen) {
    layer.querySelector<HTMLTextAreaElement>("#preflight-goal")?.focus();
  }
}

function releaseVoiceWorker(): void {
  void invoke("release_voice_worker").catch(() => null);
}

function leaveVoiceScreen(): void {
  if (clipQueue?.phase === "working") {
    clipQueue.stopAfter = true;
  } else {
    clipQueue = null;
  }
  stopVoicePreview();
  releaseVoiceWorker();
}

async function startSession(): Promise<boolean> {
  return invokeSnapshot("start_session", {
    input: {
      goal: draft.goal,
      duration_minutes: draft.duration_minutes,
      strictness: draft.strictness,
      exceptions: cloneRules(preflightExceptions),
    },
  });
}

async function runAction(action: string, source?: HTMLElement): Promise<void> {
  if (action === "toggle-theme") {
    theme = theme === "dark" ? "light" : "dark";
    localStorage.setItem("supervisor-theme", theme);
    applyTheme();
    const button = document.querySelector(".theme-toggle");
    if (button) {
      button.textContent = theme === "dark" ? "Dark" : "Light";
    }
    return;
  }

  if (action === "dismiss-distraction") {
    const live = snapshot?.current;
    if (live) {
      distractionDismissedFor = distractionIdentity(live);
    }
    distractionLatchKey = "";
    distractionLatchLabel = "";
    overlayMoreOpen = false;
    render();
    return;
  }

  if (action === "allow-session" || action === "allow-always") {
    const live = snapshot?.current;
    if (live) {
      distractionDismissedFor = distractionIdentity(live);
    }
    distractionLatchKey = "";
    distractionLatchLabel = "";
    overlayMoreOpen = false;
    const name = live?.focus?.activity.label || "This window";
    if (await invokeSnapshot(action === "allow-session" ? "allow_current_for_session" : "allow_current_always")) {
      showFeedback(action === "allow-session" ? `${name} is allowed for this session` : `${name} is no longer a distraction`);
    }
    return;
  }

  if (action === "toggle-exception") {
    const rule = permanentRules[Number(source?.dataset.ruleIndex)];
    if (rule) {
      preflightExceptions = preflightExceptions.some((exception) => sameRule(exception, rule))
        ? preflightExceptions.filter((exception) => !sameRule(exception, rule))
        : [...preflightExceptions, { ...rule }];
      render();
    }
    return;
  }

  if (action === "open-session-rules") {
    if (snapshot?.current) {
      sessionRulesOpen = true;
      ruleEditor = null;
      render();
    }
    return;
  }

  if (action === "close-session-rules") {
    sessionRulesOpen = false;
    ruleEditor = null;
    render();
    return;
  }

  if (action === "onboard-hide" || action === "onboard-show") {
    onboardingOpen = action === "onboard-show";
    render();
    return;
  }

  if (action === "copy-card") {
    await copyCard();
    return;
  }

  if (action === "onboard-accessibility") {
    try {
      await invoke("open_accessibility_settings");
    } catch (error) {
      lastError = errorText(error);
      render();
    }
    return;
  }

  if (action === "onboard-voice" || action === "onboard-webcam") {
    onboardingOpen = false;
    activeTab = "settings";
    await runAction(action === "onboard-voice" ? "open-voice-settings" : "open-webcam-settings");
    return;
  }

  if (action === "onboard-session") {
    onboardingOpen = false;
    draft.duration_minutes = 15;
    await runAction("open-preflight");
    // A first session should show the rules it runs on.
    preflightBlocksOpen = true;
    render();
    return;
  }

  if (action === "open-preflight") {
    preflightExceptions = [];
    draft.duration_minutes = Math.min(90, Math.max(15, draft.duration_minutes));
    preflightBlocksOpen = false;
    preflightAdvancedOpen = false;
    preflightOpen = true;
    ruleEditor = null;
    paintPreflightLayer();
    return;
  }

  if (action === "close-preflight") {
    preflightOpen = false;
    ruleEditor = null;
    paintPreflightLayer();
    return;
  }

  if (action === "confirm-start-session") {
    if (!draft.goal.trim()) {
      showFeedback("Name the session");
      return;
    }
    preflightOpen = false;
    ruleEditor = null;
    if (!(await startSession())) {
      preflightOpen = true;
      render();
    }
    return;
  }

  if (action === "open-trends") {
    historyView = "trends";
    sessionReport = null;
    editingRecapNames = false;
    historyRename = null;
    await refreshTrends(false);
    render();
    return;
  }

  if (action === "close-recap") {
    historyView = "list";
    editingRecapNames = false;
    historyRename = null;
    await refreshHistoryWeek();
    return;
  }

  if (action === "toggle-week-picker") {
    if (weekPickerOpen) {
      closeWeekPicker();
      render();
      return;
    }
    weekPickerOpen = true;
    pickerMonth = monthKey(historyWeekStart || thisMondayKey());
    render();
    await loadPickerDays();
    return;
  }

  if (action === "picker-month-prev" || action === "picker-month-next") {
    pickerMonth = addMonthsKey(pickerMonth || monthKey(historyWeekStart || thisMondayKey()), action === "picker-month-prev" ? -1 : 1);
    pickerDays = {};
    render();
    await loadPickerDays();
    return;
  }

  if (action === "pick-week") {
    const day = source?.dataset.day;
    closeWeekPicker();
    const oldestMonday = historyWeek?.oldest_day ? mondayKey(historyWeek.oldest_day) : null;
    if (day && day <= dayKey(new Date()) && (oldestMonday === null || mondayKey(day) >= oldestMonday)) {
      historyWeekStart = mondayKey(day);
    }
    await refreshHistoryWeek();
    return;
  }

  if (action === "history-week-today") {
    closeWeekPicker();
    historyWeekStart = thisMondayKey();
    await refreshHistoryWeek();
    return;
  }

  if (action === "history-pane") {
    historyPane = source?.dataset.pane === "apps" ? "apps" : "sessions";
    editingRecapNames = false;
    historyRename = null;
    render();
    return;
  }

  if (action === "toggle-recap-names") {
    editingRecapNames = !editingRecapNames;
    historyRename = null;
    render();
    return;
  }

  if (action === "history-week-prev") {
    closeWeekPicker();
    historyWeekStart = addDaysKey(historyWeekStart || thisMondayKey(), -7);
    await refreshHistoryWeek();
    return;
  }

  if (action === "history-week-next") {
    closeWeekPicker();
    const current = thisMondayKey();
    const next = addDaysKey(historyWeekStart || current, 7);
    if (next <= current) {
      historyWeekStart = next;
      await refreshHistoryWeek();
    }
    return;
  }

  if (action === "open-report") {
    const id = Number(source?.dataset.sessionId);
    if (Number.isInteger(id) && id > 0) {
      await openHistoryReport(id);
    }
    return;
  }

  if (action === "close-report") {
    historyView = "list";
    sessionReport = null;
    historyConfirm = null;
    await refreshHistoryWeek();
    return;
  }

  if (action === "ask-delete-session") {
    const id = Number(source?.dataset.sessionId);
    if (Number.isInteger(id) && id > 0) {
      historyConfirm = { kind: "delete-session", id };
      render();
    }
    return;
  }

  if (action === "ask-delete-all") {
    const bytes = await invoke<number>("get_history_size").catch(() => null);
    historyConfirm = { kind: "delete-all", bytes };
    render();
    return;
  }

  if (action === "cancel-history-confirm") {
    historyConfirm = null;
    render();
    return;
  }

  if (action === "confirm-history") {
    await confirmHistoryAction();
    return;
  }

  if (action === "open-voice-settings") {
    settingsView = "voice";
    await refreshVoiceStatus(false);
    render();
    return;
  }

  if (action === "open-webcam-settings") {
    settingsView = "webcam";
    webcamError = "";
    render();
    void refreshCameras();
    await startWebcamPreview();
    return;
  }

  if (action === "back-settings") {
    const leavingWebcam = settingsView === "webcam";
    const leavingVoice = settingsView === "voice";
    settingsView = "home";
    phraseEditor = null;
    dismissVoiceInfo();
    if (leavingVoice) {
      leaveVoiceScreen();
    }
    render();
    if (leavingWebcam) {
      await stopWebcamPreview();
    }
    return;
  }

  if (action === "voice-info") {
    const voiceId = source?.dataset.voiceId;
    if (!voiceId) {
      return;
    }
    await refreshVoiceStatus(false);
    if (voiceInfoId !== voiceId) {
      stopVoicePreview();
    }
    voiceInfoId = voiceId;
    render();
    return;
  }

  if (action === "close-voice-info") {
    dismissVoiceInfo();
    render();
    return;
  }

  if (action === "toggle-voice-preview") {
    await toggleVoicePreview();
    return;
  }

  if (action === "play-clip") {
    const path = source?.dataset.clipPath;
    if (path) {
      await playClip(path);
    }
    return;
  }

  if (action === "check-ram") {
    await refreshVoiceStatus(true);
    return;
  }

  if (action === "open-memory-info" || action === "close-memory-info") {
    memoryInfoOpen = action === "open-memory-info";
    render();
    return;
  }

  if (action === "open-model-info" || action === "close-model-info") {
    modelInfoOpen = action === "open-model-info";
    render();
    return;
  }

  if (action === "open-voice-help" || action === "close-voice-help") {
    voiceHelpOpen = action === "open-voice-help";
    render();
    return;
  }

  if (action === "pick-tone") {
    clearFailedQueue();
    clipsTone = source?.dataset.tone ?? null;
    paintTonePick();
    return;
  }

  if (action === "use-tone") {
    const tone = source?.dataset.tone;
    if (tone) {
      await setVoiceTone(clipsVoice(), tone);
    }
    return;
  }

  if (action === "generate-missing") {
    const voice = clipsVoice();
    const missing = tonePhrases(viewedTone(voice)).filter((phrase) => !clipForPhrase(voice, phrase));
    await runClipQueue(missing.map((phrase) => phrase.id));
    return;
  }

  if (action === "stop-clip-queue") {
    if (clipQueue?.phase === "working") {
      clipQueue.stopAfter = true;
      paintClipsPanel();
    }
    return;
  }

  if (action === "open-tones" || action === "close-tones") {
    closeTonesPopup();
    tonesOpen = action === "open-tones";
    paintVoiceLayer();
    return;
  }

  if (action === "new-tone" || action === "cancel-new-tone") {
    newToneDraft = action === "new-tone" ? "" : null;
    toneRename = null;
    toneDeleteArmed = null;
    paintVoiceLayer();
    if (action === "new-tone") {
      document.getElementById("new-tone-input")?.focus();
    }
    return;
  }

  if (action === "add-line") {
    if (canEditPhrases()) {
      openPhraseEditor(null, source?.dataset.tone || viewedTone(clipsVoice()));
    }
    return;
  }

  if (action === "save-line") {
    await saveLine(false);
    return;
  }

  if (action === "rename-tone") {
    const tone = source?.dataset.tone ?? "";
    if (isCustomTone(tone)) {
      toneRename = { from: tone, value: tone };
      toneDeleteArmed = null;
      newToneDraft = null;
      paintVoiceLayer();
      (document.getElementById("tone-rename-input") as HTMLInputElement | null)?.select();
    }
    return;
  }

  if (action === "cancel-rename-tone") {
    toneRename = null;
    paintVoiceLayer();
    return;
  }

  if (action === "arm-delete-tone" || action === "cancel-delete-tone") {
    const tone = source?.dataset.tone ?? "";
    toneDeleteArmed = action === "arm-delete-tone" && isCustomTone(tone) ? tone : null;
    toneRename = null;
    newToneDraft = null;
    paintVoiceLayer();
    return;
  }

  if (action === "delete-tone") {
    await deleteTone(source?.dataset.tone ?? "");
    return;
  }

  if (action === "select-voice") {
    const voiceId = source?.dataset.voiceId;
    const voice = voiceId ? availableVoices().find((item) => item.id === voiceId) : null;
    if (voiceId && voice?.selectable) {
      draft.voiceId = voiceId;
      render();
      void updateSelectedVoice(voiceId).then((saved) => {
        if (saved) {
          showFeedback(`${voice.name} selected`);
        }
      });
    }
    return;
  }

  if (action === "download-model-pack") {
    await startModelDownload();
    return;
  }

  if (action === "cancel-model-download") {
    await cancelModelDownload();
    return;
  }

  if (action === "close-model-download") {
    dismissDownloadModal();
    return;
  }

  if (action === "record-voice") {
    if (!modelInstalled()) {
      showFeedback("Download voice pack first");
      return;
    }
    openRecordingDialog();
    return;
  }

  if (action === "ask-delete-model") {
    dismissVoiceInfo();
    modelDeleteOpen = true;
    render();
    return;
  }

  if (action === "cancel-delete-model") {
    modelDeleteOpen = false;
    render();
    return;
  }

  if (action === "delete-model-files") {
    modelDeleteOpen = false;
    await runVoiceBusy("delete-model", async () => {
      const model = await invoke<VoiceModelStatus>("delete_voice_cloning_model");
      if (voiceBackend) {
        voiceBackend = { ...voiceBackend, model };
      }
      await refreshVoiceStatus(false);
      showFeedback("Model files deleted");
    });
    return;
  }

  if (action === "open-phrase-editor") {
    if (canEditPhrases()) {
      dismissVoiceInfo();
      openPhraseEditor(source?.dataset.phraseId || null, viewedTone(clipsVoice()));
    }
    return;
  }

  if (action === "delete-phrase") {
    const phraseId = source?.dataset.phraseId;
    if (phraseId && canEditPhrases()) {
      await deletePhrase(phraseId);
    }
    return;
  }

  if (action === "set-up-clips") {
    clearFailedQueue();
    clipsVoiceId = source?.dataset.voiceId ?? clipsVoiceId;
    clipsVoiceTouched = true;
    clipsTone = null;
    render();
    document.querySelector(".phrase-panel")?.scrollIntoView({ behavior: "smooth", block: "start" });
    return;
  }

  if (action === "cancel-phrase-editor") {
    phraseEditor = null;
    paintVoiceLayer();
    return;
  }

  if (action === "close-recording") {
    await closeRecordingDialog();
    return;
  }

  if (action === "start-recording") {
    await startVoiceRecording();
    return;
  }

  if (action === "stop-recording") {
    await stopVoiceRecording();
    return;
  }

  if (action === "rerecord-voice") {
    resetRecordedVoice();
    await clearVoiceRecording();
    await startVoiceRecording();
    return;
  }

  if (action === "save-recorded-voice") {
    await saveRecordedVoice();
    return;
  }

  if (action === "confirm-delete-voice") {
    voicePreviewAudio?.pause();
    voiceDeleteTargetId = source?.dataset.profileId || null;
    render();
    return;
  }

  if (action === "cancel-delete-voice") {
    voiceDeleteTargetId = null;
    render();
    return;
  }

  if (action === "delete-voice-profile") {
    await deleteVoiceProfile();
    return;
  }

  if (action === "delete-voice-files") {
    await deleteVoiceFilesOnly();
    return;
  }

  if (action === "generate-clip") {
    const phraseId = source?.dataset.phraseId;
    if (phraseId) {
      clearFailedQueue();
      await runClipQueue([phraseId]);
    }
    return;
  }

  if (action === "pin-window" || action === "unpin-window") {
    const enabled = action === "pin-window";
    if (await invokeSnapshot("set_status_window_on_top", { enabled }, false)) {
      showFeedback(enabled ? "Window pinned" : "Window unpinned");
    }
    return;
  }

  const commands: Record<string, string> = {
    pause: "pause_session",
    resume: "resume_session",
    snooze: "snooze_session",
    stop: "stop_session",
    "mark-distracting": "flag_current_distracting",
  };
  const command = commands[action];
  if (!command) {
    return;
  }

  const focus = snapshot?.current?.focus;
  if (action.startsWith("mark-") && focus && !focus.markable) {
    showFeedback(
      focus.activity.kind === "app"
        ? "A browser can't be a distraction on its own. Mark the website instead."
        : `${focus.activity.label} can't be marked. Mark a website or an app.`,
    );
    return;
  }
  const ok = await invokeSnapshot(command, undefined, !action.startsWith("mark-"));
  if (ok && action.startsWith("mark-")) {
    const name = snapshot?.current?.focus?.activity.label || "last window";
    showFeedback(`Distraction: ${name}`);
  }
  if (ok && action === "stop" && history[0]) {
    await openHistoryReport(history[0].id);
  }
}

async function handleRuleAction(button: HTMLButtonElement): Promise<void> {
  const action = button.dataset.ruleAction;
  if (!action) {
    return;
  }

  if (action === "add") {
    ruleEditor = { index: null, target: "website", value: "", name: "", nameTouched: false, preview: null };
    render();
    document.getElementById("rule-value")?.focus();
    return;
  }

  if (action === "cancel") {
    ruleEditor = null;
    render();
    return;
  }

  const index = Number(button.dataset.ruleIndex);
  const rules = permanentRules;
  if (!Number.isInteger(index) || !rules[index]) {
    return;
  }

  if (action === "edit") {
    const rule = rules[index];
    ruleEditor = {
      index,
      ...rule,
      name: rule.target === "website" ? siteNames[rule.value] ?? "" : "",
      nameTouched: false,
      preview: { target: rule.target, value: rule.value },
    };
    render();
    return;
  }

  if (action === "delete") {
    const previous = cloneRules(rules);
    rules.splice(index, 1);
    ruleEditor = null;
    await persistPermanentRuleChanges(previous);
  }
}

async function saveRuleForm(form: HTMLFormElement): Promise<void> {
  const editor = ruleEditor;
  if (!editor) {
    return;
  }

  const text = formValue(form, "value").trim();
  if (!text) {
    showFeedback("Enter a website or an app name");
    return;
  }
  let rule: AppRule;
  try {
    rule = await invoke<AppRule>("resolve_rule_input", { text });
  } catch (error) {
    showFeedback(errorText(error));
    return;
  }
  if (ruleEditor !== editor) {
    return;
  }
  if (rule.target === "website" && previewSite(editor) === rule.value) {
    const name = formValue(form, "name").trim();
    if (name !== (siteNames[rule.value] ?? "")) {
      try {
        siteNames = await invoke<Record<string, string>>("set_site_name", { key: rule.value, name });
      } catch (error) {
        showFeedback(errorText(error));
        return;
      }
    }
  }
  if (ruleEditor !== editor) {
    return;
  }

  const previous = cloneRules(permanentRules);
  if (editor.index === null) {
    permanentRules.unshift(rule);
  } else {
    permanentRules[editor.index] = rule;
  }
  ruleEditor = null;
  await persistPermanentRuleChanges(previous);
}

function scheduleRulePreview(delay = 140): void {
  if (rulePreviewTimer !== null) {
    window.clearTimeout(rulePreviewTimer);
  }
  rulePreviewTimer = window.setTimeout(() => {
    rulePreviewTimer = null;
    void resolveRulePreview();
  }, delay);
}

async function resolveRulePreview(): Promise<void> {
  const editor = ruleEditor;
  if (!editor) {
    return;
  }
  const text = editor.value.trim();
  let preview: RulePreview = null;
  if (text) {
    try {
      const rule = await invoke<AppRule>("resolve_rule_input", { text });
      preview = { target: rule.target, value: rule.value };
    } catch (error) {
      preview = { error: errorText(error) };
    }
  }
  if (ruleEditor !== editor || editor.value.trim() !== text) {
    return;
  }
  const before = previewSite(editor);
  editor.preview = preview;
  const site = previewSite(editor);
  if (site !== before) {
    editor.nameTouched = false;
  }
  if (!editor.nameTouched) {
    editor.name = site ? siteNames[site] ?? "" : "";
  }
  paintRuleEditorLive();
}

function paintRuleEditorLive(): void {
  const editor = ruleEditor;
  if (!editor) {
    return;
  }
  const preview = document.getElementById("rule-preview");
  const nameInput = document.getElementById("rule-name") as HTMLInputElement | null;
  if (preview) {
    preview.innerHTML = renderRulePreview(editor);
  }
  if (nameInput) {
    const site = previewSite(editor);
    const field = nameInput.closest("label");
    if (field) {
      field.hidden = !site;
    }
    nameInput.placeholder = site ?? "";
    if (!editor.nameTouched) {
      nameInput.value = editor.name;
    }
  }
}

async function saveHistoryRename(): Promise<void> {
  const rename = historyRename;
  if (!rename) {
    return;
  }
  historyRename = null;
  try {
    if (rename.kind === "website") {
      siteNames = await invoke<Record<string, string>>("set_site_name", { key: rename.key, name: rename.value });
    } else {
      appNames = await invoke<Record<string, string>>("set_app_name", { key: rename.key, name: rename.value });
    }
    weekRecap?.apps.forEach((app) => {
      if (app.kind === rename.kind && app.key === rename.key) {
        app.label = rename.kind === "website" ? siteName(rename.key) : appNames[rename.key] ?? app.label;
      }
    });
  } catch (error) {
    showFeedback(errorText(error));
  }
  render();
}

async function persistPermanentRuleChanges(previous: AppRule[]): Promise<void> {
  try {
    const saved = await invoke<RulesSaved>("update_rules", { rules: permanentRules });
    permanentRules = saved.rules;
    lastError = "";
    render();
    return;
  } catch (error) {
    permanentRules = previous;
    lastError = errorText(error);
  }
  render();
}

function libraryBackup() {
  return {
    customPhrases: customPhrases.map((phrase) => ({ ...phrase })),
    customPhraseSets: [...customPhraseSets],
    phraseOverrides: { ...phraseOverrides },
    hiddenPhrases: new Set(hiddenPhrases),
  };
}

function restoreLibrary(backup: ReturnType<typeof libraryBackup>): void {
  ({ customPhrases, customPhraseSets, phraseOverrides, hiddenPhrases } = backup);
}

/** Saves the Line popup. With `regenerate`, the line's clip is made right after, in the queue.
 * Saving never waits on the model: a changed line keeps its old clip until a new one exists. */
async function saveLine(regenerate: boolean): Promise<void> {
  const editor = phraseEditor;
  if (!editor || !canEditPhrases()) {
    return;
  }
  const text = editor.text.replace(/\s+/g, " ").trim();
  const tone = phraseToneOrder().find((item) => sameTone(item, editor.tone)) ?? editor.tone;
  if (!text) {
    showFeedback("The line needs words");
    return;
  }
  const existing = editor.phraseId ? effectivePhrases().find((item) => item.id === editor.phraseId) : null;
  const backup = libraryBackup();
  if (!phraseToneOrder().some((item) => sameTone(item, tone))) {
    customPhraseSets.push(tone);
  }
  let id: string;
  if (existing) {
    id = existing.id;
    const index = customPhrases.findIndex((phrase) => phrase.id === existing.id);
    if (index >= 0) {
      customPhrases[index] = { id, tone, text };
    } else {
      phraseOverrides[id] = { id, tone, text };
    }
  } else {
    id = uniqueCustomPhraseId(text);
    customPhrases.push({ id, tone, text });
  }
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  clipsTone = tone;
  phraseEditor = null;
  render();
  if (regenerate) {
    reopenLineAfterQueue = id;
    if (!(await runClipQueue([id]))) {
      reopenLineAfterQueue = null;
      showFeedback(existing ? "Line saved" : "Line added");
    }
    return;
  }
  const moved = existing && existing.text === text && !sameTone(existing.tone, tone);
  showFeedback(moved ? `Moved to ${toneLabel(tone)}` : existing ? "Line saved" : "Line added");
}

/** A new, empty tone. Its first line comes next, through the Line popup. */
async function createTone(): Promise<void> {
  const name = (newToneDraft ?? "").trim().slice(0, 80);
  if (!name) {
    showFeedback("Name the tone");
    return;
  }
  if (phraseToneOrder().some((tone) => sameTone(tone, name))) {
    showFeedback(`There is already a ${toneLabel(name)} tone`);
    return;
  }
  const backup = libraryBackup();
  customPhraseSets.push(name);
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  closeTonesPopup();
  clipsTone = name;
  openPhraseEditor(null, name);
}

/** Custom phrases go; bundled ones are hidden, so they stay deletable without a clip cleanup. */
async function deletePhrase(phraseId: string): Promise<void> {
  const backup = libraryBackup();
  if (customPhrases.some((phrase) => phrase.id === phraseId)) {
    customPhrases = customPhrases.filter((phrase) => phrase.id !== phraseId);
  } else {
    hiddenPhrases.add(phraseId);
    delete phraseOverrides[phraseId];
  }
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  phraseEditor = null;
  render();
  showFeedback("Line deleted");
}

async function persistPhraseLibrary(): Promise<boolean> {
  try {
    const library = await invoke<PhraseLibraryState>("update_phrase_library", {
      library: {
        custom_tones: customPhraseSets,
        custom_phrases: customPhrases,
        phrase_overrides: Object.values(phraseOverrides),
        hidden_phrases: [...hiddenPhrases],
      },
    });
    customPhraseSets = library.custom_tones;
    customPhrases = library.custom_phrases;
    phraseOverrides = Object.fromEntries(library.phrase_overrides.map((phrase) => [phrase.id, phrase]));
    hiddenPhrases = new Set(library.hidden_phrases);
    voiceError = "";
    return true;
  } catch (error) {
    voiceError = errorText(error);
    return false;
  }
}

async function invokeSnapshot(command: string, args?: Record<string, unknown>, paint = true): Promise<boolean> {
  try {
    snapshot = await invoke<SessionSnapshot>(command, args);
    settings = snapshot.settings;
    history = snapshot.history;
    history.forEach((item) => knownHistoryIds.add(item.id));
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
    render();
    reconcilePolling();
    return false;
  }

  if (paint) {
    render();
  }
  reconcilePolling();
  return true;
}

async function updateDefaultStrictness(default_strictness: Strictness): Promise<void> {
  try {
    settings = await invoke<SessionSettings>("update_settings", {
      update: { default_strictness },
    });
    draft.strictness = default_strictness;
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }
  render();
}

async function updateStatusWindowEnabled(enabled: boolean): Promise<void> {
  if (await invokeSnapshot("set_status_window_enabled", { enabled }, false)) {
    showFeedback(enabled ? "Show session timer" : "Session timer hidden");
  }
}

async function updateSelectedVoice(voiceId: string): Promise<boolean> {
  try {
    settings = await invoke<SessionSettings>("update_settings", {
      update: { voice_profile: voiceId },
    });
    voiceError = "";
    return true;
  } catch (error) {
    voiceError = errorText(error);
    render();
    return false;
  }
}

async function startModelDownload(): Promise<void> {
  if (voiceBusy !== null) {
    return;
  }

  voiceBusy = "download-model";
  render();
  try {
    const download = await invoke<VoiceDownloadSnapshot>("start_voice_model_download");
    applyDownloadSnapshot(download);
    voiceError = "";
    startDownloadPolling();
    if (download.phase === "complete") {
      stopDownloadPolling();
      await refreshVoiceStatus(false);
      showFeedback(download.up_to_date ? "Up to date" : "Model pack ready");
    }
  } catch (error) {
    voiceBusy = null;
    voiceError = errorText(error);
  }
  render();
}

async function cancelModelDownload(): Promise<void> {
  try {
    voiceBusy = "download-model";
    const download = await invoke<VoiceDownloadSnapshot>("cancel_voice_model_download");
    applyDownloadSnapshot(download);
    startDownloadPolling();
  } catch (error) {
    voiceError = errorText(error);
  }
  render();
}

function startDownloadPolling(): void {
  if (downloadPollId !== null) {
    return;
  }
  downloadPollId = window.setInterval(() => {
    void pollModelDownload();
  }, 750);
}

function stopDownloadPolling(): void {
  if (downloadPollId !== null) {
    window.clearInterval(downloadPollId);
    downloadPollId = null;
  }
}

async function pollModelDownload(): Promise<void> {
  if (voiceBusy !== "download-model") {
    stopDownloadPolling();
    return;
  }
  if (downloadPollInFlight) {
    return;
  }
  downloadPollInFlight = true;
  try {
    await pollModelDownloadNow();
  } finally {
    downloadPollInFlight = false;
  }
}

async function pollModelDownloadNow(): Promise<void> {
  try {
    const download = await invoke<VoiceDownloadSnapshot>("get_voice_model_download");
    applyDownloadSnapshot(download);
    if (download.phase === "complete") {
      stopDownloadPolling();
      await refreshVoiceStatus(false);
      showFeedback(download.up_to_date ? "Up to date" : "Model pack ready");
      render();
      return;
    }
    if (download.phase === "idle") {
      stopDownloadPolling();
      voiceBusy = null;
      await refreshVoiceStatus(false);
      render();
      showFeedback("Download cancelled");
      return;
    }
    if (download.phase === "error") {
      stopDownloadPolling();
      voiceBusy = null;
      voiceError = download.message;
      await refreshVoiceStatus(false);
      render();
      return;
    }
  } catch (error) {
    stopDownloadPolling();
    voiceBusy = null;
    voiceError = errorText(error);
    render();
    return;
  }
  paintDownloadProgress();
}

function dismissDownloadModal(): void {
  stopDownloadPolling();
  voiceBusy = null;
  if (voiceDownload?.phase === "complete" || voiceDownload?.phase === "idle") {
    voiceDownload = null;
  }
  render();
}

function paintDownloadProgress(): void {
  if (voiceDownload?.phase === "complete" || voiceDownload?.phase === "idle" || voiceDownload?.phase === "error") {
    return;
  }
  const download = voiceDownload;
  if (!download || !document.querySelector(".busy-panel")) {
    return;
  }
  // Checking has no determinate bar; the first real fetch flips the phase and the modal is
  // rebuilt by the next render. Only the downloading and cancelling phases repaint in place.
  if (download.phase === "checking" || !document.querySelector(".loading-bar.determinate")) {
    if (download.phase !== "checking") {
      render();
    }
    return;
  }
  const total = Math.max(0, download.total_bytes ?? voiceBackend?.model.required_bytes ?? 0);
  const downloaded = Math.max(0, Math.min(download.downloaded_bytes ?? 0, total || Number.MAX_SAFE_INTEGER));
  const percent = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  const bar = document.querySelector<HTMLElement>(".loading-bar.determinate > span");
  if (bar) {
    bar.style.width = `${percent}%`;
  }
  const title = document.querySelector(".busy-panel h2");
  if (title) {
    title.textContent = download.phase === "cancelling" ? "Cancelling download" : "Downloading model pack";
  }
  const copy = document.querySelector(".busy-panel h2 + p");
  if (copy) {
    copy.textContent = download.message || "Downloading model pack.";
  }
  const meta = document.querySelectorAll(".download-meta span");
  if (meta[0]) {
    meta[0].textContent = total > 0 ? `${formatBytes(downloaded)} / ${formatBytes(total)}` : formatBytes(downloaded);
  }
  if (meta[1]) {
    meta[1].textContent =
      download.phase === "cancelling"
        ? "Deleting partial files"
        : download.eta_seconds === null || download.eta_seconds === undefined
          ? "Estimating time left"
          : `${formatDuration(download.eta_seconds)} left`;
  }
}

function applyDownloadSnapshot(download: VoiceDownloadSnapshot): void {
  voiceDownload = download;
  if (voiceBackend) {
    voiceBackend = { ...voiceBackend, model: download.model, runtime: download.runtime };
  }
}

async function runVoiceBusy(kind: Exclude<VoiceBusy, null>, task: () => Promise<void>): Promise<boolean> {
  if (voiceBusy !== null) {
    return false;
  }

  voiceBusy = kind;
  render();

  try {
    await task();
    voiceError = "";
    return true;
  } catch (error) {
    if (ignoreVoiceWorkerError) {
      ignoreVoiceWorkerError = false;
      return false;
    }
    voiceError = errorText(error);
    return false;
  } finally {
    ignoreVoiceWorkerError = false;
    voiceBusy = null;
    render();
  }
}

function openRecordingDialog(): void {
  dismissVoiceInfo();
  recordingOpen = true;
  recordingState = recordedVoice ? "recorded" : "idle";
  recordingError = "";
  render();
}

async function closeRecordingDialog(): Promise<void> {
  await stopVoiceRecording(false);
  await clearVoiceRecording();
  recordingOpen = false;
  recordingState = "idle";
  recordingError = "";
  resetRecordedVoice();
  render();
}

async function startVoiceRecording(): Promise<void> {
  if (recordingState === "recording") {
    return;
  }

  resetRecordedVoice();
  recordingError = "";

  try {
    const recording = await invoke<VoiceRecordingSnapshot>("start_voice_recording");
    applyRecordingSnapshot(recording);
    if (recording.phase === "recording" && recordingTickId === null) {
      recordingTickId = window.setInterval(paintRecordingClock, 500);
    }
  } catch (error) {
    recordingState = "error";
    recordingError = errorText(error);
  }

  render();
}

async function stopVoiceRecording(keepClip = true): Promise<void> {
  if (recordingState !== "recording") {
    return;
  }

  clearRecordingTick();

  if (!keepClip) {
    await invoke<VoiceRecordingSnapshot>("stop_voice_recording").catch(() => null);
    return;
  }

  try {
    const recording = await invoke<VoiceRecordingSnapshot>("stop_voice_recording");
    applyRecordingSnapshot(recording);
  } catch (error) {
    recordingState = "error";
    recordingError = errorText(error);
  }

  render();
}

async function saveRecordedVoice(): Promise<void> {
  const recording = recordedVoice;
  if (!recording?.path) {
    return;
  }
  const name = recording.name.trim();
  if (!name) {
    showFeedback("Name the voice");
    return;
  }
  try {
    const saved = await invoke<VoiceProfileSummary>("save_voice_reference_from_path", {
      input: { profile_id: uniqueVoiceProfileId(name), display_name: name, reference_audio_path: recording.path },
    });
    if (!saved.reference_path) {
      throw new Error("Recorded voice reference was not saved.");
    }
    upsertProfile(saved);
    await refreshVoiceStatus(false);
    clipsVoiceId = voiceOptionId(saved.profile_id);
    await clearVoiceRecording();
    resetRecordedVoice();
    recordingOpen = false;
    recordingState = "idle";
    voiceError = "";
    render();
    showFeedback("Voice saved. Generate its clips below.");
    document.querySelector(".phrase-panel")?.scrollIntoView({ behavior: "smooth", block: "start" });
  } catch (error) {
    recordingError = errorText(error); // the dialog is still open; a banner would sit behind it
    render();
  }
}

/** A failed line stays on screen until the user moves on; the next action clears it. */
function clearFailedQueue(): void {
  if (clipQueue?.phase === "error") {
    clipQueue = null;
  }
}

/** Makes the given lines for the Clips-for voice, one after another, in place. Returns false
 * when nothing started. The busy modal stays away: the rows themselves show the progress. */
async function runClipQueue(phraseIds: string[]): Promise<boolean> {
  clearFailedQueue();
  if (clipQueue || voiceBusy !== null || phraseIds.length === 0) {
    return false;
  }
  const voice = clipsVoice();
  const profile = voice.profile;
  const reference = profile?.reference_path;
  const lock = clipGenerateLockTitle(voice);
  if (lock || !profile || !reference) {
    showFeedback(lock || "Pick a cloned voice under Clips for.");
    return false;
  }
  dismissVoiceInfo();
  clipQueue = { voiceId: voice.id, phraseIds, index: 0, phase: "working", error: "", startedAt: performance.now(), stopAfter: false };
  clipQueueTicker = window.setInterval(paintClipQueue, 1000);
  paintClipsPanel();
  await runVoiceBusy("generate-clips", async () => {
    while (clipQueue && clipQueue.phase === "working" && clipQueue.index < clipQueue.phraseIds.length) {
      const phraseId = clipQueue.phraseIds[clipQueue.index];
      const phrase = effectivePhrases().find((item) => item.id === phraseId);
      if (!phrase) {
        clipQueue = { ...clipQueue, index: clipQueue.index + 1 };
        continue;
      }
      clipQueue = { ...clipQueue, startedAt: performance.now() };
      paintClipsPanel();
      try {
        const generated = await invoke<VoiceProfileSummary>("generate_voice_clips", {
          input: {
            profile_id: profile.profile_id,
            display_name: profile.display_name,
            reference_audio_path: reference,
            phrases: [{ id: phrase.id, tone: phrase.tone, text: phrase.text }],
            delete_reference_after_generation: false,
            merge_clips: true,
          },
        });
        upsertProfile(generated);
        const ready = clipForPhraseId(generated.clips, phrase.id);
        if (ready) {
          bumpClipAsset(ready.path);
        }
      } catch (error) {
        if (clipQueue) {
          clipQueue = { ...clipQueue, phase: "error", error: errorText(error) };
        }
        paintClipsPanel();
        break;
      }
      if (!clipQueue) {
        break;
      }
      clipQueue = { ...clipQueue, index: clipQueue.index + 1 };
      if (clipQueue.stopAfter) {
        break;
      }
    }
  });
  if (clipQueueTicker !== null) {
    window.clearInterval(clipQueueTicker);
    clipQueueTicker = null;
  }
  const finished = clipQueue;
  if (finished?.phase !== "error") {
    clipQueue = null;
  }
  await refreshVoiceStatus(false);
  if (finished && finished.phase === "working" && finished.index >= finished.phraseIds.length) {
    const now = clipsVoice();
    showFeedback(
      now.source === "custom" && now.selectable
        ? `${now.name} is ready to select`
        : finished.phraseIds.length === 1
          ? "Clip ready"
          : "Clips ready",
    );
  }
  const reopen = reopenLineAfterQueue;
  reopenLineAfterQueue = null;
  const done = finished && finished.phase === "working" && finished.index >= finished.phraseIds.length;
  const onPage = appView === "main" && activeTab === "settings" && settingsView === "voice";
  const phrase = reopen ? effectivePhrases().find((item) => item.id === reopen) : null;
  if (done && onPage && phrase && !editorIsOpen() && clipForPhrase(clipsVoice(), phrase)) {
    render();
    openPhraseEditor(phrase.id, phrase.tone);
    return true;
  }
  render();
  return true;
}

async function setVoiceTone(voice: VoiceOption, tone: VoiceTone): Promise<void> {
  const profile = voice.profile;
  if (!profile) {
    return;
  }
  try {
    upsertProfile(await invoke<VoiceProfileSummary>("set_voice_tone", { profileId: profile.profile_id, tone }));
    clipsTone = tone;
    voiceError = "";
    showFeedback(`${voice.name} speaks ${toneLabel(tone)}`);
  } catch (error) {
    voiceError = errorText(error);
  }
  render();
}

/** Renames a custom tone everywhere it is written: its lines, its set, and the voices that speak it. */
async function renameTone(): Promise<void> {
  const edit = toneRename;
  if (!edit || !canEditPhrases()) {
    return;
  }
  const next = edit.value.trim().slice(0, 80);
  if (!next) {
    showFeedback("Name the tone");
    return;
  }
  if (sameTone(next, edit.from)) {
    toneRename = null;
    render();
    return;
  }
  if (phraseToneOrder().some((tone) => sameTone(tone, next))) {
    showFeedback(`There is already a ${toneLabel(next)} tone`);
    return;
  }
  const backup = libraryBackup();
  customPhraseSets = customPhraseSets.map((tone) => (sameTone(tone, edit.from) ? next : tone));
  customPhrases = customPhrases.map((phrase) => (sameTone(phrase.tone, edit.from) ? { ...phrase, tone: next } : phrase));
  for (const [id, phrase] of Object.entries(phraseOverrides)) {
    if (sameTone(phrase.tone, edit.from)) {
      phraseOverrides[id] = { ...phrase, tone: next };
    }
  }
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  for (const profile of voiceBackend?.profiles ?? []) {
    if (profile.tone && sameTone(profile.tone, edit.from)) {
      try {
        upsertProfile(await invoke<VoiceProfileSummary>("set_voice_tone", { profileId: profile.profile_id, tone: next }));
      } catch (error) {
        voiceError = errorText(error);
      }
    }
  }
  clipsTone = next;
  toneRename = null;
  render();
  showFeedback(`Renamed to ${toneLabel(next)}`);
}

/** Removes a custom tone and its lines. Voices that spoke it fall back to the first tone. */
async function deleteTone(tone: VoiceTone): Promise<void> {
  toneDeleteArmed = null;
  if (!isCustomTone(tone) || !canEditPhrases()) {
    render();
    return;
  }
  const backup = libraryBackup();
  customPhraseSets = customPhraseSets.filter((item) => !sameTone(item, tone));
  customPhrases = customPhrases.filter((phrase) => !sameTone(phrase.tone, tone));
  for (const [id, phrase] of Object.entries(phraseOverrides)) {
    if (sameTone(phrase.tone, tone)) {
      delete phraseOverrides[id];
      hiddenPhrases.add(id);
    }
  }
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  clipsTone = null;
  render();
  showFeedback(`${toneLabel(tone)} deleted`);
}

async function deleteVoiceProfile(): Promise<void> {
  const profileId = voiceDeleteTargetId;
  if (!profileId) {
    return;
  }

  await runVoiceBusy("delete-voice", async () => {
    const profiles = await invoke<VoiceProfileSummary[]>("delete_voice_profile", { profileId });
    if (voiceBackend) {
      voiceBackend = { ...voiceBackend, profiles };
    }
    voiceDeleteTargetId = null;
    if (voiceInfoId === voiceOptionId(profileId)) {
      dismissVoiceInfo();
    }
    if (draft.voiceId === voiceOptionId(profileId)) {
      draft.voiceId = "starter-female";
      await updateSelectedVoice(draft.voiceId);
    }
    if (clipsVoiceId === voiceOptionId(profileId)) {
      clipsVoiceId = "starter-female";
    }
    showFeedback("Voice deleted");
  });
}

async function deleteVoiceFilesOnly(): Promise<void> {
  const profileId = voiceDeleteTargetId;
  if (!profileId) {
    return;
  }

  await runVoiceBusy("delete-voice", async () => {
    const profile = await invoke<VoiceProfileSummary>("delete_voice_profile_clips", { profileId });
    upsertProfile(profile);
    await refreshVoiceStatus(false);
    stopVoicePreview();
    voiceDeleteTargetId = null;
    if (draft.voiceId === voiceOptionId(profileId)) {
      draft.voiceId = "starter-female";
      await updateSelectedVoice(draft.voiceId);
    }
    showFeedback("Voice files deleted");
  });
}

function resetRecordedVoice(): void {
  if (recordedVoice?.url) {
    URL.revokeObjectURL(recordedVoice.url);
  }
  recordedVoice = null;
}

async function clearVoiceRecording(): Promise<void> {
  clearRecordingTick();
  recordingStartedAtMs = null;
  await invoke<VoiceRecordingSnapshot>("clear_voice_recording").catch(() => null);
}

function applyRecordingSnapshot(recording: VoiceRecordingSnapshot): void {
  recordingState = recording.phase;
  recordingError = recording.phase === "error" ? recording.message : "";
  if (recording.phase === "recording" && recordingStartedAtMs === null) {
    recordingStartedAtMs = Date.now() - recording.elapsed_seconds * 1000;
  }
  if (recording.phase !== "recording") {
    recordingStartedAtMs = null;
  }
  if (recording.phase === "recorded" && recording.path) {
    recordedVoice = {
      path: recording.path,
      url: clipAssetUrl(recording.path),
      bytes: recording.bytes,
      seconds: recording.duration_seconds,
      name: recordedVoice?.name || defaultVoiceName(),
    };
  }
}

function clearRecordingTick(): void {
  if (recordingTickId !== null) {
    window.clearInterval(recordingTickId);
    recordingTickId = null;
  }
}

function reconcilePolling(): void {
  const shouldPoll = appView === "overlay" || snapshot?.current !== null;
  if (shouldPoll && pollId === null) {
    pollId = window.setInterval(refreshSnapshot, 1000);
  } else if (!shouldPoll && pollId !== null) {
    window.clearInterval(pollId);
    pollId = null;
  }
}

function tabButton(tab: Tab, text: string): string {
  return `<button data-tab="${tab}" class="${activeTab === tab ? "active" : ""}" type="button">${text}</button>`;
}

function metric(labelText: string, value: string): string {
  return `
    <div class="metric">
      <span>${escapeHtml(labelText)}</span>
      <strong>${escapeHtml(value)}</strong>
    </div>
  `;
}

function paintFeedback(): void {
  let toast = document.getElementById("feedback-toast");
  if (!feedbackMessage) {
    toast?.remove();
    return;
  }
  if (!toast) {
    toast = document.createElement("div");
    toast.id = "feedback-toast";
    toast.className = "feedback-toast";
    toast.setAttribute("role", "status");
    document.body.appendChild(toast);
  }
  toast.textContent = feedbackMessage;
}

function showFeedback(message: string, durationMs = 1400): void {
  feedbackMessage = message;
  paintFeedback();

  if (feedbackTimer !== null) {
    window.clearTimeout(feedbackTimer);
  }

  feedbackTimer = window.setTimeout(() => {
    feedbackMessage = "";
    feedbackTimer = null;
    paintFeedback();
  }, durationMs);
}

function paintRecordingClock(): void {
  const el = document.getElementById("recording-elapsed");
  if (el && recordingState === "recording") {
    el.textContent = formatDuration(recordingDurationSeconds());
    // The backend releases the mic at 60 s; stop here too so the clock doesn't run on.
    if (recordingDurationSeconds() >= 60) {
      void stopVoiceRecording();
    }
  }
}

/** Saved, because sessions run the webcam only when this is on. */
async function setWebcamEnabled(enabled: boolean): Promise<void> {
  webcamSettings.phoneUse = enabled;
  webcamHitStreak = 0;
  try {
    settings = await invoke<SessionSettings>("update_settings", {
      update: { webcam_enabled: enabled },
    });
  } catch (error) {
    webcamError = errorText(error);
    const box = document.getElementById("webcam-phone-use") as HTMLInputElement | null;
    if (box) {
      box.checked = settings.webcam_enabled;
    }
  }
  webcamSettings.phoneUse = settings.webcam_enabled;
  paintWebcamDom();
}

async function setWebcamGpu(enabled: boolean): Promise<void> {
  await updateWebcamWorkerSetting({ webcam_gpu: enabled }, () => {
    const box = document.getElementById("webcam-gpu") as HTMLInputElement | null;
    if (box) {
      box.checked = settings.webcam_gpu;
    }
  });
}

async function setWebcamCamera(camera: number): Promise<void> {
  await updateWebcamWorkerSetting({ webcam_camera: camera }, () => {
    const select = document.getElementById("webcam-camera") as HTMLSelectElement | null;
    if (select) {
      select.value = String(settings.webcam_camera);
    }
  });
}

/** A setting the worker reads at launch: a running preview restarts. On failure the control is put back. */
async function updateWebcamWorkerSetting(update: Partial<SessionSettings>, revert: () => void): Promise<void> {
  try {
    settings = await invoke<SessionSettings>("update_settings", { update });
    if (webcamPreviewStarted) {
      await stopWebcamPreview();
      webcamFrame = null;
      await startWebcamPreview();
    }
    webcamError = "";
  } catch (error) {
    webcamError = errorText(error);
    revert();
  }
  paintWebcamDom();
}

async function startWebcamPreview(): Promise<void> {
  if (webcamPreviewStarted) {
    return;
  }
  try {
    await invoke("start_webcam_preview");
    if (settingsView !== "webcam" || activeTab !== "settings") {
      await invoke("stop_webcam_preview");
      return;
    }
    webcamPreviewStarted = true;
    webcamError = "";
  } catch (error) {
    webcamPreviewStarted = false;
    webcamError = errorText(error);
    render();
  }
}

async function stopWebcamPreview(): Promise<void> {
  webcamPreviewStarted = false;
  webcamHitStreak = 0;
  try {
    await invoke("stop_webcam_preview");
  } catch {
    // Worker may already be gone.
  }
}

function applyWebcamFrame(frame: WebcamFrame): void {
  if (frame.error) {
    webcamError = frame.error;
    if (!frame.jpeg) {
      webcamHitStreak = 0;
      if (settingsView === "webcam" && activeTab === "settings") {
        paintWebcamDom();
      }
      return;
    }
  } else {
    webcamError = "";
  }
  webcamFrame = frame;
  const alerting = webcamWouldAlert(frame);
  if (frame.detected !== false) {
    webcamHitStreak = alerting ? webcamHitStreak + 1 : 0;
  }
  // During a session the session nudges, through the same voice and popup as everything else.
  if (webcamHitStreak >= 8 && !snapshot?.current) {
    void fireWebcamAlert();
  }
  if (settingsView === "webcam" && activeTab === "settings") {
    paintWebcamDom();
  }
}

function paintWebcamDom(): void {
  const frame = webcamFrame;
  const alerting = webcamWouldAlert(frame);
  const img = document.getElementById("webcam-preview") as HTMLImageElement | null;
  const wrap = document.getElementById("webcam-frame-wrap");
  if (img && frame?.jpeg) {
    img.src = `data:image/jpeg;base64,${frame.jpeg}`;
    img.alt = "Webcam preview";
    wrap?.classList.add("is-live");
  } else {
    wrap?.classList.remove("is-live");
    if (img) {
      img.removeAttribute("src");
      img.alt = "";
    }
  }
  const chip = document.getElementById("webcam-chip");
  if (chip) {
    chip.textContent = webcamStatusChip();
    chip.classList.toggle("is-off", !webcamSettings.phoneUse);
  }
  const title = document.getElementById("webcam-title");
  if (title) {
    title.textContent = webcamTitle(frame, alerting);
  }
  const copy = document.getElementById("webcam-copy");
  if (copy) {
    copy.textContent = webcamCopy(frame, alerting);
  }
  const waiting = document.getElementById("webcam-waiting");
  if (waiting) {
    waiting.textContent = webcamWaitingCopy();
  }
  const slab = document.querySelector(".webcam-stage");
  if (slab) {
    slab.className = `state-slab webcam-stage ${webcamStateClass(frame, alerting)}`;
  }
  const liveError = document.getElementById("webcam-live-error");
  if (liveError) {
    liveError.textContent = webcamError;
    liveError.hidden = !webcamSettings.phoneUse || !webcamError;
  }
  const metrics = document.querySelector(".webcam-metrics");
  if (metrics) {
    metrics.outerHTML = renderWebcamMetrics(frame);
  }
}

function webcamWouldAlert(frame: WebcamFrame | null): boolean {
  if (!frame || !webcamSettings.phoneUse) {
    return false;
  }
  if (webcamSettings.strict) {
    return frame.phone_in_frame;
  }
  return frame.looking_at_phone;
}

function webcamStateClass(frame: WebcamFrame | null, alerting: boolean): string {
  if (alerting) {
    return "state-distracted";
  }
  if (!frame?.face_in_frame) {
    return "";
  }
  if (frame.gaze === "at_pc") {
    return "state-focused";
  }
  if (frame.gaze === "away_from_pc") {
    return "state-muted";
  }
  return "state-distracted";
}

function webcamStatusChip(): string {
  return webcamSettings.phoneUse ? "LIVE" : "OFF";
}

function webcamWaitingCopy(): string {
  if (!webcamSettings.phoneUse) {
    return "Monitoring off";
  }
  if (webcamError) {
    return "Camera unavailable";
  }
  return "Starting camera";
}

function webcamTitle(frame: WebcamFrame | null, alerting: boolean): string {
  if (!webcamSettings.phoneUse || !frame) {
    return "Webcam preview";
  }
  if (alerting) {
    return "On the phone";
  }
  if (!frame.face_in_frame) {
    return "Out of frame";
  }
  if (frame.gaze === "at_pc") {
    return "At the screen";
  }
  if (frame.gaze === "away_from_pc") {
    return "Looking away";
  }
  if (frame.gaze === "at_phone") {
    return "On the phone";
  }
  return "Watching";
}

function webcamCopy(frame: WebcamFrame | null, alerting: boolean): string {
  if (!webcamSettings.phoneUse) {
    return "Turn on webcam monitoring to get reminded to put down your phone.";
  }
  if (alerting) {
    return webcamSettings.strict ? "Phone is in frame." : "Looking at a phone in the camera.";
  }
  if (!frame?.face_in_frame) {
    return "Sit in view of the webcam.";
  }
  if (frame.gaze === "at_pc") {
    return "Head toward the screen.";
  }
  if (frame.phone_in_frame) {
    return "Phone is visible. Looking at the screen, so this is not a phone hit yet.";
  }
  return "Looking away. No phone in the camera.";
}

async function fireWebcamAlert(): Promise<void> {
  const now = Date.now();
  if (now - webcamAlarmAtMs < 8000) {
    return;
  }
  webcamAlarmAtMs = now;
  if (webcamSettings.reaction === "voice") {
    const clip = selectedNudgeClip(1);
    if (clip) {
      try {
        nudgeAudio?.pause();
        nudgeAudio = new Audio(clipAssetUrl(clip.path));
        await nudgeAudio.play();
        return;
      } catch {
        // Fall through to the alarm.
      }
    }
  }
  playWebcamAlarm();
}

function playWebcamAlarm(): void {
  const context = new AudioContext();
  const oscillator = context.createOscillator();
  const gain = context.createGain();
  oscillator.type = "square";
  oscillator.frequency.value = 880;
  oscillator.connect(gain);
  gain.connect(context.destination);
  gain.gain.setValueAtTime(0.08, context.currentTime);
  gain.gain.exponentialRampToValueAtTime(0.001, context.currentTime + 0.28);
  oscillator.onended = () => void context.close();
  oscillator.start();
  oscillator.stop(context.currentTime + 0.28);
}

function permissionNotice(_current: CurrentSession | null): string {
  // macOS Accessibility is a Getting started step instead: a banner here would push Home past
  // the window, which is sized to fit it exactly.
  return "";
}

/** "TODAY · 1H 12M", or a nudge to start when nothing has been done yet. */
function todayChip(): string {
  const seconds = dayFocusMap().get(dayKey(new Date())) ?? 0;
  return seconds > 0 ? `Today · ${formatHours(seconds)}` : "Today · nothing yet";
}

function statusChip(current: CurrentSession | null): string {
  if (!current) {
    return "STANDBY";
  }

  if (current.status === "paused") {
    return "PAUSED";
  }

  if (current.status === "snoozed") {
    return "MUTED";
  }

  return current.focus?.should_nudge ? "PAY ATTENTION" : "LIVE";
}

function stateTitle(current: CurrentSession | null): string {
  if (!current) {
    return "Ready";
  }

  if (current.status === "paused") {
    return "Paused";
  }

  if (current.camera.phone) {
    return current.camera.phone_distracted ? "On the phone" : "Phone in view";
  }

  switch (current.focus?.state) {
    case "focused":
      return "Locked in";
    case "suspicious":
      return "Distraction open";
    case "distracted":
      return "Distracted";
    case "idle":
      return "Away";
    default:
      return "Watching";
  }
}

function stateCopy(current: CurrentSession | null): string {
  if (!current) {
    return "One clear start. Distractions and voice live in the preflight.";
  }

  if (current.status === "paused") {
    return "No judgment recorded.";
  }

  if (current.camera.phone_distracted) {
    return "Put the phone down.";
  }

  if (current.focus?.should_nudge) {
    return "You have been here long enough. Pay attention.";
  }

  return current.goal;
}

function stateClass(current: CurrentSession | null): string {
  if (!current) {
    return "state-ready";
  }

  if (current.status === "paused") {
    return "state-paused";
  }

  if (current.camera.phone) {
    return current.camera.phone_distracted ? "state-distracted" : "state-suspicious";
  }

  return `state-${current.focus?.state ?? "waiting"}`;
}

async function playNudge(current: CurrentSession | null): Promise<void> {
  if (!current || current.status !== "active" || draft.voiceId === "none") {
    return;
  }
  const clip = selectedNudgeClip(current.metrics.nudges);
  if (!clip) {
    return;
  }
  try {
    nudgeAudio?.pause();
    nudgeAudio = new Audio(clipAssetUrl(clip.path));
    nudgeAudio.preload = "auto";
    await nudgeAudio.play();
    voiceError = "";
  } catch (error) {
    voiceError = `Could not play voice clip: ${errorText(error)}`;
    render();
  }
}

/** Lines from the voice's own tone; any clip at all only when that tone has none ready. */
function selectedNudgeClip(nudgeCount: number): VoiceClipSummary | null {
  const voice = selectedVoice();
  const phrases = tonePhrases(voiceTone(voice));
  const ready = phrases
    .map((phrase) => clipForPhrase(voice, phrase))
    .filter((clip): clip is VoiceClipSummary => Boolean(clip));
  if (ready.length === 0) {
    const clips = voice.profile?.clips ?? [];
    if (clips.length === 0) {
      return null;
    }
    return clips[(Math.max(1, nudgeCount) - 1) % clips.length];
  }
  return ready[(Math.max(1, nudgeCount) - 1) % ready.length];
}

function previewClipForVoice(voice: VoiceOption): VoiceClipSummary | null {
  const reference = voice.profile?.reference_path;
  if (reference) {
    return {
      clip_id: "reference",
      phrase_id: "",
      file_name: "reference.wav",
      path: reference,
      bytes: 44,
    };
  }
  const clips = voice.profile?.clips ?? [];
  for (const phrase of effectivePhrases()) {
    const clip = clipForPhraseId(clips, phrase.id);
    if (clip) {
      return clip;
    }
  }
  return clips.find((clip) => clip.bytes >= 44) ?? null;
}

function clipForPhrase(voice: VoiceOption, phrase: PhraseDefinition): VoiceClipSummary | null {
  return clipForPhraseId(voice.profile?.clips ?? [], phrase.id);
}

function clipForPhraseId(clips: VoiceClipSummary[], phraseId: string): VoiceClipSummary | null {
  if (!phraseId) {
    return null;
  }
  return clips.find((clip) => clip.bytes >= 44 && clipBelongsToPhrase(clip, phraseId)) ?? null;
}

function clipBelongsToPhrase(clip: VoiceClipSummary, phraseId: string): boolean {
  if (clip.phrase_id === phraseId) {
    return true;
  }
  return clip.clip_id === phraseId || clip.clip_id.endsWith(`_${phraseId}`);
}

function readyClipCount(voice: VoiceOption, phrases: PhraseDefinition[]): number {
  return phrases.filter((phrase) => clipForPhrase(voice, phrase)).length;
}

async function playClip(path: string): Promise<void> {
  try {
    voicePreviewAudio?.pause();
    paintVoicePreview();
    nudgeAudio?.pause();
    nudgeAudio = new Audio(clipAssetUrl(path));
    nudgeAudio.preload = "auto";
    await nudgeAudio.play();
    voiceError = "";
  } catch (error) {
    voiceError = `Could not play voice clip: ${errorText(error)}`;
    render();
  }
}

function dismissVoiceInfo(): void {
  stopVoicePreview();
  voiceInfoId = null;
}

function stopVoicePreview(): void {
  if (previewBarDragging) {
    previewBarDragging = false;
    window.removeEventListener("pointerup", endPreviewBarDrag);
  }
  if (previewWaveRaf) {
    cancelAnimationFrame(previewWaveRaf);
    previewWaveRaf = 0;
  }
  const audio = voicePreviewAudio;
  voicePreviewAudio = null;
  voicePreviewPath = "";
  if (!audio) {
    return;
  }
  audio.pause();
  audio.removeAttribute("src");
}

function ensureVoicePreviewAudio(path: string): HTMLAudioElement {
  if (voicePreviewAudio && voicePreviewPath === path) {
    return voicePreviewAudio;
  }
  stopVoicePreview();
  const audio = new Audio(clipAssetUrl(path));
  audio.preload = "auto";
  audio.addEventListener("timeupdate", paintVoicePreview);
  audio.addEventListener("loadedmetadata", paintVoicePreview);
  audio.addEventListener("play", paintVoicePreview);
  audio.addEventListener("pause", paintVoicePreview);
  audio.addEventListener("ended", () => {
    if (voicePreviewAudio) {
      voicePreviewAudio.currentTime = 0;
    }
    paintVoicePreview();
  });
  voicePreviewAudio = audio;
  voicePreviewPath = path;
  return audio;
}

function transportPreviewClip(): VoiceClipSummary | null {
  const voice = availableVoices().find((item) => item.id === voiceInfoId);
  return voice ? previewClipForVoice(voice) : null;
}

function bindVoicePreview(): void {
  const bar = document.getElementById("voice-preview-bar") as HTMLInputElement | null;
  if (!bar) {
    if (!voiceInfoId) {
      stopVoicePreview();
    } else {
      voicePreviewAudio?.pause();
    }
    return;
  }

  const clip = transportPreviewClip();
  if (clip) {
    ensureVoicePreviewAudio(clip.path);
  } else {
    stopVoicePreview();
  }
  bar.addEventListener("pointerdown", () => {
    previewBarDragging = true;
    window.addEventListener("pointerup", endPreviewBarDrag, { once: true });
  });
  bar.addEventListener("pointerup", endPreviewBarDrag);
  bar.addEventListener("pointercancel", endPreviewBarDrag);
  bar.addEventListener("change", endPreviewBarDrag);
  bar.addEventListener("input", () => {
    const next = Number(bar.value);
    const audio = voicePreviewAudio;
    if (audio && Number.isFinite(next) && audio.duration > 0) {
      audio.currentTime = next;
    }
    paintVoicePreview();
  });
  paintVoicePreview();
}

function endPreviewBarDrag(): void {
  if (!previewBarDragging) {
    return;
  }
  previewBarDragging = false;
  window.removeEventListener("pointerup", endPreviewBarDrag);
  paintVoicePreview();
}

const waveformFailed = new Set<string>();

function bindVoiceWaveforms(): void {
  document.querySelectorAll<HTMLCanvasElement>("canvas[data-wave-path]").forEach((canvas) => {
    const path = canvas.dataset.wavePath ?? "";
    const bars = Number(canvas.dataset.waveBars) || 11;
    if (!path || waveformFailed.has(path)) {
      return;
    }
    void fillWaveform(canvas, path, bars, canvas.id === "voice-preview-wave");
  });
}

function paintVoicePreview(): void {
  const button = document.querySelector<HTMLButtonElement>("[data-action='toggle-voice-preview']");
  const bar = document.getElementById("voice-preview-bar") as HTMLInputElement | null;
  const audio = voicePreviewAudio;
  const playing = Boolean(audio && !audio.paused && !audio.ended);
  if (button) {
    button.classList.toggle("is-playing", playing);
    button.setAttribute("aria-label", playing ? "Pause" : "Play");
  }
  const duration = audio && Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : 0;
  const current =
    previewBarDragging && bar ? Number(bar.value) : duration && audio ? audio.currentTime : 0;
  const progress = duration ? Math.min(1, Math.max(0, current / duration)) : 0;
  const elapsed = document.getElementById("voice-preview-elapsed");
  const total = document.getElementById("voice-preview-total");
  if (elapsed) {
    elapsed.textContent = formatDuration(current);
  }
  if (total) {
    total.textContent = formatDuration(duration);
  }
  const wave = document.getElementById("voice-preview-wave") as HTMLCanvasElement | null;
  const peaks = wave ? peaksForCanvas(wave) : null;
  if (wave && peaks) {
    drawWaveform(wave, peaks, progress, true);
  }
  if (playing) {
    if (!previewWaveRaf) {
      previewWaveRaf = requestAnimationFrame(() => {
        previewWaveRaf = 0;
        paintVoicePreview();
      });
    }
  } else if (previewWaveRaf) {
    cancelAnimationFrame(previewWaveRaf);
    previewWaveRaf = 0;
  }
  if (!bar) {
    return;
  }
  if (!previewBarDragging) {
    bar.max = duration ? String(duration) : "1";
    bar.value = duration ? String(current) : "0";
  }
  bar.disabled = !duration;
  bar.style.setProperty("--range-progress", `${progress * 100}%`);
}

async function fillWaveform(
  canvas: HTMLCanvasElement,
  path: string,
  bars: number,
  timed: boolean,
): Promise<void> {
  let peaks: number[];
  try {
    peaks = await loadWaveformPeaks(path, bars);
  } catch {
    waveformFailed.add(path);
    return;
  }
  if (!canvas.isConnected) {
    return;
  }
  const audio = voicePreviewAudio;
  const duration = audio && Number.isFinite(audio.duration) && audio.duration > 0 ? audio.duration : 0;
  const progress = timed && duration && audio ? audio.currentTime / duration : 1;
  drawWaveform(canvas, peaks, timed ? progress : 1, timed);
  if (canvas.clientWidth < 2) {
    requestAnimationFrame(() => {
      if (canvas.isConnected) {
        drawWaveform(canvas, peaks, timed ? progress : 1, timed);
      }
    });
  }
}

function peaksForCanvas(canvas: HTMLCanvasElement): number[] | null {
  const path = canvas.dataset.wavePath ?? "";
  const bars = Number(canvas.dataset.waveBars) || 11;
  return waveformPeaks.get(`${path}:${bars}`) ?? null;
}

async function loadWaveformPeaks(path: string, bars: number): Promise<number[]> {
  const key = `${path}:${bars}`;
  const cached = waveformPeaks.get(key);
  if (cached) {
    return cached;
  }
  const buffer = await loadWaveformBuffer(path);
  const data = buffer.getChannelData(0);
  const size = Math.max(1, Math.floor(data.length / bars));
  const peaks: number[] = [];
  for (let i = 0; i < bars; i += 1) {
    let peak = 0;
    const start = i * size;
    for (let j = 0; j < size; j += 1) {
      peak = Math.max(peak, Math.abs(data[start + j] ?? 0));
    }
    peaks.push(peak);
  }
  waveformPeaks.set(key, peaks);
  return peaks;
}

/** Not cached: peaks are, and a decoded buffer is seconds of float32 per clip. */
async function loadWaveformBuffer(path: string): Promise<AudioBuffer> {
  let pending = waveformPending.get(path);
  if (!pending) {
    pending = (async () => {
      const response = await fetch(clipAssetUrl(path));
      if (!response.ok) {
        throw new Error("waveform fetch failed");
      }
      const bytes = await response.arrayBuffer();
      const context = waveformContext ?? (waveformContext = new AudioContext());
      return context.decodeAudioData(bytes.slice(0));
    })();
    waveformPending.set(path, pending);
  }
  try {
    return await pending;
  } finally {
    waveformPending.delete(path);
  }
}

function sizeWaveformCanvas(canvas: HTMLCanvasElement): void {
  const dpr = window.devicePixelRatio || 1;
  const width = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const height = Math.max(1, Math.round(canvas.clientHeight * dpr));
  if (canvas.width !== width || canvas.height !== height) {
    canvas.width = width;
    canvas.height = height;
  }
}

function drawWaveform(canvas: HTMLCanvasElement, peaks: number[], progress: number, timed: boolean): void {
  sizeWaveformCanvas(canvas);
  const ctx = canvas.getContext("2d");
  if (!ctx || peaks.length === 0) {
    return;
  }
  const width = canvas.width;
  const height = canvas.height;
  if (width < 2 || height < 2) {
    return;
  }
  // Static card waves are drawn once per size and theme; only the preview redraws while playing.
  const drawnKey = `${width}x${height}:${theme}:${peaks.length}:${timed ? progress.toFixed(3) : "static"}`;
  if (canvas.dataset.drawnKey === drawnKey) {
    return;
  }
  canvas.dataset.drawnKey = drawnKey;
  ctx.clearRect(0, 0, width, height);
  const gap = Math.max(1, Math.round((window.devicePixelRatio || 1) * 2));
  const barWidth = Math.max(1, (width - gap * (peaks.length - 1)) / peaks.length);
  const playedUntil = timed ? progress * peaks.length : peaks.length;
  const ink = getComputedStyle(canvas).color;
  const played = getComputedStyle(document.documentElement).getPropertyValue("--sv-green").trim() || "#a8eb37";
  const rest = getComputedStyle(document.documentElement).getPropertyValue("--sv-muted").trim() || "#8d8a80";
  peaks.forEach((peak, index) => {
    const barHeight = Math.max(3, peak * (height - 4));
    const x = index * (barWidth + gap);
    const y = (height - barHeight) / 2;
    ctx.fillStyle = timed ? (index < playedUntil ? played : rest) : ink;
    if (typeof ctx.roundRect === "function") {
      ctx.beginPath();
      ctx.roundRect(
        x,
        y,
        barWidth,
        barHeight,
        Math.min(Math.round((window.devicePixelRatio || 1) * 2), barWidth / 2),
      );
      ctx.fill();
    } else {
      ctx.fillRect(x, y, barWidth, barHeight);
    }
  });
}

async function toggleVoicePreview(): Promise<void> {
  const clip = transportPreviewClip();
  if (!clip) {
    return;
  }
  nudgeAudio?.pause();
  const audio = ensureVoicePreviewAudio(clip.path);
  try {
    if (audio.paused) {
      await audio.play();
    } else {
      audio.pause();
    }
    if (voicePreviewAudio !== audio) {
      return;
    }
    voiceError = "";
    paintVoicePreview();
  } catch (error) {
    if (voicePreviewAudio !== audio) {
      return;
    }
    voiceError = `Could not play voice clip: ${errorText(error)}`;
    render();
  }
}

/** Mirrors `Strictness::focus_config` in the backend: the seconds a distraction may stay open
 * before Supervisor speaks up. */
const STRICTNESS_HELP = "How long a distraction can stay open before a nudge.";
const STRICTNESS_SECONDS: Record<Strictness, number> = { gentle: 90, normal: 60, strict: 30 };

function strictnessOptions(selected: Strictness): string {
  return (["gentle", "normal", "strict"] as Strictness[])
    .map((value) => `<option value="${value}" ${value === selected ? "selected" : ""}>${label(value)} · ${STRICTNESS_SECONDS[value]} s</option>`)
    .join("");
}

function voiceOptions(selected: string, forClips = false): string {
  return availableVoices()
    .map((voice) => {
      if (forClips && voice.id === "none") {
        return "";
      }
      const enabled = forClips ? Boolean(voice.profile?.reference_path) : voice.selectable;
      return `<option value="${voice.id}" ${voice.id === selected ? "selected" : ""} ${enabled ? "" : "disabled"}>${escapeHtml(voice.name)}</option>`;
    })
    .join("");
}

function sameTone(a: VoiceTone, b: VoiceTone): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

function tonePhrases(tone: VoiceTone): PhraseDefinition[] {
  return effectivePhrases().filter((phrase) => sameTone(phrase.tone, tone));
}

/** Bundled tones keep their names; only the user's own sets can be renamed or deleted. */
function isCustomTone(tone: VoiceTone): boolean {
  return !toneOrder.some((item) => sameTone(item, tone));
}

/** The tone a voice speaks in sessions: its saved one while that tone exists, else the first. */
function profileTone(profile: VoiceProfileSummary | null): VoiceTone {
  const tones = phraseToneOrder();
  const saved = profile?.tone;
  return (saved && tones.find((tone) => sameTone(tone, saved))) || tones[0] || toneOrder[0];
}

function voiceTone(voice: VoiceOption): VoiceTone {
  return profileTone(voice.profile);
}

/** The tone the Clips panel shows: the picked one while it exists, else the voice's own. */
function viewedTone(voice: VoiceOption): VoiceTone {
  const picked = clipsTone ? phraseToneOrder().find((tone) => sameTone(tone, clipsTone as string)) : null;
  return picked || voiceTone(voice);
}

/** Every line of the voice's tone has a clip, and the tone has lines at all. */
function profileToneReady(profile: VoiceProfileSummary | null): boolean {
  const phrases = tonePhrases(profileTone(profile));
  return phrases.length > 0 && phrases.every((phrase) => clipForPhraseId(profile?.clips ?? [], phrase.id));
}

function availableVoices(): VoiceOption[] {
  const customVoices =
    voiceBackend?.profiles
      .filter((profile) => !(STARTER_PROFILE_IDS as readonly string[]).includes(profile.profile_id))
      .map((profile) => {
      const phrases = effectivePhrases();
      const ready = phrases.filter((phrase) => clipForPhraseId(profile.clips, phrase.id)).length;
      const hasReference = profile.reference_retained && Boolean(profile.reference_path);
      return {
        id: voiceOptionId(profile.profile_id),
        name: profile.display_name,
        kind: ready > 0 ? "Generated" : hasReference ? "Reference saved" : "Incomplete",
        detail: ready > 0
          ? "Generated phrase clips are ready for playback and sessions."
          : hasReference
            ? "Reference clip is saved. Generate audio before using this voice."
            : "Reference clip is missing. Delete this profile or record again.",
        coverage: `${ready}/${phrases.length} clips`,
        source: "custom" as VoiceSource,
        selectable: profileToneReady(profile),
        profile,
      };
    }) ?? [];

  return [noneVoice, ...starterVoiceOptions(), ...customVoices];
}

function selectedVoice(): VoiceOption {
  return availableVoices().find((voice) => voice.id === draft.voiceId) ?? starterVoiceOptions()[1];
}

function clipsVoice(): VoiceOption {
  return availableVoices().find((voice) => voice.id === clipsVoiceId) ?? starterVoiceOptions()[1];
}

function modelInstalled(): boolean {
  return Boolean(voiceBackend?.model.installed && voiceBackend.runtime.installed);
}

function ramGenerationLocked(): boolean {
  return voiceBackend?.system.has_enough_ram === false;
}

function ramLockMessage(): string {
  const system = voiceBackend?.system;
  const need = system?.min_free_ram_bytes ? formatBytes(system.min_free_ram_bytes) : "enough";
  const free = system?.available_ram_bytes != null ? formatBytes(system.available_ram_bytes) : "unknown";
  return `Clone locked. Need about ${need} available RAM, this computer has ${free} available.`;
}

function canGenerateClip(voice: VoiceOption): boolean {
  return modelInstalled() && voiceBusy === null && Boolean(voice.profile?.reference_path) && !ramGenerationLocked();
}

function clipGenerateLockTitle(voice: VoiceOption): string {
  if (!modelInstalled()) {
    return "Download voice pack first.";
  }
  if (ramGenerationLocked()) {
    return ramLockMessage();
  }
  if (!voice.profile?.reference_path) {
    return "Pick a cloned voice under Clips for.";
  }
  return "";
}

function canEditPhrases(): boolean {
  return voiceBusy === null;
}

function phraseCountLabel(count: number): string {
  return `${count} ${count === 1 ? "phrase" : "phrases"}`;
}

function profileById(profileId: string): VoiceProfileSummary | null {
  return voiceBackend?.profiles.find((profile) => profile.profile_id === profileId) ?? null;
}

function upsertProfile(profile: VoiceProfileSummary): void {
  if (!voiceBackend) {
    return;
  }

  const profiles = voiceBackend.profiles.filter((item) => item.profile_id !== profile.profile_id);
  profiles.push(profile);
  profiles.sort((left, right) => left.display_name.localeCompare(right.display_name));
  voiceBackend = { ...voiceBackend, profiles };
}

function voiceOptionId(profileId: string): string {
  return `profile:${profileId}`;
}

function voiceIdForProfile(profileId: string): string {
  if (profileId === "derek") {
    return "starter-male";
  }
  if (profileId === "elise") {
    return "starter-female";
  }
  return voiceOptionId(profileId);
}

function normalizeStoredVoiceId(value: string): string {
  if (value === "none" || value === "starter-male" || value === "starter-female" || value.startsWith("profile:")) {
    return value;
  }
  return "starter-female";
}

function ensureSelectedVoice(): void {
  if (draft.voiceId !== "none" && draft.voiceId !== "starter-male" && draft.voiceId !== "starter-female") {
    if (voiceBackend && !availableVoices().some((item) => item.id === draft.voiceId)) {
      draft.voiceId = "starter-female";
    }
  }
  if (!availableVoices().some((item) => item.id === clipsVoiceId)) {
    clipsVoiceId = "starter-female";
  }
  if (!clipsVoiceTouched && !clipsVoice().profile?.reference_path) {
    const cloned = availableVoices().find((item) => item.profile?.reference_path);
    if (cloned) {
      clipsVoiceId = cloned.id;
    }
  }
}

function openPhraseEditor(phraseId: string | null, tone: VoiceTone): void {
  const phrase = phraseId ? effectivePhrases().find((item) => item.id === phraseId) : null;
  phraseEditor = {
    phraseId,
    tone: phrase?.tone ?? tone,
    text: phrase?.text ?? "",
  };
  paintVoiceLayer();
  (document.getElementById("phrase-editor-text") as HTMLInputElement | null)?.focus();
}

function phraseToneOrder(): VoiceTone[] {
  const tones: VoiceTone[] = [...toneOrder];
  for (const tone of customPhraseSets) {
    if (!tones.some((item) => item.toLowerCase() === tone.toLowerCase())) {
      tones.push(tone);
    }
  }
  for (const phrase of effectivePhrases()) {
    if (!tones.some((tone) => tone.toLowerCase() === phrase.tone.toLowerCase())) {
      tones.push(phrase.tone);
    }
  }
  return tones;
}

function phraseToneOptions(selected: VoiceTone): string {
  const tones = phraseToneOrder();
  if (!tones.some((tone) => tone.toLowerCase() === selected.toLowerCase())) {
    tones.push(selected);
  }
  return tones
    .map(
      (tone) =>
        `<option value="${attr(tone)}" ${tone.toLowerCase() === selected.toLowerCase() ? "selected" : ""}>${escapeHtml(toneLabel(tone))}</option>`,
    )
    .join("");
}

function effectivePhrases(): PhraseDefinition[] {
  return [
    ...phraseBank.filter((phrase) => !hiddenPhrases.has(phrase.id)).map((phrase) => phraseOverrides[phrase.id] ?? phrase),
    ...customPhrases,
  ];
}

function voicePhraseInputs(): PhraseDefinition[] {
  return effectivePhrases().map((phrase) => ({
    id: phrase.id,
    tone: phrase.tone,
    text: phrase.text,
  }));
}

function phraseKind(phrase: PhraseDefinition): string {
  if (phrase.id.startsWith("custom_")) {
    return "Custom";
  }
  if (phraseOverrides[phrase.id]) {
    return "Edited";
  }
  return "Bundled";
}

function uniqueCustomPhraseId(text: string): string {
  const base = cleanSlug(text).slice(0, 48) || "phrase";
  const existing = new Set(effectivePhrases().map((phrase) => phrase.id));
  let candidate = `custom_${base}`;
  let suffix = 2;
  while (existing.has(candidate)) {
    candidate = `custom_${base}_${suffix}`;
    suffix += 1;
  }
  return candidate;
}

function uniqueVoiceProfileId(name: string): string {
  const base = cleanSlug(name).slice(0, 42) || "voice";
  const existing = new Set(voiceBackend?.profiles.map((profile) => profile.profile_id) ?? []);
  let candidate = base;
  let suffix = 2;
  while (existing.has(candidate)) {
    candidate = `${base}_${suffix}`;
    suffix += 1;
  }
  return candidate;
}

function cleanSlug(value: string): string {
  return value
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "");
}

function defaultVoiceName(): string {
  const count = voiceBackend?.profiles.length ?? 0;
  return `Custom Local ${count + 1}`;
}

function recordingDurationSeconds(): number {
  if (recordingState === "recording" && recordingStartedAtMs !== null) {
    return (Date.now() - recordingStartedAtMs) / 1000;
  }
  return recordedVoice?.seconds ?? 0;
}

function bumpClipAsset(path: string): void {
  clipAssetRevision.set(path, Date.now());
  waveformFailed.delete(path);
  for (const key of [...waveformPeaks.keys()]) {
    if (key.startsWith(`${path}:`)) {
      waveformPeaks.delete(key);
    }
  }
  waveformPending.delete(path);
  if (voicePreviewPath === path) {
    stopVoicePreview();
  }
}

function clipAssetUrl(path: string): string {
  let src = path;
  try {
    src = convertFileSrc(path);
  } catch {
    src = path;
  }
  const rev = clipAssetRevision.get(path);
  return rev ? `${src}?v=${rev}` : src;
}

function voiceBusyTitle(): string {
  switch (voiceBusy) {
    case "download-model":
      return "Downloading model pack";
    case "delete-model":
      return "Deleting model files";
    case "generate-clips":
      return "Generating a clip";
    case "delete-voice":
      return "Deleting voice";
    default:
      return "Working";
  }
}

function voiceBusyCopy(): string {
  if (voiceBusy === "generate-clips") {
    return "About a minute, all on this computer. Saved clips play without the model.";
  }
  if (voiceBusy === "download-model") {
    return "The model pack is large. Supervisor will return to small mode after generation.";
  }
  return "Local voice files are being updated.";
}

function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) {
    return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
  }
  if (bytes >= 1024 ** 2) {
    return `${Math.round(bytes / 1024 ** 2)} MB`;
  }
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}

function toneLabel(tone: VoiceTone): string {
  switch (tone) {
    case "gentle":
      return "Gentle";
    case "mom":
      return "Mom";
    case "stern":
      return "Stern";
    case "genz":
      return "Gen Z";
    default:
      return tone;
  }
}

function cloneRules(rules: AppRule[]): AppRule[] {
  return rules.map((rule) => ({ ...rule }));
}

function formValue(form: HTMLFormElement, name: string): string {
  const field = form.elements.namedItem(name);
  if (
    field instanceof HTMLInputElement ||
    field instanceof HTMLSelectElement ||
    field instanceof HTMLTextAreaElement
  ) {
    return field.value;
  }
  return "";
}

async function refreshTrends(shouldRender = true): Promise<void> {
  try {
    trends = await invoke<TrendsData>("get_trends");
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }
  if (shouldRender) {
    if (activeTab === "session" && !preflightOpen) {
      animateCalendarSoon();
    }
    render();
  }
}

async function refreshHistoryWeek(shouldRender = true): Promise<void> {
  if (!historyWeekStart) {
    historyWeekStart = thisMondayKey();
  }
  const request = ++historyWeekRequest;
  try {
    const [week, recap, names] = await Promise.all([
      invoke<HistoryWeek>("get_history_week", { weekStart: historyWeekStart }),
      invoke<WeekRecap>("get_week_recap", { start: historyWeekStart }),
      invoke<Record<string, string>>("get_app_names"),
    ]);
    if (request !== historyWeekRequest) {
      return;
    }
    historyWeek = week;
    weekRecap = recap;
    appNames = names;
    historyWeekStart = week.week_start;
    if (historySearch.trim()) {
      await runHistorySearch(historySearch.trim());
      return;
    }
  } catch (error) {
    if (request !== historyWeekRequest) {
      return;
    }
    lastError = errorText(error);
  }
  if (shouldRender) {
    render();
  }
}

function scheduleHistorySearch(): void {
  if (historySearchTimer !== null) {
    window.clearTimeout(historySearchTimer);
    historySearchTimer = null;
  }
  const query = historySearch.trim();
  if (!query) {
    historySearchRequest += 1;
    historySearchResults = null;
    render();
    return;
  }
  historySearchTimer = window.setTimeout(() => {
    historySearchTimer = null;
    void runHistorySearch(query);
  }, 150);
}

async function runHistorySearch(query: string): Promise<void> {
  const request = ++historySearchRequest;
  try {
    const results = await invoke<SessionSummary[]>("search_sessions", { query });
    if (request !== historySearchRequest) {
      return;
    }
    historySearchResults = results;
  } catch (error) {
    if (request !== historySearchRequest) {
      return;
    }
    lastError = errorText(error);
  }
  render();
}

function noteNewHistory(items: SessionSummary[]): number | null {
  let newest: number | null = null;
  for (const item of items) {
    if (!knownHistoryIds.has(item.id)) {
      if (newest === null) {
        newest = item.id;
      }
      knownHistoryIds.add(item.id);
    }
  }
  return newest;
}

async function openHistoryReport(id: number): Promise<void> {
  if (settingsView === "webcam") {
    settingsView = "home";
    await stopWebcamPreview();
  }
  try {
    sessionReport = await invoke<SessionReport>("get_session_report", { id });
    historyView = "report";
    activeTab = "history";
    lastError = "";
    knownHistoryIds.add(id);
  } catch (error) {
    lastError = errorText(error);
    historyView = "list";
    sessionReport = null;
  }
  await refreshTrends(false);
  render();
  reconcilePolling();
}

async function confirmHistoryAction(): Promise<void> {
  const confirm = historyConfirm;
  if (!confirm) {
    return;
  }
  historyConfirm = null;
  if (confirm.kind === "delete-session" && confirm.id) {
    await runHistoryCommand("delete_session", { id: confirm.id }, "Session deleted");
    historyView = "list";
    sessionReport = null;
    await refreshHistoryWeek();
    return;
  }
  if (confirm.kind === "delete-all") {
    await runHistoryCommand("delete_all_history", undefined, "History cleared");
    historyView = "list";
    sessionReport = null;
    await refreshHistoryWeek();
  }
}

async function runHistoryCommand(
  command: string,
  args: Record<string, unknown> | undefined,
  message: string,
): Promise<void> {
  try {
    await invoke(command, args ?? {});
    snapshot = await invoke<SessionSnapshot>("get_session_snapshot");
    settings = snapshot.settings;
    history = snapshot.history;
    history.forEach((item) => knownHistoryIds.add(item.id));
    lastError = "";
    await refreshTrends(false);
    await refreshHistoryWeek(false);
    showFeedback(message);
  } catch (error) {
    lastError = errorText(error);
    render();
  }
}

/** "18 October 2026, 11:13 AM · Normal" */
function reportSummaryLine(report: SessionReport): string {
  const started = new Date(report.started_at);
  const date = started.toLocaleDateString("en-GB", { day: "numeric", month: "long", year: "numeric" });
  const time = started.toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
  return `${date}, ${time} · ${label(report.strictness)}`;
}


function formatMinutes(totalSeconds: number): string {
  if (totalSeconds > 0 && totalSeconds < 60) return "<1 min";
  return `${Math.max(0, Math.round(totalSeconds / 60))} min`;
}

function formatStretchChip(totalSeconds: number): string {
  if (totalSeconds < 60) {
    return "<1m";
  }
  const minutes = Math.round(totalSeconds / 60);
  if (minutes >= 60) {
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest ? `${hours}h ${rest}m` : `${hours}h`;
  }
  return `${minutes}m`;
}

/** One per day of the year, cycling. Short enough to read at a glance and to stand alone. */
const DAILY_NOTES = [
  { text: "To be everywhere is to be nowhere.", by: "Seneca" },
  { text: "How we spend our days is, of course, how we spend our lives.", by: "Annie Dillard" },
  { text: "Tell me to what you pay attention and I will tell you who you are.", by: "José Ortega y Gasset" },
  { text: "Well begun is half done.", by: "Aristotle" },
  { text: "Inspiration exists, but it has to find you working.", by: "Pablo Picasso" },
  { text: "Confine yourself to the present.", by: "Marcus Aurelius" },
  { text: "It is not enough to be busy. So are the ants. The question is: what are we busy about?", by: "Henry David Thoreau" },
  { text: "Resolve to perform what you ought; perform without fail what you resolve.", by: "Benjamin Franklin" },
  { text: "No great thing is created suddenly.", by: "Epictetus" },
  { text: "Lost time is never found again.", by: "Benjamin Franklin" },
  { text: "Attention is the rarest and purest form of generosity.", by: "Simone Weil" },
  { text: "You don't have to be great to start, but you have to start to be great.", by: "Zig Ziglar" },
  { text: "The mind is not a vessel to be filled, but a fire to be kindled.", by: "Plutarch" },
  { text: "Do not wait to strike till the iron is hot; but make it hot by striking.", by: "W. B. Yeats" },
  { text: "My experience is what I agree to attend to.", by: "William James" },
  { text: "Tomorrow is often the busiest day of the week.", by: "Spanish proverb" },
  { text: "It is not that we have a short time to live, but that we waste a good deal of it.", by: "Seneca" },
  { text: "Amateurs sit and wait for inspiration, the rest of us just get up and go to work.", by: "Stephen King" },
  { text: "Concentration is the secret of strength.", by: "Ralph Waldo Emerson" },
  { text: "The best time to plant a tree was twenty years ago. The second best time is now.", by: "Chinese proverb" },
  { text: "Nothing will work unless you do.", by: "Maya Angelou" },
  { text: "Focus is a matter of deciding what things you are not going to do.", by: "John Carmack" },
  { text: "If you seek tranquillity, do less. Or, more accurately, do what is essential.", by: "Marcus Aurelius" },
  { text: "Work expands so as to fill the time available for its completion.", by: "C. Northcote Parkinson" },
  { text: "Begin at once to live, and count each separate day as a separate life.", by: "Seneca" },
  { text: "Finish each day and be done with it.", by: "Ralph Waldo Emerson" },
  { text: "The scariest moment is always just before you start.", by: "Stephen King" },
  { text: "Great things are done by a series of small things brought together.", by: "Vincent van Gogh" },
  { text: "Don't mistake activity with achievement.", by: "John Wooden" },
  { text: "A wealth of information creates a poverty of attention.", by: "Herbert Simon" },
  { text: "How long are you going to wait before you demand the best for yourself?", by: "Epictetus" },
  { text: "Someday is not a day of the week.", by: "Janet Dailey" },
  { text: "The first and greatest victory is to conquer yourself.", by: "Plato" },
  { text: "Do what you can, with what you have, where you are.", by: "Theodore Roosevelt" },
  { text: "Things which matter most must never be at the mercy of things which matter least.", by: "Goethe" },
  { text: "Never leave that till tomorrow which you can do today.", by: "Benjamin Franklin" },
  { text: "Almost everything will work again if you unplug it for a few minutes, including you.", by: "Anne Lamott" },
  { text: "Our life is frittered away by detail. Simplify, simplify.", by: "Henry David Thoreau" },
  { text: "Deciding what not to do is as important as deciding what to do.", by: "Steve Jobs" },
  { text: "Waste no more time arguing what a good man should be. Be one.", by: "Marcus Aurelius" },
  { text: "Dig the well before you are thirsty.", by: "Chinese proverb" },
  { text: "Nothing is so fatiguing as the eternal hanging on of an uncompleted task.", by: "William James" },
  { text: "Little by little, one travels far.", by: "Spanish proverb" },
  { text: "What is important is seldom urgent and what is urgent is seldom important.", by: "Dwight D. Eisenhower" },
  { text: "First say to yourself what you would be; and then do what you have to do.", by: "Epictetus" },
  { text: "Do every act of your life as though it were the very last act of your life.", by: "Marcus Aurelius" },
  { text: "Energy and persistence conquer all things.", by: "Benjamin Franklin" },
  { text: "The impediment to action advances action. What stands in the way becomes the way.", by: "Marcus Aurelius" },
  { text: "You will never find time for anything. If you want time, you must make it.", by: "Charles Buxton" },
  { text: "When walking, walk. When eating, eat.", by: "Zen proverb" },
  { text: "Knowing is not enough; we must apply. Willing is not enough; we must do.", by: "Goethe" },
  { text: "There is no substitute for hard work.", by: "Thomas Edison" },
  { text: "The art of being wise is the art of knowing what to overlook.", by: "William James" },
  { text: "Either you run the day or the day runs you.", by: "Jim Rohn" },
  { text: "Do the thing and you shall have the power.", by: "Ralph Waldo Emerson" },
  { text: "Fall seven times, stand up eight.", by: "Japanese proverb" },
  { text: "We suffer more often in imagination than in reality.", by: "Seneca" },
  { text: "Make each day your masterpiece.", by: "John Wooden" },
  { text: "The journey of a thousand miles begins with a single step.", by: "Laozi" },
  { text: "Procrastination is the thief of time.", by: "Edward Young" },
  { text: "Attention is the beginning of devotion.", by: "Mary Oliver" },
  { text: "Don't explain your philosophy. Embody it.", by: "Epictetus" },
  { text: "Plans are worthless, but planning is everything.", by: "Dwight D. Eisenhower" },
  { text: "A year from now you may wish you had started today.", by: "Karen Lamb" },
  { text: "Give yourself a gift: the present moment.", by: "Marcus Aurelius" },
  { text: "The reward of a thing well done is to have done it.", by: "Ralph Waldo Emerson" },
  { text: "If you spend too much time thinking about a thing, you will never get it done.", by: "Bruce Lee" },
  { text: "Motivation is what gets you started. Habit is what keeps you going.", by: "Jim Rohn" },
  { text: "Action is the antidote to despair.", by: "Joan Baez" },
  { text: "You can't cross the sea merely by standing and staring at the water.", by: "Rabindranath Tagore" },
  { text: "Little strokes fell great oaks.", by: "Benjamin Franklin" },
  { text: "Measure twice, cut once.", by: "Proverb" },
  { text: "There is nothing so useless as doing efficiently that which should not be done at all.", by: "Peter Drucker" },
  { text: "It does not matter how slowly you go as long as you do not stop.", by: "Confucius" },
  { text: "Difficulties strengthen the mind, as labor does the body.", by: "Seneca" },
  { text: "We are what we repeatedly do. Excellence, then, is not an act, but a habit.", by: "Will Durant" },
  { text: "Do not let what you cannot do interfere with what you can do.", by: "John Wooden" },
  { text: "We are kept from our goal not by obstacles but by a clear path to a lesser goal.", by: "Robert Brault" },
  { text: "The best way out is always through.", by: "Robert Frost" },
  { text: "Begin, be bold, and venture to be wise.", by: "Horace" },
  { text: "Don't count the days, make the days count.", by: "Muhammad Ali" },
  { text: "If you want to improve, be content to be thought foolish and stupid.", by: "Epictetus" },
  { text: "A ship in harbor is safe, but that is not what ships are built for.", by: "John A. Shedd" },
  { text: "Tell me, what is it you plan to do with your one wild and precious life?", by: "Mary Oliver" },
];

function dailyNote(): { date: string; text: string; by: string } {
  const notes = DAILY_NOTES;
  const now = new Date();
  const start = new Date(now.getFullYear(), 0, 0);
  const day = Math.floor((now.getTime() - start.getTime()) / 86400000);
  const note = notes[day % notes.length];
  const date = now.toLocaleDateString("en-GB", { day: "2-digit", month: "short" }).toUpperCase();
  return { date, text: note.text, by: note.by };
}

/** A day counts toward the streak at this much focused time. */
const STREAK_DAY_SECONDS = 25 * 60;

function dayFocusMap(): Map<string, number> {
  return new Map(trends.daily_focus.map((day) => [day.date, day.focus_seconds]));
}

function isWeekend(date: Date): boolean {
  return date.getDay() === 0 || date.getDay() === 6;
}

/**
 * The current streak: consecutive counted days ending today or yesterday. Weekends never
 * break it; a weekend inside the run is a bridge. `chain` is every day drawn as part of it.
 */
function streakInfo(asOf = dayKey(new Date())): { days: number; chain: Set<string> } {
  const focus = dayFocusMap();
  const run: { key: string; counted: boolean }[] = [];
  const todayKey = dayKey(new Date());
  const cursor = parseDayKey(asOf);
  for (let step = 0; step < 400; step += 1) {
    const key = dayKey(cursor);
    const counted = (focus.get(key) ?? 0) >= STREAK_DAY_SECONDS;
    if (counted) {
      run.push({ key, counted: true });
    } else if (isWeekend(cursor)) {
      run.push({ key, counted: false });
    } else if (!(step === 0 && key === todayKey)) {
      // A past weekday with no counted time broke the streak; today still has time to count.
      break;
    }
    cursor.setDate(cursor.getDate() - 1);
  }
  // Bridges only count between counted days.
  while (run.length && !run[0].counted) run.shift();
  while (run.length && !run[run.length - 1].counted) run.pop();
  return {
    days: run.filter((day) => day.counted).length,
    chain: new Set(run.map((day) => day.key)),
  };
}

/**
 * The calendar pops in when Home comes into view (arriving on the tab, data landing, the
 * preflight closing), not on every rebuild behind an open preflight.
 */
let calendarAnimateUntil = performance.now() + 1500;

/** Opens a short window in which rebuilds of Home pop the calendar in. */
function animateCalendarSoon(): void {
  calendarAnimateUntil = performance.now() + 1500;
}

function takeCalendarAnimation(): boolean {
  return performance.now() < calendarAnimateUntil;
}

function monthLabel(date: Date): string {
  return date.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

function renderCalendar(streak: { chain: Set<string> }): string {
  const focus = dayFocusMap();
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const first = new Date(today.getFullYear(), today.getMonth(), 1);
  const start = mondayOf(first);
  const last = new Date(today.getFullYear(), today.getMonth() + 1, 0);
  const cells = Math.ceil(((last.getTime() - start.getTime()) / 86400000 + 1) / 7) * 7;
  const letters = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
  const days: string[] = [];
  for (let index = 0; index < cells; index += 1) {
    const date = new Date(start);
    date.setDate(start.getDate() + index);
    const key = dayKey(date);
    const seconds = focus.get(key) ?? 0;
    const classes = ["cal-day"];
    if (date.getMonth() !== today.getMonth()) classes.push("is-out");
    if (date > today) classes.push("is-future");
    else if (key === dayKey(today)) classes.push("is-today");
    else if (seconds >= STREAK_DAY_SECONDS) classes.push("is-on");
    if (streak.chain.has(key)) {
      classes.push("in-chain");
      if (seconds < STREAK_DAY_SECONDS && key !== dayKey(today)) classes.push("is-bridge");
      const next = new Date(date);
      next.setDate(date.getDate() + 1);
      if (date.getDay() === 0 || !streak.chain.has(dayKey(next))) classes.push("chain-end");
    }
    const label = `${date.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" })}, ${formatMinutes(seconds)}`;
    days.push(
      `<span class="${classes.join(" ")}" style="animation-delay: ${index * 12}ms" title="${attr(label)}">${date.getDate()}</span>`,
    );
  }
  return `
    <div class="cal-grid ${takeCalendarAnimation() ? "is-animated" : ""}" role="img" aria-label="${attr(monthLabel(today))}, days with focus shaded">
      ${letters.map((letter) => `<span class="cal-dow">${letter}</span>`).join("")}
      ${days.join("")}
    </div>
  `;
}

/**
 * The day the Home card shows: today once it has a session, else the latest day with one. The
 * card never goes blank in the morning; it flips to Today with the first session.
 */
let dayCard: { day: string; reports: SessionReport[] } | null = null;
/** Day plus the session ids in it; a new session on that day refetches. */
let dayCardKey = "";

async function ensureDayCard(): Promise<void> {
  const newest = history[0];
  if (!newest) {
    dayCard = null;
    dayCardKey = "";
    return;
  }
  const day = dayKey(new Date(newest.started_at));
  const items = history.filter((item) => dayKey(new Date(item.started_at)) === day);
  const key = `${day}:${items.map((item) => item.id).join(",")}`;
  if (key === dayCardKey) {
    return;
  }
  dayCardKey = key;
  dayCard = null;
  try {
    const reports = await Promise.all(items.map((item) => invoke<SessionReport>("get_session_report", { id: item.id })));
    if (dayCardKey === key) {
      reports.sort((a, b) => a.started_at.localeCompare(b.started_at));
      dayCard = { day, reports };
      render();
    }
  } catch {
    // The card stays off until the next snapshot.
  }
}

/** Mirrors the backend: on-task time runs together until a drift, an absence or a gap breaks it. */
function focusedStretches(spans: ReportSpan[]): number[] {
  const stretches: number[] = [];
  let current = 0;
  let previousEnd = 0;
  for (const span of spans) {
    if (span.started_offset_seconds > previousEnd && current > 0) {
      stretches.push(current);
      current = 0;
    }
    previousEnd = span.started_offset_seconds + span.seconds;
    if (stripOnTask(span.state)) {
      current += span.seconds;
    } else if (current > 0) {
      stretches.push(current);
      current = 0;
    }
  }
  if (current > 0) {
    stretches.push(current);
  }
  return stretches;
}

function medianOf(values: number[]): number {
  if (values.length === 0) {
    return 0;
  }
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? Math.floor((sorted[mid - 1] + sorted[mid]) / 2) : sorted[mid];
}

/** The strip has two colours: work and distraction. Time away is left blank. */
function stripOnTask(state: string): boolean {
  return state === "focused";
}

function stripOffTask(state: string): boolean {
  return state === "distracted" || state === "phone" || state === "suspicious";
}

interface DaySummary {
  day: string;
  isToday: boolean;
  focusedSeconds: number;
  attentionSeconds: number;
  /** Sessions laid end to end: each segment's share of the strip and its spans and nudges. */
  segments: { start: number; width: number; report: SessionReport; duration: number }[];
}

function summarizeDay(card: { day: string; reports: SessionReport[] }): DaySummary {
  const durations = card.reports.map((report) => timelineSeconds(report));
  const total = Math.max(1, durations.reduce((sum, seconds) => sum + seconds, 0));
  let start = 0;
  const segments = card.reports.map((report, index) => {
    const segment = { start, width: durations[index] / total, report, duration: durations[index] };
    start += segment.width;
    return segment;
  });
  return {
    day: card.day,
    isToday: card.day === dayKey(new Date()),
    focusedSeconds: card.reports.reduce((sum, report) => sum + report.focused_seconds, 0),
    attentionSeconds: medianOf(card.reports.flatMap((report) => focusedStretches(report.spans))),
    segments,
  };
}

/** "Today", "Yesterday", else the weekday. The date itself is said once, in the corner. */
function dayTitle(day: string): string {
  const today = dayKey(new Date());
  if (day === today) {
    return "Today";
  }
  if (day === addDaysKey(today, -1)) {
    return "Yesterday";
  }
  return parseDayKey(day).toLocaleDateString(undefined, { weekday: "long" });
}

/** The full date with the year, on the app card and the PNG alike. */
function cardDate(day: string): string {
  return parseDayKey(day).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

/** While today is short of the streak threshold, the card says how far there is to go. */
function cardNote(summary: DaySummary): string {
  if (summary.isToday && summary.focusedSeconds < STREAK_DAY_SECONDS) {
    return `${Math.round(summary.focusedSeconds / 60)} / ${STREAK_DAY_SECONDS / 60} min to keep the streak`;
  }
  return cardDate(summary.day);
}

function streakLabel(days: number): string {
  return `${days} day${days === 1 ? "" : "s"}`;
}

/** The day's strip as runs of on/off-task, cut into equal bins and each bin given the state
 *  that filled most of it. A few seconds in a tab can't paint a line, a real drift still shows,
 *  and the picture stays readable for any length of day. The stats stay exact. */
const STRIP_BINS = 48;

function stripRuns(segments: DaySummary["segments"], bins = STRIP_BINS): { on: boolean; left: number; width: number }[] {
  const onSeconds = new Array<number>(bins).fill(0);
  const offSeconds = new Array<number>(bins).fill(0);
  for (const segment of segments) {
    for (const span of segment.report.spans) {
      const on = stripOnTask(span.state);
      if (!on && !stripOffTask(span.state)) continue;
      const a = segment.start + (span.started_offset_seconds / segment.duration) * segment.width;
      const b = a + (span.seconds / segment.duration) * segment.width;
      for (let k = Math.floor(a * bins); k < Math.min(bins, Math.ceil(b * bins)); k += 1) {
        const overlap = Math.min(b, (k + 1) / bins) - Math.max(a, k / bins);
        if (overlap > 0) (on ? onSeconds : offSeconds)[k] += overlap;
      }
    }
  }
  const runs: { on: boolean; left: number; width: number }[] = [];
  for (let k = 0; k < bins; k += 1) {
    if (onSeconds[k] + offSeconds[k] === 0) continue;
    const on = onSeconds[k] >= offSeconds[k];
    const last = runs[runs.length - 1];
    if (last && last.on === on && Math.abs(last.left + last.width - k / bins) < 1e-9) {
      last.width += 1 / bins;
    } else {
      runs.push({ on, left: k / bins, width: 1 / bins });
    }
  }
  return runs;
}

function renderDayCard(todayStreakDays: number): string {
  if (!dayCard) {
    if (history.length > 0) {
      return "";
    }
    // Nothing recorded yet: the card's outline, so the first session has a visible place to land.
    return `
      <section class="panel last-session-card">
        <p class="eyebrow">Your day</p>
        <div class="share-card is-empty" aria-label="No sessions yet">
          <span class="share-head">
            <strong>Your first card</strong>
            <span>Today</span>
          </span>
          <span class="share-strip" aria-hidden="true"></span>
          <span class="share-stats">
            <span><small>Focused</small><strong>—</strong></span>
            <span><small>Attention span</small><strong>—</strong></span>
            <span><small>Streak</small><strong>${streakLabel(todayStreakDays)}</strong></span>
          </span>
        </div>
        <p class="share-hint">Finish a session and its card lands here</p>
      </section>
    `;
  }
  const summary = summarizeDay(dayCard);
  const streakText = streakLabel(streakInfo(summary.day).days);
  const strip = stripRuns(summary.segments)
    .map((run) => `<i class="${run.on ? "strip-on" : "strip-off"}" style="left: ${(run.left * 100).toFixed(2)}%; width: ${(run.width * 100).toFixed(2)}%"></i>`)
    .join("");
  return `
    <section class="panel last-session-card">
      <p class="eyebrow">Your day</p>
      <button class="share-card" type="button" data-action="copy-card" title="Copy the card as an image">
        <span class="share-head">
          <strong>${escapeHtml(dayTitle(summary.day))}</strong>
          <span>${escapeHtml(cardNote(summary))}</span>
        </span>
        <span class="share-strip" aria-hidden="true"><span class="share-strip-fill">${strip}</span></span>
        <span class="share-stats">
          <span><small>Focused</small><strong>${escapeHtml(formatHours(summary.focusedSeconds))}</strong></span>
          <span><small>Attention span</small><strong>${escapeHtml(formatMinutes(summary.attentionSeconds))}</strong></span>
          <span><small>Streak</small><strong>${streakText}</strong></span>
        </span>
      </button>
      <p class="share-hint">Click the card to copy it and share it</p>
    </section>
  `;
}

/** The card's colours in the current theme (the same tokens as styles.css). */
function cardPalette() {
  return theme === "dark"
    ? { canvas: "#40403c", panel: "#353530", grid: "rgba(255,255,255,0.12)", ink: "#fff8e9", muted: "#ddd5c5", border: "#151512", track: "#4c4b46", shadow: "#0b0b09" }
    : { canvas: "#f4ead7", panel: "#fff4cf", grid: "rgba(16,16,16,0.08)", ink: "#101010", muted: "#5d564a", border: "#101010", track: "#fff7df", shadow: "rgba(17,16,14,0.68)" };
}

/** Sets the largest font (down to `min`) at which `text` fits in `maxWidth`; returns the size used. */
function fitCanvasText(ctx: CanvasRenderingContext2D, text: string, weight: number, family: string, max: number, min: number, maxWidth: number): number {
  let size = max;
  ctx.font = `${weight} ${size}px ${family}`;
  while (size > min && ctx.measureText(text).width > maxWidth) {
    size -= 1;
    ctx.font = `${weight} ${size}px ${family}`;
  }
  return size;
}

const STRIP_GREEN = "#a8eb37";
const STRIP_RED = "#ff684f";

/** Draws the card to a canvas and puts the PNG on the clipboard, in the current theme's colours. */
/** The Enter that confirms Japanese or Chinese input is not a submit. WebKit reports it
 *  with isComposing false, so keyCode 229 (an IME key) is checked too. */
function composing(event: KeyboardEvent): boolean {
  return event.isComposing || event.keyCode === 229;
}

async function copyCard(): Promise<void> {
  if (!dayCard) {
    return;
  }
  const summary = summarizeDay(dayCard);
  const streakDays = streakInfo(summary.day).days;
  const scale = 2;
  const width = 640;
  const height = 316;
  // Room around the card for its hard shadow; the rest of the PNG is transparent.
  const pad = 14;
  const shadow = 8;
  const radius = 12;
  const canvas = document.createElement("canvas");
  canvas.width = (width + pad * 2) * scale;
  canvas.height = (height + pad * 2) * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return;
  }
  ctx.scale(scale, scale);
  ctx.translate(pad, pad);
  const palette = cardPalette();
  const ink = palette.ink;
  const mono = "ui-monospace, Menlo, Consolas, monospace";
  const sans = "Inter, system-ui, sans-serif";

  ctx.fillStyle = palette.shadow;
  ctx.beginPath();
  ctx.roundRect(shadow, shadow, width, height, radius);
  ctx.fill();

  ctx.save();
  ctx.beginPath();
  ctx.roundRect(0, 0, width, height, radius);
  ctx.clip();
  ctx.fillStyle = palette.canvas;
  ctx.fillRect(0, 0, width, height);
  ctx.strokeStyle = palette.grid;
  ctx.lineWidth = 1;
  for (let x = 0; x < width; x += 14) {
    ctx.beginPath();
    ctx.moveTo(x + 0.5, 0);
    ctx.lineTo(x + 0.5, height);
    ctx.stroke();
  }
  for (let y = 0; y < height; y += 14) {
    ctx.beginPath();
    ctx.moveTo(0, y + 0.5);
    ctx.lineTo(width, y + 0.5);
    ctx.stroke();
  }
  // Half the stroke falls outside the clip, so 3px of border shows.
  ctx.strokeStyle = palette.border;
  ctx.lineWidth = 6;
  ctx.beginPath();
  ctx.roundRect(0, 0, width, height, radius);
  ctx.stroke();
  ctx.restore();

  ctx.fillStyle = palette.muted;
  ctx.font = `600 14px ${mono}`;
  ctx.textBaseline = "top";
  ctx.fillText("SUPERVISOR", 36, 32);
  ctx.textAlign = "right";
  ctx.fillText(cardDate(summary.day).toUpperCase(), width - 36, 32);
  ctx.textAlign = "left";

  ctx.fillStyle = ink;
  fitCanvasText(ctx, dayTitle(summary.day), 700, sans, 30, 20, width - 72 - 170);
  ctx.fillText(dayTitle(summary.day), 36, 62);

  const stripX = 36;
  const stripY = 112;
  const stripW = width - 72;
  const stripH = 44;
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(stripX, stripY, stripW, stripH, 6);
  ctx.clip();
  ctx.fillStyle = palette.track;
  ctx.fillRect(stripX, stripY, stripW, stripH);
  for (const run of stripRuns(summary.segments)) {
    ctx.fillStyle = run.on ? STRIP_GREEN : STRIP_RED;
    ctx.fillRect(stripX + run.left * stripW, stripY, run.width * stripW, stripH);
  }
  ctx.restore();
  ctx.strokeStyle = palette.border;
  ctx.lineWidth = 3;
  ctx.beginPath();
  ctx.roundRect(stripX, stripY, stripW, stripH, 6);
  ctx.stroke();

  const stats: [string, string][] = [
    ["FOCUSED", formatHours(summary.focusedSeconds)],
    ["ATTENTION SPAN", formatMinutes(summary.attentionSeconds)],
    ["STREAK", streakLabel(streakDays)],
  ];
  // Three tiles in the app's card language: border, hard shadow, slightly rounded.
  const tileGap = 12;
  const tileW = (width - 72 - tileGap * 2) / 3;
  const tileY = 186;
  const tileH = 94;
  stats.forEach(([label, value], index) => {
    const x = 36 + index * (tileW + tileGap);
    ctx.fillStyle = palette.shadow;
    ctx.beginPath();
    ctx.roundRect(x + 4, tileY + 4, tileW, tileH, 6);
    ctx.fill();
    ctx.fillStyle = palette.panel;
    ctx.beginPath();
    ctx.roundRect(x, tileY, tileW, tileH, 6);
    ctx.fill();
    ctx.strokeStyle = palette.border;
    ctx.lineWidth = 3;
    ctx.stroke();
    const inner = tileW - 32;
    ctx.fillStyle = palette.muted;
    fitCanvasText(ctx, label, 600, mono, 11, 8, inner);
    ctx.fillText(label, x + 16, tileY + 16);
    ctx.fillStyle = ink;
    const valueSize = fitCanvasText(ctx, value, 800, sans, 34, 18, inner);
    // Smaller values sit on the same baseline as full-size ones.
    ctx.fillText(value, x + 16, tileY + 40 + (34 - valueSize));
  });

  // The write starts now, with the PNG still being encoded: WebKit (the Mac webview) allows
  // a clipboard write only during the click itself, not after an await.
  const png = new Promise<Blob>((resolve, reject) =>
    canvas.toBlob((blob) => (blob ? resolve(blob) : reject(new Error("Could not draw the card"))), "image/png"),
  );
  try {
    await navigator.clipboard.write([new ClipboardItem({ "image/png": png })]);
    showFeedback("Card copied");
  } catch (error) {
    showFeedback(`Could not copy: ${errorText(error)}`);
  }
}

function dayKey(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

function parseDayKey(key: string): Date {
  const [year, month, day] = key.split("-").map(Number);
  return new Date(year, month - 1, day);
}

function mondayOf(date: Date): Date {
  const monday = new Date(date);
  monday.setHours(0, 0, 0, 0);
  monday.setDate(monday.getDate() - ((monday.getDay() + 6) % 7));
  return monday;
}

function thisMondayKey(): string {
  return dayKey(mondayOf(new Date()));
}

function mondayKey(day: string): string {
  return dayKey(mondayOf(parseDayKey(day)));
}

function addDaysKey(key: string, days: number): string {
  const date = parseDayKey(key);
  date.setDate(date.getDate() + days);
  return dayKey(date);
}

function formatWeekRange(startKey: string): string {
  const start = parseDayKey(startKey);
  const end = parseDayKey(addDaysKey(startKey, 6));
  const startMonth = start.toLocaleDateString(undefined, { month: "short" });
  const endMonth = end.toLocaleDateString(undefined, { month: "short" });
  if (startMonth === endMonth) {
    return `${startMonth} ${start.getDate()}–${end.getDate()}`;
  }
  return `${startMonth} ${start.getDate()}–${endMonth} ${end.getDate()}`;
}

function formatWeekStart(startKey: string): string {
  return parseDayKey(startKey).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function groupSessionsByDay(items: SessionSummary[]): { label: string; items: SessionSummary[] }[] {
  const groups: { key: string; label: string; items: SessionSummary[] }[] = [];
  for (const item of items) {
    const date = new Date(item.started_at);
    const key = dayKey(date);
    const last = groups[groups.length - 1];
    if (last && last.key === key) {
      last.items.push(item);
      continue;
    }
    const thisYear = date.getFullYear() === new Date().getFullYear();
    groups.push({
      key,
      label: date.toLocaleDateString(undefined, {
        weekday: "long",
        month: "short",
        day: "numeric",
        ...(thisYear ? {} : { year: "numeric" }),
      }),
      items: [item],
    });
  }
  return groups;
}

function dayTotalLine(items: SessionSummary[]): string {
  const sessions = items.length === 1 ? "1 session" : `${items.length} sessions`;
  const focused = items.reduce((sum, item) => sum + item.focus_seconds, 0);
  return `${formatHours(focused)} focused · ${sessions}`;
}

/** A bar's height as a share of its track. The biggest value lands near the top of the track
 * whatever the week held, so a quiet week still draws a full chart; the others keep their order
 * and, eased by a mild power, stay readable next to it. The figure above the chart carries the
 * magnitude; the bars carry the shape. A zero is the caller's business; anything else is at
 * least `floor`. */
function smartBarHeight(value: number, max: number, floor: number): number {
  if (value <= 0) {
    return 0;
  }
  const share = Math.min(1, value / Math.max(max, 1));
  return Math.round(floor + (92 - floor) * Math.pow(share, 0.7));
}

function formatHours(totalSeconds: number): string {
  if (totalSeconds > 0 && totalSeconds < 60) {
    return "<1m";
  }
  const minutes = Math.round(Math.max(0, totalSeconds) / 60);
  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;
  if (hours === 0) {
    return `${rest}m`;
  }
  return `${hours}h ${String(rest).padStart(2, "0")}m`;
}

function formatDuration(totalSeconds: number): string {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(safe / 60);
  const seconds = safe % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}

function formatClockWithUnit(totalSeconds: number): string {
  const safe = Math.max(0, Math.floor(totalSeconds));
  if (safe >= 3600) {
    const hours = Math.floor(safe / 3600);
    const minutes = Math.floor((safe % 3600) / 60);
    return `${hours}:${String(minutes).padStart(2, "0")} hr`;
  }
  return `${formatDuration(safe)} min`;
}

function label(value: string): string {
  return value.replaceAll("_", " ").replace(/^\w/, (letter) => letter.toUpperCase());
}

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function attr(value: string): string {
  return escapeHtml(value).replaceAll('"', "&quot;");
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (char) => {
    switch (char) {
      case "&":
        return "&amp;";
      case "<":
        return "&lt;";
      case ">":
        return "&gt;";
      case '"':
        return "&quot;";
      default:
        return "&#039;";
    }
  });
}

void init();
