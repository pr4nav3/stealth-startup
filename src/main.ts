import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
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
  /** The distraction card is up: past the strictness's nudge time. Its first nudge plays with it. */
  interrupt?: boolean;
  should_nudge: boolean;
  next_nudge_in_seconds: number | null;
}

interface PomodoroStatus {
  round: number;
  rounds: number;
  work_seconds: number;
  break_seconds: number;
  /** "break_over": the break ran out and the session waits for the next round. */
  phase: "work" | "break" | "break_over";
  phase_remaining_seconds: number;
  rounds_focused_seconds: number[];
  moments: number;
}

/** Work / break rhythms on the start screen. */
const POMODORO_RHYTHMS = [
  { work: 25, rest: 5 },
  { work: 50, rest: 10 },
  { work: 90, rest: 20 },
] as const;

/** What the main window, which plays every nudge, tells the distraction card. */
interface NudgeVoice {
  speaking: boolean;
  voice: string;
  line: string;
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
  /** Distractions allowed for this session. */
  exceptions: AppRule[];
  camera: CameraStatus;
  /** Present for Pomodoro sessions. */
  pomodoro?: PomodoroStatus | null;
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
  /** The session was paused, or on a break, just before this span. */
  after_pause?: boolean;
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

/** One week (from Monday) or month (from the 1st); `start` is its first day. */
interface TrendPeriod {
  start: string;
  session_count: number;
  focus_seconds: number;
  median_stretch_seconds: number;
  recovery_median_seconds: number | null;
}

type TrendUnit = "week" | "month";

interface FocusRecap {
  start: string;
  end: string;
  focus_seconds: number;
  session_count: number;
}

/** A calendar month for the share card; `days[i].label` is the day ("2026-10-01"). */
interface MonthRecap {
  start: string;
  end: string;
  focus_seconds: number;
  session_count: number;
  longest_stretch_seconds: number;
  days: RecapBucket[];
}

interface HistoryWeek {
  week_start: string;
  sessions: SessionSummary[];
  oldest_day: string | null;
}

interface TrendsData {
  daily_focus: DailyFocus[];
  /** The last eight weeks, oldest first, ending with the current week. */
  weeks: TrendPeriod[];
  week_recap: FocusRecap;
}

type HistoryView = "list" | "report" | "trends";

/** Trends need this many completed weeks before week-over-week lines mean anything. */
const TRENDS_UNLOCK_WEEKS = 2;
/** Bars per screen of Trends: about three months of weeks, or a year of months. */
const TREND_WINDOW = 12;

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
  /** Delete was pressed once; the actions ask before the rule goes. */
  confirmDelete?: boolean;
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

/** When a line plays. `nudge` is Back to work: drifting, and a break ending. */
type PhraseMoment = "nudge" | "phone" | "roundover" | "sessionover";

/** One line. `tone` is the personality it belongs to. */
interface PhraseDefinition {
  id: string;
  tone: VoiceTone;
  moment: PhraseMoment;
  text: string;
}

interface PhraseEditor {
  phraseId: string | null;
  tone: VoiceTone;
  moment: PhraseMoment;
  text: string;
  /** Delete was pressed once; the action row asks before the line goes. */
  confirmDelete?: boolean;
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
  bindVoiceWaveforms();
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

/** Every personality has lines for each moment. The two that repeat all session want a few. */
const MOMENTS: { id: PhraseMoment; label: string; when: string; often: boolean }[] = [
  { id: "nudge", label: "Back to work", when: "When you get distracted", often: true },
  { id: "phone", label: "Phone down", when: "When the camera sees your phone", often: true },
  { id: "roundover", label: "Take a break", when: "When a Pomodoro round ends", often: false },
  { id: "sessionover", label: "Session done", when: "When a session ends", often: false },
];
const toneOrder: VoiceTone[] = ["gentle", "mom", "genz", "stern"];

const phraseBank: PhraseDefinition[] = [
  { id: "gentle_back_to_it", tone: "gentle", moment: "nudge", text: "Hey. Back to it." },
  { id: "gentle_that_can_wait", tone: "gentle", moment: "nudge", text: "That can wait. This can't." },
  { id: "gentle_you_drifted", tone: "gentle", moment: "nudge", text: "You drifted. It happens. Come back." },
  { id: "gentle_one_more_push", tone: "gentle", moment: "nudge", text: "One more push on the real thing." },
  { id: "gentle_pick_up_where", tone: "gentle", moment: "nudge", text: "Close that, and pick up where you were." },
  { id: "phone_down_back_to_it", tone: "gentle", moment: "phone", text: "Phone down. Back to it." },
  { id: "phone_it_can_wait", tone: "gentle", moment: "phone", text: "Put the phone away. It can wait." },
  { id: "phone_i_can_see_it", tone: "gentle", moment: "phone", text: "I can see your phone." },
  { id: "phone_eyes_up", tone: "gentle", moment: "phone", text: "Wrong screen. Eyes up." },
  { id: "roundover_good_round", tone: "gentle", moment: "roundover", text: "Good round. Stand up, drink some water." },
  { id: "roundover_take_five", tone: "gentle", moment: "roundover", text: "That's one done. Take a break." },
  { id: "roundover_eyes_off", tone: "gentle", moment: "roundover", text: "Nice work. Eyes off the screen for a bit." },
  { id: "sessionover_thats_a_wrap", tone: "gentle", moment: "sessionover", text: "That's a wrap. Nice work." },
  { id: "sessionover_session_done", tone: "gentle", moment: "sessionover", text: "Session done. Go take a breather." },
  { id: "sessionover_see_you_next", tone: "gentle", moment: "sessionover", text: "That's it for this one. See you next time." },
  { id: "mom_i_can_see", tone: "mom", moment: "nudge", text: "Beta, I can see the screen." },
  { id: "mom_what_is_this", tone: "mom", moment: "nudge", text: "Arre, what is this now? Finish your work first." },
  { id: "mom_again", tone: "mom", moment: "nudge", text: "Again? Close it. Then we'll talk." },
  { id: "mom_did_you_finish", tone: "mom", moment: "nudge", text: "Did you finish? No? Then why is this open." },
  { id: "mom_two_minutes", tone: "mom", moment: "nudge", text: "Two minutes becomes two hours. I know you." },
  { id: "mom_phone_down", tone: "mom", moment: "phone", text: "Beta, phone down. Now." },
  { id: "mom_phone_texting", tone: "mom", moment: "phone", text: "Who are you texting in the middle of work?" },
  { id: "mom_phone_away", tone: "mom", moment: "phone", text: "I can see that phone. Put it away." },
  { id: "mom_roundover", tone: "mom", moment: "roundover", text: "Good job, beta. Go drink some water." },
  { id: "mom_sessionover", tone: "mom", moment: "sessionover", text: "All done? Good. I'm proud of you." },
  { id: "genz_bro_the_tab", tone: "genz", moment: "nudge", text: "Bro. The tab." },
  { id: "genz_not_the_assignment", tone: "genz", moment: "nudge", text: "That's not the assignment." },
  { id: "genz_lock_in", tone: "genz", moment: "nudge", text: "Lock in. Like, now." },
  { id: "genz_why_behind", tone: "genz", moment: "nudge", text: "This is why you're behind." },
  { id: "genz_not_today", tone: "genz", moment: "nudge", text: "We're not doing this today. Back." },
  { id: "genz_phone_down", tone: "genz", moment: "phone", text: "Phone down, bestie." },
  { id: "genz_phone_wait", tone: "genz", moment: "phone", text: "The phone can wait. Lock in." },
  { id: "genz_phone_scrolling", tone: "genz", moment: "phone", text: "Doomscrolling? Right now? Bro." },
  { id: "genz_roundover", tone: "genz", moment: "roundover", text: "Round done. Go touch grass for five." },
  { id: "genz_sessionover", tone: "genz", moment: "sessionover", text: "Session done. You ate that." },
  { id: "stern_not_the_task", tone: "stern", moment: "nudge", text: "This isn't the task." },
  { id: "stern_close_it_now", tone: "stern", moment: "nudge", text: "Close it. Now." },
  { id: "stern_you_picked_this", tone: "stern", moment: "nudge", text: "You picked this. Finish it." },
  { id: "stern_no_negotiating", tone: "stern", moment: "nudge", text: "Back to work. No negotiating." },
  { id: "stern_want_it_back", tone: "stern", moment: "nudge", text: "Every minute here is one you'll want back." },
  { id: "stern_phone_down", tone: "stern", moment: "phone", text: "Phone. Down." },
  { id: "stern_phone_away", tone: "stern", moment: "phone", text: "Put it away. Now." },
  { id: "stern_phone_task", tone: "stern", moment: "phone", text: "That phone isn't the task." },
  { id: "stern_roundover", tone: "stern", moment: "roundover", text: "Round done. Five minutes. Then back." },
  { id: "stern_sessionover", tone: "stern", moment: "sessionover", text: "Done. Same time tomorrow." },
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
    starterVoiceCard("starter-male", "derek", "Derek", "Starter voice. Its lines come ready."),
    starterVoiceCard("starter-female", "elise", "Elise", "Warmer starter voice. Its lines come ready."),
  ];
}

function starterVoiceCard(id: string, profileId: string, name: string, detail: string): VoiceOption {
  const profile = profileById(profileId);
  const phrases = effectivePhrases();
  const ready = phrases.filter((phrase) => clipForPhraseId(profile?.clips ?? [], phrase.id)).length;
  return {
    id,
    name: profile?.display_name || name,
    kind: "Bundled",
    detail,
    coverage: `${ready}/${phrases.length} lines ready`,
    source: "starter",
    selectable: true,
    profile,
  };
}

const draft = {
  goal: "",
  mode: "timer" as "timer" | "pomodoro",
  rhythm: 0,
  rounds: 4,
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
/** False until the first get_trends answers: the calendar waits for it before popping in. */
let trendsLoaded = false;
/** Trends: every week and month from the first session through now (null until opened), which
 *  of the two the bars show, and how many windows of TREND_WINDOW back from now. */
let trendPeriods: Record<TrendUnit, TrendPeriod[]> | null = null;
let trendUnit: TrendUnit = "week";
let trendPage = 0;
let trends: TrendsData = {
  daily_focus: [],
  weeks: [],
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
/** The week picker under the toolbar: open, the month it shows (its first day), and that
 * month's days with sessions. */
let weekPickerOpen = false;
let pickerDismissBound = false;
let pickerMonth = "";
let pickerDays: Record<string, DailyFocus> = {};
let pickerRequest = 0;
let historyConfirm: HistoryConfirm | null = null;
/** Quit was picked from the tray while a session runs. */
let quitConfirmOpen = false;
/** The Share popup over History, and the month it shows (loaded when it opens). */
let shareOpen = false;
let shareMonth: MonthRecap | null = null;
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
/** History's data is loading after a tab switch; the poll leaves the page to that one draw. */
let historyArriving = false;
let overlayMoreOpen = false;
let distractionDismissedFor = "";
let distractionLatchKey = "";
let distractionLatchLabel = "";
let overlayPlacement = "corner";
let lastOverlayFit = "";
const OVERLAY_CORNER_WIDTH = 228;
const OVERLAY_INTERRUPT_WIDTH = 420;
let ruleEditor: RuleEditor | null = null;
let rulePreviewTimer: number | null = null;
let siteNames: Record<string, string> = {};
let appNames: Record<string, string> = {};
let editingRecapNames = false;
let historyRename: { kind: "app" | "website"; key: string; value: string } | null = null;
/** "Any distractions here?" after a session: what it counted as work, each one tapped or renamed. */
interface ReviewItem extends RecapApp {
  distraction: boolean;
  renaming: boolean;
  name: string;
}
let sessionReview: { id: number; items: ReviewItem[]; saving: boolean } | null = null;
let customPhrases: PhraseDefinition[] = [];
let customPhraseSets: string[] = [];
let phraseOverrides: Record<string, PhraseDefinition> = {};
let phraseEditor: PhraseEditor | null = null;
let hiddenPhrases = new Set<string>();
let clipsVoiceId = "starter-female";
let clipsVoiceTouched = false;
/** The personality forms under the cards: a rename, a delete awaiting its confirm, a new name. */
let toneRename: { from: VoiceTone; value: string } | null = null;
let toneDeleteArmed: VoiceTone | null = null;
let newToneDraft: string | null = null;
/** The ⋯ menu on your own personality's card: Rename and Delete. */
let toneMenuOpen = false;
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
/** The voice and line of the nudge playing or last played (main window). */
let lastNudgeVoice: Omit<NudgeVoice, "speaking"> | null = null;
/** The closing line that just played and the session it closed, shown over the report until
 *  clicked away. */
let sessionOver: { voice: NudgeVoice; session: SessionSummary | null } | null = null;
/** The card's copy of it (overlay window), stamped when it arrived. */
let overlayVoice: (NudgeVoice & { at: number }) | null = null;
/** When the current distraction card first showed. */
let distractionLatchedAt = 0;
/** The Pomodoro moment card on screen ("<session>:<moment>"), when it showed, and the last one
 *  the user closed. A new moment (the break running out) brings the next card. */
let momentKey = "";
let momentShownAt = 0;
let momentDismissed = "";
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
    // An edited line whose bundled original is gone (the old break-over set) stays the user's.
    const bundled = new Set(phraseBank.map((phrase) => phrase.id));
    customPhrases = [
      ...loadedPhraseLibrary.custom_phrases,
      ...loadedPhraseLibrary.phrase_overrides.filter((phrase) => !bundled.has(phrase.id)),
    ];
    phraseOverrides = Object.fromEntries(
      loadedPhraseLibrary.phrase_overrides.filter((phrase) => bundled.has(phrase.id)).map((phrase) => [phrase.id, phrase]),
    );
    // Every past session is known, except the newest one no report was ever offered for: a
    // session ended by quitting, or finished while the window was away, is offered on launch.
    const offered = Number(readLocal("review.lastOffered") ?? "0");
    history.forEach((item, index) => {
      if (index > 0 || item.id <= offered) {
        knownHistoryIds.add(item.id);
      }
    });
    void refreshCaptureAccess();
    // Coming back from System Settings is when a permission changes.
    window.addEventListener("focus", () => void refreshCaptureAccess());
    draft.strictness = loadedSettings.default_strictness;
    draft.voiceId = normalizeStoredVoiceId(loadedSettings.voice_profile);
    // A practice the app closed on: never offered as a report, ended and removed now.
    const stalePractice = Number(readLocal(PRACTICE_KEY) ?? "0");
    if (stalePractice) {
      practiceIds.add(stalePractice);
      knownHistoryIds.add(stalePractice);
      void (async () => {
        try {
          const now = await invoke<SessionSnapshot>("get_session_snapshot");
          if (now.current?.id === stalePractice) {
            practiceQuietUntil = Date.now() + 5000;
            await invoke("stop_session");
          }
        } catch {
          // Removed below either way.
        }
        await discardPractice(stalePractice);
      })();
    }
    // The first launch opens the first run; after that it waits on Home's Getting started line.
    if (appView === "main" && history.length === 0 && readLocal(FIRST_RUN_KEY) !== "1") {
      firstRunOpen = true;
    }
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
      paintFirstRunLive(snapshot?.current ?? null);
    });
    // These read the fresh snapshot, so they skip the poll's in-flight guard.
    await listen<string>("session-moment", async (event) => {
      await refreshSnapshotNow();
      void playMoment(event.payload);
    });
    // Sent once when a session ends, whether it ran out or was ended early.
    await listen("session-end", async () => {
      await refreshSnapshotNow();
      void playSessionOver();
    });
    // Quit from the tray with a session running: ask first.
    await listen("quit-requested", () => {
      quitConfirmOpen = true;
      paintModalLayer();
    });
    // The card's orb: stop the line, or hear it again.
    await listen<string>("nudge-control", (event) => {
      if (!nudgeAudio) {
        return;
      }
      if (event.payload === "stop") {
        nudgeAudio.pause();
      } else {
        nudgeAudio.currentTime = 0;
        void nudgeAudio.play().then(() => announceNudge(true)).catch(() => announceNudge(false));
      }
    });
  }
  if (appView === "overlay") {
    await listen<NudgeVoice>("nudge-voice", (event) => {
      overlayVoice = { ...event.payload, at: Date.now() };
      const moment = momentCardActive(snapshot?.current ?? null);
      if (!moment && !distractionIsActive()) {
        return;
      }
      // Speaking or done: swap the voice row alone, so the card's own entrance (a new tomato
      // dropping in) doesn't replay. The row's first appearance needs the card rebuilt.
      const row = document.querySelector(".interrupt-voice");
      const html = moment ? renderInterruptVoice(momentShownAt) : renderInterruptVoice();
      if (!row || !html.trim()) {
        render();
        return;
      }
      const holder = document.createElement("div");
      holder.innerHTML = html;
      const next = holder.firstElementChild;
      if (next) {
        row.replaceWith(next);
        bindEvents(next);
      }
    });
  }
  await refreshSnapshot();
  historyHydrated = true;

  if (appView === "main") {
    // One rebuild once the focus days and the voice list are both in, so the calendar pops in
    // with its data, once.
    void Promise.all([refreshTrends(false), refreshVoiceStatus(false)]).then(() => render());
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

/** Takes a snapshot in, from the poll or from a command's reply. A session that just ended
 *  opens its report and review once, from whichever arrives first; while a dialog or editor is
 *  open it stays pending and opens when the next snapshot finds the way clear. True when the
 *  report is opening (the caller draws nothing). */
function applySnapshot(next: SessionSnapshot): boolean {
  snapshot = next;
  settings = next.settings;
  history = next.history;
  if (!next.current && sessionRulesOpen) {
    sessionRulesOpen = false;
    ruleEditor = null;
  }
  // The session ended on its own while Quit was asking: there is nothing left to end.
  if (!next.current) {
    quitConfirmOpen = false;
  }
  // The practice ended without the first run ending it (End session, or it ran out).
  if (practice && next.current?.id !== practice.id) {
    const run = practice;
    practice = null;
    practiceQuietUntil = Date.now() + 5000;
    if (run.caught) {
      firstRunStep = "done";
    }
    void discardPractice(run.id);
  }
  const newest = noteNewHistory(history);
  if (!newest || !historyHydrated) {
    return false;
  }
  if (practiceIds.has(newest)) {
    knownHistoryIds.add(newest);
    return false;
  }
  if (appView !== "main" || next.current) {
    // The status window never shows reports; a new session already running has moved on.
    knownHistoryIds.add(newest);
    return false;
  }
  // Not while a dialog or editor is open: a recording or clip job would vanish behind History.
  if (!editorIsOpen()) {
    void openHistoryReport(newest, true);
    return true;
  }
  return false;
}

async function refreshSnapshotNow(): Promise<void> {
  try {
    const next = await invoke<SessionSnapshot>("get_session_snapshot");
    if (applySnapshot(next)) {
      return;
    }
    if (appView === "main") {
      ensureSelectedVoice();
      if (activeTab === "history" && history[0] && !knownHistoryIds.has(history[0].id)) {
        knownHistoryIds.add(history[0].id);
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
    if (editorIsOpen() || typingInField() || historyArriving || viewKey() === paintedViewKey) {
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
  if (sessionOver) {
    sessionOver = null;
    paintModalLayer();
    return;
  }
  if (shareOpen) {
    shareOpen = false;
    paintModalLayer();
    return;
  }
  if (voiceHelpOpen || modelInfoOpen || memoryInfoOpen) {
    voiceHelpOpen = false;
    modelInfoOpen = false;
    memoryInfoOpen = false;
    paintVoiceLayer();
    return;
  }
  if (newToneDraft !== null) {
    newToneDraft = null;
    paintVoiceLayer();
    return;
  }
  if (toneMenuOpen || toneRename || toneDeleteArmed) {
    closeToneForms();
    paintClipsPanel();
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
    paintRuleEditorHost();
    return;
  }
  if (phraseEditor) {
    phraseEditor = null;
    paintVoiceLayer();
    return;
  }
  if (quitConfirmOpen) {
    quitConfirmOpen = false;
    paintModalLayer();
    return;
  }
  if (historyConfirm) {
    historyConfirm = null;
    paintModalLayer();
    return;
  }
  if (voiceDeleteTargetId) {
    voiceDeleteTargetId = null;
    paintVoiceLayer();
    return;
  }
  if (voiceInfoId) {
    dismissVoiceInfo();
    paintVoiceLayer();
    return;
  }
  if (modelDeleteOpen) {
    modelDeleteOpen = false;
    paintVoiceLayer();
    return;
  }
  if (recordingOpen) {
    void closeRecordingDialog();
    return;
  }
  if (sessionRulesOpen) {
    sessionRulesOpen = false;
    ruleEditor = null;
    paintSessionRulesLayer();
    return;
  }
  // Escape skips the first run, but never walks out on a practice that is running.
  if (firstRunOpen) {
    if (!practice) {
      closeFirstRun();
    }
    return;
  }
  if (preflightOpen) {
    preflightOpen = false;
    ruleEditor = null;
    animateOnArrival("session");
    render();
    return;
  }
  if (sessionReview) {
    // Like Ask me later: nothing saved.
    sessionReview = null;
    paintReviewLayer();
    return;
  }
  if (historyView === "report" || historyView === "trends") {
    historyView = "list";
    sessionReport = null;
    historyConfirm = null;
    animateOnArrival("history");
    void refreshHistoryWeek();
    return;
  }
  if (weekPickerOpen) {
    closeWeekPicker();
    paintWeekPicker();
    return;
  }
  if (activeTab === "history" && historySearch) {
    historySearch = "";
    const input = document.querySelector<HTMLInputElement>("#history-search");
    if (input) {
      input.value = "";
    }
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
    firstRunOpen ||
    preflightOpen ||
    sessionRulesOpen ||
    ruleEditor !== null ||
    historyRename !== null ||
    phraseEditor !== null ||
    newToneDraft !== null ||
    toneRename !== null ||
    recordingOpen ||
    historyConfirm !== null ||
    quitConfirmOpen ||
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
    focus?.interrupt ? 1 : 0,
    focus?.matched_rule?.value ?? "",
    sample?.app_name ?? "",
    focus?.activity.key ?? "",
    current.metrics.nudges,
    current.camera.phone ? 1 : 0,
    current.camera.phone_distracted ? 1 : 0,
    current.pomodoro ? `${current.pomodoro.round}:${current.pomodoro.phase}` : "",
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
    // Home's Getting started line and tips follow history, permissions and settings.
    `${onboardingVisible()}${homeTip()?.id ?? ""}${firstRunOpen ? currentFirstRunStep() : ""}${practice ? `${practice.id}${practice.caught}` : ""}`,
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
  paintFirstRunLive(current);
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
  const { label, percent } = liveTimer(current);
  document.querySelectorAll<HTMLElement>(".active-timer strong, .interrupt-time").forEach((node) => {
    node.textContent = label;
  });
  const overlayTime = document.querySelector<HTMLElement>(".overlay-progress > span");
  if (overlayTime) {
    overlayTime.textContent = `${label} left`;
  }
  paintSoFar(current);
  document
    .querySelectorAll<HTMLElement>(".overlay-progress .progress > span")
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
          ${current ? `<button class="status-chip" type="button" data-tab="session" aria-label="Session status">${escapeHtml(statusChip(current))}</button>` : renderStreakChip()}
          <button class="theme-toggle" type="button" data-action="toggle-theme">${theme === "dark" ? "Dark" : "Light"}</button>
        </div>
      </header>

      <nav class="tabs" aria-label="Supervisor sections">
        ${tabButton("session", "Session")}
        ${tabButton("settings", "Settings")}
        ${tabButton("history", "History")}
      </nav>

      ${lastError ? `<section class="notice">${escapeHtml(lastError)}</section>` : ""}
      ${voiceError && activeTab === "settings" ? `<section class="notice">${escapeHtml(voiceError)}</section>` : ""}
      ${permissionNotice(current)}

      ${activeTab === "session" ? renderSession(current) : ""}
      ${activeTab === "settings" ? renderSettings() : ""}
      ${activeTab === "history" ? renderHistory() : ""}
      <div id="first-run-layer" class="slot">${renderFirstRun()}</div>
      <div id="voice-layer">${renderVoiceDialogs()}</div>
      <div id="modal-layer" class="slot">${renderModalLayer()}</div>
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
    const streak = { days: streakDays() };
    return `
      <section class="home-ready">
        <div class="home-grid">
          <article class="panel cal-card">
            <p class="eyebrow">Calendar · ${escapeHtml(monthLabel(new Date()))}</p>
            <div class="streak-line">
              <strong>${streak.days}</strong>
              <span>day streak</span>
            </div>
            <p class="streak-rule">25 focused minutes a day keeps it going.</p>
            ${renderCalendar()}
          </article>
          <div class="home-side">
            ${onboardingVisible() ? renderOnboardingLine() : renderHomeTip()}
            <article class="panel quote-card">
              <p class="eyebrow">Daily note · ${escapeHtml(note.date)}</p>
              <blockquote>“${escapeHtml(note.text)}”</blockquote>
              <p class="quote-by">— ${escapeHtml(note.by)}</p>
            </article>
            <div id="day-card-slot" class="slot">${renderDayCard(streak.days)}</div>
          </div>
        </div>
        ${renderStartAction()}
      </section>
      <div id="preflight-layer">${preflightOpen ? renderPreflightModal() : ""}</div>
    `;
  }

  const timer = liveTimer(current);
  const onBreak = isOnBreak(current);
  return `
    <section class="active-session-page">
      <div class="active-session-grid">
        <section class="state-slab active-state-card ${stateClass(current)}" aria-live="polite">
          ${
            current.pomodoro
              ? `<div class="eyebrow-row"><p class="eyebrow">${escapeHtml(roundLabel(current.pomodoro))}</p>${renderRoundDots(current.pomodoro)}</div>`
              : `<p class="eyebrow">Current session</p>`
          }
          <h2>${escapeHtml(stateTitle(current))}</h2>
          <div class="active-goal">
            <span>Session goal</span>
            <strong>${escapeHtml(current.goal)}</strong>
          </div>
          <div class="active-timer">
            <strong>${timer.label}</strong>
            <span>${escapeHtml(timer.caption)}</span>
          </div>
          <div class="live-strip" aria-label="Session so far: what happened, then the time left">${liveStripRuns(current)}</div>
        </section>

        <div class="session-side">
        <section class="panel session-sofar">
          <p class="eyebrow">So far</p>
          <div class="sofar-tiles">${renderSoFarTiles(current)}</div>
        </section>
        <aside class="panel session-rules-card">
          <p class="eyebrow">Distractions</p>
          ${onBreak ? `<p class="session-break-note">Don't count during a break.</p>` : renderSessionDistractions(current.exceptions)}
          <button type="button" data-action="open-session-rules">Edit distractions</button>
        </aside>
        </div>
      </div>

      ${renderActiveSessionControls(current)}
    </section>
    <div id="session-rules-layer" class="slot">${sessionRulesOpen ? renderSessionRulesModal() : ""}</div>
  `;
}

/** A Pomodoro session between rounds: on its break, or waiting for the next round. */
function isOnBreak(current: CurrentSession | null): boolean {
  return Boolean(current?.pomodoro && current.pomodoro.phase !== "work");
}

/** "Round 2", or after a round "After round 2". */
function roundLabel(pomodoro: PomodoroStatus): string {
  return pomodoro.phase === "work" ? `Round ${pomodoro.round}` : `After round ${pomodoro.round}`;
}

/** Pause's place: Resume, or on a break "Start round 3". */
function resumeLabel(current: CurrentSession): string {
  if (current.status !== "paused") {
    return "Pause";
  }
  return isOnBreak(current) && current.pomodoro ? `Start round ${current.pomodoro.round + 1}` : "Resume";
}

/** The big countdown and its bar: the session's time, or in a Pomodoro the current round or break. */
function liveTimer(current: CurrentSession): { label: string; caption: string; percent: number } {
  const pomodoro = current.pomodoro;
  if (!pomodoro) {
    const percent = current.duration_seconds
      ? Math.min(100, Math.round((current.elapsed_seconds / current.duration_seconds) * 100))
      : 0;
    return { label: formatDuration(current.remaining_seconds), caption: "Remaining", percent };
  }
  if (pomodoro.phase === "work") {
    const left = pomodoro.phase_remaining_seconds;
    const last = pomodoro.round >= pomodoro.rounds;
    return {
      label: formatDuration(left),
      caption: last ? "Until done" : "Until break",
      percent: Math.min(100, Math.round(((pomodoro.work_seconds - left) / pomodoro.work_seconds) * 100)),
    };
  }
  const left = pomodoro.phase_remaining_seconds;
  return {
    label: formatDuration(left),
    caption: pomodoro.phase === "break_over" ? `Until round ${pomodoro.round + 1} starts by itself` : `Until round ${pomodoro.round + 1}`,
    percent: Math.min(100, Math.round(((pomodoro.break_seconds - left) / pomodoro.break_seconds) * 100)),
  };
}

/** One dot per round. Finished ones take the colour opposite the slab (yellow on a round,
 *  green on a break), so they always stand out; the current round is ink. */
function renderRoundDots(pomodoro: PomodoroStatus): string {
  const finished = pomodoro.phase === "work" ? pomodoro.round - 1 : pomodoro.round;
  return `<span class="round-dots" aria-label="${finished} of ${pomodoro.rounds} rounds finished">${Array.from(
    { length: pomodoro.rounds },
    (_, index) => `<i class="${index < finished ? "done" : index === finished && pomodoro.phase === "work" ? "now" : ""}"></i>`,
  ).join("")}</span>`;
}

/** The running session's saved spans (checkpointed every 15 s), fetched every few seconds
 *  while the Session tab shows it. */
let liveReport: SessionReport | null = null;
let liveReportFetchedAt = 0;
let liveReportFetching = false;

/** The live bar and the So far numbers, repainted in place; a fresh report every 5 s. */
function paintSoFar(current: CurrentSession): void {
  const strip = document.querySelector<HTMLElement>(".live-strip");
  if (!strip) {
    return;
  }
  strip.innerHTML = liveStripRuns(current);
  const tiles = document.querySelector<HTMLElement>(".sofar-tiles");
  if (tiles) {
    tiles.innerHTML = renderSoFarTiles(current);
  }
  if (liveReportFetching || Date.now() - liveReportFetchedAt < 5000) {
    return;
  }
  liveReportFetching = true;
  liveReportFetchedAt = Date.now();
  void invoke<SessionReport>("get_session_report", { id: current.id })
    .then((report) => {
      liveReport = report;
    })
    .catch(() => {})
    .finally(() => {
      liveReportFetching = false;
    });
}

/** What each span was, for the live bar: work, a distraction, away; a pause draws nothing. */
function liveSpanClass(state: string): string {
  if (stripOnTask(state)) {
    return "strip-on";
  }
  if (stripOffTask(state)) {
    return "strip-off";
  }
  return state === "idle" || state === "unknown" ? "strip-away" : "";
}

/** The bar spans the whole session: filled up to now with what happened, empty for the time
 *  left. The newest saved span runs on to now, since the last checkpoint can be 15 s old. */
function liveStripRuns(current: CurrentSession): string {
  const total = Math.max(1, current.duration_seconds);
  const spans = liveReport?.id === current.id ? liveReport.spans : [];
  const runs: { cls: string; from: number; to: number }[] = [];
  for (const span of spans) {
    const cls = liveSpanClass(span.state);
    const from = span.started_offset_seconds;
    const to = from + span.seconds;
    const last = runs[runs.length - 1];
    if (last && last.cls === cls && from - last.to < 2) {
      last.to = to;
    } else if (cls) {
      runs.push({ cls, from, to });
    }
  }
  const last = runs[runs.length - 1];
  if (last && current.elapsed_seconds > last.to) {
    last.to = current.elapsed_seconds;
  }
  const pct = (seconds: number) => Math.min(100, (Math.max(0, seconds) / total) * 100);
  const now = pct(current.elapsed_seconds);
  return (
    runs
      .map((run) => `<i class="${run.cls}" style="left: ${pct(run.from).toFixed(2)}%; width: ${(pct(run.to) - pct(run.from)).toFixed(2)}%"></i>`)
      .join("") + `<b class="live-now" style="left: ${now.toFixed(2)}%"></b>`
  );
}

/** Focused so far, the longest unbroken stretch, and nudges: the report's numbers, live. */
function renderSoFarTiles(current: CurrentSession): string {
  const spans = liveReport?.id === current.id ? liveReport.spans : [];
  const longest = Math.max(0, ...focusedStretches(spans));
  const tile = (label: string, value: string, title: string) =>
    `<div class="sofar-tile" title="${attr(title)}"><span>${label}</span><strong>${escapeHtml(value)}</strong></div>`;
  return [
    tile("Focused", formatHours(current.metrics.focused_seconds), "Time on work: not on a distraction, your phone, or away"),
    tile("Longest", formatHours(longest), "Longest unbroken run of work"),
    tile("Nudges", String(current.metrics.nudges), "Times Supervisor spoke up"),
  ].join("");
}

/** The distractions, with the ones allowed for this session struck through. */
function renderSessionDistractions(exceptions: AppRule[]): string {
  const rules = permanentRules.slice().reverse();
  if (rules.length === 0) {
    return `<p class="session-rules-empty">Nothing on the list yet.</p>`;
  }
  const chips = rules
    .map((rule, index) => {
      const allowed = exceptions.some((exception) => exception.target === rule.target && exception.value === rule.value);
      return `<button type="button" class="rule-chip rule-chip-distracting ${allowed ? "is-allowed" : ""}" data-action="toggle-session-exception" data-rule-index="${index}" aria-pressed="${allowed}" title="${attr(allowed ? "Allowed once. Tap to count it as a distraction again." : "Tap to allow it once, for this session.")}">${escapeHtml(ruleName(rule))}</button>`;
    })
    .join("");
  const allowed = rules.filter((rule) => exceptions.some((exception) => exception.target === rule.target && exception.value === rule.value)).map(ruleName);
  return `
    <div class="chips session-rule-chips">${chips}</div>
    ${allowed.length ? `<p class="session-allowed">Allowed once: ${escapeHtml(allowed.join(", "))}</p>` : ""}
  `;
}

function sameRule(a: AppRule, b: AppRule): boolean {
  return a.target === b.target && a.value.toLowerCase() === b.value.toLowerCase();
}

function renderActiveSessionControls(current: CurrentSession): string {
  const held = current.status === "paused";
  return `
    <div class="active-session-controls">
      <button class="${held ? "primary" : "overlay-pause"}" type="button" data-action="${held ? "resume" : "pause"}">${escapeHtml(resumeLabel(current))}</button>
      <button class="danger" type="button" data-action="stop">End session</button>
    </div>
  `;
}

function renderSessionReview(review: { items: ReviewItem[]; saving: boolean }): string {
  const marked = review.items.filter((item) => item.distraction).length;
  const chips = review.items
    .map((item, index) => {
      const main = item.renaming
        ? `<input class="review-name" data-review-name="${index}" maxlength="60" value="${attr(item.name)}" aria-label="New name for ${attr(item.label)}" />`
        : `<button class="review-toggle" type="button" data-review-toggle="${index}" aria-pressed="${item.distraction}">
            <span>${escapeHtml(item.name)}${item.name !== item.label ? `<small class="review-was">was ${escapeHtml(item.label)}</small>` : ""}</span>
            <small>${escapeHtml(formatHours(item.seconds))}</small>
          </button>`;
      return `<div class="review-chip ${item.distraction ? "is-distraction" : ""}">${main}<button class="review-rename" type="button" data-review-rename="${index}">${item.renaming ? "Save" : "Rename"}</button></div>`;
    })
    .join("");
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel review-panel" role="dialog" aria-modal="true" aria-labelledby="review-title">
        <p class="eyebrow">Session done</p>
        <h2 id="review-title">Any distractions here?</h2>
        <p class="review-intro">These counted as work. Tap any that weren't.</p>
        <div class="review-chips">${chips}</div>
        <div class="review-foot">
          <button class="review-later" type="button" data-action="review-later">Skip</button>
          <button class="${marked ? "danger" : "primary"}" type="button" data-action="review-save" ${review.saving ? "disabled" : ""}>${marked ? `Add ${marked} to distractions` : "All work"}</button>
        </div>
      </section>
    </div>
  `;
}

/** Renames first, then the answers: the distractions go on the list and count in this session. */
async function saveSessionReview(): Promise<void> {
  const review = sessionReview;
  if (!review || review.saving) {
    return;
  }
  review.saving = true;
  try {
    for (const item of review.items.filter((item) => item.name.trim() && item.name !== item.label)) {
      if (item.kind === "website") {
        siteNames = await invoke<Record<string, string>>("set_site_name", { key: item.key, name: item.name });
      } else {
        appNames = await invoke<Record<string, string>>("set_app_name", { key: item.key, name: item.name });
      }
    }
    const saved = await invoke<RulesSaved>("review_session", {
      id: review.id,
      distractions: review.items
        .filter((item) => item.distraction)
        .map((item) => ({ target: item.kind === "website" ? "website" : "app", value: item.key })),
      work: review.items.filter((item) => !item.distraction).map((item) => item.key),
    });
    permanentRules = saved.rules;
    sessionReview = null;
    dayCardKey = "";
    await openHistoryReport(review.id);
  } catch (error) {
    review.saving = false;
    showFeedback(errorText(error));
    paintReviewLayer();
  }
}

function bindSessionReview(root: ParentNode): void {
  const review = sessionReview;
  if (!review) {
    return;
  }
  root.querySelectorAll<HTMLButtonElement>("[data-review-toggle]").forEach((button) => {
    button.addEventListener("click", () => {
      // Tapping away keeps a name being typed, as its tick would.
      root.querySelectorAll<HTMLInputElement>("[data-review-name]").forEach((input) => {
        const open = review.items[Number(input.dataset.reviewName)];
        open.name = input.value.trim() || open.label;
        open.renaming = false;
      });
      const item = review.items[Number(button.dataset.reviewToggle)];
      item.distraction = !item.distraction;
      paintReviewLayer();
    });
  });
  const finishRename = (item: ReviewItem, input: HTMLInputElement | null, keep: boolean) => {
    if (keep && input) {
      item.name = input.value.trim() || item.label;
    }
    item.renaming = false;
    paintReviewLayer();
  };
  root.querySelectorAll<HTMLButtonElement>("[data-review-rename]").forEach((button) => {
    button.addEventListener("click", () => {
      const index = Number(button.dataset.reviewRename);
      const item = review.items[index];
      if (item.renaming) {
        finishRename(item, root.querySelector<HTMLInputElement>(`[data-review-name="${index}"]`), true);
        return;
      }
      review.items.forEach((other) => (other.renaming = false));
      item.renaming = true;
      paintReviewLayer();
      root.querySelector<HTMLInputElement>(`[data-review-name="${index}"]`)?.select();
    });
  });
  root.querySelectorAll<HTMLInputElement>("[data-review-name]").forEach((input) => {
    const item = review.items[Number(input.dataset.reviewName)];
    input.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && !composing(event)) {
        event.preventDefault();
        finishRename(item, input, true);
      } else if (event.key === "Escape") {
        // Escape drops the rename, not the popup behind it.
        event.preventDefault();
        event.stopPropagation();
        finishRename(item, null, false);
      }
    });
  });
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
  if (current && momentCardActive(current)) {
    return renderMomentOverlay(current);
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
            <div class="overlay-actions ${isOnBreak(current) ? "is-break" : ""}">
              <button class="${held ? "primary" : "overlay-pause"}" type="button" data-action="${held ? "resume" : "pause"}">${escapeHtml(resumeLabel(current as CurrentSession))}</button>
              <button class="danger" type="button" data-action="stop">End session</button>
            </div>
            <details class="overlay-more" ${overlayMoreOpen ? "open" : ""}>
              <summary>More</summary>
              <div class="overlay-more-body">
                ${isOnBreak(current) ? "" : renderOverlayWatchControls(current as CurrentSession, held, nudgesMuted)}
                <button class="overlay-pin" type="button" data-action="${pinned ? "unpin-window" : "pin-window"}">${pinned ? "Unpin window" : "Pin window"}</button>
              </div>
            </details>
          `
          : ""
      }
    </main>
  `;
}

/** A round just ended or its break ran out, and the user hasn't closed that card. */
function momentCardActive(current: CurrentSession | null): boolean {
  const pomodoro = current?.pomodoro;
  if (!current || !pomodoro || pomodoro.phase === "work" || pomodoro.moments === 0) {
    return false;
  }
  const key = `${current.id}:${pomodoro.moments}`;
  if (key !== momentKey) {
    momentKey = key;
    momentShownAt = Date.now();
  }
  return momentDismissed !== key;
}

/** Round over (yellow): the round's title and time, a tomato per round, the voice, one button.
 *  Break over (green): just that, the voice, and the button that starts the next round. */
function renderMomentOverlay(current: CurrentSession): string {
  const pomodoro = current.pomodoro as PomodoroStatus;
  if (pomodoro.phase === "break_over") {
    return `
      <main class="overlay-shell overlay-interrupt">
        <section class="state-slab interrupt-alert moment-break-over">
          <h2 class="moment-title">Break over.</h2>
          <p class="moment-goal">Round ${pomodoro.round + 1} starts by itself in a few minutes, or now.</p>
          <div class="moment-gap"></div>
        </section>
        <div class="interrupt-actions moment-actions">
          <button class="primary" type="button" data-action="resume">Start round ${pomodoro.round + 1}</button>
        </div>
      </main>
    `;
  }
  const focused = pomodoro.rounds_focused_seconds[pomodoro.round - 1] ?? 0;
  return `
    <main class="overlay-shell overlay-interrupt">
      <section class="state-slab interrupt-alert moment-round-over">
        <h2 class="moment-title">Round ${pomodoro.round} done.</h2>
        <p class="moment-goal">${escapeHtml(current.goal)}</p>
        <div class="tomatoes" aria-label="${pomodoro.round} of ${pomodoro.rounds} rounds done">
          ${Array.from({ length: pomodoro.rounds }, (_, index) => `<span class="tomato ${index < pomodoro.round ? "is-done" : ""} ${index === pomodoro.round - 1 ? "is-new" : ""}"></span>`).join("")}
        </div>
        <p class="moment-focused"><b>${Math.round(focused / 60)} min</b> focused</p>
        ${renderInterruptVoice(momentShownAt)}
        <div class="moment-gap"></div>
      </section>
      <div class="interrupt-actions moment-actions">
        <button class="primary" type="button" data-action="dismiss-moment">Got it</button>
      </div>
    </main>
  `;
}

function renderDistractionOverlay(current: CurrentSession): string {
  const phone = distractionLatchKey === PHONE_LATCH;
  const name = distractionLatchLabel || distractionLabel(current);
  return `
    <main class="overlay-shell overlay-interrupt">
      <section class="state-slab interrupt-alert">
        <p class="interrupt-kicker">${phone ? "Phone in view" : "Distraction"}</p>
        <h2>${phone ? "Your phone is out." : `You opened ${escapeHtml(name)}.`}</h2>
        ${renderInterruptVoice()}
        <div class="interrupt-task">
          <strong>${escapeHtml(current.goal)}</strong>
          <span><span class="interrupt-time">${formatDuration(current.remaining_seconds)}</span> left</span>
        </div>
      </section>
      <div class="interrupt-actions">
        <button class="primary" type="button" data-action="dismiss-distraction">Back to my task</button>
        ${phone ? "" : `<button type="button" data-action="allow-session" title="Allowed until this session ends">Allow once</button>
        <button type="button" data-action="allow-always" title="Takes it off your distractions list">Always allow</button>`}
      </div>
    </main>
  `;
}

/** The voice that spoke for this card: an orb that is the play button, with bars while it
 *  speaks, and the line under the speaker's name. Nothing when no voice spoke for this card
 *  (no voice picked, or no line generated yet). A line can land a moment before the card. */
function renderInterruptVoice(since = distractionLatchedAt): string {
  const voice = overlayVoice;
  if (!voice || voice.at < since - 5000) {
    return "";
  }
  return renderVoiceRow(voice);
}

/** The orb (play, or bars while speaking) and the line under the speaker's name. */
function renderVoiceRow(voice: NudgeVoice): string {
  const speaking = voice.speaking;
  return `
    <div class="interrupt-voice ${speaking ? "is-speaking" : ""}">
      <button class="voice-orb" type="button" data-action="${speaking ? "nudge-stop" : "nudge-replay"}" aria-label="${speaking ? "Stop" : "Play the line again"}">
        <span class="voice-orb-core">
          <svg class="voice-orb-play" viewBox="0 0 12 12" aria-hidden="true"><path d="M2.5 1v10L11 6z" fill="currentColor"/></svg>
          <span class="voice-orb-bars" aria-hidden="true"><i></i><i></i><i></i><i></i></span>
        </span>
      </button>
      <div class="interrupt-voice-text">
        <p class="interrupt-voice-who">${escapeHtml(voice.voice)} ${speaking ? "is speaking" : "said"}</p>
        <p class="interrupt-voice-line">“${escapeHtml(voice.line)}”</p>
      </div>
    </div>
  `;
}

/** Mute, and "Add YouTube to distractions" for what is in front: only while the session is
 *  watching something. A break judges nothing, so these stay out of it. */
function renderOverlayWatchControls(current: CurrentSession, held: boolean, nudgesMuted: boolean): string {
  const focus = current.focus;
  const name = focus ? (focus.activity.kind === "website" ? siteNames[focus.activity.key] : appNames[focus.activity.key]) ?? focus.activity.label : "";
  const addable = Boolean(focus && focus.markable && !focus.distracting) && !held;
  const why = held
    ? "Paused: nothing is being watched."
    : !focus
      ? "Nothing in front yet."
      : focus.distracting
        ? `${name} is already a distraction.`
        : `${name} can't be a distraction on its own.`;
  return `
    <label class="overlay-mute">
      <span>Mute for 5 min</span>
      <span class="toggle-field">
        <input id="overlay-snooze" class="toggle-input" type="checkbox" ${nudgesMuted ? "checked" : ""} ${held ? "disabled" : ""} aria-label="Mute nudges for 5 minutes" />
        <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
      </span>
    </label>
    <div class="overlay-marks">
      ${
        addable
          ? `<button class="mark-distraction" type="button" data-action="mark-distracting">Add ${escapeHtml(name)} to distractions</button>`
          : `<p class="overlay-why">${escapeHtml(why)}</p>`
      }
    </div>
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
      if (distractionLatchKey !== key) {
        distractionLatchedAt = Date.now();
      }
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
  if (focus.interrupt) {
    if (distractionDismissedFor !== key) {
      if (distractionLatchKey !== key) {
        distractionLatchedAt = Date.now();
      }
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
  const interrupt = distractionIsActive() || momentCardActive(current);
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
  const { label, percent } = liveTimer(current);
  return `
    ${current.pomodoro ? renderRoundDots(current.pomodoro) : ""}
    <div class="overlay-progress">
      <span>${label} left</span>
      <div class="progress" aria-label="Session progress"><span style="width: ${percent}%"></span></div>
    </div>
  `;
}

/**
 * The first run: a short, hands-on start built from the app's own pieces (the voice cards, the
 * distraction list and the webcam switch exactly as Settings shows them), so what is learned
 * here is what the user finds later. It opens by itself on the first launch, and from Home's
 * Getting started line until the first real session. The practice step runs a real Strict
 * session, so the card and the voice are the real ones; it is deleted the moment it is done.
 */
type FirstRunStep = "access" | "voice" | "list" | "catch" | "done";
let firstRunOpen = false;
/** Null: the first step there is (Accessibility on a Mac, once the platform is known). */
let firstRunStep: FirstRunStep | null = null;
/** The practice session while it runs; `caught` once its first nudge came. */
let practice: { id: number; caught: boolean } | null = null;
/** Practice sessions on their way out of History: they never open a report. */
const practiceIds = new Set<number>();
/** The closing line of a practice session stays quiet; the session-end event can come late. */
let practiceQuietUntil = 0;
const FIRST_RUN_KEY = "firstRun.done";
const PRACTICE_KEY = "practice.id";

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

/** macOS reads browser addresses through Accessibility; without it every site is Other websites. */
function accessibilityMissing(): boolean {
  return captureAccess?.platform === "macos" && captureAccess.permissions.accessibility === "missing";
}

function firstRunSteps(): FirstRunStep[] {
  const steps: FirstRunStep[] = ["voice", "list", "catch", "done"];
  return captureAccess?.platform === "macos" ? ["access", ...steps] : steps;
}

function currentFirstRunStep(): FirstRunStep {
  const steps = firstRunSteps();
  return firstRunStep && steps.includes(firstRunStep) ? firstRunStep : steps[0];
}

/** Home's Getting started line: until the first real session, and while a Mac still needs Accessibility. */
function onboardingVisible(): boolean {
  return history.length === 0 || accessibilityMissing();
}

function renderOnboardingLine(): string {
  return `
    <article class="panel onboard-card">
      <div class="onboard-head">
        <p class="eyebrow">Getting started</p>
        <button class="small-button" type="button" data-action="onboard-show">Show</button>
      </div>
    </article>
  `;
}

function openFirstRun(): void {
  firstRunOpen = true;
  firstRunStep = null;
  ruleEditor = null;
  paintFirstRun();
}

/** Closed for good on this computer: it no longer opens by itself (Home's line still can). */
function closeFirstRun(): void {
  firstRunOpen = false;
  firstRunStep = null;
  ruleEditor = null;
  writeLocal(FIRST_RUN_KEY, "1");
  render();
}

function moveFirstRun(by: number): void {
  const steps = firstRunSteps();
  const next = steps[Math.min(steps.length - 1, Math.max(0, steps.indexOf(currentFirstRunStep()) + by))];
  firstRunStep = next;
  ruleEditor = null;
  if (next === "done") {
    writeLocal(FIRST_RUN_KEY, "1");
  }
  paintFirstRun();
}

function paintFirstRun(): void {
  paintSlot("first-run-layer", renderFirstRun());
}

/** The name the practice asks the user to open, as its chip shows it: the newest website on the
 *  list (a tab is quicker to open than an app), else the newest app. */
function practiceTarget(): string | null {
  const rule = permanentRules.find((item) => item.target === "website") ?? permanentRules[0];
  return rule ? ruleName(rule) : null;
}

function practiceVoiceName(): string | null {
  return draft.voiceId === "none" ? null : selectedVoice().name;
}

function renderFirstRun(): string {
  if (!firstRunOpen || appView !== "main") {
    return "";
  }
  const steps = firstRunSteps();
  const step = currentFirstRunStep();
  const index = steps.indexOf(step);
  const counted = steps.length - 1;
  const titles: Record<FirstRunStep, string> = {
    access: "Let Supervisor see the address bar",
    voice: "Who should nudge you?",
    list: "These count as distractions",
    catch: "Get caught once",
    done: "You're all set",
  };
  const back = index > 0 && !practice ? `<button type="button" data-action="first-run-back">Back</button>` : "<span></span>";
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel first-run-panel" role="dialog" aria-modal="true" aria-labelledby="first-run-title">
        <div class="modal-head first-run-head">
          <div>
            <p class="eyebrow">Getting started${step === "done" ? "" : ` · ${index + 1} of ${counted}`}</p>
            <h2 id="first-run-title">${titles[step]}</h2>
          </div>
          ${practice || step === "done" ? "" : `<button class="small-button" type="button" data-action="first-run-skip">Skip</button>`}
        </div>
        ${renderFirstRunBody(step, back)}
      </section>
    </div>
  `;
}

function renderFirstRunBody(step: FirstRunStep, back: string): string {
  if (step === "access") {
    const missing = accessibilityMissing();
    return `
      <p class="first-run-copy">That's how it tells YouTube from Google Docs. Only the address is read, never the page, and nothing leaves this Mac.</p>
      <section class="panel first-run-block">
        <div class="setting-row">
          <span>Accessibility</span>
          ${missing ? `<button type="button" data-action="onboard-accessibility">Open System Settings</button>` : `<span class="first-run-ok">Allowed</span>`}
        </div>
      </section>
      <p class="first-run-where">${missing ? "Turn Supervisor on in the list, then come back here. Without it, Supervisor can't tell one website from another." : "macOS remembers this. You won't be asked again."}</p>
      <div class="first-run-actions">${back}<button class="primary" type="button" data-action="first-run-next">Next</button></div>
    `;
  }
  if (step === "voice") {
    return `
      <p class="first-run-copy">Press play to hear each voice. The one you pick speaks up when you get distracted.</p>
      <div class="voice-list first-run-voices">${availableVoices().map((voice) => renderVoiceCard(voice, false)).join("")}</div>
      <p class="first-run-where">You can change it, or record anyone's voice, in Settings › Voice.</p>
      <div class="first-run-actions">${back}<button class="primary" type="button" data-action="first-run-next">Next</button></div>
    `;
  }
  if (step === "list") {
    return `
      <section class="panel settings-rules-panel first-run-block">
        ${renderRuleManager("Distractions")}
      </section>
      <section class="panel first-run-block">
        <p class="eyebrow">Webcam</p>
        <label class="setting-row toggle-row">
          <span>Turn on webcam monitoring</span>
          <span class="toggle-field">
            <input id="first-run-webcam" class="toggle-input" type="checkbox" ${settings.webcam_enabled ? "checked" : ""} aria-label="Turn on webcam monitoring" />
            <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
          </span>
        </label>
        <span class="explain">Vision models spot a phone in your hand. They run only on this computer.</span>
      </section>
      <p class="first-run-where">You can change both later in Settings.</p>
      <div class="first-run-actions">${back}<button class="primary" type="button" data-action="first-run-next">Next</button></div>
    `;
  }
  if (step === "catch") {
    const target = practiceTarget();
    const voice = practiceVoiceName();
    const speaks = voice ? `a card pops up on your screen and ${escapeHtml(voice)} tells you to get back to work` : "a card pops up on your screen";
    if (!target) {
      return `
        <p class="first-run-copy">There's nothing on your distractions list to practise with. Go back and add one.</p>
        <div class="first-run-actions">${back}<span></span></div>
      `;
    }
    if (!practice) {
      return `
        <p class="first-run-copy">Press Start practice, then open <b>${escapeHtml(target)}</b> and leave it open. After about 30 seconds, ${speaks}. This practice isn't saved.</p>
        <div class="first-run-actions">${back}<button class="primary" type="button" data-action="first-run-practice">Start practice</button></div>
      `;
    }
    if (!practice.caught) {
      return `
        <p class="first-run-copy">Open <b>${escapeHtml(target)}</b> now and leave it open. Supervisor is watching.</p>
        <p class="first-run-live" id="first-run-live">${escapeHtml(firstRunLiveText(snapshot?.current ?? null))}</p>
        <div class="first-run-actions"><button type="button" data-action="first-run-end-practice">End practice</button><span></span></div>
      `;
    }
    return `
      <p class="first-run-copy">That's how it works. In a session, whenever a distraction stays open, this card pops up${voice ? ` and ${escapeHtml(voice)} speaks` : ""}.</p>
      <p class="first-run-where">On the card, Back to work closes it. Allow once lets that site or app stay open for the rest of the session.</p>
      <div class="first-run-actions"><span></span><button class="primary" type="button" data-action="first-run-end-practice">Next</button></div>
    `;
  }
  return `
    <p class="first-run-copy">Start a session whenever you want to focus: press Start Focus Session on the Session tab.</p>
    <p class="first-run-where">Everything you set up is in Settings, and finished sessions show up in History.</p>
    <div class="first-run-actions"><span></span><button class="primary" type="button" data-action="first-run-close">Done</button></div>
  `;
}

/** What the practice is watching right now, in the words a report would use. */
function firstRunLiveText(current: CurrentSession | null): string {
  const focus = current?.focus;
  if (!focus) {
    return "Watching · nothing in front yet";
  }
  return `Watching · ${focus.distracting ? "distraction" : "in front"}: ${focus.activity.label}`;
}

/** The practice's line and its catch, painted in place by the poll. */
function paintFirstRunLive(current: CurrentSession | null): void {
  if (!practice || !current || current.id !== practice.id) {
    return;
  }
  if (!practice.caught && current.metrics.nudges > 0) {
    practice.caught = true;
    paintFirstRun();
    return;
  }
  const line = document.getElementById("first-run-live");
  if (line) {
    line.textContent = firstRunLiveText(current);
  }
}

/** A real session on the Strict setting, so the first nudge comes in about 30 seconds. */
async function startPractice(): Promise<void> {
  const target = practiceTarget();
  if (!target || snapshot?.current) {
    return;
  }
  ruleEditor = null;
  const started = await invokeSnapshot("start_session", {
    input: { goal: `Practice: open ${target}`, duration_minutes: 5, strictness: "strict", exceptions: [], pomodoro: null },
  });
  const current = snapshot?.current;
  if (!started || !current) {
    paintFirstRun();
    return;
  }
  practice = { id: current.id, caught: false };
  writeLocal(PRACTICE_KEY, String(current.id));
  activeTab = "session";
  render();
}

/** Stops the practice and removes it from History; once it caught something, on to the end. */
async function endPractice(): Promise<void> {
  const run = practice;
  if (!run) {
    return;
  }
  practice = null;
  practiceIds.add(run.id);
  practiceQuietUntil = Date.now() + 5000;
  if (run.caught) {
    firstRunStep = "done";
    writeLocal(FIRST_RUN_KEY, "1");
  }
  if (snapshot?.current?.id === run.id) {
    await invokeSnapshot("stop_session", undefined, false);
  }
  await discardPractice(run.id);
}

async function discardPractice(id: number): Promise<void> {
  practiceIds.add(id);
  knownHistoryIds.add(id);
  if (id > 0) {
    try {
      await invoke("delete_session", { id });
    } catch {
      // Already gone, or never saved.
    }
  }
  try {
    localStorage.removeItem(PRACTICE_KEY);
  } catch {
    // Nothing to clear.
  }
  try {
    snapshot = await invoke<SessionSnapshot>("get_session_snapshot");
    settings = snapshot.settings;
    history = snapshot.history;
    await refreshTrends(false);
  } catch (error) {
    lastError = errorText(error);
  }
  render();
}

/** Later, one at a time on Home: the next thing worth knowing, once it's worth knowing. */
interface HomeTip {
  id: string;
  text: string;
  label: string;
  action: string;
}

function homeTip(): HomeTip | null {
  const hidden = (id: string) => readLocal(`tip.${id}`) === "1";
  const recorded = (voiceBackend?.profiles ?? []).some(
    (profile) => !(STARTER_PROFILE_IDS as readonly string[]).includes(profile.profile_id) && Boolean(profile.reference_path),
  );
  if (history.length >= 1 && !recorded && !hidden("voice")) {
    return { id: "voice", text: "Record a voice you know", label: "Open", action: "tip-voice" };
  }
  if (history.length >= 3 && !settings.webcam_enabled && !hidden("webcam")) {
    return { id: "webcam", text: "Catch your phone too", label: "Turn on", action: "tip-webcam" };
  }
  const oldest = history[history.length - 1];
  if (oldest && Date.now() - new Date(oldest.started_at).getTime() >= 6 * 86_400_000 && !hidden("share")) {
    return { id: "share", text: "Share your week", label: "Open", action: "tip-share" };
  }
  return null;
}

function renderHomeTip(): string {
  const tip = homeTip();
  if (!tip) {
    return "";
  }
  return `
    <article class="panel onboard-card">
      <div class="onboard-head">
        <p class="eyebrow">${escapeHtml(tip.text)}</p>
        <div class="home-tip-actions">
          <button class="small-button" type="button" data-action="${tip.action}">${escapeHtml(tip.label)}</button>
          <button class="small-button" type="button" data-action="tip-hide" data-tip="${tip.id}">Hide</button>
        </div>
      </div>
    </article>
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
            <p class="eyebrow">New session</p>
            <h2 id="preflight-title">Start a focus session</h2>
          </div>
          <button class="small-button" type="button" data-action="close-preflight">Cancel</button>
        </div>

        <div class="preflight-main">
          <label class="preflight-goal">
            <span>What are you working on?</span>
            <input id="preflight-goal" maxlength="220" value="${attr(draft.goal)}" placeholder="Finish the Supervisor UI overhaul" />
          </label>

          <div class="mode-tabs" role="group" aria-label="Session kind">
            <button class="${draft.mode === "timer" ? "active" : ""}" type="button" data-action="session-mode" data-mode="timer" aria-pressed="${draft.mode === "timer"}">Timer</button>
            <button class="${draft.mode === "pomodoro" ? "active" : ""}" type="button" data-action="session-mode" data-mode="pomodoro" aria-pressed="${draft.mode === "pomodoro"}">Pomodoro</button>
          </div>

          ${draft.mode === "pomodoro" ? renderPomodoroField() : `
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
          </section>`}

          <details class="preflight-disclosure" data-preflight-section="blocks" ${preflightBlocksOpen ? "open" : ""}>
            <summary>
              <span>Distractions</span>
              <span>${ruleCount} on the list${preflightExceptions.length ? ` · ${preflightExceptions.length} allowed once` : ""}</span>
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

/** Work / break chips on the left, rounds on the right, the finish time under them. */
function renderPomodoroField(): string {
  const rhythm = POMODORO_RHYTHMS[draft.rhythm];
  const minutes = draft.rounds * rhythm.work + (draft.rounds - 1) * rhythm.rest;
  const done = new Date(Date.now() + minutes * 60_000).toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  return `
    <section class="duration-field pomodoro-field">
      <div class="duration-head"><span>Work / break</span><span>Rounds</span></div>
      <div class="pomodoro-row">
        <div class="pomodoro-rhythms" role="group" aria-label="Work and break minutes">
          ${POMODORO_RHYTHMS.map(
            (item, index) =>
              `<button class="${index === draft.rhythm ? "active" : ""}" type="button" data-action="pomodoro-rhythm" data-index="${index}" aria-pressed="${index === draft.rhythm}">${item.work} / ${item.rest}</button>`,
          ).join("")}
        </div>
        <span class="pomodoro-rounds">
          <button type="button" data-action="pomodoro-rounds" data-step="-1" aria-label="One round fewer" ${draft.rounds <= 1 ? "disabled" : ""}>−</button>
          <b>${draft.rounds}</b>
          <button type="button" data-action="pomodoro-rounds" data-step="1" aria-label="One round more" ${draft.rounds >= 8 ? "disabled" : ""}>+</button>
        </span>
      </div>
      <p class="pomodoro-done">Done around ${escapeHtml(done)}</p>
    </section>
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
            <span class="explain">Record anyone's voice. It speaks up when you get distracted.</span>
            <button type="button" data-action="open-voice-settings">Customize</button>
          </div>
        </section>

        <section class="panel settings-feature-card">
          <p class="eyebrow">Webcam</p>
          <div class="voice-summary">
            <strong>Live camera check</strong>
            <span class="explain">Vision models spot a phone in your hand. They run only on this computer.</span>
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
            <p class="quiet">No images saved. Vision models run only on this computer. A phone in view counts as a distraction and gets a nudge; leaving your desk counts as Away.</p>
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
        <button class="rule-chip rule-chip-distracting ${allowed ? "is-allowed" : ""}" type="button" aria-pressed="${allowed}" data-action="toggle-exception" data-rule-index="${index}" title="${attr(allowed ? "Allowed once. Tap to count it as a distraction again." : "Tap to allow it once, for this session.")}">
          ${escapeHtml(ruleName(rule))}
        </button>
      `;
    })
    .join("");
  return `
    <div class="rule-manager preflight-rule-manager">
      <p class="explain">Tap one to allow it once, for this session. Everything else counts as work.</p>
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

/** True once what was typed resolved to a site or an app. */
function rulePreviewReady(editor: RuleEditor): boolean {
  return Boolean(editor.value.trim() && editor.preview && "value" in editor.preview);
}

function renderRulePreview(editor: RuleEditor): string {
  const preview = editor.preview;
  if (!editor.value.trim() || !preview) {
    return `<span class="rule-preview-note">Paste a link or type a name.</span>`;
  }
  if ("error" in preview) {
    return `<span class="rule-preview-error">${escapeHtml(preview.error)}</span>`;
  }
  const site = preview.target === "website";
  // A site reads as the name typed beside it, or its address until there is one.
  const value = site ? editor.name.trim() || preview.value : appNames[preview.value.toLowerCase()] ?? preview.value;
  return `
    <span class="rule-kind">${site ? "Website" : "App"}</span>
    <strong class="rule-preview-value">${escapeHtml(value)}</strong>
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
        <span>Name</span>
        <input id="rule-name" name="name" maxlength="60" value="${attr(editor.name)}" placeholder="${attr(site ?? "")}" />
      </label>
      <p class="rule-preview" id="rule-preview" aria-live="polite">${renderRulePreview(editor)}</p>
      ${
        editor.index !== null && editor.confirmDelete
          ? `<div class="rule-editor-actions">
              <span class="rule-editor-ask">Stop counting ${escapeHtml(permanentRules[editor.index] ? ruleName(permanentRules[editor.index]) : "this")} as a distraction?</span>
              <button class="danger" type="button" data-rule-action="delete" data-rule-index="${editor.index}">Delete</button>
              <button type="button" data-rule-action="keep">Keep it</button>
            </div>`
          : `<div class="rule-editor-actions">
              <button class="primary ${rulePreviewReady(editor) ? "" : "is-quiet"}" type="submit">Save</button>
              ${editor.index === null ? "" : `<button class="danger" type="button" data-rule-action="delete" data-rule-index="${editor.index}">Delete</button>`}
              <button type="button" data-rule-action="cancel">Cancel</button>
            </div>`
      }
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
            ${voices.map((voice) => renderVoiceCard(voice)).join("")}
          </div>
        </section>

        ${renderVoiceTiles()}
        <div class="slot" id="voice-lines">${renderVoiceLinesArea()}</div>
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

/** `editable`: the ⋯ that opens the voice's settings, which live on the Voice page only. */
function renderVoiceCard(voice: VoiceOption, editable = true): string {
  const selected = draft.voiceId === voice.id;
  const profile = voice.profile;
  const needsGeneration = voice.source === "custom" && Boolean(profile?.reference_retained) && !voice.selectable;
  // The selected card still presses like the others; clicking it again changes nothing.
  const pickable = voiceBusy === null && (selected || needsGeneration || voice.selectable);
  const action = needsGeneration ? "set-up-clips" : "select-voice";
  const label = needsGeneration ? `Generate ${voice.name}'s lines` : selected ? `${voice.name}, selected` : `Select ${voice.name}`;
  const card = `class="voice-card ${selected ? "selected" : ""} ${pickable ? "is-pickable" : ""}" role="button" tabindex="0" data-action="${action}" data-voice-id="${attr(voice.id)}" aria-pressed="${selected}" aria-disabled="${!pickable}" aria-label="${attr(label)}"`;
  if (voice.source === "none") {
    return `
      <article ${card}>
        <strong>${escapeHtml(voice.name)}</strong>
        <span class="voice-card-quiet">Stays quiet</span>
      </article>
    `;
  }
  const preview = previewClipForVoice(voice);
  return `
    <article ${card}>
      <div class="voice-card-top">
        <strong>${escapeHtml(voice.name)}</strong>
        ${voice.source === "starter" ? `<span class="voice-card-tag">Starter</span>` : ""}
      </div>
      <span class="voice-card-speaks">speaks like <b>${escapeHtml(toneLabel(voiceTone(voice)))}</b></span>
      ${needsGeneration ? `<span class="voice-card-note">Lines not generated yet</span>` : ""}
      <div class="voice-card-foot">
        <button class="voice-play" type="button" data-action="play-clip" data-clip-path="${attr(preview?.path ?? "")}" ${preview ? "" : "disabled"} aria-label="Play ${attr(voice.name)}'s recording">
          <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.2v9.6L11 6z" fill="currentColor"/></svg>${MINI_ORB}
        </button>
        ${editable ? `<button class="voice-card-more" type="button" data-action="voice-info" data-voice-id="${attr(voice.id)}" aria-label="Edit ${attr(voice.name)}" aria-haspopup="dialog">⋯</button>` : ""}
      </div>
    </article>
  `;
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

/** Redraws the personality and lines panels alone, so the voices, the tiles and the page scroll
 *  stay put. */
function paintClipsPanel(): void {
  if (!paintSlot("voice-lines", renderVoiceLinesArea())) {
    render();
  }
}

/** After a line or personality change: the lines panel and any open voice dialog. Voice cards don't show
 *  lines, so the rest of the page stays; an error shows above the page, so it needs the page. */
function paintLines(): void {
  if (voiceError) {
    render();
    return;
  }
  paintClipsPanel();
  paintVoiceLayer();
}

/** The personality forms and the voice rename form, inside `root`. */
function bindClipsControls(root: ParentNode): void {
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
  root.querySelector<HTMLFormElement>("#voice-rename")?.addEventListener("submit", (event) => {
    event.preventDefault();
    (document.getElementById("voice-rename-input") as HTMLInputElement | null)?.blur();
  });
  root.querySelector<HTMLInputElement>("#voice-rename-input")?.addEventListener("blur", () => void renameVoice());
}
function bindPhraseEditor(root: ParentNode): void {
  const phraseForm = root.querySelector<HTMLFormElement>("#phrase-editor");
  phraseForm?.addEventListener("submit", (event) => {
    event.preventDefault();
    void saveLine(true);
  });
  const text = phraseForm?.querySelector<HTMLTextAreaElement>('textarea[name="text"]');
  if (text) {
    fitLineBox(text);
  }
  text?.addEventListener("input", () => {
    fitLineBox(text);
    if (phraseEditor) {
      phraseEditor.text = text.value;
      const count = document.querySelector(".line-count");
      if (count) {
        count.textContent = `${text.value.length}/140`;
      }
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
  phraseForm?.querySelector<HTMLSelectElement>('select[name="moment"]')?.addEventListener("change", (event) => {
    if (phraseEditor) {
      phraseEditor.moment = (event.target as HTMLSelectElement).value as PhraseMoment;
      paintLinePopupButtons();
    }
  });
}

/** The line box starts one row tall and grows with the words. */
function fitLineBox(text: HTMLTextAreaElement): void {
  text.style.height = "auto";
  text.style.height = `${text.scrollHeight + text.offsetHeight - text.clientHeight}px`;
}

/** Typing changes which buttons make sense; redraw just the popup's action row and audio note. */
function paintLinePopupButtons(): void {
  const holder = document.createElement("div");
  holder.innerHTML = renderLinePopup();
  for (const selector of [".line-popup-actions", ".line-audio-note"]) {
    const now = document.querySelector(selector);
    const next = holder.querySelector(selector);
    if (!now || !next) {
      continue;
    }
    now.replaceWith(next);
    next.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
      button.addEventListener("click", () => {
        void runAction(button.dataset.action || "", button);
      });
    });
  }
}
/** A voice popup opening or closing repaints only the dialog layer; the page under it stays put. */
function paintVoiceLayer(): void {
  paintSlot("voice-layer", renderVoiceDialogs());
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
        <div><p class="eyebrow">Voice engine</p><strong>${escapeHtml(modelValue)}</strong></div>
        <button type="button" data-action="open-model-info" aria-label="Voice engine details" aria-haspopup="dialog">›</button>
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

/** "Derek speaks like Mom": the personality cards, then that personality's lines by moment. */
function renderVoiceLinesArea(): string {
  return `${renderPersonalityPanel()}${renderVoiceClipsPanel()}`;
}

function renderPersonalityPanel(): string {
  const voice = clipsVoice();
  const speaks = voiceTone(voice);
  const locked = !voice.profile || voiceBusy !== null || clipQueue?.phase === "working";
  const cards = phraseToneOrder()
    .map((tone) => {
      const on = sameTone(tone, speaks);
      const card = `<button class="personality-card ${on ? "is-on" : ""}" type="button" data-action="use-tone" data-tone="${attr(tone)}" aria-pressed="${on}" title="${attr(toneLabel(tone))}" ${locked ? "disabled" : ""}><span>${escapeHtml(toneLabel(tone))}</span></button>`;
      if (!on || !isCustomTone(tone)) {
        return card;
      }
      const menu = toneMenuOpen
        ? `<div class="personality-menu" role="menu">
            <button type="button" role="menuitem" data-action="rename-tone" data-tone="${attr(tone)}">Rename</button>
            <button class="danger" type="button" role="menuitem" data-action="arm-delete-tone" data-tone="${attr(tone)}">Delete</button>
          </div>`
        : "";
      return `
        <div class="personality-own">
          ${card}
          <button class="personality-more" type="button" data-action="toggle-tone-menu" aria-label="Rename or delete ${attr(toneLabel(tone))}" aria-haspopup="menu" aria-expanded="${toneMenuOpen}" ${locked ? "disabled" : ""}>⋯</button>
          ${menu}
        </div>`;
    })
    .join("");
  let form = "";
  if (toneRename) {
    form = `
      <form class="personality-form" id="tone-rename">
        <input id="tone-rename-input" maxlength="80" value="${attr(toneRename.value)}" aria-label="Personality name" autocomplete="off" />
        <button class="primary small-button" type="submit">Save</button>
        <button class="small-button" type="button" data-action="cancel-rename-tone">Cancel</button>
      </form>`;
  } else if (toneDeleteArmed) {
    const count = tonePhrases(toneDeleteArmed).length;
    form = `
      <div class="personality-form">
        <span>Delete ${escapeHtml(toneLabel(toneDeleteArmed))}${count ? ` and its ${count === 1 ? "1 line" : `${count} lines`}` : ""}? Voices that speak like it go back to ${escapeHtml(toneLabel(toneOrder[0]))}.</span>
        <button class="small-button danger" type="button" data-action="delete-tone" data-tone="${attr(toneDeleteArmed)}">Delete</button>
        <button class="small-button" type="button" data-action="cancel-delete-tone">Cancel</button>
      </div>`;
  }
  return `
    <section class="panel personality-panel">
      <p class="eyebrow">Personality</p>
      <div class="personality-head">
        <h2 class="speaks-like"><span class="speaks-voice" title="${attr(voice.name)}">${escapeHtml(voice.name)}</span> speaks like <span class="speaks-tone" title="${attr(toneLabel(speaks))}">${escapeHtml(toneLabel(speaks))}</span></h2>
        <button class="small-button" type="button" data-action="new-tone" aria-haspopup="dialog" ${canEditPhrases() ? "" : "disabled"}>+ New personality</button>
      </div>
      <div class="personality-list">${cards}</div>
      ${form}
    </section>
  `;
}

/** A name, and where its lines start: a copy of one you like, or nothing. */
function renderNewPersonalityModal(): string {
  if (newToneDraft === null) {
    return "";
  }
  const speaks = voiceTone(clipsVoice());
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup new-personality-popup" role="dialog" aria-modal="true" aria-labelledby="new-personality-title">
        <h2 id="new-personality-title">New personality</h2>
        <p>A personality is what your voices say. Start from one you like and change a few of its lines, or start with nothing and write your own.</p>
        <form class="phrase-editor" id="new-tone">
          <label>
            <span>Name</span>
            <input id="new-tone-input" maxlength="80" value="${attr(newToneDraft)}" placeholder="Coach, dry humor, drill sergeant…" autocomplete="off" />
          </label>
          <label>
            <span>Start from</span>
            <select id="new-tone-from">
              ${phraseToneOrder().map((tone) => `<option value="${attr(tone)}" ${sameTone(tone, speaks) ? "selected" : ""}>${escapeHtml(toneLabel(tone))}</option>`).join("")}
              <option value="">Nothing</option>
            </select>
          </label>
          <div class="modal-actions">
            <button type="button" data-action="cancel-new-tone">Cancel</button>
            <button class="primary" type="submit">Create</button>
          </div>
        </form>
      </section>
    </div>
  `;
}

/** The voice's personality, one card per moment. */
function renderVoiceClipsPanel(): string {
  const voice = clipsVoice();
  const tone = voiceTone(voice);
  return `
    <section class="panel phrase-panel">
      <h3 class="lines-title">What ${escapeHtml(voice.name)} says as ${escapeHtml(toneLabel(tone))}</h3>
      <div class="moment-grid">
        ${MOMENTS.map((moment) => renderMomentCard(voice, tone, moment)).join("")}
      </div>
      ${renderClipsFooter(voice, tonePhrases(tone))}
    </section>
  `;
}

function renderMomentCard(voice: VoiceOption, tone: VoiceTone, moment: (typeof MOMENTS)[number]): string {
  const phrases = momentPhrases(tone, moment.id);
  const count = phrases.length;
  const quiet: Record<PhraseMoment, string> = {
    nudge: `${voice.name} stays quiet when you get distracted.`,
    phone: `${voice.name} says a back to work line instead.`,
    roundover: `${voice.name} stays quiet when a round ends.`,
    sessionover: `${voice.name} stays quiet when a session ends.`,
  };
  const more = 3 - count;
  const nudge =
    moment.often && more > 0
      ? `<div class="moment-nudge" role="note"><strong>${count ? `Only ${count}? That gets old fast.` : "This one plays a lot."}</strong><span>We recommend 3 or more lines here. Trust us, you'll be hearing them all session. Write ${more} more and have some fun with it.</span></div>`
      : "";
  return `
    <div class="clips-card moment-card">
      <div class="moment-head">
        <p class="eyebrow">${escapeHtml(moment.label)}</p>
        <span>${count === 1 ? "1 line" : `${count} lines`}</span>
        <span class="moment-when">${escapeHtml(moment.when)}</span>
      </div>
      ${count ? `<div class="line-list">${phrases.map((phrase) => renderLineRow(voice, phrase)).join("")}</div>` : `<p class="empty">No lines yet. ${escapeHtml(quiet[moment.id])}</p>`}
      ${nudge}
      <div class="moment-foot">
        <button class="small-button" type="button" data-action="add-line" data-moment="${moment.id}" ${canEditPhrases() ? "" : "disabled"}>+ Add line</button>
      </div>
    </div>
  `;
}

function renderLineRow(voice: VoiceOption, phrase: PhraseDefinition): string {
  const clip = clipForPhrase(voice, phrase);
  const job = clipQueue && clipQueue.voiceId === voice.id ? clipQueue : null;
  const position = job ? job.phraseIds.indexOf(phrase.id) : -1;
  const busy = Boolean(job && position === job.index && job.phase === "working");
  const failed = Boolean(job && position === job.index && job.phase === "error");
  const queued = Boolean(job && job.phase === "working" && position > job.index);
  const edit = `<button class="small-button" type="button" data-action="open-phrase-editor" data-phrase-id="${attr(phrase.id)}" ${!busy && canEditPhrases() ? "" : "disabled"}>Edit</button>`;
  const play = clip
    ? `<button class="voice-play" type="button" data-action="play-clip" data-clip-path="${attr(clip.path)}" aria-label="Play">
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.2v9.6L11 6z" fill="currentColor"/></svg>${MINI_ORB}
      </button>`
    : "";
  let trail: string;
  if (busy) {
    trail = `<span class="line-state is-busy" data-clip-elapsed>${escapeHtml(clipElapsedText())}</span>`;
  } else if (failed) {
    trail = `<span class="line-state is-bad" title="${attr(job?.error ?? "")}">Failed</span><button class="small-button" type="button" data-action="generate-clip" data-phrase-id="${attr(phrase.id)}">Retry</button>${edit}`;
  } else if (queued) {
    trail = `<span class="line-state">Queued</span>${edit}${play}`;
  } else if (clip) {
    trail = `${edit}${play}`;
  } else {
    trail = `<span class="line-state">Not generated</span>${edit}`;
  }
  return `
    <div class="line-row ${clip ? "" : "is-missing"} ${busy ? "is-busy" : ""}">
      <span class="line-dot ${clip ? "is-ready" : ""}" aria-hidden="true"></span>
      <span class="line-text">${escapeHtml(phrase.text)}</span>
      <span class="line-actions">${trail}</span>
    </div>
  `;
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

function renderClipsFooter(voice: VoiceOption, phrases: PhraseDefinition[]): string {
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
  if (phrases.length === 0) {
    return `
      <div class="clips-foot">
        <span class="clips-hint">A personality needs at least one back to work line before a voice can use it.</span>
      </div>
    `;
  }
  const missing = phrases.filter((phrase) => !clipForPhrase(voice, phrase));
  if (missing.length === 0) {
    return `
      <div class="clips-foot">
        <span class="clips-hint is-ready">All ${phrases.length} lines ready.</span>
      </div>
    `;
  }
  const lock = clipGenerateLockTitle(voice);
  // A failed line says why here, where the eye already is; the row only says "Failed".
  const failed = job?.phase === "error" && !lock ? job.error || "This line could not be generated." : "";
  const hint = lock || (failed ? `Last line failed: ${failed}` : `The first line takes about ${FIRST_CLIP_SECONDS} s while the voice engine warms up, then about ${NEXT_CLIP_SECONDS} s each.`);
  return `
    <div class="clips-foot">
      <div class="clips-foot-actions">
        <button class="primary small-button clips-generate" type="button" data-action="generate-missing" ${lock || clipQueue?.phase === "working" ? "disabled" : ""}>Generate ${missing.length} ${missing.length === 1 ? "line" : "lines"} · ~${escapeHtml(formatQueueTime(clipQueueSeconds(missing.length)))}</button>
        <span class="clips-hint ${failed || lock ? "is-bad" : ""}" role="${failed ? "alert" : "status"}">${escapeHtml(hint)}</span>
      </div>
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
  const moved = Boolean(existing && existing.moment !== editor.moment);
  const lock = clipQueue?.phase === "working" ? "Wait for the current line to finish" : clipGenerateLockTitle(voice);
  const primary = !existing ? "Add and generate" : changed ? "Save and regenerate" : clip ? "Regenerate" : "Generate";
  const primaryDisabled = Boolean(lock) || !text || voiceBusy !== null;
  const audio = clip
    ? `<div class="line-audio-block">
        <span class="line-audio-label">Audio</span>
        <div class="line-audio ${changed ? "is-stale" : ""}">
          <button class="voice-preview-play" type="button" data-action="play-clip" data-clip-path="${attr(clip.path)}" aria-label="Play"><span class="voice-preview-icon" aria-hidden="true"></span>${MINI_ORB}</button>
          <canvas class="line-audio-wave" width="300" height="36" data-wave-path="${attr(clip.path)}" data-wave-bars="32"></canvas>
        </div>
        <p class="line-audio-note">${changed ? "This is the old wording until you regenerate." : ""}</p>
      </div>`
    : "";
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-popup line-popup" role="dialog" aria-modal="true" aria-labelledby="line-popup-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">${escapeHtml(toneLabel(editor.tone))} · ${escapeHtml(voice.name)}</p>
            <h2 id="line-popup-title">${existing ? "Edit line" : "New line"}</h2>
          </div>
          <button class="small-button" type="button" data-action="cancel-phrase-editor">Close</button>
        </div>
        <form class="phrase-editor" id="phrase-editor">
          <label>
            <textarea id="phrase-editor-text" aria-label="Line" name="text" rows="1" maxlength="140" placeholder="Back to the task." autocomplete="off">${escapeHtml(editor.text)}</textarea>
          </label>
          <span class="line-count">${editor.text.length}/140</span>
          <label>
            <span>Plays when</span>
            <select id="phrase-editor-moment" name="moment">
              ${MOMENTS.map((moment) => `<option value="${moment.id}" ${moment.id === editor.moment ? "selected" : ""}>${escapeHtml(moment.label)}: ${escapeHtml(moment.when.charAt(0).toLowerCase() + moment.when.slice(1))}</option>`).join("")}
            </select>
          </label>
          ${audio}
          ${
            existing && editor.confirmDelete
              ? `<div class="modal-actions line-popup-actions">
                  <span class="line-popup-ask">Delete this line?</span>
                  <button type="button" data-action="keep-phrase">Keep it</button>
                  <button class="danger" type="button" data-action="delete-phrase" data-phrase-id="${attr(existing.id)}">Delete</button>
                </div>`
              : `<div class="modal-actions line-popup-actions">
                  ${existing ? `<button class="danger" type="button" data-action="delete-phrase" data-phrase-id="${attr(existing.id)}" ${canEditPhrases() ? "" : "disabled"}>Delete</button>` : ""}
                  <span class="line-popup-gap"></span>
                  ${changed || moved || !existing ? `<button type="button" data-action="save-line" ${text && canEditPhrases() ? "" : "disabled"}>${existing ? "Save only" : "Add only"}</button>` : ""}
                  <button class="primary" type="submit" ${primaryDisabled ? "disabled" : ""} title="${attr(lock)}">${primary}</button>
                </div>`
          }
        </form>
        ${lock ? `<p class="quiet">${escapeHtml(lock)}</p>` : ""}
      </section>
    </div>
  `;
}

function closeToneForms(): void {
  toneRename = null;
  toneDeleteArmed = null;
  newToneDraft = null;
  toneMenuOpen = false;
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
        <p>Supervisor nudges you with short spoken lines. Record anyone for about twenty seconds, generate their lines once, and from then on they play instantly, offline.</p>
        <ol class="voice-help-steps">
          <li><strong>Record a voice.</strong> Press + New voice and read the prompt aloud. The starter voices come ready.</li>
          <li><strong>Pick a personality.</strong> It's what the voice says, from Gentle to Stern, or one you write. Each has lines for when you get distracted, when your phone shows up, and when a round or session ends.</li>
          <li><strong>Generate its lines.</strong> Press Generate under the lines. The first takes about ${FIRST_CLIP_SECONDS} seconds while the voice engine warms up, the rest about ${NEXT_CLIP_SECONDS} seconds each. It needs the voice engine installed and some free memory.</li>
          <li><strong>Select it.</strong> Once its back to work lines are generated, select the voice under Voices.</li>
        </ol>
        <p class="quiet">Listen to each line. If one sounds off, press Edit and regenerate it.</p>
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
            <p class="eyebrow">Voice engine</p>
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
            ? "Up to date: nothing to download. It only runs while lines are being generated."
            : installed
            ? "It only runs while lines are being generated. Check for updates downloads only what is missing."
            : `A one-time download of about ${escapeHtml(formatBytes(model?.required_bytes ?? 0))}. Generated lines play without it; generating new ones needs it.`
        }</p>
        <div class="modal-actions">
          ${installed ? `<button class="danger" type="button" data-action="ask-delete-model" ${busy ? "disabled" : ""}>Delete voice engine</button>` : ""}
          <button class="${installed ? "" : "primary"}" type="button" data-action="download-model-pack" ${busy ? "disabled" : ""}>${installed ? "Check for updates" : "Download voice engine"}</button>
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
        <p>Generating lines starts the voice engine (about ${FIRST_CLIP_SECONDS} seconds the first time) and needs:</p>
        <div class="voice-kv">
          <div><span>Needs free</span><strong>${escapeHtml(need ? formatBytes(need) : "—")}</strong></div>
          <div><span>Free now</span><strong class="${locked ? "is-bad" : ""}" data-ram-free>${escapeHtml(free === null ? "—" : formatBytes(free))}</strong></div>
          <div><span>In this machine</span><strong data-ram-machine>${escapeHtml(system?.total_ram_bytes ? formatBytes(system.total_ram_bytes) : "—")}</strong></div>
        </div>
        <p data-ram-copy>${escapeHtml(ramLockCopy(locked, peak))}</p>
        <p class="quiet">Lines already generated keep playing. They never need the voice engine or this memory.</p>
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
  return peak ? `Enough right now. The last line used up to ${formatBytes(peak)}; the memory is freed again after each line.` : "Enough right now. The memory is freed again after each line.";
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
    ${showVoiceDialog && !phraseEditor ? renderNewPersonalityModal() : ""}
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

function renderQuitConfirm(): string {
  if (!quitConfirmOpen) {
    return "";
  }
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel confirm-panel" role="dialog" aria-modal="true" aria-labelledby="quit-confirm-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">QUIT</p>
            <h2 id="quit-confirm-title">End this session and quit?</h2>
          </div>
        </div>
        <p>The session so far is saved to History.</p>
        <div class="modal-actions">
          <button type="button" data-action="cancel-quit">Cancel</button>
          <button class="danger" type="button" data-action="confirm-quit">End and quit</button>
        </div>
      </section>
    </div>
  `;
}

/** Session done (yellow, the round-over card's shape): the session's title and focused time,
 *  the voice with its orb, and one button to the report under it. */
function renderSessionOver(): string {
  if (!sessionOver) {
    return "";
  }
  const { voice, session } = sessionOver;
  return `
    <div class="modal-backdrop session-over-backdrop" role="presentation">
      <section class="modal-panel session-over-card" role="dialog" aria-modal="true" aria-labelledby="session-over-title">
        <section class="state-slab interrupt-alert moment-round-over">
          <h2 class="moment-title" id="session-over-title">Session done.</h2>
          ${session ? `<p class="moment-goal">${escapeHtml(session.goal)}</p>
          <p class="moment-focused"><b>${Math.round(session.focus_seconds / 60)} min</b> focused</p>` : ""}
          ${renderVoiceRow(voice)}
          <div class="moment-gap"></div>
        </section>
        <div class="interrupt-actions moment-actions">
          <button class="primary" type="button" data-action="close-session-over">See how it went</button>
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
            <p class="eyebrow">New voice</p>
            <h2 id="record-title">Record voice</h2>
          </div>
          <button class="small-button" type="button" data-action="close-recording">Close</button>
        </div>

        <div class="recording-grid">
          <section class="recording-stage">
            <button class="mic-button ${isRecording ? "recording" : ""}" type="button" data-action="${isRecording ? "stop-recording" : "start-recording"}" ${recordedVoice && !isRecording ? "disabled" : ""} aria-label="${isRecording ? "Stop recording" : "Start recording"}">
              <span></span>
            </button>
            <strong>${isRecording ? "Recording" : recordedVoice ? "Recorded" : "Ready"}</strong>
            <p id="recording-elapsed">${isRecording ? formatDuration(recordingDurationSeconds()) : recordedVoice ? `${formatDuration(recordedVoice.seconds)} captured` : "Read aloud for 20 to 30 seconds somewhere quiet."}</p>
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
                  <p class="quiet">Saving is instant. You pick which lines to generate next.</p>
                `
                : `<p class="empty">Your recording shows up here so you can listen before saving it.</p>`
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
            <h2 id="delete-model-title">Delete the voice engine?</h2>
          </div>
        </div>
        <p>Your voices and their lines stay on this computer. To generate new lines you'll need to download the voice engine again.</p>
        <div class="modal-actions">
          <button type="button" data-action="cancel-delete-model">Cancel</button>
          <button class="danger" type="button" data-action="delete-model-files">Delete voice engine</button>
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
  const canRename = Boolean(voice.profile) && voiceBusy === null;

  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel voice-info-panel" role="dialog" aria-modal="true" aria-labelledby="voice-info-title">
        <div class="voice-info-head">
          <div>
            <p class="eyebrow">Edit voice</p>
            <h2 id="voice-info-title">${escapeHtml(voice.name)}</h2>
          </div>
        </div>
        <form class="voice-rename" id="voice-rename">
          <label>
            <span>Name <em class="voice-rename-status" aria-live="polite"></em></span>
            <input id="voice-rename-input" maxlength="80" value="${attr(voice.name)}" autocomplete="off" ${canRename ? "" : "disabled"} />
          </label>
        </form>
        <div>
          <p class="eyebrow">Recording</p>
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
        </div>
        <div class="voice-info-foot">
          ${
            profileId
              ? `<button class="danger" type="button" data-action="confirm-delete-voice" data-profile-id="${attr(profileId)}">Delete voice</button>`
              : `<p class="voice-info-note">Starter voices stay on this device.</p>`
          }
          <button class="primary" type="button" data-action="close-voice-info">Done</button>
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
        <p class="quiet">Its lines can be regenerated from the recording. Deleting the voice removes the recording too.</p>
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
      ? "Voice engine ready"
      : phase === "checking"
        ? "Checking installed files"
        : phase === "cancelling"
          ? "Cancelling download"
          : "Downloading the voice engine";
  const copy = upToDate
    ? "Already up to date. Nothing was downloaded."
    : done
      ? "The voice engine is installed. Generated lines play without it."
      : checking
        ? "Checking the installed files. Nothing is downloading."
        : download?.message ?? "Downloading the voice engine.";
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
      ? "Nothing is downloaded unless part of it is missing."
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
    return `${renderSessionReport(sessionReport)}<div id="review-layer" class="slot">${renderReviewLayer()}</div>`;
  }
  if (historyView === "trends") {
    return renderTrends();
  }
  return `
    <section class="history-page">
      ${renderHistoryToolbar(historyWeekShown())}
      <div id="history-body" class="slot">${renderHistoryBody()}</div>
    </section>
  `;
}

function historyWeekShown(): string {
  return historyWeek?.week_start || historyWeekStart || thisMondayKey();
}

/** Everything under the toolbar: search results, or the week's charts and lists. */
function renderHistoryBody(): string {
  if (historySearch.trim()) {
    return renderSearchResults();
  }
  const weekStart = historyWeekShown();
  const sessions = historyWeek?.sessions ?? [];
  const recap = weekRecap && weekRecap.start === weekStart ? weekRecap : null;
  // Both charts rise together on arriving at the tab and on the first draw of another week's
  // data; a pane switch or a poll later leaves the bars standing.
  const animate = recap !== null && (takeAnimation("history") || historyChartsFor !== weekStart);
  if (recap) {
    historyChartsFor = weekStart;
  }
  const emptyWeek = historyWeek?.oldest_day
    ? "No sessions this week."
    : "Finish a session and its report shows up here. Nothing leaves this computer.";
  return `
    <div class="history-hero">
      ${renderWeekHero(weekStart, recap, animate)}
      <div class="history-side">
        ${renderShareDoor(weekStart, recap)}
        ${renderTrendsDoor(weekStart, recap, animate)}
      </div>
    </div>
    <div class="history-seg-row">
      <div class="history-seg" role="group" aria-label="Show">
        <button type="button" class="${historyPane === "sessions" ? "is-on" : ""}" aria-pressed="${historyPane === "sessions"}" data-action="history-pane" data-pane="sessions">Sessions · ${sessions.length}</button>
        <button type="button" class="${historyPane === "apps" ? "is-on" : ""}" aria-pressed="${historyPane === "apps"}" data-action="history-pane" data-pane="apps">Apps &amp; sites · ${recap?.apps.length ?? 0}</button>
      </div>
    </div>
    ${historyPane === "apps" ? renderWeekAppsPane(recap) : renderSessionsPane(sessions, emptyWeek)}
  `;
}

/** A small card above the Trends door that opens the Share popup: the viewed week and its
 *  month as pictures. Before the first session it stays, dimmed, so the column keeps its shape. */
function renderShareDoor(weekStart: string, recap: WeekRecap | null): string {
  const ready = recap !== null && recap.start === weekStart && historyWeek?.oldest_day != null;
  return `
    <button type="button" class="panel share-door" data-action="open-share" ${ready ? "" : 'aria-disabled="true"'}>
      <span class="share-door-text">
        <span class="eyebrow">Share</span>
        <strong>${ready ? "Week and month cards" : "Nothing to share yet"}</strong>
      </span>
      ${ready ? '<span class="trend-door-go" aria-hidden="true">Open ›</span>' : ""}
    </button>
  `;
}

/** The Share popup: the week card and the month card drawn as they will be copied, each with
 *  its button. The month is fetched before it opens, so it opens once, at its full size. */
function renderShareModal(): string {
  if (!shareOpen) {
    return "";
  }
  // The cards go in as canvases, not encoded images: encoding two large PNGs on every paint
  // held up the popup. They are swapped in once this markup is in the page, before it paints.
  queueMicrotask(mountShareCards);
  const week = weekRecap && weekRecap.start === historyWeekStart ? weekRecap : null;
  const month = shareMonth && shareMonth.start.slice(0, 7) === historyWeekStart.slice(0, 7) ? shareMonth : null;
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel share-panel" role="dialog" aria-modal="true" aria-labelledby="share-title">
        <div class="modal-head share-head">
          <div>
            <p class="eyebrow">History</p>
            <h2 id="share-title">Share</h2>
            <p class="share-sub">Click a card to copy it, then paste it anywhere.</p>
          </div>
          <button type="button" data-action="close-share">Cancel</button>
        </div>
        <div class="share-grid">
          ${renderShareColumn(week !== null, "week", "copy-week-card", week !== null && week.session_count > 0, "No sessions this week")}
          ${renderShareColumn(month !== null, "month", "copy-month-card", month !== null && month.session_count > 0, "No sessions this month")}
        </div>
      </section>
    </div>
  `;
}

/** The card is the button, like the day card on Home: a click copies it. A period with no
 *  sessions shows its card, still, with a line saying why it can't be copied. */
function renderShareColumn(drawn: boolean, card: "week" | "month", action: string, ready: boolean, note: string): string {
  if (!drawn) {
    return `<div class="share-col"><div class="share-preview share-preview-empty" aria-hidden="true"></div></div>`;
  }
  const slot = `<span class="share-preview" data-share-card="${card}"></span>`;
  return `
    <div class="share-col">
      ${ready ? `<button type="button" class="share-pick" data-action="${action}" aria-label="Copy the ${card} card">${slot}</button>` : slot}
      ${ready ? "" : `<p class="share-note">${escapeHtml(note)}</p>`}
    </div>
  `;
}

/** Draws each card into its place in the popup. */
function mountShareCards(): void {
  document.querySelectorAll<HTMLElement>("[data-share-card]").forEach((slot) => {
    const week = slot.dataset.shareCard === "week";
    const canvas = week ? (weekRecap ? drawWeekCard(weekRecap) : null) : shareMonth ? drawMonthCard(shareMonth) : null;
    if (!canvas) {
      return;
    }
    canvas.className = "share-preview";
    canvas.setAttribute("role", "img");
    canvas.setAttribute("aria-label", week ? "Week card" : "Month card");
    slot.replaceWith(canvas);
  });
}

async function fetchShareMonth(): Promise<MonthRecap | null> {
  try {
    return await invoke<MonthRecap>("get_month_recap", { start: historyWeekStart });
  } catch (error) {
    showFeedback(`Could not load the month: ${errorText(error)}`);
    return null;
  }
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
      <div id="week-picker-slot" class="slot">${weekPickerOpen ? renderWeekPicker(weekStart, oldestMonday) : ""}</div>
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
  });
  // Each week is one row, so hovering any of its days lights the whole week: the thing a click picks.
  const rows = Array.from({ length: cells.length / 7 }, (_, week) => `<div class="wp-week">${cells.slice(week * 7, week * 7 + 7).join("")}</div>`).join("");
  return `
    <div class="panel week-picker" role="dialog" aria-label="Pick a week">
      <div class="week-picker-head">
        <button type="button" data-action="picker-month-prev" ${prevDisabled ? "disabled" : ""} aria-label="Previous month">‹</button>
        <strong>${escapeHtml(title)}</strong>
        <button type="button" data-action="picker-month-next" ${nextDisabled ? "disabled" : ""} aria-label="Next month">›</button>
      </div>
      <div class="week-picker-grid">
        ${["M", "T", "W", "T", "F", "S", "S"].map((d) => `<span class="wp-dow" aria-hidden="true">${d}</span>`).join("")}
        ${rows}
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
    render();
    return;
  }
  paintWeekPicker();
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
          ${weekStat("Longest stretch", longest, empty ? "Longest run of work in one go, once there is one." : "Longest run of work without a distraction or stepping away.")}
          ${weekStat("Recovery", recovery, recovery === "None" ? "No distraction this week, so nothing to recover from." : "Typical time back to work after a distraction.")}
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
  const previous = weeks.find((week) => week.start === previousStart);
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
    value = weeks[0].start === weekStart ? "First week" : "Off the chart";
    caption = weeks[0].start === weekStart ? "nothing before it" : "older weeks are in Trends";
  } else if (previous.session_count === 0 && recap.session_count === 0) {
    value = "Quiet";
    caption = current ? "no sessions this week or last" : "no sessions either week";
  } else if (recap && previous && (previous.session_count > 0 || recap.session_count > 0)) {
    const before = current && todayIndex < 6 ? focusThrough(previousStart, todayIndex) : previous.focus_seconds;
    // A running week is measured against last week up to the same weekday; "so far" says so.
    const against = current ? "last week" : "the week before";
    const soFar = current && todayIndex < 6 ? ", so far" : "";
    const change = recap.focus_seconds - before;
    if (Math.abs(change) < 60) {
      value = "Steady";
      caption = `same as ${against}${soFar}`;
    } else {
      value = `${change > 0 ? "↑" : "↓"} ${formatHours(Math.abs(change))}`;
      caption = `vs ${against}${soFar}`;
    }
  }
  const shown = weeks.slice(-DOOR_WEEKS);
  // The card is the door: a click anywhere on it opens Trends, except on a week's bar, which
  // is its own button and opens that week here. Keyboard: the card takes focus; Enter opens.
  return `
    <section class="panel trend-door ${shown.length ? "has-chart" : ""}" role="button" tabindex="0" data-action="open-trends" aria-label="View trends">
      <div class="trend-door-head">
        <p class="eyebrow">Trends</p>
        <span class="trend-door-go" aria-hidden="true">View trends ›</span>
      </div>
      <div class="trend-door-line">
        <strong class="trend-door-value">${escapeHtml(value)}</strong>
        <span class="trend-door-caption">${escapeHtml(caption)}</span>
      </div>
      ${shown.length ? renderDoorChart(shown, weekStart, animate) : ""}
    </section>
  `;
}

/** Weeks on the door's chart. */
const DOOR_WEEKS = 4;

/** Plain bars, newest week at the right, no axis: the figure above says what they mean. */
function renderDoorChart(shown: TrendPeriod[], weekStart: string, animate: boolean): string {
  const maxWeek = Math.max(...shown.map((week) => week.focus_seconds), 1);
  const pad = DOOR_WEEKS - shown.length;
  const running = thisMondayKey();
  return `
    <div class="trend-spark ${animate ? "is-animated" : ""}" role="img" aria-label="Focused hours per week, last ${shown.length} weeks">
      ${'<span class="trend-spark-track"><span class="trend-spark-bar is-empty"></span></span>'.repeat(pad)}
      ${shown
        .map((week, index) => {
          const classes = ["trend-spark-bar", week.start === weekStart ? "is-viewed" : "", week.start === running ? "is-running" : "", week.focus_seconds === 0 ? "is-empty" : ""].filter(Boolean).join(" ");
          const sessions = week.session_count === 1 ? "1 session" : `${week.session_count} sessions`;
          const tip = `${formatWeekStart(week.start)} · ${formatHours(week.focus_seconds)}${week.start === running ? " so far" : ""}`;
          return `
            <button type="button" class="trend-spark-track" data-action="pick-week" data-day="${attr(week.start)}" aria-label="${attr(`${tip} · ${sessions}. Show this week`)}">
              <span class="${classes}" style="--i: ${index}; ${week.focus_seconds ? `height: ${smartBarHeight(week.focus_seconds, maxWeek, 14)}%` : ""}">
                <span class="week-tip">${escapeHtml(tip)}</span>
              </span>
            </button>
          `;
        })
        .join("")}
    </div>
    <div class="trend-spark-labels" aria-hidden="true">
      ${"<span></span>".repeat(pad)}
      ${shown
        .map((week) => {
          const text = week.start === running ? "So far" : formatWeekStart(week.start);
          return `<span class="${week.start === weekStart ? "is-viewed" : ""}">${escapeHtml(text)}</span>`;
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
    return `<section class="panel history-list-panel"><p class="empty">Sessions ran, but Supervisor couldn't name any app or site. Time on the desktop or in a window it couldn't read isn't listed.</p></section>`;
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
          <div title="Time on work: not on a distraction, your phone, or away"><dt>Focused</dt><dd>${escapeHtml(formatHours(item.focus_seconds))}</dd></div>
          <div title="How long the session ran"><dt>Duration</dt><dd>${formatDuration(item.duration_seconds)}</dd></div>
          <div title="Times Supervisor spoke up about a distraction"><dt>Nudges</dt><dd>${item.nudges}</dd></div>
        </dl>
        ${renderMiniTimeline(item)}
      </div>
    </button>
  `;
}

/** A row the user can rename or move: a real app or site, not the phone, Away or Supervisor. */
function editableApp(app: RecapApp): boolean {
  return (app.kind === "website" || app.kind === "app") && !["untracked", "supervisor", "phone"].includes(app.key);
}

function appOnList(app: RecapApp): boolean {
  const target = app.kind === "website" ? "website" : "app";
  return permanentRules.some((rule) => rule.target === target && rule.value.toLowerCase() === app.key.toLowerCase());
}

/** In Edit mode on a report: move the row to the other group for this session, and offer the
 *  list to a distraction not yet on it. */
function renderAppRowActions(app: RecapApp, sessionId: number): string {
  if (!editableApp(app)) {
    return "";
  }
  const toDistraction = app.group !== "Distractions";
  const move = `<button class="small-button" type="button" data-action="move-app" data-session-id="${sessionId}" data-kind="${attr(app.kind)}" data-key="${attr(app.key)}" data-label="${attr(app.label)}" data-to="${toDistraction ? "distraction" : "work"}">${toDistraction ? "Count as distraction" : "Count as work"}</button>`;
  const list = appOnList(app)
    ? `<button class="small-button" type="button" data-action="remove-rule-from-report" data-kind="${attr(app.kind)}" data-key="${attr(app.key)}">Remove from list</button>`
    : !toDistraction
      ? `<button class="small-button" type="button" data-action="add-rule-from-report" data-kind="${attr(app.kind)}" data-key="${attr(app.key)}">Add to list</button>`
      : "";
  return `<span class="recap-app-actions">${move}${list}</span>`;
}

function renderAppGroups(apps: RecapApp[], groups: string[], sessionId: number | null = null): string {
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
            <div class="recap-app-group">${renderAppRows(items, maxApp, sessionId)}</div>
          </div>
        `;
      })
      .join("")}
  </div>`;
}

/** One line per app or site: name, a bar against the busiest row, the time. Read-only; the
 * distractions list is edited in Settings or during a session, never from a past week. */
function renderAppRows(items: RecapApp[], maxApp: number, sessionId: number | null = null): string {
  return items
    .map((app) => {
      const width = Math.round(app.seconds / 60) > 0 ? Math.max(4, Math.round((app.seconds / maxApp) * 100)) : 0;
      const actions = sessionId !== null && editingRecapNames ? renderAppRowActions(app, sessionId) : "";
      return `
        <div class="recap-app-row ${app.group === "Distractions" ? "is-distraction" : ""} ${actions ? "has-actions" : ""}">
          ${renderRecapAppName(app)}
          <span class="recap-app-track">${width ? `<span style="width: ${width}%"></span>` : ""}</span>
          <strong>${escapeHtml(formatHours(app.seconds))}</strong>
          ${actions}
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
  const weeks = trendPeriods?.week ?? [];
  // Completed weeks with a session, across all of history; the last week is still running.
  const full = weeks.slice(0, -1).filter((week) => week.session_count > 0).length;
  return `
    <section class="history-page recap-page">
      <div class="subpage-head recap-head">
        <button type="button" data-action="close-recap">Back</button>
        <div>
          <p class="eyebrow">History</p>
          <h2>Trends</h2>
        </div>
      </div>
      ${full < TRENDS_UNLOCK_WEEKS ? renderTrendsLocked(full) : renderTrendsBody()}
    </section>
  `;
}

function renderTrendsLocked(full: number): string {
  const have = Math.min(full, TRENDS_UNLOCK_WEEKS);
  return `
    <section class="panel span-card trends-slab trends-locked">
      <p class="eyebrow">Trends</p>
      <p class="span-value">Unlocks after two full weeks</p>
      <p class="span-caption">Lines for focused time, attention span, and recovery need a couple of complete weeks before they mean anything.</p>
      <div class="trend-pips" role="img" aria-label="${have} of ${TRENDS_UNLOCK_WEEKS} full weeks">
        ${Array.from({ length: TRENDS_UNLOCK_WEEKS }, (_, index) => `<span class="${index < have ? "is-on" : ""}"></span>`).join("")}
        <strong>${have} of ${TRENDS_UNLOCK_WEEKS} full weeks</strong>
      </div>
    </section>
  `;
}

/** The `page`th window of TREND_WINDOW periods back from now, never before the first session. */
function trendWindow(list: TrendPeriod[], page: number): TrendPeriod[] {
  const end = list.length - page * TREND_WINDOW;
  return end > 0 ? list.slice(Math.max(0, end - TREND_WINDOW), end) : [];
}

function trendPageCount(list: TrendPeriod[]): number {
  return Math.max(1, Math.ceil(list.length / TREND_WINDOW));
}

function trendUnitWord(count: number): string {
  return `${count} ${trendUnit}${count === 1 ? "" : "s"}`;
}

/** The last day of the period that starts on `start`. */
function trendPeriodEnd(start: string): string {
  if (trendUnit === "week") {
    return addDaysKey(start, 6);
  }
  const first = parseDayKey(start);
  return dayKey(new Date(first.getFullYear(), first.getMonth() + 1, 0));
}

/** "Aug 10 – Nov 1, 2026", "Dec 29, 2025 – Mar 22, 2026"; months: "Mar – Oct 2026". */
function formatTrendRange(startKey: string, endKey: string): string {
  const start = parseDayKey(startKey);
  const end = parseDayKey(endKey);
  const sameYear = start.getFullYear() === end.getFullYear();
  if (trendUnit === "month") {
    const month = (date: Date, year: boolean) => date.toLocaleDateString(undefined, year ? { month: "short", year: "numeric" } : { month: "short" });
    return start.getMonth() === end.getMonth() && sameYear ? month(end, true) : `${month(start, !sameYear)} – ${month(end, true)}`;
  }
  const day = (date: Date, year: boolean) =>
    date.toLocaleDateString(undefined, year ? { month: "short", day: "numeric", year: "numeric" } : { month: "short", day: "numeric" });
  return `${day(start, !sameYear)} – ${day(end, true)}`;
}

/** What one bar or point is, for its tip and screen readers: "Week of Oct 5", "October 2026". */
function trendPeriodName(start: string): string {
  return trendUnit === "week"
    ? `Week of ${formatWeekStart(start)}`
    : parseDayKey(start).toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

/** Axis labels: a week's month once ("Aug 10", "17", "24"), a month's year when it turns. With
 *  `every` 2 only every other period is labelled, counted from the newest, and the month or year
 *  goes on the first label that shows it. */
function trendTicks(shown: TrendPeriod[], every = 1): string[] {
  let previous: Date | null = null;
  return shown.map((period, index) => {
    if ((shown.length - 1 - index) % every !== 0) {
      return "";
    }
    const date = parseDayKey(period.start);
    const before = previous;
    previous = date;
    if (trendUnit === "month") {
      const month = date.toLocaleDateString(undefined, { month: "short" });
      return before && before.getFullYear() !== date.getFullYear() ? `${month} ${date.getFullYear()}` : month;
    }
    return before && before.getMonth() === date.getMonth() ? String(date.getDate()) : formatWeekStart(period.start);
  });
}

function averageOf(values: number[]): number | null {
  return values.length ? values.reduce((sum, value) => sum + value, 0) / values.length : null;
}

/** The median, or null when there is nothing to take it of. */
function trendMedian(values: number[]): number | null {
  return values.length ? medianOf(values) : null;
}

interface TrendPill {
  text: string;
  good: boolean;
}

/** "↑ 42m vs the 12 before", or Steady inside `threshold`; null with nothing to compare. */
function trendPill(
  now: number | null,
  before: number | null,
  count: number,
  threshold: number,
  format: (seconds: number) => string,
  lowerIsBetter: boolean,
): TrendPill | null {
  if (now == null || before == null) {
    return null;
  }
  const versus = `vs the ${count} before`;
  const change = now - before;
  if (Math.abs(change) < threshold) {
    return { text: `Steady ${versus}`, good: true };
  }
  const up = change > 0;
  const amount = format(Math.abs(change));
  const text = lowerIsBetter ? `${amount} ${up ? "slower" : "faster"} ${versus}` : `${up ? "↑" : "↓"} ${amount} ${versus}`;
  return { text, good: lowerIsBetter ? !up : up };
}

function renderTrendsBody(): string {
  const list = trendPeriods?.[trendUnit] ?? [];
  trendPage = Math.min(trendPage, trendPageCount(list) - 1);
  const shown = trendWindow(list, trendPage);
  if (!shown.length) {
    return "";
  }
  const before = trendWindow(list, trendPage + 1);
  // The period in progress is drawn paler and left out of every average.
  const running = list[list.length - 1].start;
  const done = (periods: TrendPeriod[]) => periods.filter((period) => period.start !== running);
  const worked = (periods: TrendPeriod[]) => done(periods).filter((period) => period.session_count > 0);
  const animate = takeAnimation("trends");

  const focusNow = averageOf(done(shown).map((period) => period.focus_seconds));
  const focusBefore = averageOf(done(before).map((period) => period.focus_seconds));
  const total = shown.reduce((sum, period) => sum + period.focus_seconds, 0);
  const sessions = shown.reduce((sum, period) => sum + period.session_count, 0);
  const spanNow = trendMedian(worked(shown).map((period) => period.median_stretch_seconds));
  const spanBefore = trendMedian(worked(before).map((period) => period.median_stretch_seconds));
  const recoveries = (periods: TrendPeriod[]) =>
    done(periods).flatMap((period) => (period.recovery_median_seconds == null ? [] : [period.recovery_median_seconds]));
  const recoveryNow = trendMedian(recoveries(shown));
  // With no finished period yet, the running one stands in, marked "so far".
  const runningPeriod = shown[shown.length - 1].start === running ? shown[shown.length - 1] : null;
  const spanSoFar = spanNow == null && runningPeriod && runningPeriod.session_count > 0 ? runningPeriod.median_stretch_seconds : null;
  const recoverySoFar = recoveryNow == null ? runningPeriod?.recovery_median_seconds ?? null : null;
  const recoveryBefore = trendMedian(recoveries(before));
  const ticks = trendTicks(shown);
  // The half-width line charts fit every other label.
  const lineTicks = trendTicks(shown, shown.length > 6 ? 2 : 1);
  const atFirst = trendPage >= trendPageCount(list) - 1;

  return `
    <div class="panel history-toolbar trends-toolbar">
      <div class="history-week-nav">
        <button type="button" class="history-week-arrow" data-action="trend-older" ${atFirst ? "disabled" : ""} aria-label="Earlier ${trendUnit}s">‹</button>
        <h2>${escapeHtml(formatTrendRange(shown[0].start, trendPeriodEnd(shown[shown.length - 1].start)))}<span>${atFirst ? "From your first session" : trendUnitWord(shown.length)}</span></h2>
        <button type="button" class="history-week-arrow" data-action="trend-newer" ${trendPage === 0 ? "disabled" : ""} aria-label="Later ${trendUnit}s">›</button>
      </div>
      <div class="history-seg" role="group" aria-label="Bars show">
        <button type="button" class="${trendUnit === "week" ? "is-on" : ""}" aria-pressed="${trendUnit === "week"}" data-action="trend-unit" data-unit="week">Weeks</button>
        <button type="button" class="${trendUnit === "month" ? "is-on" : ""}" aria-pressed="${trendUnit === "month"}" data-action="trend-unit" data-unit="month">Months</button>
      </div>
    </div>
    <div class="trends-grid">
      ${renderTrendCard({
        tone: "focus",
        title: "Focused time",
        value: focusNow == null ? formatHours(shown[shown.length - 1].focus_seconds) : formatHours(focusNow),
        suffix: focusNow == null ? "so far" : `a ${trendUnit}`,
        pill: trendPill(focusNow, focusBefore, before.length, 60, formatHours, false),
        line: `${formatHours(total)} total · ${sessions === 1 ? "1 session" : `${sessions} sessions`}`,
        plot: renderTrendBars(shown, running, focusNow, ticks, animate),
      })}
      ${renderTrendCard({
        tone: "span",
        title: "Attention span",
        hint: "typical unbroken stretch of work",
        value: spanNow != null ? formatMinutes(spanNow) : spanSoFar != null ? formatMinutes(spanSoFar) : "—",
        suffix: spanNow != null ? "typical" : spanSoFar != null ? "so far" : "",
        pill: trendPill(spanNow, spanBefore, before.length, 60, formatMinutes, false),
        plot: renderTrendLine(
          shown.map((period) => ({ start: period.start, value: period.session_count > 0 ? period.median_stretch_seconds : null, label: formatMinutes(period.median_stretch_seconds) })),
          running,
          spanNow,
          lineTicks,
          "No stretch of work recorded yet",
        ),
      })}
      ${renderTrendCard({
        tone: "recovery",
        title: "Recovery",
        hint: "time back to work after a distraction · lower is better",
        value: recoveryNow != null ? formatRecovery(recoveryNow) : recoverySoFar != null ? formatRecovery(recoverySoFar) : "—",
        suffix: recoveryNow != null ? "typical" : recoverySoFar != null ? "so far" : "",
        pill: trendPill(recoveryNow, recoveryBefore, before.length, 5, formatRecovery, true),
        plot: renderTrendLine(
          shown.map((period) => ({
            start: period.start,
            value: period.recovery_median_seconds,
            label: period.recovery_median_seconds == null ? "" : formatRecovery(period.recovery_median_seconds),
          })),
          running,
          recoveryNow,
          lineTicks,
          "No distractions yet, so nothing to recover from",
        ),
      })}
    </div>
  `;
}

function renderTrendCard(card: {
  tone: "focus" | "span" | "recovery";
  title: string;
  hint?: string;
  value: string;
  suffix: string;
  pill: TrendPill | null;
  line?: string;
  plot: string;
}): string {
  return `
    <section class="panel trend-card trend-${card.tone}">
      <div class="trend-head">
        <p class="eyebrow">${escapeHtml(card.title)}</p>
        ${card.hint ? `<p class="trend-hint">${escapeHtml(card.hint)}</p>` : ""}
        <div class="trend-figure">
          <p class="trend-value">${escapeHtml(card.value)}${card.suffix ? `<small>${escapeHtml(card.suffix)}</small>` : ""}</p>
          ${card.pill ? `<span class="trend-pill ${card.pill.good ? "is-good" : "is-bad"}">${escapeHtml(card.pill.text)}</span>` : ""}
        </div>
        ${card.line ? `<p class="trend-line">${escapeHtml(card.line)}</p>` : ""}
      </div>
      ${card.plot}
    </section>
  `;
}

/** Focused time, one bar per period, as the week chart in History draws its days: a hover nudges
 *  the bar and its tag rises. A dashed line behind the bars marks the average of the finished
 *  periods. */
function renderTrendBars(shown: TrendPeriod[], running: string, average: number | null, ticks: string[], animate: boolean): string {
  const max = Math.max(...shown.map((period) => period.focus_seconds), 1);
  const columns = `grid-template-columns: repeat(${shown.length}, minmax(0, 1fr))`;
  // Bars widen when there are few and slim down as the window fills: 116px for one, the full
  // column at twelve (never wider than their column).
  const barWidth = Math.round(Math.max(64, 120 - shown.length * 4.5));
  const bars = shown
    .map((period, index) => {
      const on = period.focus_seconds > 0;
      const now = period.start === running;
      const sessions = period.session_count === 1 ? "1 session" : `${period.session_count} sessions`;
      const tip = on ? `${formatHours(period.focus_seconds)}${now ? " so far" : ""} · ${sessions}` : "No sessions";
      return `
        <button type="button" class="recap-bar-col" aria-label="${attr(`${trendPeriodName(period.start)}: ${tip}`)}">
          <span class="recap-bar-track">
            <span class="recap-bar ${on ? "is-on" : ""} ${now ? "is-now" : ""}" style="--i: ${index}; ${on ? `height: ${smartBarHeight(period.focus_seconds, max, 14)}%` : ""}">
              <span class="week-tip">${escapeHtml(tip)}</span>
            </span>
          </span>
        </button>
      `;
    })
    .join("");
  const line =
    average != null && average > 0
      ? `<div class="trend-avg" style="bottom: calc(var(--axis-gap) + var(--track) * ${(smartBarHeight(average, max, 14) / 100).toFixed(3)})"><b>Avg ${escapeHtml(formatHours(average))}</b></div>`
      : "";
  return `
    <div class="trend-bars-plot" style="--bar-w: ${barWidth}px">
      ${line}
      <div class="recap-bars trend-bars ${animate ? "is-animated" : ""}" style="${columns}">${bars}</div>
    </div>
    <div class="trend-ticks ${shown.length > 8 ? "is-crowded" : ""}" style="${columns}" aria-hidden="true">${ticks.map((tick) => `<span>${escapeHtml(tick)}</span>`).join("")}</div>
  `;
}

/** A line through the finished periods with a dashed typical line; the period in progress is a
 *  hollow point off the line, so a part-period never reads as a drop. The scale starts at zero, so
 *  a small change looks small, and each point sits over its label, in the bars' columns. */
function renderTrendLine(
  points: { start: string; value: number | null; label: string }[],
  running: string,
  typical: number | null,
  ticks: string[],
  emptyNote: string,
): string {
  const known = points.flatMap((point, index) => (point.value == null ? [] : [{ ...point, value: point.value, index }]));
  if (!known.length) {
    return `<p class="trend-empty">${escapeHtml(emptyNote)}</p>`;
  }
  const W = 400;
  const H = 120;
  const top = Math.max(...known.map((point) => point.value), typical ?? 0) * 1.2 || 1;
  const x = (index: number) => ((index + 0.5) / points.length) * W;
  // A zero sits 12 units above the axis, so no point ever touches it.
  const y = (value: number) => 8 + (H - 20) * (1 - value / top);
  const finished = known.filter((point) => point.start !== running);
  const path = finished.map((point) => `${x(point.index).toFixed(1)},${y(point.value).toFixed(1)}`).join(" ");
  const dots = known
    .map((point) => {
      const now = point.start === running;
      const tip = `${point.label}${now ? " so far" : ""}`;
      return `
        <span class="trend-point ${now ? "is-now" : ""}" tabindex="0" role="img" aria-label="${attr(`${trendPeriodName(point.start)}: ${tip}`)}" style="left: ${((x(point.index) / W) * 100).toFixed(2)}%; top: ${((y(point.value) / H) * 100).toFixed(2)}%">
          <span class="week-tip">${escapeHtml(tip)}</span>
        </span>
      `;
    })
    .join("");
  return `
    <div class="trend-line-plot">
      <svg viewBox="0 0 ${W} ${H}" preserveAspectRatio="none" aria-hidden="true">
        <line class="trend-baseline" x1="0" x2="${W}" y1="${H}" y2="${H}" />
        ${typical != null ? `<line class="trend-typical" x1="0" x2="${W}" y1="${y(typical).toFixed(1)}" y2="${y(typical).toFixed(1)}" />` : ""}
        ${finished.length > 1 ? `<polyline class="trend-path" points="${path}" />` : ""}
      </svg>
      ${dots}
    </div>
    <div class="trend-ticks" style="grid-template-columns: repeat(${points.length}, minmax(0, 1fr))" aria-hidden="true">${ticks.map((tick) => `<span>${escapeHtml(tick)}</span>`).join("")}</div>
  `;
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
      <section class="state-slab report-slab">
        <h2>${escapeHtml(report.goal)}</h2>
        <p class="report-meta">${escapeHtml(reportSummaryLine(report))}</p>
        <div class="report-bar-head">
          <p class="report-meta report-hint ${report.spans.length ? "" : "is-placeholder"}" ${report.spans.length ? "" : `aria-hidden="true"`}>Hover over the bar to see each app and how long it lasted.</p>
          ${legend ? `<div class="report-inset">${legend}</div>` : ""}
        </div>
        ${renderReportTimeline(report)}
      </section>
      <section class="report-stats" aria-label="Session stats">
          ${reportStat("Duration", formatClockWithUnit(report.duration_seconds), "How long the session ran. Paused time isn't counted.")}
        ${reportStat("Focused", `${report.focus_percent}%`, "Of your time at the desk, the share you were on work. Away and asleep time count for neither side.")}
        ${reportStat("Longest stretch", formatMinutes(report.longest_stretch_seconds), "Your longest run of work without a distraction or stepping away.")}
        ${reportStat("Nudges", String(report.nudge_count), "How many times Supervisor spoke up to pull you back.")}
        ${pomodoroStat(report)}
        ${report.phone_seconds > 0 ? reportStat("Phone", formatClockWithUnit(report.phone_seconds), "How long the webcam saw a phone in your hand.") : ""}
      </section>
      <section class="panel recap-apps">
        <div class="recap-apps-head">
          <div>
            <p class="eyebrow">WHERE IT WENT</p>
            <h2>Apps & sites</h2>
          </div>
          ${
            report.apps.some((app) => editableApp(app))
              ? `<button type="button" data-action="toggle-recap-names" aria-pressed="${editingRecapNames}">${editingRecapNames ? "Done" : "Edit"}</button>`
              : ""
          }
        </div>
        ${
          report.apps.length === 0
            ? `<p class="empty">No app time recorded.</p>`
            : renderAppGroups(report.apps, APP_GROUPS, report.id)
        }
        ${editingRecapNames ? `<p class="quiet">Moving a row changes this session only. Your distractions list stays as it is unless you add to it.</p>` : ""}
      </section>
    </section>
  `;
}

/** Stat tiles turned over to their explanation; cleared when another report opens. */
const flippedStats = new Set<string>();

/** The week card's two tiles turn over like the report's; their keys stay apart from the report's. */
function weekStat(labelText: string, value: string, help: string): string {
  const key = `week:${labelText}`;
  const flipped = flippedStats.has(key);
  return `
    <button class="week-hero-stat ${flipped ? "is-flipped" : ""}" type="button" data-stat-flip="${attr(key)}" aria-pressed="${flipped}">
      <span class="week-hero-stat-face"><span class="eyebrow">${escapeHtml(labelText)}</span><strong>${escapeHtml(value)}</strong></span>
      <span class="week-hero-stat-face week-hero-stat-back">${escapeHtml(help)}</span>
    </button>
  `;
}

/** A stat tile that turns over on click to say, in plain words, what the number is. */
/** "4 × 25 min" for a Pomodoro session, from the event its start wrote ("25/5x4"). */
function pomodoroStat(report: SessionReport): string {
  const plan = report.events.find((event) => event.kind === "pomodoro")?.detail?.match(/^(\d+)\/(\d+)x(\d+)$/);
  if (!plan) {
    return "";
  }
  const rounds = report.events.filter((event) => event.kind === "round_over").length + 1;
  return reportStat("Rounds", `${Math.min(rounds, Number(plan[3]))} of ${plan[3]}`, `Pomodoro: ${plan[1]}-minute rounds with ${plan[2]}-minute breaks. Breaks are not in the timeline or the duration.`);
}

function reportStat(labelText: string, value: string, help: string): string {
  const flipped = flippedStats.has(labelText);
  return `
    <button class="report-stat ${flipped ? "is-flipped" : ""}" type="button" data-stat-flip="${attr(labelText)}" aria-pressed="${flipped}">
      <span class="report-stat-face"><span>${escapeHtml(labelText)}</span><strong>${escapeHtml(value)}</strong></span>
      <span class="report-stat-face report-stat-back">${escapeHtml(help)}</span>
    </button>
  `;
}

/** Pauses are cut out, so the bar is the time the session ran; older sessions recorded before
 *  the gap-free recorder can still run a little past it. */
function timelineSeconds(report: SessionReport): number {
  const spanEnd = report.spans.reduce((end, span) => Math.max(end, span.started_offset_seconds + span.seconds), 0);
  return Math.max(report.duration_seconds, 1, spanEnd);
}

/** One stretch of the report's bar: back-to-back spans of the same kind joined. */
interface TimelineRun {
  state: string;
  from: number;
  to: number;
  labels: string[];
}

/** Suspicious reads as a distraction and lost time as away, so each run is one colour. */
function timelineKind(state: string): string {
  return state === "suspicious" ? "distracted" : state === "unknown" ? "idle" : state;
}

/** Spans joined into runs when back to back and the same kind. Nothing short is hidden: the
 *  bar draws every run at least a few pixels wide. A gap (an unrecorded stretch) is kept. */
function timelineRuns(spans: ReportSpan[]): TimelineRun[] {
  const runs: TimelineRun[] = [];
  for (const span of spans) {
    const from = span.started_offset_seconds;
    const to = from + span.seconds;
    const state = timelineKind(span.state);
    const last = runs[runs.length - 1];
    const touching = last && from - last.to < 2;
    if (touching && last.state === state) {
      last.to = to;
      if (!last.labels.includes(span.label)) {
        last.labels.push(span.label);
      }
    } else {
      runs.push({ state, from, to, labels: [span.label] });
    }
  }
  return runs;
}

const TIMELINE_NAMES: Record<string, string> = { phone: "Phone", idle: "Away" };

/** What a run is called on hover: the apps and sites, and how long. Whether it was work or a
 *  distraction is left to the colour and the legend; Away and Phone have no names of their own. */
function timelineRunLine(run: TimelineRun): string {
  const names = run.labels.length > 3 ? `${run.labels.slice(0, 3).join(", ")} +${run.labels.length - 3}` : run.labels.join(", ");
  const head = run.state === "idle" || run.state === "phone" ? TIMELINE_NAMES[run.state] : names;
  const seconds = run.to - run.from;
  return `${head} · ${seconds < 60 ? `${Math.round(seconds)}s` : formatHours(seconds)}`;
}

function renderReportTimeline(report: SessionReport): string {
  const duration = timelineSeconds(report);
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
  const runs = timelineRuns(report.spans);
  const pct = (seconds: number) => (seconds / duration) * 100;
  // The hover reads each run from these; the lifted run and the tip sit outside the clipped bar.
  const data = runs.map((run) => ({
    from: pct(run.from),
    to: pct(run.to),
    line: timelineRunLine(run),
    when: `${formatDuration(run.from)} to ${formatDuration(run.to)}`,
  }));
  return `
    <div class="timeline-wrap" tabindex="0" aria-label="Session timeline. Use the left and right arrow keys to step through it." data-runs="${attr(JSON.stringify(data))}">
      <div class="timeline-bar" role="img" aria-label="Session timeline">
        ${runs
          .map((run) => `<span class="timeline-span state-${escapeHtml(run.state)}" style="left: ${pct(run.from).toFixed(3)}%; width: max(3px, ${(pct(run.to) - pct(run.from)).toFixed(3)}%)${pct(run.to) - pct(run.from) < 1 ? "; z-index: 1" : ""}"></span>`)
          .join("")}
      </div>
      <b class="timeline-lift" aria-hidden="true"></b>
      <div class="timeline-tip" hidden></div>
    </div>
  `;
}

type TimelineHoverRun = { from: number; to: number; line: string; when: string };

/** Parts of the bar too thin to point at (under 10px) that sit side by side hover as one group. */
const TIMELINE_THIN = 10;

function timelineGroups(runs: TimelineHoverRun[], width: number): number[][] {
  const thin = (index: number) => ((runs[index].to - runs[index].from) / 100) * width < TIMELINE_THIN;
  const groups: number[][] = [];
  runs.forEach((_, index) => {
    const last = groups[groups.length - 1];
    if (last && thin(index) && thin(last[last.length - 1])) {
      last.push(index);
    } else {
      groups.push([index]);
    }
  });
  return groups;
}

function clearTimeline(wrap: HTMLElement): void {
  wrap.querySelector<HTMLElement>(".timeline-lift")!.classList.remove("is-up");
  wrap.querySelector<HTMLElement>(".timeline-tip")!.hidden = true;
  delete wrap.dataset.pick;
}

/** Pops a part, or a group of thin parts, out of the bar and points the tip at it. A group pops
 *  out as a little zoomed-in bar, each part 16px wide, and the tip lists them in order. */
function showTimelineGroup(wrap: HTMLElement, group: number[]): void {
  const bar = wrap.querySelector<HTMLElement>(".timeline-bar")!;
  const lift = wrap.querySelector<HTMLElement>(".timeline-lift")!;
  const tip = wrap.querySelector<HTMLElement>(".timeline-tip")!;
  const runs = JSON.parse(wrap.dataset.runs ?? "[]") as TimelineHoverRun[];
  const spans = bar.querySelectorAll<HTMLElement>(".timeline-span");
  const parts = group.map((index) => runs[index]);
  wrap.dataset.pick = String(group[0]);
  const inner = bar.clientWidth;
  const left = (parts[0].from / 100) * inner;
  const width = Math.max(((parts[parts.length - 1].to - parts[0].from) / 100) * inner, 3);
  const liftWidth = Math.max(width, group.length > 1 ? group.length * 16 : 12);
  const center = bar.offsetLeft + bar.clientLeft + left + width / 2;
  const top = bar.offsetTop + bar.clientTop;
  const colour = (index: number) => getComputedStyle(spans[index]).backgroundColor;
  // A group shows each part as an equal slice, with an ink line between.
  const slice = 100 / group.length;
  lift.style.background =
    group.length === 1
      ? getComputedStyle(spans[group[0]]).background
      : `linear-gradient(90deg, ${group.map((index, n) => `${colour(index)} ${n * slice}% ${(n + 1) * slice}%`).join(", ")})`;
  if (group.length > 1) {
    lift.style.backgroundImage = `repeating-linear-gradient(90deg, transparent 0 calc(${slice}% - 2px), #101010 calc(${slice}% - 2px) ${slice}%), ${lift.style.backgroundImage}`;
  }
  // Coming up from nothing: start flush with the bar, then grow; a popped part slides along.
  if (!lift.classList.contains("is-up")) {
    lift.style.transition = "none";
    lift.style.top = `${top}px`;
    lift.style.height = `${bar.clientHeight}px`;
    lift.style.left = `${center - width / 2}px`;
    lift.style.width = `${width}px`;
    void lift.offsetWidth;
    lift.style.transition = "";
  }
  lift.style.top = `${top - 9}px`;
  lift.style.height = `${bar.clientHeight + 18}px`;
  lift.style.left = `${center - liftWidth / 2 - 6}px`;
  lift.style.width = `${liftWidth + 12}px`;
  lift.classList.add("is-up");
  const from = parts[0].when.split(" to ")[0];
  const to = parts[parts.length - 1].when.split(" to ")[1];
  tip.innerHTML =
    group.length === 1
      ? `<b>${escapeHtml(parts[0].line)}</b><small>${escapeHtml(parts[0].when)}</small>`
      : `${parts.map((part, n) => `<b class="timeline-tip-row"><i style="background: ${colour(group[n])}"></i>${escapeHtml(part.line)}</b>`).join("")}<small>${escapeHtml(from)} to ${escapeHtml(to)}</small>`;
  tip.hidden = false;
  const half = tip.offsetWidth / 2;
  const tipLeft = Math.min(Math.max(center, bar.offsetLeft + half), bar.offsetLeft + bar.offsetWidth - half);
  tip.style.left = `${tipLeft}px`;
  tip.style.setProperty("--caret", `${center - tipLeft + half}px`);
}

/** One hover for the whole bar: the part under the cursor, or a thin one (or a group of thin
 *  ones) within 8px, so even a sliver can be found without aiming at it. */
function hoverTimeline(event: MouseEvent): void {
  const wrap = (event.target as HTMLElement).closest<HTMLElement>(".timeline-wrap[data-runs]");
  document.querySelectorAll<HTMLElement>(".timeline-wrap[data-pick]").forEach((other) => {
    if (other !== wrap) {
      clearTimeline(other);
    }
  });
  const bar = wrap?.querySelector<HTMLElement>(".timeline-bar");
  if (!wrap || !bar) {
    return;
  }
  const box = bar.getBoundingClientRect();
  const x = event.clientX - box.left;
  if (x < -8 || x > box.width + 8) {
    clearTimeline(wrap);
    return;
  }
  const runs = JSON.parse(wrap.dataset.runs ?? "[]") as TimelineHoverRun[];
  const px = (percent: number) => (percent / 100) * box.width;
  const groups = timelineGroups(runs, box.width);
  const reach = (group: number[]) => {
    const from = px(runs[group[0]].from);
    return [from, Math.max(px(runs[group[group.length - 1]].to), from + 3)];
  };
  let pick = groups.find((group) => x >= reach(group)[0] && x < reach(group)[1]) ?? groups[groups.length - 1];
  let nearest = Infinity;
  groups.forEach((group) => {
    const [from, to] = reach(group);
    const distance = x < from ? from - x : x > to ? x - to : 0;
    if ((group.length > 1 || to - from < TIMELINE_THIN) && distance <= 8 && distance < nearest) {
      nearest = distance;
      pick = group;
    }
  });
  showTimelineGroup(wrap, pick);
}

document.addEventListener("mousemove", hoverTimeline);

// Left and Right step through the parts one by one, however thin. Escape lets go of a part,
// and only with none picked does it go on to close the report.
document.addEventListener("keydown", (event) => {
  const wrap = (event.target as HTMLElement).closest?.<HTMLElement>(".timeline-wrap[data-runs]");
  if (!wrap || !["ArrowLeft", "ArrowRight", "Escape"].includes(event.key)) {
    return;
  }
  if (event.key === "Escape") {
    if (wrap.dataset.pick !== undefined) {
      event.stopPropagation();
      clearTimeline(wrap);
    }
    return;
  }
  event.preventDefault();
  const runs = JSON.parse(wrap.dataset.runs ?? "[]") as TimelineHoverRun[];
  const current = wrap.dataset.pick === undefined ? (event.key === "ArrowRight" ? -1 : runs.length) : Number(wrap.dataset.pick);
  const pick = Math.min(Math.max(current + (event.key === "ArrowRight" ? 1 : -1), 0), runs.length - 1);
  showTimelineGroup(wrap, [pick]);
});
document.addEventListener("focusout", (event) => {
  const wrap = (event.target as HTMLElement).closest?.<HTMLElement>(".timeline-wrap[data-pick]");
  if (wrap) {
    clearTimeline(wrap);
  }
});

/** Only the states this session's bar actually shows. */
function renderTimelineLegend(report: SessionReport, unrecorded: boolean): string {
  const present = new Set(report.spans.map((span) => span.state));
  const items = [
    [["focused"], "state-focused", "Work"],
    [["suspicious", "distracted"], "state-distracted", "Distraction"],
    [["phone"], "state-phone", "Phone"],
    // Idle (no input) and device asleep / unknown are one "Away"; hovering a segment says which.
    [["idle", "unknown"], "state-idle", "Away"],
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

/** Every listener for the markup inside `root`: the whole document after a full rebuild, or one
 *  slot after paintSlot, so a partial redraw is wired exactly like a full one. */
function bindEvents(root: ParentNode = document): void {
  // The closing line's pop-up also goes away on a click outside it.
  root.querySelector(".session-over-backdrop")?.addEventListener("click", (event) => {
    if (event.target === event.currentTarget) {
      sessionOver = null;
      paintModalLayer();
    }
  });
  root.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const next = button.dataset.tab as Tab;
      if (next === activeTab) {
        return;
      }
      animateOnArrival(next);
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
      closeWeekPicker();
      if (next === "history") {
        // Fresh data first, then one draw: a second draw mid-rise would snap the bars to full height.
        historyArriving = true;
        void Promise.all([refreshTrends(false), refreshHistoryWeek(false)]).then(() => {
          historyArriving = false;
          if (activeTab === "history") {
            render();
          }
        });
      } else {
        render();
      }
      if (leavingWebcam) {
        void stopWebcamPreview();
      }
    });
  });

  root.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", (event) => {
      // A button inside an action card (a voice card) runs only its own action.
      if ((event.target as Element).closest("[data-action]") !== button || button.getAttribute("aria-disabled") === "true") {
        return;
      }
      void runAction(button.dataset.action || "", button);
    });
    if (button.tagName !== "BUTTON") {
      button.addEventListener("keydown", (event) => {
        if (event.target === button && (event.key === "Enter" || event.key === " ")) {
          event.preventDefault();
          button.click();
        }
      });
    }
  });

  root.querySelectorAll<HTMLButtonElement>("[data-rule-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void handleRuleAction(button);
    });
  });

  root.querySelectorAll<HTMLFormElement>("[data-rule-form]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      void saveRuleForm(form);
    });
  });

  root.querySelector("#rule-value")?.addEventListener("input", (event) => {
    if (ruleEditor) {
      ruleEditor.value = (event.currentTarget as HTMLInputElement).value;
      paintRuleEditorLive();
      scheduleRulePreview();
    }
  });

  root.querySelector("#rule-name")?.addEventListener("input", (event) => {
    if (ruleEditor) {
      ruleEditor.name = (event.currentTarget as HTMLInputElement).value;
      ruleEditor.nameTouched = true;
      const preview = document.getElementById("rule-preview");
      if (preview) {
        preview.innerHTML = renderRulePreview(ruleEditor);
      }
    }
  });

  root.querySelectorAll<HTMLButtonElement>("[data-history-rename]").forEach((button) => {
    button.addEventListener("click", () => {
      const key = button.dataset.historyRename ?? "";
      const kind = button.dataset.renameKind as "app" | "website";
      historyRename = { kind, key, value: kind === "website" ? siteNames[key] ?? "" : appNames[key] ?? "" };
      paintHistoryBody();
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
      paintWeekPicker();
    });
  }
  const searchInput = root.querySelector("#history-search") as HTMLInputElement | null;
  searchInput?.addEventListener("input", () => {
    historySearch = searchInput.value;
    scheduleHistorySearch();
  });
  const renameInput = root.querySelector("#history-rename") as HTMLInputElement | null;
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

  bindPhraseEditor(root);

  root.querySelector("details.overlay-more")?.addEventListener("toggle", (event) => {
    const open = (event.currentTarget as HTMLDetailsElement).open;
    if (open === overlayMoreOpen) {
      return;
    }
    overlayMoreOpen = open;
    lastOverlayFit = "";
    requestAnimationFrame(() => fitOverlayWindow());
  });

  root.querySelector("#overlay-snooze")?.addEventListener("change", (event) => {
    const mute = (event.target as HTMLInputElement).checked;
    void invokeSnapshot(mute ? "snooze_session" : "resume_session");
  });

  bindPreflight(root);
  bindClipsControls(root);
  bindSessionReview(root);
  // The chart on the Trends door: a click in a gap between its bars is a miss, not the door.
  root.querySelectorAll<HTMLElement>(".trend-door .trend-spark").forEach((chart) => {
    chart.addEventListener("click", (event) => {
      if (!(event.target as Element).closest("button")) {
        event.stopPropagation();
      }
    });
  });
  root.querySelectorAll<HTMLButtonElement>("[data-stat-flip]").forEach((button) => {
    button.addEventListener("click", () => {
      const key = button.dataset.statFlip ?? "";
      const flipped = !flippedStats.has(key);
      if (flipped) {
        flippedStats.add(key);
      } else {
        flippedStats.delete(key);
      }
      button.classList.toggle("is-flipped", flipped);
      button.setAttribute("aria-pressed", String(flipped));
    });
  });

  root.querySelector("#default-strictness")?.addEventListener("change", (event) => {
    void updateDefaultStrictness((event.target as HTMLSelectElement).value as Strictness);
  });

  root.querySelector("#status-window-enabled")?.addEventListener("change", (event) => {
    void updateStatusWindowEnabled((event.target as HTMLInputElement).checked);
  });

  root.querySelectorAll<HTMLInputElement>("#webcam-phone-use, #first-run-webcam").forEach((box) => {
    box.addEventListener("change", () => void setWebcamEnabled(box.checked));
  });

  root.querySelector("#webcam-gpu")?.addEventListener("change", (event) => {
    void setWebcamGpu((event.target as HTMLInputElement).checked);
  });

  root.querySelector("#webcam-camera")?.addEventListener("change", (event) => {
    void setWebcamCamera(Number((event.target as HTMLSelectElement).value));
  });

  root.querySelector("#voice-name")?.addEventListener("input", (event) => {
    if (recordedVoice) {
      recordedVoice.name = (event.target as HTMLInputElement).value;
      const canSave = Boolean(recordedVoice.bytes && recordedVoice.name.trim() && voiceBusy === null);
      document.querySelector<HTMLButtonElement>('[data-action="save-recorded-voice"]')?.toggleAttribute("disabled", !canSave);
    }
  });

  // The preview transport lives in the voice dialogs; another slot's paint leaves it playing.
  if (root === document || (root as Element).id === "voice-layer") {
    bindVoicePreview();
  }
  bindVoiceWaveforms(root);
  paintPlayingClip(root);
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
/**
 * Redraws one slot and wires it with the same bindEvents a full render uses; the rest of the page
 * stays as it is (its animations, hover and focus). A slot that isn't on the page has nothing to
 * redraw: the next full render draws it from the same state.
 */
function paintSlot(id: string, html: string): HTMLElement | null {
  const slot = document.getElementById(id);
  if (!slot) {
    return null;
  }
  const ui = captureUiState();
  slot.innerHTML = html;
  bindEvents(slot);
  restoreUiState(ui);
  return slot;
}

function paintPreflightLayer(): void {
  // The name field takes focus when the start screen opens, not on every rebuild after a click.
  const opening = !document.querySelector("#preflight-layer #preflight-goal");
  const layer = paintSlot("preflight-layer", preflightOpen ? renderPreflightModal() : "");
  if (layer && preflightOpen && opening) {
    layer.querySelector<HTMLTextAreaElement>("#preflight-goal")?.focus();
  }
}

/** The confirm boxes and the session-over card, over whatever page is showing. */
function renderModalLayer(): string {
  return renderHistoryConfirm() + renderQuitConfirm() + renderSessionOver() + renderShareModal();
}

function paintModalLayer(): void {
  paintSlot("modal-layer", renderModalLayer());
}

function renderReviewLayer(): string {
  return sessionReview && sessionReview.id === sessionReport?.id ? renderSessionReview(sessionReview) : "";
}

function paintReviewLayer(): void {
  paintSlot("review-layer", renderReviewLayer());
}

function paintSessionRulesLayer(): void {
  paintSlot("session-rules-layer", sessionRulesOpen ? renderSessionRulesModal() : "");
}

/** The History list under its toolbar; anywhere else in History, the whole page. */
function paintHistoryBody(): void {
  if (!paintSlot("history-body", renderHistoryBody())) {
    render();
  }
}

// A click anywhere but the ⋯ menu closes it.
window.addEventListener("pointerdown", (event) => {
  if (toneMenuOpen && !(event.target as Element | null)?.closest(".personality-own")) {
    toneMenuOpen = false;
    paintClipsPanel();
  }
});

/** Whether a mouse or pen is held down, so a rebuild can wait for its click. */
let pointerHeld = false;
window.addEventListener("pointerdown", () => (pointerHeld = true), true);
window.addEventListener("pointerup", () => (pointerHeld = false), true);
window.addEventListener("pointercancel", () => (pointerHeld = false), true);

/**
 * Runs `paint` now, or once the held pointer is released and its click has landed. A field's blur
 * fires on the press, so a rebuild it starts would replace the button before the release and
 * swallow the click.
 */
function afterPointer(paint: () => void): void {
  if (!pointerHeld) {
    paint();
    return;
  }
  const release = () => {
    window.removeEventListener("pointerup", release);
    window.removeEventListener("pointercancel", release);
    window.setTimeout(paint);
  };
  window.addEventListener("pointerup", release);
  window.addEventListener("pointercancel", release);
}

/** The picker and the toolbar button that opens it. */
function paintWeekPicker(): void {
  const oldestMonday = historyWeek?.oldest_day ? mondayKey(historyWeek.oldest_day) : null;
  paintSlot("week-picker-slot", weekPickerOpen ? renderWeekPicker(historyWeekShown(), oldestMonday) : "");
  const label = document.querySelector<HTMLElement>(".history-week-label");
  label?.classList.toggle("is-open", weekPickerOpen);
  label?.setAttribute("aria-expanded", String(weekPickerOpen));
}

/** The rule editor shows on the start screen, in the session's distractions box, or in Settings. */
function paintRuleEditorHost(): void {
  if (firstRunOpen) {
    paintFirstRun();
  } else if (preflightOpen) {
    paintPreflightLayer();
  } else if (sessionRulesOpen) {
    paintSessionRulesLayer();
  } else {
    render();
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
      pomodoro:
        draft.mode === "pomodoro"
          ? {
              work_minutes: POMODORO_RHYTHMS[draft.rhythm].work,
              break_minutes: POMODORO_RHYTHMS[draft.rhythm].rest,
              rounds: draft.rounds,
            }
          : null,
    },
  });
}

