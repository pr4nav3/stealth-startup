import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";

type PermissionState = "granted" | "missing" | "not_required";
type RuleClassification = "focused" | "neutral" | "distracting";
type RuleTarget = "app_name" | "process_path" | "window_title";
type RuleScope = "settings" | "preflight";
type Strictness = "gentle" | "normal" | "strict";
type SessionStatus = "idle" | "active" | "paused" | "snoozed" | "complete";
type FocusState = "focused" | "neutral" | "suspicious" | "distracted" | "idle";
type Tab = "session" | "settings" | "history";
type AppView = "main" | "overlay";
type SettingsView = "home" | "voice" | "webcam";
type WebcamGaze = "at_pc" | "away_from_pc" | "at_phone";
type WebcamReaction = "alarm" | "voice";
type VoiceTone = string;
type VoiceSource = "none" | "starter" | "custom";
type VoiceBusy = "download-model" | "delete-model" | "generate-clips" | "delete-voice" | null;
type ClipGeneratePhase = "working" | "ready" | "error";
type ClipGenerate = {
  phraseId: string;
  profileId: string;
  phase: ClipGeneratePhase;
  error: string;
};
type VoiceDownloadPhase = "idle" | "downloading" | "cancelling" | "complete" | "error";
type RecordingState = "idle" | "recording" | "recorded" | "error";

interface Bounds {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface CapturePermissions {
  screen_recording: PermissionState;
}

interface ActivitySample {
  captured_at: string;
  platform: string;
  app_name: string;
  process_path: string;
  process_id: number;
  window_title: string | null;
  window_id: string;
  bounds: Bounds;
  idle_seconds: number | null;
  permissions: CapturePermissions;
}

interface AppRule {
  target: RuleTarget;
  value: string;
  classification: RuleClassification;
}

interface FocusSnapshot {
  sample: ActivitySample;
  state: FocusState;
  classification: RuleClassification;
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
}

interface SessionMetrics {
  focused_seconds: number;
  neutral_seconds: number;
  suspicious_seconds: number;
  distracted_seconds: number;
  idle_seconds: number;
  nudges: number;
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
  rules: AppRule[];
}

interface SessionSummary {
  id: number;
  goal: string;
  started_at: string;
  duration_seconds: number;
  focus_seconds: number;
  distracted_seconds: number;
  idle_seconds: number;
  nudges: number;
  top_hints: string[];
}

interface ReportSpan {
  started_offset_seconds: number;
  seconds: number;
  app: string;
  context: string | null;
  category: string;
  state: string;
}

interface CategoryTotal {
  category: string;
  seconds: number;
}

interface DriftEntry {
  at_offset_seconds: number;
  label: string;
  seconds: number;
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
  category_totals: CategoryTotal[];
  top_apps: string[];
  top_drifts: DriftEntry[];
  events: ReportEvent[];
}

interface TrendSession {
  id: number;
  started_at: string;
  median_stretch_seconds: number;
  focus_percent: number;
}

interface DailyFocus {
  date: string;
  focus_percent: number;
  focus_seconds: number;
  session_count: number;
}

interface WeeklyStretch {
  week_start: string;
  median_stretch_seconds: number;
  session_count: number;
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
  sessions: TrendSession[];
  daily_focus: DailyFocus[];
  weekly_stretch: WeeklyStretch[];
  week_recap: FocusRecap;
  month_recap: FocusRecap;
}

type HistoryView = "list" | "report" | "recap";

interface RecapBucket {
  label: string;
  focus_seconds: number;
}

interface RecapApp {
  group: string;
  label: string;
  seconds: number;
}

interface PeriodRecap {
  kind: "week" | "month";
  start: string;
  end: string;
  focus_seconds: number;
  session_count: number;
  buckets: RecapBucket[];
  median_stretch_seconds: number;
  longest_stretch_seconds: number;
  stretch_range_low: number | null;
  stretch_range_high: number | null;
  first_half_focus_percent: number | null;
  second_half_focus_percent: number | null;
  apps: RecapApp[];
}

interface HistoryConfirm {
  kind: "delete-all" | "scrub" | "delete-session";
  id?: number;
}

const REPORT_CATEGORIES = ["work", "comms", "browse", "media", "offscreen"] as const;

interface SessionSnapshot {
  status: SessionStatus;
  settings: SessionSettings;
  current: CurrentSession | null;
  history: SessionSummary[];
}

interface RuleEditor {
  scope: RuleScope;
  index: number | null;
  target: RuleTarget;
  classification: RuleClassification;
  value: string;
}

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
window.addEventListener("keydown", (event) => {
  if (event.key === "Escape") {
    closeTopLayer();
  }
});

const starterRules: AppRule[] = ["YouTube", "Netflix", "TikTok", "Instagram", "Reddit"].map((value) => ({
  target: "window_title",
  value,
  classification: "distracting",
}));

const toneOrder: VoiceTone[] = ["gentle", "mom", "stern", "kind-reset"];

const phraseBank: PhraseDefinition[] = [
  { id: "gentle_deep_breath", tone: "gentle", text: "Deep breath. Back to the thing." },
  { id: "gentle_drifted_a_little", tone: "gentle", text: "You drifted a little. Come back." },
  { id: "gentle_one_next_action", tone: "gentle", text: "One next action. That is all." },
  { id: "gentle_soft_reset", tone: "gentle", text: "Soft reset. Look at the task again." },
  { id: "gentle_back_to_plan", tone: "gentle", text: "No hurry. Just back to the plan." },
  { id: "mom_beta_focus", tone: "mom", text: "Beta, focus. You opened this for what?" },
  { id: "mom_enough_scrolling", tone: "mom", text: "Enough scrolling. Finish the thing first." },
  { id: "mom_wander_later", tone: "mom", text: "You can wander later. Work now." },
  { id: "mom_arre_come_back", tone: "mom", text: "Arre, come back. One tab, one task." },
  { id: "mom_what_was_the_goal", tone: "mom", text: "What was the goal? Say it, then do it." },
  { id: "stern_not_the_task", tone: "stern", text: "This is not the task." },
  { id: "stern_close_it", tone: "stern", text: "Close it and return." },
  { id: "stern_you_said_it_mattered", tone: "stern", text: "You said this mattered. Act like it." },
  { id: "stern_decide_now", tone: "stern", text: "Decide now. Work or stop pretending." },
  { id: "stern_back_to_work", tone: "stern", text: "Back to work. No more bargaining." },
  { id: "kind_reset_pause_inhale", tone: "kind-reset", text: "Pause. Inhale. Now back to the file." },
  { id: "kind_reset_no_drama", tone: "kind-reset", text: "No drama. Just return." },
  { id: "kind_reset_not_failing", tone: "kind-reset", text: "You are not failing. You are drifting. Come back." },
  { id: "kind_reset_small_next_step", tone: "kind-reset", text: "Small next step. That is enough." },
  { id: "kind_reset_begin_again", tone: "kind-reset", text: "Begin again. The work is still here." },
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
};
let history: SessionSummary[] = [];
const EMPTY_RECAP: FocusRecap = { start: "", end: "", focus_seconds: 0, session_count: 0 };
let trends: TrendsData = {
  sessions: [],
  daily_focus: [],
  weekly_stretch: [],
  week_recap: EMPTY_RECAP,
  month_recap: EMPTY_RECAP,
};
let historyWeekStart = "";
let historyWeek: HistoryWeek | null = null;
let sessionReport: SessionReport | null = null;
let periodRecap: PeriodRecap | null = null;
let historyView: HistoryView = "list";
let historyConfirm: HistoryConfirm | null = null;
let historyHydrated = false;
const knownHistoryIds = new Set<number>();
let permanentRules = cloneRules(starterRules);
let preflightOpen = false;
let preflightRules = cloneRules(starterRules);
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
let customPhrases: PhraseDefinition[] = [];
let customPhraseSets: string[] = [];
let phraseOverrides: Record<string, PhraseDefinition> = {};
let phraseEditor: PhraseEditor | null = null;
let phraseSetPickerOpen = false;
let newPhraseSetName = "";
const openPhraseGroups = new Set<string>();
let clipsVoiceId = "starter-female";
let clipsVoiceTouched = false;
let readyClipsOnly = false;
let clipGenerate: ClipGenerate | null = null;
let clipGenerateTickId: number | null = null;
let clipGenerateStatusIndex = 0;
let createVoiceOpen = true;
let modelDeleteOpen = false;
let voiceBackend: VoiceBackendStatus | null = null;
let voiceDownload: VoiceDownloadSnapshot | null = null;
let voiceBusy: VoiceBusy = null;
let voiceDeleteTargetId: string | null = null;
let voiceInfoId: string | null = null;
let voicePreviewAudio: HTMLAudioElement | null = null;
const waveformPeaks = new Map<string, number[]>();
const waveformBuffers = new Map<string, AudioBuffer>();
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
let downloadPollId: number | null = null;
let nudgeAudio: HTMLAudioElement | null = null;
const playedNudgesBySession = new Map<number, number>();
const pendingNudgesBySession = new Map<number, number>();
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
    const [loadedSettings, loadedHistory, loadedRules, loadedPhraseLibrary] = await Promise.all([
      invoke<SessionSettings>("get_settings"),
      invoke<SessionSummary[]>("get_history"),
      invoke<AppRule[]>("get_rules"),
      invoke<PhraseLibraryState>("get_phrase_library"),
    ]);
    settings = loadedSettings;
    history = loadedHistory;
    permanentRules = loadedRules;
    customPhraseSets = loadedPhraseLibrary.custom_tones;
    customPhrases = loadedPhraseLibrary.custom_phrases;
    phraseOverrides = Object.fromEntries(
      loadedPhraseLibrary.phrase_overrides.map((phrase) => [phrase.id, phrase]),
    );
    history.forEach((item) => knownHistoryIds.add(item.id));
    draft.strictness = loadedSettings.default_strictness;
    draft.voiceId = normalizeStoredVoiceId(loadedSettings.voice_profile);
  } catch (error) {
    lastError = errorText(error);
  }

  if (appView === "main") {
    await listen<WebcamFrame>("webcam-frame", (event) => {
      applyWebcamFrame(event.payload);
    });
  }
  await refreshSnapshot();
  historyHydrated = true;

  if (appView === "main") {
    void refreshTrends(false);
    void refreshVoiceStatus(false);
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
    if (download.phase === "downloading" || download.phase === "cancelling") {
      voiceBusy = "download-model";
      startDownloadPolling();
    }
  } catch {
    voiceDownload = null;
  }

  if (shouldRender) {
    render();
  }
}

async function refreshSnapshot(): Promise<void> {
  try {
    snapshot = await invoke<SessionSnapshot>("get_session_snapshot");
    settings = snapshot.settings;
    history = snapshot.history;
    if (!snapshot.current) {
      sessionRulesOpen = false;
      if (ruleEditor?.scope === "preflight") {
        ruleEditor = null;
      }
    }
    const newest = noteNewHistory(history);
    if (appView === "main") {
      ensureSelectedVoice();
      void playNudgeAudioIfNeeded(snapshot.current);
      if (historyHydrated && newest && !snapshot.current) {
        void openHistoryReport(newest);
        return;
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
    if (editorIsOpen() || viewKey() === paintedViewKey) {
      paintClock(current);
    } else {
      render();
    }
  }
  reconcilePolling();
}

function closeTopLayer(): void {
  if (voiceBusy === "download-model" && voiceDownload?.phase === "complete") {
    dismissDownloadModal();
    return;
  }
  if (clipGenerate?.phase === "working") {
    return;
  }
  if (clipGenerate) {
    stopClipGenerateStatus();
    clipGenerate = null;
    render();
    return;
  }
  if (voiceBusy) {
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
  if (phraseSetPickerOpen) {
    phraseSetPickerOpen = false;
    newPhraseSetName = "";
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
  if (preflightOpen) {
    preflightOpen = false;
    ruleEditor = null;
    render();
    return;
  }
  if (historyView === "report" || historyView === "recap") {
    historyView = "list";
    sessionReport = null;
    periodRecap = null;
    historyConfirm = null;
    void refreshHistoryWeek();
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
    phraseEditor !== null ||
    phraseSetPickerOpen ||
    recordingOpen ||
    historyConfirm !== null ||
    voiceDeleteTargetId !== null ||
    voiceInfoId !== null ||
    clipGenerate !== null ||
    modelDeleteOpen ||
    voiceBusy !== null
  );
}

function sessionViewKey(current: CurrentSession | null): string {
  if (!current) {
    return "";
  }
  const focus = current.focus;
  const sample = focus?.sample;
  const rules = current.rules
    .map((rule) => `${rule.target}\t${rule.classification}\t${rule.value}`)
    .join("\n");
  return [
    current.id,
    current.status,
    current.goal,
    current.duration_seconds,
    current.strictness,
    current.snoozed_until_seconds ?? "",
    focus?.state ?? "",
    focus?.classification ?? "",
    focus?.should_nudge ? 1 : 0,
    focus?.matched_rule?.value ?? "",
    sample?.app_name ?? "",
    sample?.window_title ?? "",
    current.metrics.nudges,
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
    periodRecap?.kind ?? "",
    periodRecap?.start ?? "",
    sessionViewKey(current),
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
    historyWeekStart,
    historyWeek?.sessions.length ?? "",
    sessionReport?.id ?? "",
  ].join("\u001f");
}

function paintClock(current: CurrentSession | null): void {
  if (!current) {
    return;
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
          <div class="status-chip" aria-label="Session status">${escapeHtml(current ? statusChip(current) : "READY")}</div>
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
      ${renderVoiceDialogs()}
      ${renderHistoryConfirm()}
    </main>
  `;

  bindEvents();
  fitVoiceCardNames();
  if (document.fonts.status !== "loaded") {
    void document.fonts.ready.then(() => {
      if (document.querySelector(".voice-card")) {
        fitVoiceCardNames();
      }
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
    const note = dailyNote();
    const week = weekSummary();
    return `
      <section class="home-ready">
        <div class="home-grid">
          <article class="quote-card ${quoteFit(note.text)}">
            <p class="eyebrow">Daily note · ${escapeHtml(note.date)}</p>
            <blockquote>“${escapeHtml(note.text)}”</blockquote>
            <p class="quote-by">— ${escapeHtml(note.by)}</p>
          </article>
          <div class="home-side">
            <article class="panel local-card">
              <p class="eyebrow">Privacy first</p>
              <p>Everything stays on this device.</p>
            </article>
            <article class="panel week-card">
              <p class="eyebrow">This week</p>
              <p class="week-total">${escapeHtml(week.total)} focused</p>
              <p class="week-caption">${escapeHtml(week.caption)}</p>
              <div class="week-bars">
                ${week.days
                  .map(
                    (day) => `
                      <div class="week-col" role="img" aria-label="${attr(day.name)}, ${Math.max(0, Math.round(day.seconds / 60))}m focused">
                        <span class="week-bar-track">
                          <span class="week-bar ${day.seconds > 0 ? "is-on" : ""}" style="height: ${barHeight(day.seconds, week.max)}%">
                            <span class="week-tip">${Math.max(0, Math.round(day.seconds / 60))}m</span>
                          </span>
                        </span>
                        <span class="week-letter">${escapeHtml(day.letter)}</span>
                      </div>
                    `,
                  )
                  .join("")}
              </div>
            </article>
          </div>
        </div>
        ${renderStartAction()}
      </section>
      ${preflightOpen ? renderPreflightModal() : ""}
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
          <p class="eyebrow">This session</p>
          <p class="eyebrow">What counts</p>
          ${renderSessionRuleSummary(current.rules)}
          <button type="button" data-action="open-session-rules">Edit session rules</button>
        </aside>
      </div>

      ${renderActiveSessionControls(current)}
    </section>
    ${sessionRulesOpen ? renderSessionRulesModal() : ""}
  `;
}

function renderSessionRuleSummary(rules: AppRule[]): string {
  const relevant = rules.filter((rule) => rule.classification === "focused");
  const distracting = rules.filter((rule) => rule.classification === "distracting");
  return `
    <div class="session-rule-summary">
      ${renderSessionRuleSet("Relevant", "focused", relevant)}
      ${renderSessionRuleSet("Distracting", "distracting", distracting)}
    </div>
  `;
}

function renderSessionRuleSet(
  title: string,
  classification: RuleClassification,
  rules: AppRule[],
): string {
  const shown = rules.slice().reverse().slice(0, 4);
  const chips = shown.map((rule) => `<span>${escapeHtml(rule.value)}</span>`);
  if (rules.length > shown.length && chips.length) {
    const last = chips.pop();
    chips.push(`<span class="session-rule-tail">${last}<span class="session-rule-more">...</span></span>`);
  }
  const body = chips.length ? chips.join("") : `<span class="no-session-rules">None yet</span>`;
  return `
    <section class="session-rule-set">
      <h3><i class="rule-dot rule-${classification}" aria-hidden="true"></i>${escapeHtml(title)}</h3>
      <div>
        ${body}
      </div>
    </section>
  `;
}

function renderActiveSessionControls(current: CurrentSession): string {
  const held = current.status === "paused" || current.status === "snoozed";
  return `
    <div class="active-session-controls">
      <button class="${held ? "primary session-resume" : "overlay-pause"}" type="button" data-action="${held ? "resume" : "pause"}">${held ? "Resume" : "Pause"}</button>
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
            <h2 id="session-rules-title">Edit session rules</h2>
          </div>
          <button class="small-button" type="button" data-action="close-session-rules">Done</button>
        </div>
        ${renderRuleManager(preflightRules, "preflight", "Session rules")}
      </section>
    </div>
  `;
}

function renderOverlay(current: CurrentSession | null): string {
  updateDistractionLatch(current);
  if (distractionIsActive()) {
    return renderDistractionOverlay(current as CurrentSession);
  }
  const paused = current?.status === "paused";
  const snoozed = current?.status === "snoozed";
  const held = paused || snoozed;
  const nudgesMuted = snoozed || (paused && current?.snoozed_until_seconds != null);
  const pinned = settings.status_window_on_top;
  return `
    <main class="overlay-shell">
      <section class="state-slab overlay-slab ${stateClass(current)}" aria-live="polite">
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
                  <span>Mute nudges</span>
                  <span class="toggle-field">
                    <input id="overlay-snooze" class="toggle-input" type="checkbox" ${nudgesMuted ? "checked" : ""} aria-label="Mute nudges" />
                    <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
                  </span>
                </label>
                <p>Help Supervisor understand this app.</p>
                <div class="overlay-marks">
                  <button type="button" data-action="mark-relevant">Relevant</button>
                  <button class="mark-distraction" type="button" data-action="mark-distracting">Distracting</button>
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
  return `
    <main class="overlay-shell overlay-interrupt">
      <section class="state-slab interrupt-alert">
        <p class="interrupt-kicker">You opened ${escapeHtml(distractionLatchLabel || distractionLabel(current))}</p>
        <h2>Distraction detected.</h2>
        <p>${escapeHtml(distractionLatchLabel || distractionLabel(current))} is outside this session's rules.</p>
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
        <button type="button" data-action="dismiss-mark-relevant">Mark as relevant</button>
      </div>
    </main>
  `;
}

function distractionLabel(current: CurrentSession): string {
  const sample = current.focus?.sample;
  const title = sample?.window_title?.trim();
  const appName = sample?.app_name?.trim();
  return title || appName || "This app";
}

function distractionIdentity(current: CurrentSession): string {
  const sample = current.focus?.sample;
  return `${sample?.process_id ?? ""}|${sample?.app_name ?? ""}|${sample?.window_title ?? ""}`;
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
  const focus = current.focus;
  if (!focus) {
    return;
  }
  const key = distractionIdentity(current);
  if (focus.state === "distracted") {
    if (distractionDismissedFor !== key) {
      distractionLatchKey = key;
      distractionLatchLabel = distractionLabel(current);
    }
    return;
  }
  if (focus.state === "focused" || focus.state === "neutral" || focus.state === "idle") {
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

function fitOverlayWindow(): void {
  if (appView !== "overlay") {
    return;
  }
  const shell = document.querySelector<HTMLElement>(".overlay-shell");
  if (!shell) {
    return;
  }
  const interrupt = overlayPlacement === "interrupt";
  const width = interrupt ? OVERLAY_INTERRUPT_WIDTH : OVERLAY_CORNER_WIDTH;
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

function renderStartAction(): string {
  return `
    <button class="primary start-watch-button" type="button" data-action="open-preflight">Start Focus Session</button>
  `;
}

function renderPreflightModal(): string {
  const ruleCount = preflightRules.length;
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
              <span>Block distractions</span>
              <span>${ruleCount} ${ruleCount === 1 ? "rule" : "rules"}</span>
            </summary>
            <div class="disclosure-body">
              ${renderRuleManager(preflightRules, "preflight", "Session rules")}
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
        ${renderRuleManager(permanentRules, "settings", "Permanent rules")}
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
          </section>
          <section class="panel webcam-test-panel">
            <p class="eyebrow">Camera test</p>
            ${renderWebcamMetrics(frame)}
            <p class="quiet">No images saved. Vision models run locally, and correct you in real time. You get a reminder if you leave your desk or if a phone is in view.</p>
          </section>
        </div>
      </section>
    </section>
  `;
}

function renderWebcamMetrics(frame: WebcamFrame | null): string {
  return `
    <div class="metric-grid webcam-metrics">
      ${metric("Face", frame?.face_in_frame ? "IN" : "OUT")}
      ${metric("Phone", frame?.phone_in_frame ? "IN FRAME" : "NO")}
    </div>
  `;
}

function renderRuleManager(rules: AppRule[], scope: RuleScope, title: string): string {
  const activeEditor = ruleEditor?.scope === scope ? ruleEditor : null;
  const helper =
    scope === "settings"
      ? "Used as the starting point for every session"
      : "These rules are temporary. They apply to this session only.";
  const applyPermanently =
    scope === "preflight"
      ? `<button class="apply-permanent-button" type="button" data-action="apply-rules-permanently">Apply permanently</button>`
      : "";
  return `
    <div class="rule-manager permanent-rule-manager">
      <div class="rule-toolbar">
        <div>
          <p class="eyebrow">${escapeHtml(title)}</p>
          ${
            scope === "settings"
              ? `<strong>${escapeHtml(helper)}</strong>`
              : `<p class="explain">${escapeHtml(helper)}</p>`
          }
        </div>
        <div class="rule-toolbar-actions">
          <button class="primary add-rule-button" type="button" data-rule-action="add" data-rule-scope="${scope}">+ Add rule</button>
          ${applyPermanently}
        </div>
      </div>
      ${activeEditor ? renderRuleEditor(activeEditor) : ""}
      <div class="rule-groups">
        ${renderRuleGroup(rules, scope, "distracting", "Distracting")}
        ${renderRuleGroup(rules, scope, "focused", "Relevant")}
        ${rules.some((rule) => rule.classification === "neutral") ? renderRuleGroup(rules, scope, "neutral", "Neutral") : ""}
      </div>
    </div>
  `;
}

function renderRuleGroup(
  rules: AppRule[],
  scope: RuleScope,
  classification: RuleClassification,
  title: string,
): string {
  const items = rules
    .map((rule, index) => ({ rule, index }))
    .filter((item) => item.rule.classification === classification)
    .reverse();
  return `
    <div class="rule-group">
      <div class="rule-group-label eyebrow">
        <i class="rule-dot rule-${classification}" aria-hidden="true"></i>
        <span>${escapeHtml(title)}</span>
      </div>
      <div class="rule-chip-list">
        ${items
          .map(
            ({ rule, index }) => `
              <button class="rule-chip rule-chip-${classification}" type="button" data-rule-action="edit" data-rule-scope="${scope}" data-rule-index="${index}">
                ${escapeHtml(rule.value)}
              </button>
            `,
          )
          .join("")}
        <button class="add-target-button" type="button" data-rule-action="add" data-rule-scope="${scope}" data-rule-classification="${classification}">+ Add</button>
      </div>
    </div>
  `;
}

function renderRuleEditor(editor: RuleEditor): string {
  return `
    <form class="rule-editor" data-rule-form="${editor.scope}">
      <label>
        <span>Mode</span>
        <select name="classification">${ruleClassificationOptions(editor.classification)}</select>
      </label>
      <label>
        <span>App/website name</span>
        <input name="value" maxlength="120" value="${attr(editor.value)}" placeholder="YouTube" />
      </label>
      <div class="rule-editor-actions">
        <button class="primary" type="submit">Save</button>
        ${
          editor.index === null
            ? ""
            : `<button class="danger" type="button" data-rule-action="delete" data-rule-scope="${editor.scope}" data-rule-index="${editor.index}">Delete</button>`
        }
        <button type="button" data-rule-action="cancel" data-rule-scope="${editor.scope}">Cancel</button>
      </div>
    </form>
  `;
}

function renderVoiceSettings(): string {
  const voices = availableVoices();
  const creating = voiceBusy !== null || voiceDownload?.phase === "downloading" || voiceDownload?.phase === "cancelling";
  return `
    <section class="voice-page">
      <div class="subpage-head">
        <button type="button" data-action="back-settings">Back</button>
        <div>
          <p class="eyebrow">VOICE SETTINGS</p>
          <h2>Voice library</h2>
        </div>
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

        <details class="panel create-voice-panel" data-create-voice ${createVoiceOpen ? "open" : ""}>
          <summary>
            <span>Clone a voice</span>
            ${
              modelInstalled()
                ? `<span class="create-voice-ready">Model installed</span>`
                : `<span class="create-voice-missing">Model not installed</span>`
            }
          </summary>
          ${renderCreateVoiceGuide()}
        </details>

        ${renderVoiceClipsPanel()}
      </section>
    </section>
  `;
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
  const needsGeneration =
    voice.source === "custom" &&
    Boolean(
      profile?.reference_retained &&
        effectivePhrases().every((phrase) => !clipForPhraseId(profile.clips, phrase.id)),
    );
  const canGenerate = needsGeneration && modelInstalled() && voiceBusy === null && ramAllowsGeneration();
  const canUse = voice.selectable && voiceBusy === null;
  const action = needsGeneration ? "generate-audio" : "select-voice";
  const disabled = needsGeneration ? !canGenerate : !canUse || selected;
  const lockTitle = needsGeneration && ramGenerationLocked() ? attr(ramLockMessage()) : "";

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
        <button class="voice-select" type="button" data-action="${action}" data-voice-id="${escapeHtml(voice.id)}" data-profile-id="${escapeHtml(profile?.profile_id ?? "")}" ${disabled ? "disabled" : ""}${lockTitle ? ` title="${lockTitle}"` : ""}>${selected ? "Selected" : "Select"}</button>
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

function renderCreateVoiceGuide(): string {
  const installed = modelInstalled();
  const downloading = voiceDownload?.phase === "downloading" || voiceDownload?.phase === "cancelling";
  const busy = voiceBusy !== null || downloading;
  const downloadLabel =
    voiceDownload?.phase === "cancelling"
      ? "Cancelling..."
      : voiceDownload?.phase === "downloading"
        ? "Downloading..."
        : "Download model";

  return `
    <div class="create-voice-body">
      <div class="create-voice-model">
        <div>
          <strong>Voice model</strong>
          <p>${
            installed
              ? "Installed, can clone voice clips. Saved clips play without the model."
              : "Download once to generate voice clips. Saved clips play without the model."
          }</p>
        </div>
        <button class="${installed ? "danger" : "overlay-pause"}" type="button" data-action="${installed ? "ask-delete-model" : "download-model-pack"}" ${busy ? "disabled" : ""}>${installed ? "Delete model" : downloadLabel}</button>
      </div>
      <ol class="create-voice-steps">
        <li><strong>1. Add a voice</strong><span>Click + New voice above, then record a short sample.</span></li>
        <li><strong>2. Generate your clips</strong><span>Set Clips for to that recorded voice, then Generate on a phrase.</span></li>
        <li><strong>3. Listen and keep</strong><span>Listen to each clip. Try again if needed, then keep the version you like.</span></li>
      </ol>
    </div>
  `;
}

function renderVoiceClipsPanel(): string {
  const voice = clipsVoice();
  const phrases = effectivePhrases();
  const ready = readyClipCount(voice, phrases);

  return `
    <section class="panel phrase-panel">
      <p class="eyebrow">Voice clips</p>
      ${renderRamLock()}
      <div class="rule-toolbar voice-clips-toolbar">
        <div class="voice-clips-meta">
          <label>
            <span>Clips for</span>
            <select id="voice-clips-voice">${voiceOptions(voice.id, true)}</select>
          </label>
          <strong class="voice-clips-count">${ready} of ${phrases.length} ready</strong>
        </div>
        <button type="button" data-action="open-phrase-editor" ${canEditPhrases() ? "" : "disabled"}>Add set</button>
          <p class="voice-clips-note">${escapeHtml(clipGenerateLockTitle(voice) || "Only ready clips play during sessions.")}</p>
        <label class="voice-clips-ready-only">
          <input id="voice-clips-ready-only" type="checkbox" ${readyClipsOnly ? "checked" : ""} />
          <span>Ready only</span>
        </label>
      </div>
      ${phraseEditor ? renderPhraseEditor() : ""}
      ${renderPhraseLibrary()}
    </section>
  `;
}

function renderRamLock(): string {
  const system = voiceBackend?.system;
  const locked = ramGenerationLocked();
  const free = system?.available_ram_bytes ?? null;
  const need = system?.min_free_ram_bytes ?? 0;
  const peak = system?.last_peak_ram_bytes ?? null;
  const fill = need > 0 && free !== null ? Math.max(4, Math.min(100, Math.round((free / need) * 100))) : 0;

  return `
    <aside class="ram-lock ${locked ? "is-locked" : "is-clear"}" role="status" aria-live="polite">
      <div class="ram-lock-head">
        <div class="ram-lock-stamp">${locked ? "Voice clone locked" : "Clone clear"}</div>
        <span class="ram-lock-machine">${escapeHtml(ramMachineLabel(system?.total_ram_bytes ?? null))}</span>
        ${locked ? `<button type="button" data-action="check-ram">Check again</button>` : ""}
      </div>
      <div class="ram-lock-copy">
        <p data-ram-copy>${escapeHtml(ramLockCopy(locked, free, need, peak))}</p>
        ${locked ? "<p>Once generated, saved clips don't need RAM.</p>" : ""}
      </div>
      ${
        locked
          ? ""
          : `<div class="ram-tank" aria-label="Free RAM versus clone budget">
        <span class="ram-tank-fill" style="width: ${fill}%"></span>
      </div>`
      }
    </aside>
  `;
}

function ramLockCopy(locked: boolean, free: number | null, need: number, peak: number | null): string {
  if (locked) {
    return `Generating voice clips needs about ${need ? formatBytes(need) : "enough"} of available RAM. You currently have ${free === null ? "?" : formatBytes(free)}. Close other apps, then check again.`;
  }
  return peak
    ? `Clone is allowed. Last job peaked at ${formatBytes(peak)}. Supervisor will refuse again if available RAM drops below ${formatBytes(need)}.`
    : `Clone is allowed. Keep about ${formatBytes(need)} available before generating. Saved clips still play.`;
}

function ramMachineLabel(total: number | null): string {
  return `Machine RAM: ${total ? formatBytes(total) : "?"}`;
}

function paintVoiceRam(): void {
  const root = document.querySelector(".ram-lock");
  if (!root || !voiceBackend) {
    return;
  }
  const locked = ramGenerationLocked();
  if (root.classList.contains("is-locked") !== locked) {
    return;
  }
  const system = voiceBackend.system;
  const free = system.available_ram_bytes ?? null;
  const need = system.min_free_ram_bytes ?? 0;
  const copy = document.querySelector("[data-ram-copy]");
  if (copy) {
    copy.textContent = ramLockCopy(locked, free, need, system.last_peak_ram_bytes ?? null);
  }
  const machine = document.querySelector(".ram-lock-machine");
  if (machine) {
    machine.textContent = ramMachineLabel(system.total_ram_bytes ?? null);
  }
  const fill = document.querySelector<HTMLElement>(".ram-tank-fill");
  if (fill && need > 0 && free !== null) {
    fill.style.width = `${Math.max(4, Math.min(100, Math.round((free / need) * 100)))}%`;
  }
}

function renderPhraseEditor(): string {
  const editor = phraseEditor;
  if (!editor) {
    return "";
  }

  return `
    <form class="phrase-editor" id="phrase-editor">
      <label>
        <span>Personality</span>
        <select name="tone">${phraseToneOptions(editor.tone)}</select>
      </label>
      <label>
        <span>Phrase</span>
        <input name="text" maxlength="140" value="${attr(editor.text)}" placeholder="Back to the task." />
      </label>
      <div class="rule-editor-actions">
        <button class="primary" type="submit">${editor.phraseId ? "Regenerate" : "Add phrase"}</button>
        <button type="button" data-action="cancel-phrase-editor">Cancel</button>
      </div>
    </form>
  `;
}

function renderPhraseLibrary(): string {
  const voice = clipsVoice();
  const phrases = effectivePhrases();
  const editable = canEditPhrases();
  return `
    <div class="phrase-groups">
      ${phraseToneOrder()
        .map((tone) => {
          const inSet = phrases.filter((phrase) => phrase.tone.toLowerCase() === tone.toLowerCase());
          const readyInSet = readyClipCount(voice, inSet);
          const items = readyClipsOnly ? inSet.filter((phrase) => clipForPhrase(voice, phrase)) : inSet;
          if (!items.length) {
            return "";
          }
          return `
            <details class="phrase-group" data-phrase-group="${attr(tone)}" ${openPhraseGroups.has(tone) ? "open" : ""}>
              <summary>
                <span>${escapeHtml(toneLabel(tone))}</span>
                <small>${readyInSet} of ${inSet.length} clips ready</small>
              </summary>
              <div class="rows phrase-group-rows">
                ${items
                  .map((phrase) => {
                    const clip = clipForPhrase(voice, phrase);
                    const ready = Boolean(clip);
                    const canGenerate = canGenerateClip(voice);
                    const generateTitle = clipGenerateLockTitle(voice);
                    return `
                      <div class="row phrase-row">
                        <span class="clip-dot ${ready ? "is-ready" : "is-empty"}" aria-hidden="true"></span>
                        <span class="clip-status ${ready ? "is-ready" : "is-empty"}">${ready ? "Ready" : "No clip"}</span>
                        <strong>${escapeHtml(phrase.text)}</strong>
                        <div class="phrase-row-actions">
                          ${
                            clip
                              ? `<button class="voice-play" type="button" data-action="play-clip" data-clip-path="${attr(clip.path)}" aria-label="Play clip">
                                  <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M3 1.2v9.6L11 6z" fill="currentColor"/></svg>
                                </button>`
                              : `<button class="small-button" type="button" data-action="generate-clip" data-phrase-id="${escapeHtml(phrase.id)}" ${canGenerate ? "" : "disabled"} title="${attr(generateTitle)}">Generate</button>`
                          }
                          <button class="small-button" type="button" data-action="open-phrase-editor" data-phrase-id="${escapeHtml(phrase.id)}" ${editable ? "" : "disabled"}>Edit</button>
                        </div>
                      </div>
                    `;
                  })
                  .join("")}
              </div>
            </details>
          `;
        })
        .join("")}
    </div>
  `;
}

function renderVoiceDialogs(): string {
  if (appView !== "main") {
    return "";
  }

  const showVoiceDialog = activeTab === "settings" && settingsView === "voice";
  return `
    ${showVoiceDialog && recordingOpen ? renderRecordingModal() : ""}
    ${showVoiceDialog && voiceInfoId && !voiceDeleteTargetId && !clipGenerate && !voiceBusy ? renderVoiceInfoModal() : ""}
    ${showVoiceDialog && voiceDeleteTargetId ? renderDeleteVoiceModal() : ""}
    ${showVoiceDialog && phraseSetPickerOpen ? renderPhraseSetPicker() : ""}
    ${showVoiceDialog && modelDeleteOpen ? renderDeleteModelModal() : ""}
    ${showVoiceDialog && clipGenerate ? renderClipGenerateModal() : ""}
    ${voiceBusy && !clipGenerate ? renderVoiceBusyModal() : ""}
  `;
}

function renderPhraseSetPicker(): string {
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel phrase-set-panel" role="dialog" aria-modal="true" aria-labelledby="phrase-set-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">ADD PHRASE</p>
            <h2 id="phrase-set-title">Choose a phrase set</h2>
          </div>
        </div>
        <p class="phrase-set-copy">Add to an existing set, or name a new one.</p>
        <div class="phrase-set-options">
          ${phraseToneOrder()
            .map(
              (tone) => `
                <button type="button" data-action="choose-phrase-set" data-tone="${attr(tone)}">
                  <span>${escapeHtml(toneLabel(tone))}</span>
                  <small>${phraseCountLabel(effectivePhrases().filter((phrase) => phrase.tone.toLowerCase() === tone.toLowerCase()).length)}</small>
                </button>
              `,
            )
            .join("")}
        </div>
        <div class="new-phrase-set">
          <label>
            <span>New set name</span>
            <input id="new-phrase-set-name" maxlength="80" value="${attr(newPhraseSetName)}" placeholder="Coach, dry humor, gentle reset..." />
          </label>
          <button class="primary" type="button" data-action="create-phrase-set">Create set</button>
        </div>
        <div class="modal-actions">
          <button type="button" data-action="close-phrase-set-picker">Cancel</button>
        </div>
      </section>
    </div>
  `;
}

function renderHistoryConfirm(): string {
  if (!historyConfirm) {
    return "";
  }

  const copy =
    historyConfirm.kind === "delete-all"
      ? {
          eyebrow: "HISTORY",
          title: "Delete all history?",
          body: "Every saved session on this computer will be removed. You cannot undo this.",
          confirm: "Delete all",
          danger: true,
        }
      : historyConfirm.kind === "scrub"
        ? {
            eyebrow: "HISTORY",
            title: "Forget app names?",
            body: "Saved sessions will keep their times. App and page names will be cleared.",
            confirm: "Forget names",
            danger: false,
          }
        : {
            eyebrow: "HISTORY",
            title: "Delete this session?",
            body: "This session will be removed from this computer. You cannot undo this.",
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
        <p>${escapeHtml(copy.body)}</p>
        <div class="modal-actions">
          <button type="button" data-action="cancel-history-confirm">Cancel</button>
          <button class="${copy.danger ? "danger" : "primary"}" type="button" data-action="confirm-history">${escapeHtml(copy.confirm)}</button>
        </div>
      </section>
    </div>
  `;
}

function renderRecordingModal(): string {
  const isRecording = recordingState === "recording";
  const ramLocked = ramGenerationLocked();
  const canSave = Boolean(recordedVoice?.bytes && recordedVoice.name.trim() && voiceBusy === null && !ramLocked);

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
                    <button class="primary" type="button" data-action="save-recorded-voice" ${canSave ? "" : "disabled"}>${ramLocked ? "Clone locked" : "Save voice and generate audio clips"}</button>
                  </div>
                  ${ramLocked ? `<p class="recording-error">Generation is locked until more RAM is free. The recording can wait.</p>` : ""}
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
        <p class="quiet">Choose whether to remove the saved reference clip too, or only remove generated phrase audio.</p>
        <div class="delete-choice-grid">
          <button class="danger" type="button" data-action="delete-voice-profile">Delete voice clip and all voice files</button>
          <button type="button" data-action="delete-voice-files" ${profile.clip_count === 0 ? "disabled" : ""}>Delete voice files only</button>
        </div>
      </section>
    </div>
  `;
}

function renderClipGenerateModal(): string {
  const job = clipGenerate;
  if (!job) {
    return "";
  }
  const phrase = effectivePhrases().find((item) => item.id === job.phraseId);
  const voice = clipsVoice();
  const clip = phrase ? clipForPhrase(voice, phrase) : null;
  const working = job.phase === "working";
  const modelName = voiceBackend?.model.display_name || "Chatterbox-Nano voice cloning model";
  const status = working
    ? "Model loading"
    : job.phase === "error"
      ? job.error || "The local worker could not generate this clip."
      : "Clip generated";

  return `
    <div class="modal-backdrop busy-backdrop" role="presentation">
      <section class="modal-panel busy-panel" role="dialog" aria-modal="true" aria-live="polite">
        <p class="eyebrow">VOICE WORKER</p>
        <h2>${escapeHtml(phrase?.text ?? "Voice clip")}</h2>
        <p class="quiet">${escapeHtml(modelName)}${phrase ? ` · ${escapeHtml(toneLabel(phrase.tone))}` : ""} · ${escapeHtml(voice.name)}</p>
        <p class="busy-note" id="clip-generate-status">${escapeHtml(status)}</p>
        ${working ? `<div class="loading-bar"><span></span></div>` : ""}
        <div class="modal-actions">
          ${
            working
              ? ""
              : `${clip ? `<button type="button" data-action="play-clip" data-clip-path="${attr(clip.path)}">Play</button>` : ""}
                 <button type="button" data-action="regenerate-clip">Try again</button>
                 <button type="button" data-action="close-clip-generate">Close</button>`
          }
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
  const total = Math.max(0, download?.total_bytes ?? voiceBackend?.model.required_bytes ?? 0);
  const downloaded = Math.max(0, Math.min(download?.downloaded_bytes ?? 0, total || Number.MAX_SAFE_INTEGER));
  const percent = done ? 100 : total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  const title = done
    ? "Model pack ready"
    : phase === "cancelling"
      ? "Cancelling download"
      : "Downloading model pack";
  const copy = done
    ? "The voice model is installed. Saved clips play without keeping this window open."
    : download?.message ?? "Downloading model pack.";
  const eta = done
    ? "Done"
    : phase === "cancelling"
      ? "Deleting partial files"
      : download?.eta_seconds === null || download?.eta_seconds === undefined
        ? "Estimating time left"
        : `${formatDuration(download.eta_seconds)} left`;
  const progressText = total > 0 ? `${formatBytes(downloaded)} / ${formatBytes(total)}` : formatBytes(downloaded);
  const note = done
    ? "You can close this and start cloning a voice."
    : `This is a ${total > 0 ? formatBytes(total) : "large"} first-time download and can take several minutes depending on your connection.`;

  return `
    <div class="modal-backdrop busy-backdrop" role="presentation">
      <section class="modal-panel busy-panel" role="dialog" aria-modal="true" aria-live="polite">
        <p class="eyebrow">VOICE WORKER</p>
        <h2>${escapeHtml(title)}</h2>
        <p>${escapeHtml(copy)}</p>
        <div class="loading-bar determinate" aria-label="Model download progress">
          <span style="width: ${percent}%"></span>
        </div>
        <div class="download-meta">
          <span>${escapeHtml(progressText)}</span>
          <span>${escapeHtml(eta)}</span>
        </div>
        <p class="quiet busy-note">${escapeHtml(note)}</p>
        <div class="modal-actions">
          ${
            done
              ? `<button type="button" data-action="close-model-download">Close</button>`
              : phase === "cancelling"
                ? ""
                : `<button class="danger" type="button" data-action="cancel-model-download">Cancel download</button>`
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
  if (historyView === "recap" && periodRecap) {
    return renderPeriodRecap(periodRecap);
  }

  const weekStart = historyWeek?.week_start || historyWeekStart || thisMondayKey();
  const sessions = historyWeek?.sessions ?? [];
  const oldestMonday = historyWeek?.oldest_day ? mondayKey(historyWeek.oldest_day) : null;
  const prevDisabled = !oldestMonday || weekStart <= oldestMonday;
  const nextDisabled = weekStart >= thisMondayKey();

  return `
    <section class="history-page">
      ${renderHistoryOverview()}
      <section class="panel history-list-panel">
        <div class="history-list-head">
          <div>
            <p class="eyebrow">SESSIONS</p>
            <h2>Local history</h2>
          </div>
          <div class="history-head-actions">
            <div class="history-week-nav">
              <button type="button" data-action="history-week-prev" ${prevDisabled ? "disabled" : ""} aria-label="Previous week">‹</button>
              <span>${escapeHtml(formatWeekRange(weekStart))}</span>
              <button type="button" data-action="history-week-next" ${nextDisabled ? "disabled" : ""} aria-label="Next week">›</button>
            </div>
            <div class="history-tools">
              <button type="button" data-action="ask-scrub">Forget names</button>
              <button class="danger" type="button" data-action="ask-delete-all">Delete all</button>
            </div>
          </div>
        </div>
        <div class="history-list">
          ${
            sessions.length === 0
              ? `<p class="empty">${historyWeek?.oldest_day ? "No sessions this week." : "Stop a session and the local report lands here. Nothing leaves this machine."}</p>`
              : groupSessionsByDay(sessions)
                  .map(
                    (group) => `
              <section class="history-day">
                <p class="eyebrow">${escapeHtml(group.label)}</p>
                ${group.items.map(renderHistoryItem).join("")}
              </section>
            `,
                  )
                  .join("")
          }
        </div>
      </section>
    </section>
  `;
}

function renderHistoryItem(item: SessionSummary): string {
  return `
    <button class="history-item" type="button" data-action="open-report" data-session-id="${item.id}">
      <div>
        <h3>${escapeHtml(item.goal)}</h3>
        <p>${escapeHtml(new Date(item.started_at).toLocaleString())}</p>
        <p class="hint-line">${escapeHtml(item.top_hints.join(" / ") || "No app hints")}</p>
      </div>
      <div class="history-item-stats">
        <dl>
          <div><dt>Focus</dt><dd>${focusPercent(item)}%</dd></div>
          <div><dt>Duration</dt><dd>${formatDuration(item.duration_seconds)}</dd></div>
          <div><dt>Nudges</dt><dd>${item.nudges}</dd></div>
        </dl>
        ${renderMiniTimeline(item)}
      </div>
    </button>
  `;
}

function renderHistoryOverview(): string {
  const weeks = trends.weekly_stretch;
  const current = weeks[weeks.length - 1];
  const currentLabel = current?.session_count
    ? formatMinutes(current.median_stretch_seconds)
    : "—";
  return `
    <div class="history-overview">
      <section class="panel span-card">
        <p class="eyebrow">Attention span</p>
        <p class="span-value">${escapeHtml(currentLabel)}</p>
        <p class="span-caption">Typical uninterrupted stretch this week</p>
        ${renderSpanPlot(weeks)}
      </section>
      <div class="history-recaps">
        ${renderRecapCard("Weekly recap", trends.week_recap, "week")}
        ${renderRecapCard("Monthly recap", trends.month_recap, "month")}
      </div>
    </div>
  `;
}

function renderRecapCard(title: string, recap: FocusRecap, kind: "week" | "month"): string {
  const range =
    kind === "week"
      ? formatWeekRange(recap.start || thisMondayKey())
      : formatMonthLabel(recap.start || dayKey(new Date(new Date().getFullYear(), new Date().getMonth(), 1)));
  const sessions =
    recap.session_count === 1 ? "1 session" : `${recap.session_count} sessions`;
  return `
    <section class="panel recap-card">
      <p class="eyebrow">${escapeHtml(title)}</p>
      <p class="recap-value">${escapeHtml(formatHours(recap.focus_seconds))} focused</p>
      <p class="recap-meta">${escapeHtml(range)} · ${escapeHtml(sessions)}</p>
      <button type="button" data-action="${kind === "week" ? "open-week-recap" : "open-month-recap"}">View recap ›</button>
    </section>
  `;
}

function renderPeriodRecap(recap: PeriodRecap): string {
  const week = recap.kind === "week";
  const todayMonday = thisMondayKey();
  const weekStart = week
    ? recap.start
    : todayMonday >= recap.start && todayMonday <= recap.end
      ? todayMonday
      : recap.start;
  const monthStart = `${recap.start.slice(0, 8)}01`;
  const range = week ? formatWeekRange(recap.start) : formatMonthLabel(recap.start);
  const sessions =
    recap.session_count === 1 ? "1 recorded session" : `${recap.session_count} recorded sessions`;
  const typical = recap.median_stretch_seconds
    ? formatMinutes(recap.median_stretch_seconds)
    : "—";
  const longest = recap.longest_stretch_seconds
    ? `Longest stretch · ${formatMinutes(recap.longest_stretch_seconds)}`
    : "No stretch recorded";
  const rangeLine =
    recap.stretch_range_low != null && recap.stretch_range_high != null
      ? `${formatMinutes(recap.stretch_range_low).replace(" min", "")} → ${formatMinutes(recap.stretch_range_high)} across full weeks`
      : "";
  const maxApp = Math.max(...recap.apps.map((app) => app.seconds), 1);
  const maxBucket = Math.max(...recap.buckets.map((bucket) => bucket.focus_seconds), 1);

  return `
    <section class="history-page recap-page">
      <div class="subpage-head recap-head">
        <button type="button" data-action="close-recap">Back</button>
        <div>
          <p class="eyebrow">History</p>
          <h2>${week ? "Weekly recap" : "Monthly recap"}</h2>
        </div>
        <div class="recap-switch">
          <button type="button" class="${week ? "active" : ""}" data-action="open-week-recap" data-start="${attr(weekStart)}">Week</button>
          <button type="button" class="${week ? "" : "active"}" data-action="open-month-recap" data-start="${attr(monthStart)}">Month</button>
          <button type="button" data-action="export-recap-pdf">Export PDF</button>
        </div>
      </div>

      <div class="recap-layout">
        <section class="panel recap-focus">
          <p class="eyebrow">Focused time · ${escapeHtml(range)}</p>
          <p class="recap-hero">${escapeHtml(formatHours(recap.focus_seconds))}</p>
          <p class="recap-caption">across ${escapeHtml(sessions)}</p>
          <div class="recap-bars" style="grid-template-columns: repeat(${recap.buckets.length}, minmax(0, 1fr))">
            ${recap.buckets
              .map((bucket) => {
                const on = bucket.focus_seconds > 0;
                const height = on ? Math.max(18, Math.round((bucket.focus_seconds / maxBucket) * 100)) : 12;
                const focused = formatHours(bucket.focus_seconds);
                return `
                  <button type="button" class="recap-bar-col" aria-label="${attr(bucket.label)}, ${attr(focused)} focused">
                    <span class="recap-bar-track">
                      <span class="recap-bar ${on ? "is-on" : ""}" style="height: ${height}%">
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

        <div class="recap-side">
          <section class="panel recap-stat-card">
            <p class="eyebrow">Sustained attention</p>
            <p class="recap-stat-value">${escapeHtml(typical)}</p>
            <p class="recap-caption">Typical uninterrupted stretch</p>
            <p class="recap-stat-note">${escapeHtml(week ? longest : rangeLine || longest)}</p>
          </section>
          <section class="panel recap-stat-card">
            <p class="eyebrow">Focus retention</p>
            <p class="recap-caption">Focused share by session half</p>
            ${renderRecapHalf("First half", recap.first_half_focus_percent)}
            ${renderRecapHalf("Second half", recap.second_half_focus_percent)}
          </section>
        </div>
      </div>

      <section class="panel recap-apps">
        <p class="eyebrow">Apps & sites</p>
        <h2>Where session time went</h2>
        ${
          recap.apps.length === 0
            ? `<p class="empty">No app time in this recap.</p>`
            : `<div class="recap-app-list">
                ${["Focus", "Neutral", "Distracting", "Miscellaneous"]
                  .map((group) => {
                    const items = recap.apps.filter((app) => app.group === group);
                    if (items.length === 0) {
                      return "";
                    }
                    return `
                      <div class="recap-app-block">
                        <p class="eyebrow">${escapeHtml(group)}</p>
                        <div class="recap-app-group">
                          ${items
                            .map((app) => {
                              const shownMinutes = Math.round(app.seconds / 60);
                              const width = shownMinutes > 0 ? Math.max(4, Math.round((app.seconds / maxApp) * 100)) : 0;
                              return `
                                <div class="recap-app-row">
                                  <span class="recap-app-name">${escapeHtml(app.label)}</span>
                                  <span class="recap-app-track">${width ? `<span style="width: ${width}%"></span>` : ""}</span>
                                  <strong>${escapeHtml(app.seconds >= 3600 ? formatHours(app.seconds) : formatMinutes(app.seconds))}</strong>
                                </div>
                              `;
                            })
                            .join("")}
                        </div>
                      </div>
                    `;
                  })
                  .join("")}
              </div>`
        }
      </section>
    </section>
  `;
}

function renderRecapHalf(labelText: string, percent: number | null): string {
  const value = percent == null ? "—" : `${percent}%`;
  const width = percent == null ? 0 : percent;
  return `
    <div class="recap-half">
      <span>${escapeHtml(labelText)}</span>
      <span class="recap-half-track"><span style="width: ${width}%"></span></span>
      <strong>${escapeHtml(value)}</strong>
    </div>
  `;
}

function renderSpanPlot(weeks: WeeklyStretch[]): string {
  if (weeks.length === 0) {
    return `<p class="empty">Not enough weeks to plot yet.</p>`;
  }

  const width = 680;
  const height = 268;
  const left = 64;
  const right = 22;
  const top = 36;
  const bottom = 56;
  const inset = 36;
  const plotW = width - left - right;
  const plotH = height - top - bottom;
  const minutes = weeks.map((week) => Math.round(week.median_stretch_seconds / 60));
  const { max: axisMax, step } = spanAxis(Math.max(0, ...minutes));
  const ticks: number[] = [];
  for (let tick = 0; tick <= axisMax; tick += step) {
    ticks.push(tick);
  }
  const last = weeks.length - 1;
  const innerW = plotW - inset;
  const xAt = (index: number) =>
    weeks.length === 1 ? left + inset + innerW / 2 : left + inset + (index / last) * innerW;
  const yAt = (value: number) => top + plotH - (value / axisMax) * plotH;
  const points = weeks.map((week, index) => ({
    x: xAt(index),
    y: yAt(minutes[index]),
    current: index === last,
    label: formatStretchChip(week.median_stretch_seconds),
    weekLabel: formatWeekStart(week.week_start),
  }));
  const line = points.map((point) => `${point.x.toFixed(1)},${point.y.toFixed(1)}`).join(" ");
  const grid = ticks
    .map((tick) => {
      const y = yAt(tick);
      return `
        <line class="span-grid" x1="${left}" y1="${y.toFixed(1)}" x2="${width - right}" y2="${y.toFixed(1)}" />
        <text class="span-axis" x="${left - 10}" y="${y.toFixed(1)}" text-anchor="end" dominant-baseline="middle">${tick}</text>
      `;
    })
    .join("");
  const xLabels = points
    .map((point) => {
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
    .map((point) => {
      const tipY = point.y - 28;
      return `
        <g class="span-hit" tabindex="0">
          <circle class="span-hotspot" cx="${point.x.toFixed(1)}" cy="${point.y.toFixed(1)}" r="22" />
          <rect class="span-dot${point.current ? " is-now" : ""}" x="${(point.x - 7).toFixed(1)}" y="${(point.y - 7).toFixed(1)}" width="14" height="14" />
          <g class="span-tip" transform="translate(${point.x.toFixed(1)} ${tipY.toFixed(1)})">
            <rect x="-22" y="-16" width="44" height="24" rx="6" />
            <text y="1" text-anchor="middle" dominant-baseline="middle">${escapeHtml(point.label)}</text>
          </g>
        </g>
      `;
    })
    .join("");

  return `
    <svg class="span-plot" viewBox="0 0 ${width} ${height}" role="img" aria-label="Typical stretch for the last five weeks">
      ${grid}
      <polyline class="span-line" points="${line}" />
      ${xLabels}
      ${hits}
    </svg>
  `;
}

function spanAxis(maxMinutes: number): { max: number; step: number } {
  if (maxMinutes <= 15) {
    return { max: 15, step: 5 };
  }
  const max = Math.max(30, Math.ceil(maxMinutes / 10) * 10);
  return { max, step: 10 };
}

function renderMiniTimeline(item: SessionSummary): string {
  const parts = [
    { cls: "state-focused", seconds: item.focus_seconds },
    { cls: "state-distracted", seconds: item.distracted_seconds },
    { cls: "state-idle", seconds: item.idle_seconds },
  ].filter((part) => part.seconds > 0);
  if (parts.length === 0) {
    return `<div class="mini-timeline" aria-hidden="true"></div>`;
  }
  return `
    <div class="mini-timeline" aria-hidden="true">
      ${parts.map((part) => `<span class="${part.cls}" style="flex: ${part.seconds} 1 0"></span>`).join("")}
    </div>
  `;
}

function renderSessionReport(report: SessionReport): string {
  return `
    <section class="history-page">
      <div class="report-toolbar">
        <button class="small-button" type="button" data-action="close-report">Back</button>
        <button class="small-button danger" type="button" data-action="ask-delete-session" data-session-id="${report.id}">Delete</button>
      </div>
      <div class="report-stage">
        <section class="state-slab report-slab">
          <h2>${escapeHtml(report.goal)}</h2>
          <p class="report-headline">${escapeHtml(reportHeadline(report))}</p>
          <p class="report-started">${escapeHtml(new Date(report.started_at).toLocaleString())} · ${escapeHtml(label(report.strictness))}</p>
          ${renderReportTimeline(report)}
          <div class="report-inset">${renderTimelineLegend()}</div>
        </section>
        <aside class="report-stats" aria-label="Session stats">
          ${reportStat("Duration", formatClockWithUnit(report.duration_seconds))}
          ${reportStat("Focus and Neutral", `${report.focus_percent}%`)}
          ${reportStat("Nudges", String(report.nudge_count))}
        </aside>
      </div>
      <section class="panel categories-card">
        <p class="eyebrow">WHERE IT WENT</p>
        <h2>Categories</h2>
        ${renderCategoryBars(report)}
        <p class="hint-line">${escapeHtml(report.top_apps.join(" / ") || "No apps recorded")}</p>
      </section>
    </section>
  `;
}

function reportStat(labelText: string, value: string): string {
  return `
    <div class="report-stat">
      <span>${escapeHtml(labelText)}</span>
      <strong>${escapeHtml(value)}</strong>
    </div>
  `;
}

function renderReportTimeline(report: SessionReport): string {
  const duration = Math.max(report.duration_seconds, 1);
  const nudges = report.events.filter((event) => event.kind === "nudge");
  if (report.spans.length === 0) {
    return `<p class="empty">No timeline for this session.</p>`;
  }
  return `
    <div class="timeline-wrap">
      <div class="timeline-bar" role="img" aria-label="Session timeline">
        ${report.spans
          .map((span) => {
            const title = `${span.app}${span.context ? ` · ${span.context}` : ""} · ${formatDuration(span.seconds)}`;
            return `<span class="timeline-span state-${escapeHtml(span.state)}" style="flex: ${Math.max(span.seconds, 1)} 1 0" title="${attr(title)}"></span>`;
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

function renderTimelineLegend(): string {
  const items = [
    ["state-focused", "Focused"],
    ["state-neutral", "Neutral"],
    ["state-suspicious", "Slipping"],
    ["state-distracted", "Distracted"],
    ["state-idle", "Idle"],
  ];
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

function renderCategoryBars(report: SessionReport): string {
  const totals = new Map(report.category_totals.map((row) => [row.category, row.seconds]));
  const max = Math.max(...REPORT_CATEGORIES.map((category) => totals.get(category) ?? 0), 1);
  return `
    <div class="category-bars">
      ${REPORT_CATEGORIES.map((category) => {
        const seconds = totals.get(category) ?? 0;
        const width = Math.round((seconds / max) * 100);
        return `
          <div class="category-row">
            <span>${escapeHtml(category)}</span>
            <div class="category-track"><span class="category-${category}" style="width: ${width}%"></span></div>
            <strong>${formatMinutes(seconds)}</strong>
          </div>
        `;
      }).join("")}
    </div>
  `;
}

function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const next = button.dataset.tab as Tab;
      if (next === activeTab) {
        return;
      }
      const leavingWebcam = settingsView === "webcam";
      if (voiceInfoId) {
        dismissVoiceInfo();
      }
      activeTab = next;
      if (next !== "settings") {
        settingsView = "home";
        ruleEditor = null;
      }
      render();
      if (activeTab === "history") {
        void refreshTrends();
        void refreshHistoryWeek();
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

  document.getElementById("phrase-editor")?.addEventListener("submit", (event) => {
    event.preventDefault();
    void savePhraseForm(event.currentTarget as HTMLFormElement);
  });

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
    if (!mute && snapshot?.current?.status === "paused") {
      void invokeSnapshot("clear_snooze");
      return;
    }
    void invokeSnapshot(mute ? "snooze_session" : "resume_session");
  });

  bindDraftField("preflight-goal", "goal");

  document.querySelectorAll<HTMLDetailsElement>("[data-preflight-section]").forEach((details) => {
    details.addEventListener("toggle", () => {
      if (details.dataset.preflightSection === "blocks") {
        preflightBlocksOpen = details.open;
      } else {
        preflightAdvancedOpen = details.open;
      }
    });
  });

  document.querySelector("details[data-create-voice]")?.addEventListener("toggle", (event) => {
    createVoiceOpen = (event.currentTarget as HTMLDetailsElement).open;
  });

  document.getElementById("voice-clips-voice")?.addEventListener("change", (event) => {
    const voiceId = (event.target as HTMLSelectElement).value;
    if (!availableVoices().some((item) => item.id === voiceId)) {
      return;
    }
    clipsVoiceId = voiceId;
    clipsVoiceTouched = true;
    render();
  });

  document.getElementById("voice-clips-ready-only")?.addEventListener("change", (event) => {
    readyClipsOnly = (event.target as HTMLInputElement).checked;
    render();
  });

  document.querySelectorAll<HTMLDetailsElement>("[data-phrase-group]").forEach((details) => {
    details.addEventListener("toggle", () => {
      const tone = details.dataset.phraseGroup;
      if (!tone) {
        return;
      }
      if (details.open) {
        openPhraseGroups.add(tone);
      } else {
        openPhraseGroups.delete(tone);
      }
    });
  });

  document.getElementById("new-phrase-set-name")?.addEventListener("input", (event) => {
    newPhraseSetName = (event.target as HTMLInputElement).value;
  });

  document.getElementById("preflight-duration")?.addEventListener("input", (event) => {
    const slider = event.target as HTMLInputElement;
    draft.duration_minutes = Number(slider.value || 25);
    slider.style.setProperty("--range-progress", `${((draft.duration_minutes - 15) / 75) * 100}%`);
    const output = document.getElementById("preflight-duration-output");
    if (output) {
      output.textContent = `${draft.duration_minutes} min`;
    }
  });

  document.getElementById("preflight-strictness")?.addEventListener("change", (event) => {
    draft.strictness = (event.target as HTMLSelectElement).value as Strictness;
  });

  document.getElementById("preflight-voice")?.addEventListener("change", (event) => {
    draft.voiceId = (event.target as HTMLSelectElement).value;
    void updateSelectedVoice(draft.voiceId);
  });

  document.getElementById("default-strictness")?.addEventListener("change", (event) => {
    void updateDefaultStrictness((event.target as HTMLSelectElement).value as Strictness);
  });

  document.getElementById("status-window-enabled")?.addEventListener("change", (event) => {
    void updateStatusWindowEnabled((event.target as HTMLInputElement).checked);
  });

  document.getElementById("webcam-phone-use")?.addEventListener("change", (event) => {
    webcamSettings.phoneUse = (event.target as HTMLInputElement).checked;
    webcamHitStreak = 0;
    paintWebcamDom();
  });

  document.getElementById("webcam-gpu")?.addEventListener("change", (event) => {
    void setWebcamGpu((event.target as HTMLInputElement).checked);
  });

  document.getElementById("voice-name")?.addEventListener("input", (event) => {
    if (recordedVoice) {
      recordedVoice.name = (event.target as HTMLInputElement).value;
    }
  });

  bindVoicePreview();
  bindVoiceWaveforms();
  bindClipGenerateStatus();
}

function bindDraftField(id: string, key: "goal"): void {
  document.getElementById(id)?.addEventListener("input", (event) => {
    draft[key] = (event.target as HTMLTextAreaElement).value;
  });
}

async function startSession(): Promise<boolean> {
  return invokeSnapshot("start_session", {
    input: {
      goal: draft.goal,
      duration_minutes: draft.duration_minutes,
      strictness: draft.strictness,
      rules: cloneRules(preflightRules),
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

  if (action === "dismiss-mark-relevant") {
    const live = snapshot?.current;
    if (live) {
      distractionDismissedFor = distractionIdentity(live);
    }
    distractionLatchKey = "";
    distractionLatchLabel = "";
    overlayMoreOpen = false;
    await invokeSnapshot("mark_current_relevant");
    return;
  }

  if (action === "open-session-rules") {
    const current = snapshot?.current;
    if (current) {
      preflightRules = cloneRules(current.rules);
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

  if (action === "open-preflight") {
    preflightRules = cloneRules(permanentRules);
    draft.duration_minutes = Math.min(90, Math.max(15, draft.duration_minutes));
    preflightBlocksOpen = false;
    preflightAdvancedOpen = false;
    preflightOpen = true;
    ruleEditor = null;
    render();
    return;
  }

  if (action === "close-preflight") {
    preflightOpen = false;
    ruleEditor = null;
    render();
    return;
  }

  if (action === "apply-rules-permanently") {
    await applyRulesPermanently();
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

  if (action === "open-week-recap") {
    const start = source?.dataset.start || trends.week_recap.start || thisMondayKey();
    await openPeriodRecap("week", start);
    return;
  }

  if (action === "open-month-recap") {
    const raw = source?.dataset.start || trends.month_recap.start;
    const start = raw ? `${raw.slice(0, 8)}01` : dayKey(new Date(new Date().getFullYear(), new Date().getMonth(), 1));
    await openPeriodRecap("month", start);
    return;
  }

  if (action === "close-recap") {
    historyView = "list";
    periodRecap = null;
    render();
    return;
  }

  if (action === "export-recap-pdf") {
    return;
  }

  if (action === "history-week-prev") {
    historyWeekStart = addDaysKey(historyWeekStart || thisMondayKey(), -7);
    await refreshHistoryWeek();
    return;
  }

  if (action === "history-week-next") {
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
    historyConfirm = { kind: "delete-all" };
    render();
    return;
  }

  if (action === "ask-scrub") {
    historyConfirm = { kind: "scrub" };
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
    await startWebcamPreview();
    return;
  }

  if (action === "back-settings") {
    const leavingWebcam = settingsView === "webcam";
    settingsView = "home";
    phraseEditor = null;
    phraseSetPickerOpen = false;
    dismissVoiceInfo();
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

  if (action === "select-voice") {
    const voiceId = source?.dataset.voiceId;
    const voice = voiceId ? availableVoices().find((item) => item.id === voiceId) : null;
    if (voiceId && voice?.selectable) {
      draft.voiceId = voiceId;
      render();
      showFeedback(`${voice.name} selected`);
      void updateSelectedVoice(voiceId);
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
      const phraseId = source?.dataset.phraseId || null;
      if (phraseId) {
        openPhraseEditor(phraseId);
      } else {
        dismissVoiceInfo();
        phraseSetPickerOpen = true;
        newPhraseSetName = "";
        render();
      }
    }
    return;
  }

  if (action === "close-phrase-set-picker") {
    phraseSetPickerOpen = false;
    newPhraseSetName = "";
    render();
    return;
  }

  if (action === "choose-phrase-set") {
    const tone = source?.dataset.tone;
    if (tone) {
      phraseSetPickerOpen = false;
      openPhraseGroups.add(tone);
      openPhraseEditor(null, tone);
    }
    return;
  }

  if (action === "create-phrase-set") {
    const enteredName = newPhraseSetName.trim().slice(0, 80);
    if (!enteredName) {
      showFeedback("Name the phrase set");
      return;
    }
    const tone =
      phraseToneOrder().find((item) => item.toLowerCase() === enteredName.toLowerCase()) ??
      enteredName;
    if (!customPhraseSets.some((item) => item.toLowerCase() === tone.toLowerCase()) && !toneOrder.includes(tone)) {
      const previousSets = [...customPhraseSets];
      customPhraseSets.push(tone);
      if (!(await persistPhraseLibrary())) {
        customPhraseSets = previousSets;
        render();
        return;
      }
    }
    phraseSetPickerOpen = false;
    newPhraseSetName = "";
    openPhraseGroups.add(tone);
    openPhraseEditor(null, tone);
    return;
  }

  if (action === "cancel-phrase-editor") {
    phraseEditor = null;
    render();
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

  if (action === "generate-audio") {
    const profileId = source?.dataset.profileId;
    if (profileId) {
      await generateProfileAudio(profileId);
    }
    return;
  }

  if (action === "generate-clip") {
    const phraseId = source?.dataset.phraseId;
    if (phraseId) {
      await generateOneClip(phraseId);
    }
    return;
  }

  if (action === "regenerate-clip") {
    if (clipGenerate?.phraseId) {
      await generateOneClip(clipGenerate.phraseId);
    }
    return;
  }

  if (action === "close-clip-generate") {
    stopClipGenerateStatus();
    clipGenerate = null;
    render();
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
    "mark-relevant": "mark_current_relevant",
    "mark-distracting": "mark_current_distracting",
  };
  const command = commands[action];
  if (!command) {
    return;
  }

  const ok = await invokeSnapshot(command, undefined, !action.startsWith("mark-"));
  if (ok && action.startsWith("mark-")) {
    const sample = snapshot?.current?.focus?.sample;
    const name = sample?.window_title?.trim() || sample?.app_name?.trim() || "last window";
    showFeedback(`${action === "mark-relevant" ? "Relevant" : "Distracting"}: ${name}`);
  }
  if (ok && action === "stop" && history[0]) {
    await openHistoryReport(history[0].id);
  }
}

async function handleRuleAction(button: HTMLButtonElement): Promise<void> {
  const scope = button.dataset.ruleScope as RuleScope | undefined;
  const action = button.dataset.ruleAction;
  if (!scope || !action) {
    return;
  }

  if (action === "add") {
    ruleEditor = {
      scope,
      index: null,
      target: "window_title",
      classification:
        (button.dataset.ruleClassification as RuleClassification | undefined) ??
        (scope === "preflight" ? "focused" : "distracting"),
      value: "",
    };
    render();
    return;
  }

  if (action === "cancel") {
    ruleEditor = null;
    render();
    return;
  }

  const index = Number(button.dataset.ruleIndex);
  const rules = rulesForScope(scope);
  if (!Number.isInteger(index) || !rules[index]) {
    return;
  }

  if (action === "edit") {
    ruleEditor = { scope, index, ...rules[index] };
    render();
    return;
  }

  if (action === "delete") {
    const previous = cloneRules(rules);
    rules.splice(index, 1);
    if (ruleEditor?.scope === scope) {
      ruleEditor = null;
    }
    if (scope === "settings") {
      await persistPermanentRuleChanges(previous);
    } else if (sessionRulesOpen) {
      await persistSessionRuleChanges(previous);
    } else {
      render();
    }
  }
}

async function saveRuleForm(form: HTMLFormElement): Promise<void> {
  const scope = form.dataset.ruleForm as RuleScope | undefined;
  if (!scope || ruleEditor?.scope !== scope) {
    return;
  }

  const rule: AppRule = {
    target: "window_title",
    classification: formValue(form, "classification") as RuleClassification,
    value: formValue(form, "value").trim(),
  };

  if (!rule.value) {
    showFeedback("Enter an app or website name");
    return;
  }

  const rules = rulesForScope(scope);
  const previous = cloneRules(rules);
  if (ruleEditor.index === null) {
    rules.unshift(rule);
  } else {
    rules[ruleEditor.index] = rule;
  }
  ruleEditor = null;
  if (scope === "settings") {
    await persistPermanentRuleChanges(previous);
  } else if (sessionRulesOpen) {
    await persistSessionRuleChanges(previous);
  } else {
    render();
  }
}

async function persistPermanentRuleChanges(previous: AppRule[]): Promise<void> {
  try {
    permanentRules = await invoke<AppRule[]>("update_rules", { rules: permanentRules });
    lastError = "";
  } catch (error) {
    permanentRules = previous;
    lastError = errorText(error);
  }
  render();
}

async function applyRulesPermanently(): Promise<void> {
  const previous = cloneRules(permanentRules);
  try {
    permanentRules = await invoke<AppRule[]>("update_rules", { rules: cloneRules(preflightRules) });
    lastError = "";
    render();
    showFeedback("Saved for every session");
    return;
  } catch (error) {
    permanentRules = previous;
    lastError = errorText(error);
  }
  render();
}

async function persistSessionRuleChanges(previous: AppRule[]): Promise<void> {
  try {
    snapshot = await invoke<SessionSnapshot>("update_session_rules", { rules: preflightRules });
    settings = snapshot.settings;
    history = snapshot.history;
    preflightRules = cloneRules(snapshot.current?.rules ?? preflightRules);
    lastError = "";
  } catch (error) {
    preflightRules = previous;
    lastError = errorText(error);
  }
  render();
}

async function savePhraseForm(form: HTMLFormElement): Promise<void> {
  if (!canEditPhrases()) {
    return;
  }

  const editor = phraseEditor ? { ...phraseEditor } : null;
  const previousCustomPhrases = customPhrases.map((phrase) => ({ ...phrase }));
  const previousPhraseOverrides = { ...phraseOverrides };
  const text = formValue(form, "text").trim();
  const tone = formValue(form, "tone") as VoiceTone;
  const wasEdit = Boolean(editor?.phraseId);
  if (!text) {
    showFeedback("Phrase needs text");
    return;
  }

  if (editor?.phraseId) {
    const existingCustomIndex = customPhrases.findIndex((phrase) => phrase.id === editor.phraseId);
    if (existingCustomIndex >= 0) {
      customPhrases[existingCustomIndex] = { id: customPhrases[existingCustomIndex].id, tone, text };
    } else {
      phraseOverrides[editor.phraseId] = { id: editor.phraseId, tone, text };
    }
  } else {
    customPhrases.push({
      id: uniqueCustomPhraseId(text),
      tone,
      text,
    });
  }

  if (!(await persistPhraseLibrary())) {
    customPhrases = previousCustomPhrases;
    phraseOverrides = previousPhraseOverrides;
    phraseEditor = editor;
    render();
    return;
  }

  openPhraseGroups.add(tone);
  phraseEditor = null;
  render();
  if (wasEdit && editor?.phraseId) {
    const started = await generateOneClip(editor.phraseId);
    if (!started) {
      showFeedback("Phrase saved");
    }
    return;
  }
  showFeedback("Phrase added");
}

async function persistPhraseLibrary(): Promise<boolean> {
  try {
    const library = await invoke<PhraseLibraryState>("update_phrase_library", {
      library: {
        custom_tones: customPhraseSets,
        custom_phrases: customPhrases,
        phrase_overrides: Object.values(phraseOverrides),
      },
    });
    customPhraseSets = library.custom_tones;
    customPhrases = library.custom_phrases;
    phraseOverrides = Object.fromEntries(library.phrase_overrides.map((phrase) => [phrase.id, phrase]));
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

async function updateSelectedVoice(voiceId: string): Promise<void> {
  try {
    settings = await invoke<SessionSettings>("update_settings", {
      update: { voice_profile: voiceId },
    });
    voiceError = "";
  } catch (error) {
    voiceError = errorText(error);
    render();
  }
}

async function startModelDownload(): Promise<void> {
  if (voiceBusy !== null) {
    return;
  }

  voiceBusy = "download-model";
  try {
    const download = await invoke<VoiceDownloadSnapshot>("start_voice_model_download");
    applyDownloadSnapshot(download);
    voiceError = "";
    startDownloadPolling();
    if (download.phase === "complete") {
      stopDownloadPolling();
      await refreshVoiceStatus(false);
      showFeedback("Model pack ready");
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

  try {
    const download = await invoke<VoiceDownloadSnapshot>("get_voice_model_download");
    applyDownloadSnapshot(download);
    if (download.phase === "complete") {
      stopDownloadPolling();
      await refreshVoiceStatus(false);
      showFeedback("Model pack ready");
      render();
      return;
    }
    if (download.phase === "idle") {
      stopDownloadPolling();
      voiceBusy = null;
      await refreshVoiceStatus(false);
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
    voiceError = errorText(error);
    return false;
  } finally {
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
  if (!recording?.path || !modelInstalled()) {
    return;
  }
  if (ramGenerationLocked()) {
    showFeedback(ramLockMessage());
    return;
  }

  const name = recording.name.trim();
  if (!name) {
    showFeedback("Name the voice");
    return;
  }

  const profileId = uniqueVoiceProfileId(name);
  recordingOpen = false;

  await runVoiceBusy("generate-clips", async () => {
    const saved = await invoke<VoiceProfileSummary>("save_voice_reference_from_path", {
      input: {
        profile_id: profileId,
        display_name: name,
        reference_audio_path: recording.path,
      },
    });
    if (!saved.reference_path) {
      throw new Error("Recorded voice reference was not saved.");
    }

    const generated = await invoke<VoiceProfileSummary>("generate_voice_clips", {
      input: {
        profile_id: saved.profile_id,
        display_name: name,
        reference_audio_path: saved.reference_path,
        phrases: voicePhraseInputs(),
        delete_reference_after_generation: false,
      },
    });

    upsertProfile(generated);
    await refreshVoiceStatus(false);
    draft.voiceId = voiceOptionId(generated.profile_id);
    clipsVoiceId = draft.voiceId;
    await updateSelectedVoice(draft.voiceId);
    await clearVoiceRecording();
    resetRecordedVoice();
    recordingState = "idle";
    showFeedback("Voice generated");
  });
}

async function generateOneClip(phraseId: string): Promise<boolean> {
  const voice = clipsVoice();
  const profile = voice.profile;
  const phrase = effectivePhrases().find((item) => item.id === phraseId);
  if (!phrase) {
    showFeedback("Phrase not found");
    return false;
  }
  const reference = profile?.reference_path;
  const lock = clipGenerateLockTitle(voice);
  if (lock || !profile || !reference) {
    showFeedback(lock || "Pick a cloned voice under Clips for.");
    return false;
  }

  stopClipGenerateStatus();
  clipGenerate = { phraseId, profileId: profile.profile_id, phase: "working", error: "" };
  const ok = await runVoiceBusy("generate-clips", async () => {
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
    await refreshVoiceStatus(false);
    clipsVoiceId = voiceIdForProfile(generated.profile_id);
  });
  if (!clipGenerate || clipGenerate.phraseId !== phraseId) {
    return true;
  }
  stopClipGenerateStatus();
  clipGenerate = ok
    ? { ...clipGenerate, phase: "ready", error: "" }
    : { ...clipGenerate, phase: "error", error: voiceError || "The local worker could not generate this clip." };
  render();
  return true;
}

async function generateProfileAudio(profileId: string, feedback = "Audio generated"): Promise<boolean> {
  const profile = profileById(profileId);
  if (!profile?.reference_path || !modelInstalled()) {
    showFeedback(modelInstalled() ? "Reference missing" : "Download voice pack first");
    return false;
  }
  if (ramGenerationLocked()) {
    showFeedback(ramLockMessage());
    return false;
  }

  return runVoiceBusy("generate-clips", async () => {
    const generated = await invoke<VoiceProfileSummary>("generate_voice_clips", {
      input: {
        profile_id: profile.profile_id,
        display_name: profile.display_name,
        reference_audio_path: profile.reference_path,
        phrases: voicePhraseInputs(),
        delete_reference_after_generation: false,
      },
    });
    upsertProfile(generated);
    await refreshVoiceStatus(false);
    clipsVoiceId = voiceIdForProfile(generated.profile_id);
    showFeedback(feedback);
  });
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

function showFeedback(message: string): void {
  feedbackMessage = message;
  paintFeedback();

  if (feedbackTimer !== null) {
    window.clearTimeout(feedbackTimer);
  }

  feedbackTimer = window.setTimeout(() => {
    feedbackMessage = "";
    feedbackTimer = null;
    paintFeedback();
  }, 1400);
}

function paintRecordingClock(): void {
  const el = document.getElementById("recording-elapsed");
  if (el && recordingState === "recording") {
    el.textContent = formatDuration(recordingDurationSeconds());
  }
}

async function setWebcamGpu(enabled: boolean): Promise<void> {
  try {
    settings = await invoke<SessionSettings>("update_settings", {
      update: { webcam_gpu: enabled },
    });
    if (webcamPreviewStarted) {
      await stopWebcamPreview();
      webcamFrame = null;
      await startWebcamPreview();
    }
    webcamError = "";
  } catch (error) {
    webcamError = errorText(error);
    const box = document.getElementById("webcam-gpu") as HTMLInputElement | null;
    if (box) {
      box.checked = settings.webcam_gpu;
    }
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
  if (alerting) {
    webcamHitStreak += 1;
  } else {
    webcamHitStreak = 0;
  }
  if (webcamHitStreak >= 8) {
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
    return "At the PC";
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
    return "Phone is visible. Looking at the PC, so this is not a phone hit yet.";
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
  oscillator.start();
  oscillator.stop(context.currentTime + 0.28);
}

function permissionNotice(current: CurrentSession | null): string {
  const sample = current?.focus?.sample;
  if (sample?.platform === "macos" && sample.permissions.screen_recording === "missing") {
    return `<section class="notice">Screen Recording permission is missing. Supervisor will fall back to app-name rules until macOS exposes window titles.</section>`;
  }
  return "";
}

function statusChip(current: CurrentSession | null): string {
  if (!current) {
    return "STANDBY";
  }

  if (current.status === "paused") {
    return "PAUSED";
  }

  if (current.status === "snoozed") {
    return "SNOOZED";
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

  if (current.status === "snoozed") {
    return "Snoozed";
  }

  switch (current.focus?.state) {
    case "focused":
      return "Locked in";
    case "neutral":
      return "Watching";
    case "suspicious":
      return "Drift watch";
    case "distracted":
      return "Drifting";
    case "idle":
      return "Away";
    default:
      return "Watching";
  }
}

function stateCopy(current: CurrentSession | null): string {
  if (!current) {
    return "One clear start. Rules and voice live in the preflight.";
  }

  if (current.status === "paused") {
    return "No judgment recorded.";
  }

  if (current.status === "snoozed") {
    return "Nudges muted.";
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

  if (current.status === "snoozed") {
    return "state-muted";
  }

  return `state-${current.focus?.state ?? "neutral"}`;
}

async function playNudgeAudioIfNeeded(current: CurrentSession | null): Promise<void> {
  if (!current || current.status !== "active" || draft.voiceId === "none") {
    return;
  }

  const nudgeCount = current.metrics.nudges;
  const alreadyPlayed = playedNudgesBySession.get(current.id) ?? 0;
  const pendingNudge = pendingNudgesBySession.get(current.id) ?? 0;
  if (nudgeCount <= Math.max(alreadyPlayed, pendingNudge)) {
    return;
  }

  const clip = selectedNudgeClip(nudgeCount);
  if (!clip) {
    return;
  }

  pendingNudgesBySession.set(current.id, nudgeCount);
  try {
    nudgeAudio?.pause();
    nudgeAudio = new Audio(clipAssetUrl(clip.path));
    nudgeAudio.preload = "auto";
    await nudgeAudio.play();
    playedNudgesBySession.set(current.id, nudgeCount);
    voiceError = "";
  } catch (error) {
    voiceError = `Could not play voice clip: ${errorText(error)}`;
    render();
  } finally {
    if (pendingNudgesBySession.get(current.id) === nudgeCount) {
      pendingNudgesBySession.delete(current.id);
    }
  }
}

function selectedNudgeClip(nudgeCount: number): VoiceClipSummary | null {
  const voice = selectedVoice();
  const phrases = effectivePhrases();
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

  const voice = availableVoices().find((item) => item.id === voiceInfoId);
  const clip = voice ? previewClipForVoice(voice) : null;
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

function bindVoiceWaveforms(): void {
  document.querySelectorAll<HTMLCanvasElement>("canvas[data-wave-path]").forEach((canvas) => {
    const path = canvas.dataset.wavePath ?? "";
    const bars = Number(canvas.dataset.waveBars) || 11;
    if (!path) {
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

async function loadWaveformBuffer(path: string): Promise<AudioBuffer> {
  const cached = waveformBuffers.get(path);
  if (cached) {
    return cached;
  }
  let pending = waveformPending.get(path);
  if (!pending) {
    pending = (async () => {
      const response = await fetch(clipAssetUrl(path));
      if (!response.ok) {
        throw new Error("waveform fetch failed");
      }
      const bytes = await response.arrayBuffer();
      const context = waveformContext ?? (waveformContext = new AudioContext());
      const buffer = await context.decodeAudioData(bytes.slice(0));
      waveformBuffers.set(path, buffer);
      return buffer;
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
  const voice = availableVoices().find((item) => item.id === voiceInfoId);
  if (!voice) {
    return;
  }
  const clip = previewClipForVoice(voice);
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

function ruleClassificationLabel(classification: RuleClassification): string {
  switch (classification) {
    case "focused":
      return "Focus";
    case "distracting":
      return "Distraction";
    case "neutral":
      return "Neutral";
  }
}

function strictnessOptions(selected: Strictness): string {
  return (["gentle", "normal", "strict"] as Strictness[])
    .map((value) => `<option value="${value}" ${value === selected ? "selected" : ""}>${label(value)}</option>`)
    .join("");
}

function voiceOptions(selected: string, forClips = false): string {
  return availableVoices()
    .map((voice) => {
      const enabled = forClips
        ? voice.id === "none" || Boolean(voice.profile?.reference_path)
        : voice.selectable;
      return `<option value="${voice.id}" ${voice.id === selected ? "selected" : ""} ${enabled ? "" : "disabled"}>${escapeHtml(voice.name)}</option>`;
    })
    .join("");
}

function ruleClassificationOptions(selected: RuleClassification): string {
  return (["focused", "neutral", "distracting"] as RuleClassification[])
    .map(
      (classification) =>
        `<option value="${classification}" ${classification === selected ? "selected" : ""}>${ruleClassificationLabel(classification)}</option>`,
    )
    .join("");
}

function rulesForScope(scope: RuleScope): AppRule[] {
  return scope === "settings" ? permanentRules : preflightRules;
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
        selectable: ready > 0,
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

function ramAllowsGeneration(): boolean {
  return voiceBackend?.system.has_enough_ram !== false;
}

function ramLockMessage(): string {
  const system = voiceBackend?.system;
  const need = system?.min_free_ram_bytes ? formatBytes(system.min_free_ram_bytes) : "enough";
  const free = system?.available_ram_bytes != null ? formatBytes(system.available_ram_bytes) : "unknown";
  return `Clone locked. Need about ${need} available RAM, this PC has ${free} available.`;
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

function clipGenerateStatusText(): string {
  return clipGenerateStatusIndex % 2 === 0 ? "Model loading" : "Model generating";
}

function stopClipGenerateStatus(): void {
  if (clipGenerateTickId !== null) {
    window.clearInterval(clipGenerateTickId);
    clipGenerateTickId = null;
  }
  clipGenerateStatusIndex = 0;
}

function paintClipGenerateStatus(): void {
  const status = document.getElementById("clip-generate-status");
  if (!status || clipGenerate?.phase !== "working") {
    return;
  }
  status.textContent = clipGenerateStatusText();
}

function bindClipGenerateStatus(): void {
  if (clipGenerate?.phase !== "working") {
    stopClipGenerateStatus();
    return;
  }
  paintClipGenerateStatus();
  if (clipGenerateTickId !== null) {
    return;
  }
  clipGenerateTickId = window.setInterval(() => {
    clipGenerateStatusIndex += 1;
    paintClipGenerateStatus();
  }, 1600);
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

function openPhraseEditor(phraseId: string | null, tone = "gentle"): void {
  const phrase = phraseId ? effectivePhrases().find((item) => item.id === phraseId) : null;
  phraseEditor = {
    phraseId,
    tone: phrase?.tone ?? tone,
    text: phrase?.text ?? "",
  };
  render();
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
    ...phraseBank.map((phrase) => phraseOverrides[phrase.id] ?? phrase),
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

function clipAssetUrl(path: string): string {
  try {
    return convertFileSrc(path);
  } catch {
    return path;
  }
}

function voiceBusyTitle(): string {
  switch (voiceBusy) {
    case "download-model":
      return "Downloading model pack";
    case "delete-model":
      return "Deleting model files";
    case "generate-clips":
      return "Generating audio clips";
    case "delete-voice":
      return "Deleting voice";
    default:
      return "Working";
  }
}

function voiceBusyCopy(): string {
  if (voiceBusy === "generate-clips") {
    return "The local worker is cloning the voice, sampling peak RAM, and will quit when the clips are written.";
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
    case "kind-reset":
      return "Kind reset";
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
  } catch (error) {
    lastError = errorText(error);
  }
  if (shouldRender) {
    render();
  }
}

async function refreshHistoryWeek(shouldRender = true): Promise<void> {
  if (!historyWeekStart) {
    historyWeekStart = thisMondayKey();
  }
  try {
    historyWeek = await invoke<HistoryWeek>("get_history_week", { weekStart: historyWeekStart });
    historyWeekStart = historyWeek.week_start;
  } catch (error) {
    lastError = errorText(error);
  }
  if (shouldRender) {
    render();
  }
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

async function openPeriodRecap(period: "week" | "month", start: string): Promise<void> {
  try {
    periodRecap = await invoke<PeriodRecap>("get_period_recap", { period, start });
    historyView = "recap";
    sessionReport = null;
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }
  render();
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
    periodRecap = null;
    render();
    return;
  }
  if (confirm.kind === "delete-all") {
    await runHistoryCommand("delete_all_history", undefined, "History cleared");
    historyView = "list";
    sessionReport = null;
    periodRecap = null;
    render();
    return;
  }
  await runHistoryCommand("scrub_span_context", undefined, "Names forgotten");
  if (sessionReport) {
    await openHistoryReport(sessionReport.id);
  } else if (periodRecap) {
    await openPeriodRecap(periodRecap.kind, periodRecap.start);
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

function reportHeadline(report: SessionReport): string {
  return `${formatMinutes(report.duration_seconds)} · ${report.focus_percent}% focused · typical stretch ${formatMinutes(report.median_stretch_seconds)}`;
}


function focusPercent(item: SessionSummary): number {
  if (item.duration_seconds === 0) {
    return 0;
  }
  return Math.round((item.focus_seconds * 100) / item.duration_seconds);
}

function formatMinutes(totalSeconds: number): string {
  return `${Math.max(0, Math.round(totalSeconds / 60))} min`;
}

function formatStretchChip(totalSeconds: number): string {
  const minutes = Math.max(0, Math.round(totalSeconds / 60));
  if (minutes >= 60) {
    const hours = Math.floor(minutes / 60);
    const rest = minutes % 60;
    return rest ? `${hours}h ${rest}m` : `${hours}h`;
  }
  return `${minutes}m`;
}

function dailyNote(): { date: string; text: string; by: string } {
  const notes = [
    { text: "To be everywhere is to be nowhere.", by: "Seneca" },
    { text: "What we do now echoes in eternity.", by: "Marcus Aurelius" },
    { text: "Well begun is half done.", by: "Aristotle" },
    { text: "The mind is not a vessel to be filled, but a fire to be kindled.", by: "Plutarch" },
    { text: "It is not that we have a short time to live, but that we waste a good deal of it.", by: "Seneca" },
    { text: "No great thing is created suddenly.", by: "Epictetus" },
    { text: "The first and greatest victory is to conquer yourself.", by: "Plato" },
  ];
  const now = new Date();
  const start = new Date(now.getFullYear(), 0, 0);
  const day = Math.floor((now.getTime() - start.getTime()) / 86400000);
  const note = notes[day % notes.length];
  const date = now.toLocaleDateString("en-GB", { day: "2-digit", month: "short" }).toUpperCase();
  return { date, text: note.text, by: note.by };
}

function quoteFit(text: string): string {
  if (text.length > 70) {
    return "quote-long";
  }
  if (text.length > 46) {
    return "quote-mid";
  }
  return "quote-short";
}

const WEEK_LETTERS = ["M", "T", "W", "T", "F", "S", "S"];
const WEEK_NAMES = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

function weekSummary(): {
  total: string;
  caption: string;
  days: { letter: string; name: string; seconds: number }[];
  max: number;
} {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const monday = new Date(today);
  monday.setDate(today.getDate() - ((today.getDay() + 6) % 7));
  const byDate = new Map(trends.daily_focus.map((day) => [day.date, day]));
  const days = WEEK_LETTERS.map((letter, index) => {
    const date = new Date(monday);
    date.setDate(monday.getDate() + index);
    const match = byDate.get(dayKey(date));
    return { letter, name: WEEK_NAMES[index], seconds: match?.focus_seconds ?? 0, sessions: match?.session_count ?? 0 };
  });
  const seconds = days.reduce((sum, day) => sum + day.seconds, 0);
  const count = days.reduce((sum, day) => sum + day.sessions, 0);
  return {
    total: formatHours(seconds),
    caption: `across ${count} session${count === 1 ? "" : "s"}`,
    days,
    max: Math.max(...days.map((day) => day.seconds), 1),
  };
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

function formatMonthLabel(startKey: string): string {
  return parseDayKey(startKey).toLocaleDateString(undefined, { month: "long", year: "numeric" });
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
    groups.push({
      key,
      label: date.toLocaleDateString(undefined, { weekday: "long", month: "short", day: "numeric" }),
      items: [item],
    });
  }
  return groups;
}

function barHeight(value: number, max: number): number {
  const floor = 16;
  if (value <= 0 || max <= 0) {
    return floor;
  }
  return Math.round(floor + (value / max) * (100 - floor));
}

function formatHours(totalSeconds: number): string {
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