async function runAction(action: string, source?: HTMLElement): Promise<void> {
  if (action === "session-mode" || action === "pomodoro-rhythm" || action === "pomodoro-rounds") {
    if (action === "session-mode") {
      draft.mode = source?.dataset.mode === "pomodoro" ? "pomodoro" : "timer";
    } else if (action === "pomodoro-rhythm") {
      draft.rhythm = Math.min(POMODORO_RHYTHMS.length - 1, Math.max(0, Number(source?.dataset.index ?? 0)));
    } else {
      draft.rounds = Math.min(8, Math.max(1, draft.rounds + Number(source?.dataset.step ?? 0)));
    }
    paintPreflightLayer();
    return;
  }

  if (action === "toggle-theme") {
    theme = theme === "dark" ? "light" : "dark";
    localStorage.setItem("supervisor-theme", theme);
    applyTheme();
    // Waves are drawn in the theme's ink; the drawn key includes the theme, so they redraw.
    bindVoiceWaveforms();
    // The share cards are drawn in the theme's colours too.
    if (shareOpen) {
      paintModalLayer();
    }
    const button = document.querySelector(".theme-toggle");
    if (button) {
      button.textContent = theme === "dark" ? "Dark" : "Light";
    }
    return;
  }

  if (action === "toggle-session-exception" && source) {
    const rule = permanentRules.slice().reverse()[Number(source.dataset.ruleIndex)];
    const current = snapshot?.current;
    if (!rule || !current) {
      return;
    }
    const allowed = !current.exceptions.some((exception) => exception.target === rule.target && exception.value === rule.value);
    if (await invokeSnapshot("set_session_exception", { rule, allowed })) {
      showFeedback(allowed ? `${ruleName(rule)} allowed for this session` : `${ruleName(rule)} counts again`);
    }
    return;
  }

  if (action === "nudge-stop" || action === "nudge-replay") {
    void emit("nudge-control", action === "nudge-stop" ? "stop" : "replay");
    return;
  }

  if (action === "dismiss-moment") {
    momentDismissed = momentKey;
    render();
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
      paintPreflightLayer();
    }
    return;
  }

  if (action === "open-session-rules") {
    if (snapshot?.current) {
      sessionRulesOpen = true;
      ruleEditor = null;
      paintSessionRulesLayer();
    }
    return;
  }

  if (action === "close-session-rules") {
    sessionRulesOpen = false;
    ruleEditor = null;
    paintSessionRulesLayer();
    return;
  }

  if (action === "onboard-show") {
    openFirstRun();
    return;
  }

  if (action === "first-run-next" || action === "first-run-back") {
    moveFirstRun(action === "first-run-next" ? 1 : -1);
    return;
  }

  if (action === "first-run-skip" || action === "first-run-close") {
    closeFirstRun();
    return;
  }

  if (action === "first-run-practice") {
    await startPractice();
    return;
  }

  if (action === "first-run-end-practice") {
    await endPractice();
    paintFirstRun();
    return;
  }

  if (action === "tip-hide") {
    writeLocal(`tip.${source?.dataset.tip ?? ""}`, "1");
    render();
    return;
  }

  if (action === "tip-voice") {
    activeTab = "settings";
    await runAction("open-voice-settings");
    return;
  }

  if (action === "tip-webcam") {
    await setWebcamEnabled(true);
    writeLocal("tip.webcam", "1");
    showFeedback("Webcam on. Change it in Settings › Webcam");
    render();
    return;
  }

  if (action === "tip-share") {
    writeLocal("tip.share", "1");
    document.querySelector<HTMLButtonElement>('nav.tabs [data-tab="history"]')?.click();
    return;
  }

  if (action === "copy-card") {
    await copyCard();
    return;
  }

  if (action === "open-share") {
    shareMonth = await fetchShareMonth();
    shareOpen = true;
    paintModalLayer();
    return;
  }

  if (action === "close-share") {
    shareOpen = false;
    paintModalLayer();
    return;
  }

  // Both draw from data already in memory, so the clipboard write stays inside the click.
  if (action === "copy-week-card") {
    const week = weekRecap && weekRecap.start === historyWeekStart ? weekRecap : null;
    const canvas = week ? drawWeekCard(week, true) : null;
    if (canvas) {
      await copyCanvas(canvas);
    }
    return;
  }

  if (action === "copy-month-card") {
    const canvas = shareMonth ? drawMonthCard(shareMonth, true) : null;
    if (canvas) {
      await copyCanvas(canvas);
    }
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

  // Cancel goes back to the session screen, so the calendar pops in again, as on Escape.
  if (action === "close-preflight") {
    preflightOpen = false;
    ruleEditor = null;
    animateOnArrival("session");
    render();
    return;
  }

  if (action === "streak-start") {
    // The start screen, set to the missing minutes as a plain timer. The goal is the user's to
    // type, as with the Start button.
    draft.mode = "timer";
    draft.duration_minutes = streakSessionMinutes(streakMinutesLeft());
    // The start screen lives on the Session tab; from History or Settings, go there first (the
    // tab button carries the leaving steps: voice screen, webcam preview, week picker).
    if (activeTab !== "session") {
      document.querySelector<HTMLButtonElement>('nav.tabs [data-tab="session"]')?.click();
    }
    await runAction("open-preflight");
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
    trendPage = 0;
    pendingAnimations.add("trends");
    await refreshTrends(false);
    await refreshTrendPeriods();
    render();
    return;
  }

  if (action === "trend-unit" || action === "trend-older" || action === "trend-newer") {
    if (action === "trend-unit") {
      const unit = source?.dataset.unit === "month" ? "month" : "week";
      if (unit === trendUnit) {
        return;
      }
      trendUnit = unit;
      trendPage = 0;
    } else {
      const pages = trendPageCount(trendPeriods?.[trendUnit] ?? []);
      trendPage = Math.max(0, Math.min(pages - 1, trendPage + (action === "trend-older" ? 1 : -1)));
    }
    // New numbers rise in, as a new week does in History.
    pendingAnimations.add("trends");
    render();
    return;
  }

  if (action === "close-recap") {
    historyView = "list";
    editingRecapNames = false;
    historyRename = null;
    animateOnArrival("history");
    await refreshHistoryWeek();
    return;
  }

  if (action === "toggle-week-picker") {
    if (weekPickerOpen) {
      closeWeekPicker();
      paintWeekPicker();
      return;
    }
    weekPickerOpen = true;
    pickerMonth = monthKey(historyWeekStart || thisMondayKey());
    paintWeekPicker();
    await loadPickerDays();
    return;
  }

  if (action === "picker-month-prev" || action === "picker-month-next") {
    pickerMonth = addMonthsKey(pickerMonth || monthKey(historyWeekStart || thisMondayKey()), action === "picker-month-prev" ? -1 : 1);
    pickerDays = {};
    paintWeekPicker();
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
    paintHistoryBody();
    return;
  }

  if (action === "toggle-recap-names") {
    editingRecapNames = !editingRecapNames;
    historyRename = null;
    paintHistoryBody();
    return;
  }

  if (action === "move-app" && source) {
    const { sessionId, kind, key, label, to } = source.dataset;
    try {
      sessionReport = await invoke<SessionReport>("reclassify_session_app", {
        id: Number(sessionId),
        kind,
        key,
        distraction: to === "distraction",
      });
      // The session's totals changed: the list, the week, Trends and the day card read them too.
      dayCardKey = "";
      await refreshSnapshotNow();
      await Promise.all([refreshHistoryWeek(false), refreshTrends(false)]);
      const name = label || key;
      showFeedback(to === "distraction" ? `${name} counts as a distraction here` : `${name} counts as work here`);
    } catch (error) {
      showFeedback(errorText(error));
    }
    paintHistoryBody();
    return;
  }

  if (action === "remove-rule-from-report" && source) {
    const { kind, key } = source.dataset;
    const target: AppRule["target"] = kind === "website" ? "website" : "app";
    try {
      const rules = permanentRules.filter((rule) => !(rule.target === target && rule.value.toLowerCase() === (key ?? "").toLowerCase()));
      const saved = await invoke<RulesSaved>("update_rules", { rules });
      permanentRules = saved.rules;
      showFeedback("Taken off your distractions");
    } catch (error) {
      showFeedback(errorText(error));
    }
    paintHistoryBody();
    return;
  }

  if (action === "add-rule-from-report" && source) {
    const { kind, key } = source.dataset;
    const target: AppRule["target"] = kind === "website" ? "website" : "app";
    try {
      const saved = await invoke<RulesSaved>("update_rules", { rules: [...permanentRules, { target, value: key ?? "" }] });
      permanentRules = saved.rules;
      showFeedback("Added to your distractions");
    } catch (error) {
      showFeedback(errorText(error));
    }
    paintHistoryBody();
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
    animateOnArrival("history");
    await refreshHistoryWeek();
    return;
  }

  if (action === "ask-delete-session") {
    const id = Number(source?.dataset.sessionId);
    if (Number.isInteger(id) && id > 0) {
      historyConfirm = { kind: "delete-session", id };
      paintModalLayer();
    }
    return;
  }

  if (action === "ask-delete-all") {
    const bytes = await invoke<number>("get_history_size").catch(() => null);
    historyConfirm = { kind: "delete-all", bytes };
    paintModalLayer();
    return;
  }

  if (action === "cancel-history-confirm") {
    historyConfirm = null;
    paintModalLayer();
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
    // Redraws the voice page itself only if the voices changed under it.
    await refreshVoiceStatus();
    if (voiceInfoId !== voiceId) {
      stopVoicePreview();
    }
    voiceInfoId = voiceId;
    paintVoiceLayer();
    return;
  }

  if (action === "close-voice-info") {
    dismissVoiceInfo();
    paintVoiceLayer();
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
    paintVoiceLayer();
    return;
  }

  if (action === "open-model-info" || action === "close-model-info") {
    modelInfoOpen = action === "open-model-info";
    paintVoiceLayer();
    return;
  }

  if (action === "open-voice-help" || action === "close-voice-help") {
    voiceHelpOpen = action === "open-voice-help";
    paintVoiceLayer();
    return;
  }

  if (action === "use-tone") {
    const tone = source?.dataset.tone;
    if (tone) {
      clearFailedQueue();
      closeToneForms();
      await setVoiceTone(clipsVoice(), tone);
    }
    return;
  }

  if (action === "generate-missing") {
    const voice = clipsVoice();
    const missing = tonePhrases(voiceTone(voice)).filter((phrase) => !clipForPhrase(voice, phrase));
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

  if (action === "new-tone" || action === "cancel-new-tone") {
    const hadForm = toneMenuOpen || toneRename !== null || toneDeleteArmed !== null;
    closeToneForms();
    newToneDraft = action === "new-tone" ? "" : null;
    if (hadForm) {
      paintClipsPanel();
    }
    paintVoiceLayer();
    if (action === "new-tone") {
      document.getElementById("new-tone-input")?.focus();
    }
    return;
  }

  if (action === "toggle-tone-menu") {
    const open = !toneMenuOpen;
    closeToneForms();
    toneMenuOpen = open;
    paintClipsPanel();
    return;
  }

  if (action === "add-line") {
    if (canEditPhrases()) {
      openPhraseEditor(null, voiceTone(clipsVoice()), (source?.dataset.moment as PhraseMoment | undefined) ?? "nudge");
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
      closeToneForms();
      toneRename = { from: tone, value: tone };
      paintClipsPanel();
      (document.getElementById("tone-rename-input") as HTMLInputElement | null)?.select();
    }
    return;
  }

  if (action === "cancel-rename-tone") {
    toneRename = null;
    paintClipsPanel();
    return;
  }

  if (action === "arm-delete-tone" || action === "cancel-delete-tone") {
    const tone = source?.dataset.tone ?? "";
    closeToneForms();
    toneDeleteArmed = action === "arm-delete-tone" && isCustomTone(tone) ? tone : null;
    paintClipsPanel();
    return;
  }

  if (action === "delete-tone") {
    await deleteTone(source?.dataset.tone ?? "");
    return;
  }

  if (action === "select-voice") {
    const voiceId = source?.dataset.voiceId;
    const voice = voiceId ? availableVoices().find((item) => item.id === voiceId) : null;
    if (voiceId && voice?.selectable && voiceId !== draft.voiceId) {
      draft.voiceId = voiceId;
      // The lines below follow the voice picked; No voice leaves them on the last one.
      if (voiceId !== "none") {
        clipsVoiceId = voiceId;
        clipsVoiceTouched = true;
        closeToneForms();
      }
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
      showFeedback("Download the voice engine first");
      return;
    }
    openRecordingDialog();
    return;
  }

  if (action === "ask-delete-model") {
    dismissVoiceInfo();
    modelDeleteOpen = true;
    paintVoiceLayer();
    return;
  }

  if (action === "cancel-delete-model") {
    modelDeleteOpen = false;
    paintVoiceLayer();
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
      openPhraseEditor(source?.dataset.phraseId || null, voiceTone(clipsVoice()), "nudge");
    }
    return;
  }

  if (action === "delete-phrase") {
    const phraseId = source?.dataset.phraseId;
    if (!phraseId || !canEditPhrases() || !phraseEditor) {
      return;
    }
    // The first press asks; the second, on the question, deletes.
    if (!phraseEditor.confirmDelete) {
      phraseEditor.confirmDelete = true;
      paintLinePopupButtons();
      return;
    }
    await deletePhrase(phraseId);
    return;
  }

  if (action === "keep-phrase") {
    if (phraseEditor) {
      phraseEditor.confirmDelete = false;
      paintLinePopupButtons();
    }
    return;
  }

  if (action === "set-up-clips") {
    clearFailedQueue();
    clipsVoiceId = source?.dataset.voiceId ?? clipsVoiceId;
    clipsVoiceTouched = true;
    closeToneForms();
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
    paintVoiceLayer();
    return;
  }

  if (action === "cancel-delete-voice") {
    voiceDeleteTargetId = null;
    paintVoiceLayer();
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

  if (action === "review-save") {
    await saveSessionReview();
    return;
  }
  if (action === "close-session-over") {
    sessionOver = null;
    const ended = history[0];
    if (ended && !snapshot?.current && sessionReport?.id !== ended.id) {
      await openHistoryReport(ended.id, !knownHistoryIds.has(ended.id));
      return;
    }
    paintModalLayer();
    return;
  }
  if (action === "cancel-quit") {
    quitConfirmOpen = false;
    paintModalLayer();
    return;
  }
  if (action === "confirm-quit") {
    await invoke("quit_app");
    return;
  }
  if (action === "review-later") {
    // Nothing saved: the same apps come up after the next session.
    sessionReview = null;
    paintReviewLayer();
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
}

async function handleRuleAction(button: HTMLButtonElement): Promise<void> {
  const action = button.dataset.ruleAction;
  if (!action) {
    return;
  }

  if (action === "add") {
    ruleEditor = { index: null, target: "website", value: "", name: "", nameTouched: false, preview: null };
    paintRuleEditorHost();
    document.getElementById("rule-value")?.focus();
    return;
  }

  if (action === "cancel") {
    ruleEditor = null;
    paintRuleEditorHost();
    return;
  }

  if (action === "keep") {
    if (ruleEditor) {
      ruleEditor.confirmDelete = false;
      paintRuleEditorHost();
    }
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
    paintRuleEditorHost();
    return;
  }

  if (action === "delete") {
    // The first press asks; the second, on the question, deletes.
    if (ruleEditor && !ruleEditor.confirmDelete) {
      ruleEditor.confirmDelete = true;
      paintRuleEditorHost();
      return;
    }
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
    preview.closest("form")?.querySelector('button[type="submit"]')?.classList.toggle("is-quiet", !rulePreviewReady(editor));
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
  afterPointer(paintHistoryBody);
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
  const moment = editor.moment;
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
      customPhrases[index] = { id, tone, moment, text };
    } else {
      phraseOverrides[id] = { id, tone, moment, text };
    }
  } else {
    id = uniqueCustomPhraseId(text);
    customPhrases.push({ id, tone, moment, text });
  }
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  phraseEditor = null;
  paintLines();
  if (regenerate) {
    reopenLineAfterQueue = id;
    if (!(await runClipQueue([id]))) {
      reopenLineAfterQueue = null;
      showFeedback(existing ? "Line saved" : "Line added");
    }
    return;
  }
  const moved = existing && existing.text === text && existing.moment !== moment;
  const label = MOMENTS.find((item) => item.id === moment)?.label ?? "";
  showFeedback(moved ? `Moved to ${label}` : existing ? "Line saved" : "Line added");
}

/** A new personality, empty or with a copy of another's lines, that the viewed voice takes on. */
async function createTone(): Promise<void> {
  const name = (newToneDraft ?? "").trim().slice(0, 80);
  if (!name) {
    showFeedback("Name the personality");
    return;
  }
  if (phraseToneOrder().some((tone) => sameTone(tone, name))) {
    showFeedback(`There is already a ${toneLabel(name)} personality`);
    return;
  }
  const from = (document.getElementById("new-tone-from") as HTMLSelectElement | null)?.value ?? "";
  const backup = libraryBackup();
  customPhraseSets.push(name);
  for (const phrase of from ? tonePhrases(from) : []) {
    customPhrases.push({ id: uniqueCustomPhraseId(phrase.text), tone: name, moment: phrase.moment, text: phrase.text });
  }
  if (!(await persistPhraseLibrary())) {
    restoreLibrary(backup);
    render();
    return;
  }
  closeToneForms();
  await setVoiceTone(clipsVoice(), name);
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
  paintLines();
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
    const next = await invoke<SessionSnapshot>(command, args);
    lastError = "";
    if (applySnapshot(next)) {
      return true;
    }
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
      showFeedback(download.up_to_date ? "Up to date" : "Voice engine ready");
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
      showFeedback(download.up_to_date ? "Up to date" : "Voice engine ready");
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
    title.textContent = download.phase === "cancelling" ? "Cancelling download" : "Downloading the voice engine";
  }
  const copy = document.querySelector(".busy-panel h2 + p");
  if (copy) {
    copy.textContent = download.message || "Downloading the voice engine.";
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

/** `redrawAfter` false: the caller redraws once it has settled what comes after. */
async function runVoiceBusy(kind: Exclude<VoiceBusy, null>, task: () => Promise<void>, redrawAfter = true): Promise<boolean> {
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
    if (redrawAfter) {
      render();
    }
  }
}

function openRecordingDialog(): void {
  dismissVoiceInfo();
  recordingOpen = true;
  recordingState = recordedVoice ? "recorded" : "idle";
  recordingError = "";
  paintVoiceLayer();
}

async function closeRecordingDialog(): Promise<void> {
  await stopVoiceRecording(false);
  await clearVoiceRecording();
  recordingOpen = false;
  recordingState = "idle";
  recordingError = "";
  resetRecordedVoice();
  paintVoiceLayer();
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

  paintVoiceLayer();
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

  paintVoiceLayer();
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
      throw new Error("The recording was not saved. Try recording again.");
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
    showFeedback("Voice saved. Generate its lines below.");
    document.querySelector(".phrase-panel")?.scrollIntoView({ behavior: "smooth", block: "start" });
  } catch (error) {
    recordingError = errorText(error); // the dialog is still open; a banner would sit behind it
    paintVoiceLayer();
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
    showFeedback(lock || "Pick one of your voices under Lines for.");
    return false;
  }
  dismissVoiceInfo();
  clipQueue = { voiceId: voice.id, phraseIds, index: 0, phase: "working", error: "", startedAt: performance.now(), stopAfter: false };
  clipQueueTicker = window.setInterval(paintClipQueue, 1000);
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
  }, false);
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
          ? "Line ready"
          : "Lines ready",
    );
  }
  const reopen = reopenLineAfterQueue;
  reopenLineAfterQueue = null;
  const done = finished && finished.phase === "working" && finished.index >= finished.phraseIds.length;
  const onPage = appView === "main" && activeTab === "settings" && settingsView === "voice";
  const phrase = reopen ? effectivePhrases().find((item) => item.id === reopen) : null;
  if (done && onPage && phrase && !editorIsOpen() && clipForPhrase(clipsVoice(), phrase)) {
    render();
    openPhraseEditor(phrase.id, phrase.tone, phrase.moment);
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
    voiceError = "";
    showFeedback(`${voice.name} speaks like ${toneLabel(tone)}`);
  } catch (error) {
    voiceError = errorText(error);
  }
  // The voice cards say who speaks like what, so the page redraws.
  render();
}

/** Saves the name in Edit voice as the field is left. An empty name puts the old one back. */
async function renameVoice(): Promise<void> {
  const voice = availableVoices().find((item) => item.id === voiceInfoId);
  const input = document.getElementById("voice-rename-input") as HTMLInputElement | null;
  if (!voice?.profile || !input) {
    return;
  }
  const name = input.value.trim().slice(0, 80);
  if (!name || name === voice.name) {
    input.value = voice.name;
    return;
  }
  let saved = false;
  try {
    upsertProfile(await invoke<VoiceProfileSummary>("rename_voice_profile", { profileId: voice.profile.profile_id, displayName: name }));
    voiceError = "";
    saved = true;
  } catch (error) {
    voiceError = errorText(error);
  }
  // Leaving the field by clicking Done or Delete: the redraw waits for that click to land.
  afterPointer(() => {
    render();
    const status = document.querySelector(".voice-rename-status");
    if (status && saved) {
      status.textContent = "Saved";
    }
  });
}

/** Renames a custom personality everywhere it is written: its lines, its set, and the voices that speak like it. */
async function renameTone(): Promise<void> {
  const edit = toneRename;
  if (!edit || !canEditPhrases()) {
    return;
  }
  const next = edit.value.trim().slice(0, 80);
  if (!next) {
    showFeedback("Name the personality");
    return;
  }
  if (sameTone(next, edit.from)) {
    toneRename = null;
    paintLines();
    return;
  }
  if (phraseToneOrder().some((tone) => sameTone(tone, next))) {
    showFeedback(`There is already a ${toneLabel(next)} personality`);
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
  toneRename = null;
  render();
  showFeedback(`Renamed to ${toneLabel(next)}`);
}

/** Removes a custom personality and its lines. Voices that spoke like it fall back to the first. */
async function deleteTone(tone: VoiceTone): Promise<void> {
  toneDeleteArmed = null;
  if (!isCustomTone(tone) || !canEditPhrases()) {
    paintLines();
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

/** Minutes today still needs to count toward the streak (0 once it counts). */
function streakMinutesLeft(): number {
  const seconds = dayFocusMap().get(dayKey(new Date())) ?? 0;
  return Math.max(0, Math.ceil((STREAK_DAY_SECONDS - seconds) / 60));
}

/** The start screen's length for the missing minutes: the slider's steps of 5, from 15. */
function streakSessionMinutes(left: number): number {
  return Math.min(90, Math.max(15, Math.ceil(left / 5) * 5));
}

/** While no session runs: yellow with the minutes today still needs, one click to start them;
 *  green once today counts. */
function renderStreakChip(): string {
  const left = streakMinutesLeft();
  const streak = streakDays();
  if (left === 0) {
    return `<button class="status-chip streak-chip is-kept" type="button" data-tab="session" aria-label="Today counts toward your streak">Day ${streak} kept</button>`;
  }
  const minutes = streakSessionMinutes(left);
  const text = streak > 0 ? `${left} min to keep your ${streak}-day streak` : `${left} min to start a streak`;
  return `<button class="status-chip streak-chip is-short" type="button" data-action="streak-start" title="Opens a ${minutes}-minute focus session">${escapeHtml(text)}</button>`;
}

function statusChip(current: CurrentSession | null): string {
  if (!current) {
    return "STANDBY";
  }

  if (isOnBreak(current)) {
    return "BREAK";
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

  if (isOnBreak(current)) {
    return current.pomodoro?.phase === "break_over" ? "Break over" : "Break";
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
    return "One clear start. Your distractions and voice are set when you start.";
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

  if (isOnBreak(current)) {
    return "state-break";
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
  await playLine(selectedNudgeClip(current.metrics.nudges, current.camera.phone));
}

/** A Pomodoro round ended: a round-over line. A break ending plays nothing: the green card and
 *  its button say it, and the user did nothing to be corrected. Breaks are pauses, so the round
 *  line plays whatever the session's status. */
async function playMoment(phase: string): Promise<void> {
  if (phase === "break_over") {
    return;
  }
  const moments = snapshot?.current?.pomodoro?.moments ?? 1;
  await playMomentLine("roundover", moments - 1);
}

/** A session ended: a closing line, taking turns by the session's id (newest in history). */
async function playSessionOver(): Promise<void> {
  if (Date.now() < practiceQuietUntil) {
    return;
  }
  if ((await playMomentLine("sessionover", history[0]?.id ?? 0)) && lastNudgeVoice?.line) {
    sessionOver = { voice: { ...lastNudgeVoice, speaking: !nudgeAudio?.paused }, session: history[0] ?? null };
    paintModalLayer();
  }
}

/** The `turn`th ready line the voice's personality has for a moment, if any. True if it played. */
async function playMomentLine(moment: PhraseMoment, turn: number): Promise<boolean> {
  if (draft.voiceId === "none") {
    return false;
  }
  const voice = selectedVoice();
  const ready = momentPhrases(voiceTone(voice), moment)
    .map((phrase) => clipForPhrase(voice, phrase))
    .filter((clip): clip is VoiceClipSummary => Boolean(clip));
  return playLine(ready.length ? ready[turn % ready.length] : null);
}

/** The bars of the speaking orb, for a play button while its clip plays. */
const MINI_ORB = `<span class="mini-orb" aria-hidden="true"><i></i><i></i><i></i><i></i></span>`;

/** Plays a line; true if it started. */
/**
 * Every recording and line plays at one level. A voice's recording is about 6 dB louder than
 * its generated lines (measured on the starter voices: speech near -19 dBFS against -25), so the
 * voice card's play button was much louder than a nudge. Each file's speech loudness is measured
 * once (20 ms blocks above -45 dBFS, so pauses don't count) and anything louder than the level
 * is turned down to it; quieter files play as they are. Volume only lowers, so nothing clips.
 */
const PLAY_LEVEL_DB = -25;
const playVolumes = new Map<string, Promise<number>>();

function playVolume(url: string): Promise<number> {
  let volume = playVolumes.get(url);
  if (!volume) {
    volume = measureVolume(url);
    playVolumes.set(url, volume);
  }
  return volume;
}

async function measureVolume(url: string): Promise<number> {
  try {
    const bytes = await (await fetch(url)).arrayBuffer();
    const audio = await new OfflineAudioContext(1, 1, 44_100).decodeAudioData(bytes);
    const samples = audio.getChannelData(0);
    const block = Math.max(1, Math.round(audio.sampleRate * 0.02));
    const floor = 10 ** (-45 / 10);
    let total = 0;
    let blocks = 0;
    for (let start = 0; start + block <= samples.length; start += block) {
      let power = 0;
      for (let index = start; index < start + block; index += 1) {
        power += samples[index] * samples[index];
      }
      power /= block;
      if (power > floor) {
        total += power;
        blocks += 1;
      }
    }
    if (!blocks) {
      return 1;
    }
    const level = 10 * Math.log10(total / blocks);
    return Math.min(1, 10 ** ((PLAY_LEVEL_DB - level) / 20));
  } catch {
    return 1;
  }
}

async function playLine(clip: VoiceClipSummary | null): Promise<boolean> {
  if (!clip) {
    return false;
  }
  try {
    nudgeAudio?.pause();
    const url = clipAssetUrl(clip.path);
    const audio = new Audio(url);
    nudgeAudio = audio;
    audio.preload = "auto";
    audio.volume = await playVolume(url);
    const phrase = effectivePhrases().find((item) => clipBelongsToPhrase(clip, item.id));
    lastNudgeVoice = { voice: selectedVoice().name, line: phrase?.text ?? "" };
    // Only the current line reports; one replaced mid-word stays quiet.
    const ended = () => {
      if (nudgeAudio === audio) {
        announceNudge(false);
      }
    };
    audio.addEventListener("ended", ended);
    audio.addEventListener("pause", ended);
    await audio.play();
    announceNudge(true);
    voiceError = "";
    return true;
  } catch (error) {
    voiceError = `Could not play the line: ${errorText(error)}`;
    render();
    return false;
  }
}

/** Tells the distraction card (another window) whether the voice is speaking, and what. */
function announceNudge(speaking: boolean): void {
  if (sessionOver && sessionOver.voice.speaking !== speaking) {
    sessionOver.voice.speaking = speaking;
    paintModalLayer();
  }
  if (lastNudgeVoice) {
    void emit("nudge-voice", { speaking, ...lastNudgeVoice } satisfies NudgeVoice);
  }
}

/** Back to work lines from the voice's personality (for a phone, its Phone down lines first); any
 *  clip at all only when it has none ready. */
function selectedNudgeClip(nudgeCount: number, phone = false): VoiceClipSummary | null {
  const voice = selectedVoice();
  const readyIn = (moment: PhraseMoment) =>
    momentPhrases(voiceTone(voice), moment)
      .map((phrase) => clipForPhrase(voice, phrase))
      .filter((clip): clip is VoiceClipSummary => Boolean(clip));
  const phoneReady = phone ? readyIn("phone") : [];
  const ready = phoneReady.length > 0 ? phoneReady : readyIn("nudge");
  // No nudge line in this personality: the voice stays quiet, as the Voice page says. A
  // session-over or round-over line is never a nudge.
  if (ready.length === 0) {
    return null;
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

/** The clip a play button started, so that button shows the speaking orb; a second click stops it. */
let playingClipPath: string | null = null;

async function playClip(path: string): Promise<void> {
  if (path === playingClipPath && nudgeAudio && !nudgeAudio.paused) {
    nudgeAudio.pause();
    return;
  }
  try {
    voicePreviewAudio?.pause();
    paintVoicePreview();
    nudgeAudio?.pause();
    const url = clipAssetUrl(path);
    const audio = new Audio(url);
    nudgeAudio = audio;
    audio.preload = "auto";
    audio.volume = await playVolume(url);
    const done = () => {
      if (playingClipPath === path && nudgeAudio === audio) {
        playingClipPath = null;
        paintPlayingClip();
      }
    };
    audio.addEventListener("ended", done);
    audio.addEventListener("pause", done);
    await audio.play();
    playingClipPath = path;
    paintPlayingClip();
    voiceError = "";
  } catch (error) {
    voiceError = `Could not play the line: ${errorText(error)}`;
    render();
  }
}

/** Play buttons follow the clip playing, without a redraw. */
function paintPlayingClip(root: ParentNode = document): void {
  root.querySelectorAll<HTMLButtonElement>("button[data-action='play-clip']").forEach((button) => {
    const on = playingClipPath !== null && button.dataset.clipPath === playingClipPath;
    button.classList.toggle("is-playing", on);
    button.setAttribute("aria-pressed", String(on));
  });
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
  const url = clipAssetUrl(path);
  const audio = new Audio(url);
  audio.preload = "auto";
  // At the same level as the lines (see PLAY_LEVEL_DB); set as soon as it is measured.
  void playVolume(url).then((volume) => {
    audio.volume = volume;
  });
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

function bindVoiceWaveforms(root: ParentNode = document): void {
  root.querySelectorAll<HTMLCanvasElement>("canvas[data-wave-path]").forEach((canvas) => {
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
    voiceError = `Could not play the line: ${errorText(error)}`;
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

function momentPhrases(tone: VoiceTone, moment: PhraseMoment): PhraseDefinition[] {
  return tonePhrases(tone).filter((phrase) => phrase.moment === moment);
}

/** Bundled personalities keep their names; only the user's own can be renamed or deleted. */
function isCustomTone(tone: VoiceTone): boolean {
  return !toneOrder.some((item) => sameTone(item, tone));
}

/** The personality a voice speaks like: its saved one while that exists, else the first. */
function profileTone(profile: VoiceProfileSummary | null): VoiceTone {
  const tones = phraseToneOrder();
  const saved = profile?.tone;
  return (saved && tones.find((tone) => sameTone(tone, saved))) || tones[0] || toneOrder[0];
}

function voiceTone(voice: VoiceOption): VoiceTone {
  return profileTone(voice.profile);
}


/** Every back to work line of the voice's personality has a clip, and it has some at all. The
 *  other moments can come later; a voice without them stays quiet there. */
function profileToneReady(profile: VoiceProfileSummary | null): boolean {
  const phrases = momentPhrases(profileTone(profile), "nudge");
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
        kind: ready > 0 ? "Ready" : hasReference ? "Recorded" : "Incomplete",
        detail: ready > 0
          ? "Its lines are generated and play in sessions."
          : hasReference
            ? "Recorded. Generate its lines before using this voice."
            : "The recording is missing. Delete this voice or record it again.",
        coverage: `${ready}/${phrases.length} lines ready`,
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
  return `Free up some memory first. Lines need about ${need} free; this computer has ${free}.`;
}

function canGenerateClip(voice: VoiceOption): boolean {
  return modelInstalled() && voiceBusy === null && Boolean(voice.profile?.reference_path) && !ramGenerationLocked();
}

/** What stops a line being generated right now, from the live engine and memory checks. */
function clipGenerateLockTitle(voice: VoiceOption): string {
  const engine = modelInstalled();
  const memory = !ramGenerationLocked();
  if (!engine && !memory) {
    return "Download the voice engine and free up some memory first.";
  }
  if (!engine) {
    return "Download the voice engine first.";
  }
  if (!memory) {
    return ramLockMessage();
  }
  if (!voice.profile?.reference_path) {
    return "This voice has no recording to make lines from.";
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
  if (!clipsVoiceTouched && draft.voiceId !== "none" && availableVoices().some((item) => item.id === draft.voiceId)) {
    clipsVoiceId = draft.voiceId;
  }
}

function openPhraseEditor(phraseId: string | null, tone: VoiceTone, moment: PhraseMoment): void {
  const phrase = phraseId ? effectivePhrases().find((item) => item.id === phraseId) : null;
  phraseEditor = {
    phraseId,
    tone: phrase?.tone ?? tone,
    moment: phrase?.moment ?? moment,
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
    moment: phrase.moment,
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
      return "Downloading the voice engine";
    case "delete-model":
      return "Deleting the voice engine";
    case "generate-clips":
      return "Generating a line";
    case "delete-voice":
      return "Deleting voice";
    default:
      return "Working";
  }
}

function voiceBusyCopy(): string {
  if (voiceBusy === "generate-clips") {
    return "About a minute, all on this computer. Generated lines play without the voice engine.";
  }
  if (voiceBusy === "download-model") {
    return "The voice engine is large. Supervisor goes back to using little memory once the line is generated.";
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

/** Every week and month since the first session, for Trends; it pages through them itself. */
async function refreshTrendPeriods(): Promise<void> {
  try {
    const [week, month] = await Promise.all([
      invoke<TrendPeriod[]>("get_trend_periods", { unit: "week" }),
      invoke<TrendPeriod[]>("get_trend_periods", { unit: "month" }),
    ]);
    trendPeriods = { week, month };
  } catch (error) {
    lastError = errorText(error);
  }
}

async function refreshTrends(shouldRender = true): Promise<void> {
  try {
    trends = await invoke<TrendsData>("get_trends");
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }
  trendsLoaded = true;
  // A finished session changes the open Trends page too.
  if (appView === "main" && activeTab === "history" && historyView === "trends") {
    await refreshTrendPeriods();
  }
  if (shouldRender) {
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
    paintHistoryBody();
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
    render();
    return;
  }
  paintHistoryBody();
}

/** Browser storage, which can be absent or throw (private windows, cleared site data). */
function readLocal(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeLocal(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Nothing to do: the report is simply offered again next time.
  }
}

/** The newest session not yet seen, left unseen until its report opens (or the caller decides
 *  it never will). Anything older than it is marked seen: only the latest gets a report. */
function noteNewHistory(items: SessionSummary[]): number | null {
  let newest: number | null = null;
  for (const item of items) {
    if (!knownHistoryIds.has(item.id)) {
      if (newest === null) {
        newest = item.id;
      } else {
        knownHistoryIds.add(item.id);
      }
    }
  }
  return newest;
}

/** `review`: the session just ended, so ask about what it counted as work first. */
async function openHistoryReport(id: number, review = false): Promise<void> {
  if (sessionReport?.id !== id) {
    flippedStats.clear();
  }
  if (settingsView === "webcam") {
    settingsView = "home";
    await stopWebcamPreview();
  }
  try {
    sessionReport = await invoke<SessionReport>("get_session_report", { id });
    if (review) {
      const items = await invoke<RecapApp[]>("review_candidates", { id }).catch(() => []);
      sessionReview = items.length
        ? { id, saving: false, items: items.map((app) => ({ ...app, distraction: false, renaming: false, name: app.label })) }
        : null;
    }
    historyView = "report";
    activeTab = "history";
    lastError = "";
    knownHistoryIds.add(id);
    if (review) {
      writeLocal("review.lastOffered", String(id));
    }
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
    animateOnArrival("history");
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

/** The current streak: consecutive days with 25 focused minutes, ending today or yesterday.
 *  Today doesn't break it while there is still time to count. */
function streakDays(asOf = dayKey(new Date())): number {
  const focus = dayFocusMap();
  const cursor = parseDayKey(asOf);
  let days = 0;
  for (let step = 0; step < 400; step += 1) {
    const key = dayKey(cursor);
    if ((focus.get(key) ?? 0) >= STREAK_DAY_SECONDS) {
      days += 1;
    } else if (!(step === 0 && key === dayKey(new Date()))) {
      break;
    }
    cursor.setDate(cursor.getDate() - 1);
  }
  return days;
}

/**
 * Session and History animate in when you arrive on them (and Session at launch), once: the
 * first draw that shows the calendar or the week's charts plays it, and every later redraw
 * leaves them standing.
 */
const pendingAnimations = new Set<"calendar" | "history" | "trends">(["calendar"]);

function animateOnArrival(tab: Tab): void {
  if (tab === "session") {
    pendingAnimations.add("calendar");
  } else if (tab === "history") {
    pendingAnimations.add("history");
  }
}

function takeAnimation(name: "calendar" | "history" | "trends"): boolean {
  return pendingAnimations.delete(name);
}

function monthLabel(date: Date): string {
  return date.toLocaleDateString(undefined, { month: "long", year: "numeric" });
}

function renderCalendar(): string {
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
    // Hovering a day lifts it and swaps its date for its focused time; the parts wrap onto
    // two lines when the day is narrow.
    const time = date > today ? "" : `<b class="cal-time">${formatHours(seconds).split(" ").map((part) => `<span>${part}</span>`).join("")}</b>`;
    days.push(
      `<span class="${classes.join(" ")}" style="animation-delay: ${index * 12}ms"><span class="cal-num">${date.getDate()}</span>${time}</span>`,
    );
  }
  return `
    <div class="cal-grid ${!trendsLoaded ? "is-waiting" : takeAnimation("calendar") ? "is-animated" : ""}" role="img" aria-label="${attr(monthLabel(today))}, days with focus shaded">
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
      // Only the card: a full rebuild here would cut the calendar's pop-in short.
      if (paintSlot("day-card-slot", renderDayCard(streakDays()))) {
        fitShareStats();
        fitQuote(); // the card's height sets the quote's room
      }
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
    if ((span.started_offset_seconds > previousEnd || span.after_pause) && current > 0) {
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
  const streakText = streakLabel(streakDays(summary.day));
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
    ? { canvas: "#40403c", panel: "#353530", grid: "rgba(255,255,255,0.12)", ink: "#fff8e9", muted: "#ddd5c5", border: "#151512", track: "#4c4b46", shadow: "#0b0b09", frame: "#353530" }
    : { canvas: "#f4ead7", panel: "#fff4cf", grid: "rgba(16,16,16,0.08)", ink: "#101010", muted: "#5d564a", border: "#101010", track: "#fff7df", shadow: "rgba(17,16,14,0.68)", frame: "#fff7df" };
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

/** Width of every shared card; the day and the week card draw at the same size. */
const CARD_WIDTH = 640;
const CARD_PAD = 36;
const CARD_MONO = "ui-monospace, Menlo, Consolas, monospace";
const CARD_SANS = "Inter, system-ui, sans-serif";

/** A card-sized canvas at 2x with the card drawn: hard shadow, the grid paper, the border.
 *  `framed`, for the image that is copied: a solid frame in the theme's cream fills the square
 *  around the card, so no pixel is transparent. Chat apps send images as JPEGs, which turn
 *  transparency white; the card's rounded corners and shadow keep their shape on the frame.
 *  Unframed (the Share popup's previews), only the corners and the shadow's notch are clear. */
function cardCanvas(height: number, framed = false): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } | null {
  const scale = 2;
  const width = CARD_WIDTH;
  const shadow = 8;
  const radius = 12;
  const frame = framed ? 14 : 0;
  const canvas = document.createElement("canvas");
  canvas.width = (width + shadow + frame * 2) * scale;
  canvas.height = (height + shadow + frame * 2) * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) {
    return null;
  }
  ctx.scale(scale, scale);
  const palette = cardPalette();
  if (framed) {
    ctx.fillStyle = palette.frame;
    ctx.fillRect(0, 0, width + shadow + frame * 2, height + shadow + frame * 2);
    ctx.translate(frame, frame);
  }

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
  ctx.textBaseline = "top";
  return { canvas, ctx };
}

/** One stat tile in the app's card language: border, hard shadow, slightly rounded. A smaller
 *  value sits on the same baseline as a full-size one; `sub` is a small line under it. */
function drawCardTile(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, label: string, value: string, sub = ""): void {
  const palette = cardPalette();
  const valueMax = sub ? 26 : 34;
  ctx.fillStyle = palette.shadow;
  ctx.beginPath();
  ctx.roundRect(x + 4, y + 4, w, h, 6);
  ctx.fill();
  ctx.fillStyle = palette.panel;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 6);
  ctx.fill();
  ctx.strokeStyle = palette.border;
  ctx.lineWidth = 3;
  ctx.stroke();
  const inset = sub ? 12 : 16;
  const inner = w - inset * 2;
  ctx.textAlign = "left";
  ctx.fillStyle = palette.muted;
  fitCanvasText(ctx, label, 600, CARD_MONO, 12, 8, inner);
  ctx.fillText(label, x + inset, y + 14);
  ctx.fillStyle = palette.ink;
  const valueSize = fitCanvasText(ctx, value, 800, CARD_SANS, valueMax, 16, inner);
  ctx.fillText(value, x + inset, y + 36 + (valueMax - valueSize));
  if (sub) {
    ctx.fillStyle = palette.muted;
    fitCanvasText(ctx, sub, 500, CARD_MONO, 10, 8, inner);
    ctx.fillText(sub, x + inset, y + 36 + valueMax + 6);
  }
}

/** Puts the canvas on the clipboard as a PNG. The write starts at once, with the PNG still
 *  being encoded: WebKit (the Mac webview) allows a clipboard write only during the click
 *  itself, not after an await, so nothing that calls this may await first. */
async function copyCanvas(canvas: HTMLCanvasElement): Promise<void> {
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

/** Draws the day card to a canvas and puts the PNG on the clipboard, in the current theme's colours. */
async function copyCard(): Promise<void> {
  if (!dayCard) {
    return;
  }
  const summary = summarizeDay(dayCard);
  const streak = streakDays(summary.day);
  const width = CARD_WIDTH;
  const drawn = cardCanvas(316, true);
  if (!drawn) {
    return;
  }
  const { canvas, ctx } = drawn;
  const palette = cardPalette();

  ctx.fillStyle = palette.muted;
  ctx.font = `600 14px ${CARD_MONO}`;
  ctx.fillText("SUPERVISOR", CARD_PAD, 32);
  ctx.textAlign = "right";
  ctx.fillText(cardDate(summary.day).toUpperCase(), width - CARD_PAD, 32);
  ctx.textAlign = "left";

  ctx.fillStyle = palette.ink;
  fitCanvasText(ctx, dayTitle(summary.day), 700, CARD_SANS, 30, 20, width - CARD_PAD * 2 - 170);
  ctx.fillText(dayTitle(summary.day), CARD_PAD, 62);

  const stripX = CARD_PAD;
  const stripY = 112;
  const stripW = width - CARD_PAD * 2;
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
    ["STREAK", streakLabel(streak)],
  ];
  const tileGap = 12;
  const tileW = (width - CARD_PAD * 2 - tileGap * 2) / 3;
  stats.forEach(([label, value], index) => drawCardTile(ctx, CARD_PAD + index * (tileW + tileGap), 186, tileW, 94, label, value));

  await copyCanvas(canvas);
}

/** The week's bars are green for any focused time. On the month's calendar a day is green once
 *  it counts (25 focused minutes), as on the Session calendar; less is white. */
const CARD_GREEN = "#a8eb37";
/** Dark text on the greens, whichever theme the card is drawn in. */
const CARD_GREEN_INK = "#101010";
const CARD_INSET = 18;

/** The header both share cards start with: the wordmark over the focused total on the left,
 *  the eyebrow over the period on the right, hanging from the same baseline. Returns the y
 *  where the body starts. */
function drawCardHeader(ctx: CanvasRenderingContext2D, eyebrow: string, period: string, focusSeconds: number): number {
  const palette = cardPalette();
  const width = CARD_WIDTH;
  ctx.textAlign = "left";
  ctx.fillStyle = palette.ink;
  ctx.font = `750 24px ${CARD_SANS}`;
  ctx.fillText("Supervisor", CARD_PAD, CARD_PAD);

  const bigTop = CARD_PAD + 42;
  const bigSize = 40;
  const big = formatHours(focusSeconds);
  ctx.font = `800 ${bigSize}px ${CARD_SANS}`;
  ctx.fillText(big, CARD_PAD, bigTop);
  const bigW = ctx.measureText(big).width;
  ctx.fillStyle = palette.muted;
  ctx.font = `600 14px ${CARD_MONO}`;
  ctx.fillText("FOCUSED", CARD_PAD + bigW + 10, bigTop + bigSize - 21);

  const bottom = bigTop + bigSize;
  ctx.textAlign = "right";
  ctx.fillStyle = palette.ink;
  const periodSize = fitCanvasText(ctx, period, 650, CARD_SANS, 28, 18, width - CARD_PAD * 2 - bigW - 120);
  ctx.fillText(period, width - CARD_PAD, bottom - periodSize);
  ctx.fillStyle = palette.muted;
  ctx.font = `600 15px ${CARD_MONO}`;
  ctx.fillText(eyebrow.toUpperCase(), width - CARD_PAD, bottom - periodSize - 24);
  ctx.textAlign = "left";
  return bottom + 18;
}

/** An inner panel in the card's language: hard shadow, fill, border. */
function drawCardPanel(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number): void {
  const palette = cardPalette();
  ctx.fillStyle = palette.shadow;
  ctx.beginPath();
  ctx.roundRect(x + 4, y + 4, w, h, 8);
  ctx.fill();
  ctx.fillStyle = palette.panel;
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, 8);
  ctx.fill();
  ctx.strokeStyle = palette.border;
  ctx.lineWidth = 3;
  ctx.stroke();
}

/** The three tiles under a card's panel; returns the card's full height. */
function drawCardTiles(ctx: CanvasRenderingContext2D, y: number, sessions: number, longestSeconds: number, bestStreak: number): number {
  const gap = 12;
  const tileW = (CARD_WIDTH - CARD_PAD * 2 - gap * 2) / 3;
  const tileH = 84;
  const tiles: [string, string][] = [
    ["SESSIONS", String(sessions)],
    ["LONGEST", formatMinutes(longestSeconds)],
    ["BEST STREAK", streakLabel(bestStreak)],
  ];
  tiles.forEach(([label, value], index) => drawCardTile(ctx, CARD_PAD + index * (tileW + gap), y, tileW, tileH, label, value));
  return y + tileH + 4 + CARD_PAD;
}

/** The longest run of days in a row with 25 focused minutes, within the card's own days. */
function bestStreakIn(daySeconds: number[]): number {
  let best = 0;
  let run = 0;
  for (const seconds of daySeconds) {
    run = seconds >= STREAK_DAY_SECONDS ? run + 1 : 0;
    best = Math.max(best, run);
  }
  return best;
}

/** The week as a card: the header, a bar per day in its own panel, three tiles. Drawn from the
 *  recap already loaded, so a copy can start inside the click (see copyCanvas). */
function drawWeekCard(recap: WeekRecap, framed = false): HTMLCanvasElement | null {
  const barsH = 200;
  const panelH = CARD_INSET + barsH + 10 + 14 + CARD_INSET;
  const bodyTop = CARD_PAD + 42 + 40 + 18;
  const height = bodyTop + panelH + 18 + 84 + 4 + CARD_PAD;
  const drawn = cardCanvas(height, framed);
  if (!drawn) {
    return null;
  }
  const { canvas, ctx } = drawn;
  const palette = cardPalette();
  const today = dayKey(new Date());
  const year = parseDayKey(recap.start).getFullYear();
  const top = drawCardHeader(ctx, "Your week", `${formatWeekRange(recap.start)}, ${year}`, recap.focus_seconds);

  const panelW = CARD_WIDTH - CARD_PAD * 2;
  drawCardPanel(ctx, CARD_PAD, top, panelW, panelH);
  const gap = 12;
  const colW = (panelW - CARD_INSET * 2 - gap * 6) / 7;
  const barsTop = top + CARD_INSET;
  const max = Math.max(...recap.buckets.map((bucket) => bucket.focus_seconds), 1);
  recap.buckets.forEach((bucket, index) => {
    const x = CARD_PAD + CARD_INSET + index * (colW + gap);
    const day = addDaysKey(recap.start, index);
    const future = day > today;
    const on = bucket.focus_seconds > 0;
    const h = on ? Math.max(14, (barsH * smartBarHeight(bucket.focus_seconds, max, 14)) / 100) : 10;
    const y = barsTop + barsH - h;
    if (on) {
      ctx.fillStyle = palette.shadow;
      ctx.beginPath();
      ctx.roundRect(x + 3, y + 3, colW, h, 6);
      ctx.fill();
    }
    ctx.fillStyle = on ? CARD_GREEN : future ? "transparent" : palette.track;
    ctx.beginPath();
    ctx.roundRect(x, y, colW, h, 6);
    ctx.fill();
    ctx.strokeStyle = future ? palette.grid : palette.border;
    ctx.lineWidth = on ? 3 : 2;
    ctx.stroke();
    ctx.fillStyle = palette.muted;
    ctx.font = `500 13px ${CARD_MONO}`;
    ctx.textAlign = "center";
    ctx.fillText(bucket.label.toUpperCase(), x + colW / 2, barsTop + barsH + 12);
    ctx.textAlign = "left";
  });

  drawCardTiles(ctx, top + panelH + 18, recap.session_count, recap.longest_stretch_seconds, bestStreakIn(recap.buckets.map((bucket) => bucket.focus_seconds)));
  return canvas;
}

/** The month as a card: the header, a calendar shaded by focused time in its own panel, three
 *  tiles. Days still ahead are faint outlines. */
function drawMonthCard(recap: MonthRecap, framed = false): HTMLCanvasElement | null {
  const panelW = CARD_WIDTH - CARD_PAD * 2;
  const gap = 8;
  const cell = (panelW - CARD_INSET * 2 - gap * 6) / 7;
  const first = parseDayKey(recap.start);
  const offset = (first.getDay() + 6) % 7;
  const rows = Math.ceil((offset + recap.days.length) / 7);
  const lettersH = 22;
  const panelH = CARD_INSET + lettersH + rows * (cell + gap) - gap + CARD_INSET;
  const bodyTop = CARD_PAD + 42 + 40 + 18;
  const height = bodyTop + panelH + 18 + 84 + 4 + CARD_PAD;
  const drawn = cardCanvas(height, framed);
  if (!drawn) {
    return null;
  }
  const { canvas, ctx } = drawn;
  const palette = cardPalette();
  const today = dayKey(new Date());
  const top = drawCardHeader(ctx, "Your month", first.toLocaleDateString(undefined, { month: "long", year: "numeric" }), recap.focus_seconds);

  drawCardPanel(ctx, CARD_PAD, top, panelW, panelH);
  const gridX = CARD_PAD + CARD_INSET;
  const gridY = top + CARD_INSET;
  ctx.fillStyle = palette.muted;
  ctx.font = `500 13px ${CARD_MONO}`;
  ctx.textAlign = "center";
  ["M", "T", "W", "T", "F", "S", "S"].forEach((letter, index) => ctx.fillText(letter, gridX + index * (cell + gap) + cell / 2, gridY));
  ctx.textAlign = "left";
  recap.days.forEach((day, index) => {
    const slot = offset + index;
    const x = gridX + (slot % 7) * (cell + gap);
    const y = gridY + lettersH + Math.floor(slot / 7) * (cell + gap);
    const future = day.label > today;
    const counts = day.focus_seconds >= STREAK_DAY_SECONDS;
    ctx.fillStyle = counts ? CARD_GREEN : future ? "transparent" : palette.track;
    ctx.beginPath();
    ctx.roundRect(x, y, cell, cell, 6);
    ctx.fill();
    ctx.strokeStyle = future ? palette.grid : palette.border;
    ctx.lineWidth = 2;
    ctx.stroke();
    ctx.fillStyle = counts ? CARD_GREEN_INK : future ? palette.muted : palette.ink;
    ctx.font = `600 13px ${CARD_MONO}`;
    ctx.fillText(String(index + 1), x + 8, y + 7);
  });

  drawCardTiles(ctx, top + panelH + 18, recap.session_count, recap.longest_stretch_seconds, bestStreakIn(recap.days.map((day) => day.focus_seconds)));
  return canvas;
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
