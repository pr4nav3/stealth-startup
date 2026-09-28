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
type VoiceTone = "gentle" | "mom" | "stern" | "kind-reset";
type VoiceSource = "starter" | "custom";
type VoiceBusy = "download-model" | "delete-model" | "generate-clips" | "delete-voice" | null;
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

const starterVoices: VoiceOption[] = [
  {
    id: "starter-male",
    name: "Starter Male",
    kind: "Bundled",
    detail: "Low-friction voice for first-run sessions.",
    coverage: "20/20 clips",
    source: "starter",
    selectable: true,
    profile: null,
  },
  {
    id: "starter-female",
    name: "Starter Female",
    kind: "Bundled",
    detail: "Warmer starter voice with the same phrase bank.",
    coverage: "20/20 clips",
    source: "starter",
    selectable: true,
    profile: null,
  },
];

const draft = {
  goal: "",
  duration_minutes: 25,
  strictness: "normal" as Strictness,
  likelyTargets: "",
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
};
let history: SessionSummary[] = [];
let permanentRules = cloneRules(starterRules);
let preflightOpen = false;
let preflightRules = cloneRules(starterRules);
let ruleEditor: RuleEditor | null = null;
let customPhrases: PhraseDefinition[] = [];
let phraseOverrides: Record<string, PhraseDefinition> = {};
let phraseEditor: PhraseEditor | null = null;
let voiceBackend: VoiceBackendStatus | null = null;
let voiceDownload: VoiceDownloadSnapshot | null = null;
let voiceBusy: VoiceBusy = null;
let voiceDeleteTargetId: string | null = null;
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
let voiceRamPollId: number | null = null;
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

void init();

function getAppRoot(): HTMLDivElement {
  const element = document.querySelector<HTMLDivElement>("#app");

  if (!element) {
    throw new Error("Missing app root");
  }

  return element;
}

async function init(): Promise<void> {
  try {
    const [loadedSettings, loadedHistory] = await Promise.all([
      invoke<SessionSettings>("get_settings"),
      invoke<SessionSummary[]>("get_history"),
    ]);
    settings = loadedSettings;
    history = loadedHistory;
    draft.strictness = loadedSettings.default_strictness;
    draft.voiceId = normalizeStoredVoiceId(loadedSettings.voice_profile);
  } catch (error) {
    lastError = errorText(error);
  }

  if (appView === "main") {
    await refreshVoiceStatus(false);
    await refreshVoiceDownload(false);
    await listen<WebcamFrame>("webcam-frame", (event) => {
      applyWebcamFrame(event.payload);
    });
  }
  await refreshSnapshot();
}

async function refreshVoiceStatus(shouldRender = true): Promise<void> {
  try {
    voiceBackend = await invoke<VoiceBackendStatus>("get_voice_backend_status");
    voiceError = "";
    ensureSelectedVoice();
  } catch (error) {
    voiceError = errorText(error);
  }

  if (shouldRender) {
    render();
  }
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
    if (appView === "main") {
      draft.voiceId = normalizeStoredVoiceId(settings.voice_profile);
      ensureSelectedVoice();
      void playNudgeAudioIfNeeded(snapshot.current);
    }
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }

  const webcamOpen = appView === "main" && activeTab === "settings" && settingsView === "webcam";
  if (!webcamOpen) {
    render();
  }
  reconcilePolling();
}

function render(): void {
  const current = snapshot?.current ?? null;
  if (appView === "overlay") {
    app.innerHTML = renderOverlay(current);
    bindEvents();
    return;
  }

  app.innerHTML = `
    <main class="app-shell">
      <header class="topbar">
        <div>
          <p class="eyebrow">LOCAL SUPERVISOR</p>
          <h1>Supervisor</h1>
        </div>
        <div class="chips" aria-label="Local status">
          <span>LOCAL</span>
          <span>CPU</span>
          <span>${escapeHtml(label(settings.default_strictness))}</span>
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
      ${renderFeedback()}
    </main>
  `;

  bindEvents();
  if (activeTab === "settings" && settingsView === "webcam") {
    paintWebcamDom();
  }
}

function renderSession(current: CurrentSession | null): string {
  return `
    <section class="session-home ${current ? "session-active" : "session-ready"}">
      <section class="state-slab ${stateClass(current)}" aria-live="polite">
        <div class="state-head">
          <span>${statusChip(current)}</span>
          <span>${current ? escapeHtml(label(current.strictness)) : "READY"}</span>
        </div>
        <h2>${escapeHtml(stateTitle(current))}</h2>
        <p>${escapeHtml(stateCopy(current))}</p>
        ${current ? renderProgress(current) : renderReadyMeta()}
      </section>

      <section class="panel signal-panel">
        <h2>Session signal</h2>
        ${renderMetrics(current)}
        ${current ? renderActivity(current) : renderSignalStandby()}
      </section>

      <section class="session-action-panel">
        ${current ? renderControls(current) : renderStartAction()}
      </section>

      ${current ? renderEvidencePanel(current) : ""}
    </section>
    ${preflightOpen ? renderPreflightModal() : ""}
  `;
}

function renderOverlay(current: CurrentSession | null): string {
  return `
    <main class="overlay-shell">
      <section class="state-slab overlay-slab ${stateClass(current)}" aria-live="polite">
        <div class="state-head">
          <span>${statusChip(current)}</span>
          <span>${current ? escapeHtml(label(current.strictness)) : "READY"}</span>
        </div>
        <h2>${escapeHtml(stateTitle(current))}</h2>
        <p>${escapeHtml(stateCopy(current))}</p>
        ${current ? renderProgress(current) : ""}
        ${current ? renderOverlayControls(current) : ""}
      </section>
      ${renderFeedback()}
    </main>
  `;
}

function renderReadyMeta(): string {
  return `
    <div class="ready-meta">
      <span>${draft.duration_minutes} min</span>
      <span>${escapeHtml(label(draft.strictness))}</span>
      <span>${escapeHtml(selectedVoice().name)}</span>
    </div>
  `;
}

function renderStartAction(): string {
  return `
    <button class="primary start-watch-button" type="button" data-action="open-preflight">Start watching</button>
  `;
}

function renderControls(current: CurrentSession): string {
  const paused = current.status === "paused";
  const snoozed = current.status === "snoozed";
  return `
    <div class="control-row">
      <button class="primary" data-action="${paused ? "resume" : "pause"}">${paused ? "Resume" : "Pause"}</button>
      <button data-action="${snoozed ? "resume" : "snooze"}">${snoozed ? "Unsnooze" : "Snooze 5 min"}</button>
      <button data-action="mark-relevant">Mark relevant</button>
      <button data-action="mark-distracting">Mark distracting</button>
      <button class="danger" data-action="stop">Stop</button>
    </div>
  `;
}

function renderOverlayControls(current: CurrentSession): string {
  const paused = current.status === "paused";
  const snoozed = current.status === "snoozed";
  const pauseLabel = paused ? "Resume" : "Pause";
  const pinned = settings.status_window_on_top;
  return `
    <div class="overlay-controls">
      <div class="overlay-control-row">
        <button class="primary icon-button" data-action="${paused ? "resume" : "pause"}" aria-label="${pauseLabel}" title="${pauseLabel}">
          <span class="control-icon ${paused ? "icon-play" : "icon-pause"}" aria-hidden="true"></span>
        </button>
        <button class="danger icon-button" data-action="stop" aria-label="Stop" title="Stop">
          <span class="control-icon icon-stop" aria-hidden="true"></span>
        </button>
        <button class="text-button" data-action="${snoozed ? "resume" : "snooze"}">${snoozed ? "Unsnooze" : "Mute nudges"}</button>
        <button class="text-button" data-action="${pinned ? "unpin-window" : "pin-window"}">${pinned ? "Unpin window" : "Pin window"}</button>
      </div>
      <div class="overlay-control-row mark-control-row">
        <button class="mark-relevant" data-action="mark-relevant">Relevant</button>
        <button class="mark-distraction" data-action="mark-distracting">Distraction</button>
      </div>
    </div>
  `;
}

function renderProgress(current: CurrentSession): string {
  const percent = current.duration_seconds
    ? Math.min(100, Math.round((current.elapsed_seconds / current.duration_seconds) * 100))
    : 0;

  return `
    <div class="progress" aria-label="Session progress">
      <span style="width: ${percent}%"></span>
    </div>
    <div class="state-meta">
      <span>${formatDuration(current.elapsed_seconds)} elapsed</span>
      <span>${formatDuration(current.remaining_seconds)} left</span>
    </div>
  `;
}

function renderMetrics(current: CurrentSession | null): string {
  if (!current) {
    return `
      <div class="metric-grid">
        ${metric("Lock", "-")}
        ${metric("Rules", String(permanentRules.length))}
        ${metric("Voice", shortVoiceName(selectedVoice().name))}
        ${metric("Side", settings.status_window_enabled ? "ON" : "OFF")}
      </div>
    `;
  }

  const accountable = Math.max(1, current.elapsed_seconds - current.metrics.idle_seconds);
  const focusSeconds = current.metrics.focused_seconds + current.metrics.neutral_seconds;
  const focusPercent = Math.round((focusSeconds / accountable) * 100);

  return `
    <div class="metric-grid">
      ${metric("Lock", `${focusPercent}%`)}
      ${metric("Drift", formatDuration(current.metrics.distracted_seconds))}
      ${metric("Nudges", String(current.metrics.nudges))}
      ${metric("Away", formatDuration(current.metrics.idle_seconds))}
    </div>
  `;
}

function renderSignalStandby(): string {
  return `
    <div class="rows compact-rows">
      <div class="row"><span>Goal</span><strong>${escapeHtml(draft.goal || "Not set")}</strong><time>READY</time></div>
      <div class="row"><span>Default rule set</span><strong>${permanentRules.length} rules</strong><time>LOCAL</time></div>
      <div class="row"><span>Status window</span><strong>${settings.status_window_enabled ? "Enabled" : "Disabled"}</strong><time>${settings.status_window_on_top ? "PIN" : "BACK"}</time></div>
    </div>
  `;
}

function renderEvidencePanel(current: CurrentSession): string {
  return `
    <section class="panel evidence-panel">
      <h2>Session log</h2>
      ${renderEvidence(current)}
    </section>
  `;
}

function renderEvidence(current: CurrentSession | null): string {
  const rows = current?.evidence ?? [];
  if (rows.length === 0) {
    return `<p class="empty">No evidence yet. Supervisor starts receipts after the first decision.</p>`;
  }

  return `
    <div class="rows">
      ${rows
        .map(
          (row) => `
            <div class="row">
              <span>${escapeHtml(row.label)}</span>
              <strong>${escapeHtml(row.value)}</strong>
              <time>${formatDuration(row.at_seconds)}</time>
            </div>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderActivity(current: CurrentSession | null): string {
  const focus = current?.focus;
  if (!focus) {
    return `
      <div class="rows compact-rows">
        <div class="row"><span>Capture</span><strong>Waiting for first sample</strong><time>-</time></div>
        <div class="row"><span>Mark target</span><strong>Last non-Supervisor window</strong><time>RULE</time></div>
      </div>
    `;
  }

  const sample = focus.sample;
  return `
    <div class="rows compact-rows">
      <div class="row"><span>App</span><strong>${escapeHtml(sample.app_name || "-")}</strong><time>${escapeHtml(sample.platform)}</time></div>
      <div class="row"><span>Title</span><strong>${escapeHtml(sample.window_title || "Unavailable")}</strong><time>${formatDuration(focus.seconds_on_current_window)}</time></div>
      <div class="row"><span>Rule</span><strong>${escapeHtml(focus.matched_rule ? ruleLabel(focus.matched_rule) : "NO MATCH")}</strong><time>${escapeHtml(ruleClassificationLabel(focus.classification))}</time></div>
      <div class="row"><span>Nudge</span><strong>${escapeHtml(nudgeLabel(focus))}</strong><time>${sample.idle_seconds === null ? "IDLE ?" : `${sample.idle_seconds.toFixed(1)}S`}</time></div>
    </div>
  `;
}

function renderPreflightModal(): string {
  return `
    <div class="modal-backdrop" role="presentation">
      <section class="modal-panel preflight-panel" role="dialog" aria-modal="true" aria-labelledby="preflight-title">
        <div class="modal-head">
          <div>
            <p class="eyebrow">SESSION PREFLIGHT</p>
            <h2 id="preflight-title">Set the watch</h2>
          </div>
          <button class="small-button" type="button" data-action="close-preflight">Close</button>
        </div>

        <div class="preflight-grid">
          <section class="preflight-main">
            <label>
              <span>Session goal</span>
              <textarea id="preflight-goal" rows="3" maxlength="220" placeholder="What are you planning to finish?">${escapeHtml(draft.goal)}</textarea>
            </label>
            <label>
              <span>Likely apps / sites</span>
              <textarea id="likely-targets" rows="3" maxlength="240" placeholder="Docs, GitHub, Linear, YouTube API docs">${escapeHtml(draft.likelyTargets)}</textarea>
            </label>
            <div class="form-row">
              <label>
                <span>Minutes</span>
                <input id="preflight-duration" type="number" min="5" max="180" step="5" value="${draft.duration_minutes}" />
              </label>
              <label>
                <span>Strictness</span>
                <select id="preflight-strictness">${strictnessOptions(draft.strictness)}</select>
              </label>
            </div>
            <label>
              <span>Voice</span>
              <select id="preflight-voice">${voiceOptions(draft.voiceId)}</select>
            </label>
          </section>

          <aside class="preflight-note">
            <h3>Rules for this session</h3>
            <p>These edits are session-only. Permanent changes live in Settings. During the session, Relevant and Distraction add a rule for the last non-Supervisor window.</p>
            <button type="button" data-action="apply-likely-targets">Add likely apps/sites as focus</button>
          </aside>
        </div>

        ${renderRuleManager(preflightRules, "preflight", "Session rules")}

        <div class="modal-actions">
          <button type="button" data-action="close-preflight">Cancel</button>
          <button class="primary" type="button" data-action="confirm-start-session">Start session</button>
        </div>
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
    <section class="settings-grid">
      <section class="panel settings-top-panel">
        <h2>Defaults</h2>
        <div class="setting-row">
          <span>Strictness</span>
          <select id="default-strictness">
            ${strictnessOptions(settings.default_strictness)}
          </select>
        </div>
        <div class="setting-row">
          <span>Storage</span>
          <strong>MEMORY ONLY</strong>
        </div>
        <label class="setting-row toggle-row">
          <span>Enable side window</span>
          <span class="toggle-field">
            <input id="status-window-enabled" class="toggle-input" type="checkbox" ${settings.status_window_enabled ? "checked" : ""} aria-label="Enable side window" />
            <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
          </span>
        </label>
      </section>

      <section class="panel settings-top-panel voice-summary-panel">
        <h2>Voice</h2>
        <div class="voice-summary">
          <strong>${escapeHtml(selectedVoice().name)}</strong>
          <span>${starterVoices.length} starter voices / ${phraseBank.length} bundled phrases</span>
          <button type="button" data-action="open-voice-settings">Customize</button>
        </div>
      </section>

      <section class="panel settings-top-panel">
        <h2>Webcam</h2>
        <div class="voice-summary">
          <strong>Live camera check</strong>
          <span>Face, gaze, phone. Local only.</span>
          <button type="button" data-action="open-webcam-settings">Open</button>
        </div>
      </section>

      <section class="panel settings-rules-panel">
        ${renderRuleManager(permanentRules, "settings", "Permanent rules")}
      </section>

      <section class="panel settings-note-panel">
        <h2>Windows signal note</h2>
        <p class="quiet">Foreground app, PID, title, bounds, and idle time use documented Win32 desktop APIs. Titles can still be empty during app switches, custom-title windows, UWP hosting, or protected-process cases.</p>
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
            <span id="webcam-chip">${webcamStatusChip(frame, alerting)}</span>
            <span id="webcam-mode">${webcamModeLabel()}</span>
          </div>
          <h2 id="webcam-title">${escapeHtml(webcamTitle(frame, alerting))}</h2>
          <p id="webcam-copy">${escapeHtml(webcamCopy(frame, alerting))}</p>
          <div class="webcam-frame-wrap${frame?.jpeg ? " is-live" : ""}" id="webcam-frame-wrap">
            <img id="webcam-preview" alt="Live webcam preview" />
            <p id="webcam-waiting" class="webcam-waiting">Waiting for the first frame</p>
          </div>
          <p id="webcam-live-error" class="webcam-live-error"${webcamError ? "" : " hidden"}>${escapeHtml(webcamError)}</p>
        </section>

        <section class="panel signal-panel">
          <h2>Phone checks</h2>
          <label class="setting-row toggle-row">
            <span>Watch phone use</span>
            <span class="toggle-field">
              <input id="webcam-phone-use" class="toggle-input" type="checkbox" ${webcamSettings.phoneUse ? "checked" : ""} aria-label="Watch phone use" />
              <span class="toggle-track" aria-hidden="true"><span class="toggle-knob"></span></span>
            </span>
          </label>
          <div class="setting-row">
            <span>Strictness</span>
            <select id="webcam-strictness" ${webcamSettings.phoneUse ? "" : "disabled"}>
              <option value="off" ${webcamSettings.strict ? "" : "selected"}>Not strict</option>
              <option value="on" ${webcamSettings.strict ? "selected" : ""}>Strict</option>
            </select>
          </div>
          <div class="setting-row">
            <span>When it fires</span>
            <select id="webcam-reaction" ${webcamSettings.phoneUse ? "" : "disabled"}>
              <option value="alarm" ${webcamSettings.reaction === "alarm" ? "selected" : ""}>Alarm</option>
              <option value="voice" ${webcamSettings.reaction === "voice" ? "selected" : ""}>Voice clip</option>
            </select>
          </div>
          ${renderWebcamMetrics(frame)}
          <p class="quiet">Frames stay on this PC. Nothing is stored. At-phone only if a phone is in the camera.</p>
        </section>
      </section>
    </section>
  `;
}

function renderWebcamMetrics(frame: WebcamFrame | null): string {
  return `
    <div class="metric-grid webcam-metrics">
      ${metric("Face", frame?.face_in_frame ? "IN" : "OUT")}
      ${metric("Present", webcamPresentLabel(frame))}
      ${metric("Phone", frame?.phone_in_frame ? "IN FRAME" : "NO")}
      ${metric("Alert", webcamWouldAlert(frame) ? "YES" : "NO")}
    </div>
  `;
}

function renderRuleManager(rules: AppRule[], scope: RuleScope, title: string): string {
  const activeEditor = ruleEditor?.scope === scope ? ruleEditor : null;
  return `
    <div class="rule-manager">
      <div class="rule-toolbar">
        <h2>${escapeHtml(title)}</h2>
        <button class="add-button" type="button" data-rule-action="add" data-rule-scope="${scope}" aria-label="Add rule">+</button>
      </div>
      ${activeEditor ? renderRuleEditor(activeEditor) : ""}
      ${renderRuleList(rules, scope)}
    </div>
  `;
}

function renderRuleEditor(editor: RuleEditor): string {
  return `
    <form class="rule-editor" data-rule-form="${editor.scope}">
      <label>
        <span>Target</span>
        <select name="target">${ruleTargetOptions(editor.target)}</select>
      </label>
      <label>
        <span>Mode</span>
        <select name="classification">${ruleClassificationOptions(editor.classification)}</select>
      </label>
      <label>
        <span>Match text</span>
        <input name="value" maxlength="120" value="${attr(editor.value)}" placeholder="YouTube API docs" />
      </label>
      <div class="rule-editor-actions">
        <button class="primary" type="submit">Save</button>
        <button type="button" data-rule-action="cancel" data-rule-scope="${editor.scope}">Cancel</button>
      </div>
    </form>
  `;
}

function renderRuleList(rules: AppRule[], scope: RuleScope): string {
  if (rules.length === 0) {
    return `<p class="empty">No rules yet.</p>`;
  }

  return `
    <div class="rule-table">
      <div class="rule-row rule-head">
        <span>Target</span>
        <span>Mode</span>
        <span>Match</span>
        <span>Actions</span>
      </div>
      ${rules
        .map(
          (rule, index) => `
            <div class="rule-row">
              <span class="rule-source">${escapeHtml(ruleTargetLabel(rule.target))}</span>
              <span>${rulePill(rule.classification)}</span>
              <strong>${escapeHtml(rule.value)}</strong>
              <span class="rule-actions">
                <button type="button" data-rule-action="edit" data-rule-scope="${scope}" data-rule-index="${index}">Edit</button>
                <button type="button" data-rule-action="delete" data-rule-scope="${scope}" data-rule-index="${index}">Delete</button>
              </span>
            </div>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderVoiceSettings(): string {
  const voices = availableVoices();
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
          <h2>Voices</h2>
          <div class="voice-list">
            ${voices.map(renderVoiceCard).join("")}
          </div>
        </section>

        <section class="panel clone-panel">
          <h2>Local clone</h2>
          ${renderClonePanel()}
        </section>

        <section class="panel phrase-panel">
          <div class="rule-toolbar">
            <h2>Phrase bank</h2>
            <button type="button" data-action="open-phrase-editor" ${canEditPhrases() ? "" : "disabled"}>Add phrase</button>
          </div>
          ${phraseEditor ? renderPhraseEditor() : ""}
          ${renderPhraseLibrary()}
        </section>
      </section>
    </section>
  `;
}

function renderVoiceCard(voice: VoiceOption): string {
  const selected = draft.voiceId === voice.id;
  const profile = voice.profile;
  const needsGeneration = Boolean(profile?.reference_retained && profile.clip_count === 0);
  const canGenerate = needsGeneration && modelInstalled() && voiceBusy === null && ramAllowsGeneration();
  const canUse = voice.selectable && voiceBusy === null;
  const action = needsGeneration ? "generate-audio" : "select-voice";
  const actionLabel = needsGeneration ? "Generate audio" : selected ? "Selected" : "Use";
  const disabled = needsGeneration ? !canGenerate : !canUse || selected;

  return `
    <article class="voice-card ${selected ? "selected" : ""}">
      <div class="voice-card-main">
        <strong>${escapeHtml(voice.name)}</strong>
        <span>${escapeHtml(voice.kind)} / ${escapeHtml(voice.coverage)}</span>
        <p>${escapeHtml(voice.detail)}</p>
        ${profile && profile.clip_count > 0 ? renderClipPreview(profile) : ""}
      </div>
      <div class="voice-actions">
        <button class="${needsGeneration ? "compact-action" : ""}" type="button" data-action="${action}" data-voice-id="${escapeHtml(voice.id)}" data-profile-id="${escapeHtml(profile?.profile_id ?? "")}" ${disabled ? "disabled" : ""} title="${needsGeneration && ramGenerationLocked() ? attr(ramLockMessage()) : ""}">${needsGeneration && ramGenerationLocked() ? "Clone locked" : actionLabel}</button>
        ${
          profile
            ? `<button class="danger" type="button" data-action="confirm-delete-voice" data-profile-id="${escapeHtml(profile.profile_id)}">Delete</button>`
            : ""
        }
      </div>
    </article>
  `;
}

function renderClonePanel(): string {
  const installed = modelInstalled();
  const status = voiceBackend?.model;
  const runtime = voiceBackend?.runtime;
  const system = voiceBackend?.system;
  const downloading = voiceDownload?.phase === "downloading" || voiceDownload?.phase === "cancelling";
  const busy = voiceBusy !== null || downloading;
  const downloadLabel =
    voiceDownload?.phase === "cancelling"
      ? "Cancelling..."
      : voiceDownload?.phase === "downloading"
        ? "Downloading..."
        : "Download model pack";

  return `
    <div class="clone-status">
      <strong>${installed ? "Voice pack ready" : "Voice pack not downloaded"}</strong>
      <p>Cloning is a short-lived local job. Focus sessions only play saved clips. Clone only your own voice or a voice you have permission to use.</p>
      <div class="model-facts">
        <span>${voiceBackend ? `${voiceBackend.profiles.length} custom voices` : "Checking voices"}</span>
        <span>${runtime?.installed ? "Runtime ready" : "Runtime missing"}</span>
        <span>${status ? `${formatBytes(status.installed_bytes)} model` : "No model status"}</span>
        <span>${status ? `${formatBytes(status.recommended_free_bytes)} disk` : "Disk ?"}</span>
      </div>
    </div>
    ${renderRamLock()}
    <div class="clone-actions">
      <button class="primary" type="button" data-action="download-model-pack" ${installed || busy ? "disabled" : ""}>${downloadLabel}</button>
      <button type="button" data-action="record-voice" ${!installed || busy ? "disabled" : ""}>Record voice</button>
      <button type="button" data-action="delete-model-files" ${!installed || busy ? "disabled" : ""}>Delete model files, keep voices</button>
    </div>
  `;
}

function renderRamLock(): string {
  const system = voiceBackend?.system;
  const locked = ramGenerationLocked();
  const free = system?.available_ram_bytes ?? null;
  const need = system?.min_free_ram_bytes ?? 0;
  const peak = system?.last_peak_ram_bytes ?? null;
  const fill = need > 0 && free !== null ? Math.max(4, Math.min(100, Math.round((free / need) * 100))) : 0;
  const copy = locked
    ? `This PC does not have enough free RAM for a clone job. Need about ${formatBytes(need)} free. Close browsers and other heavy apps, then come back. Recording a voice is cheap; generating clips is the heavy step.`
    : peak
      ? `Clone is allowed. Last job peaked at ${formatBytes(peak)}. Supervisor will refuse again if free RAM drops below ${formatBytes(need)}.`
      : `Clone is allowed, using a conservative ${formatBytes(need)} free-RAM floor until a real peak is measured on this PC.`;

  return `
    <aside class="ram-lock ${locked ? "is-locked" : "is-clear"}" role="status" aria-live="polite">
      <div class="ram-lock-stamp">${locked ? "CLONE LOCKED" : "CLONE CLEAR"}</div>
      <p>${escapeHtml(copy)}</p>
      <div class="ram-tank" aria-label="Free RAM versus clone budget">
        <span class="ram-tank-fill" style="width: ${fill}%"></span>
      </div>
      <div class="ram-lock-meta">
        <span>Free ${free === null ? "?" : formatBytes(free)}</span>
        <span>Need ${need ? formatBytes(need) : "?"}</span>
        <span>Peak ${peak === null ? "unmeasured" : formatBytes(peak)}</span>
        <span>Machine ${system?.total_ram_bytes ? formatBytes(system.total_ram_bytes) : "?"}</span>
      </div>
    </aside>
  `;
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
        <select name="tone">${toneOrder
          .map((tone) => `<option value="${tone}" ${tone === editor.tone ? "selected" : ""}>${toneLabel(tone)}</option>`)
          .join("")}</select>
      </label>
      <label>
        <span>Phrase</span>
        <input name="text" maxlength="140" value="${attr(editor.text)}" placeholder="Back to the task." />
      </label>
      <div class="rule-editor-actions">
        <button class="primary" type="submit">${editor.phraseId ? "Save phrase" : "Add phrase"}</button>
        <button type="button" data-action="cancel-phrase-editor">Cancel</button>
      </div>
    </form>
  `;
}

function renderPhraseLibrary(): string {
  const phrases = effectivePhrases();
  const editable = canEditPhrases();
  return `
    <div class="phrase-groups">
      ${toneOrder
        .map((tone) => {
          const items = phrases.filter((phrase) => phrase.tone === tone);
          return `
            <section class="phrase-group">
              <h3>${toneLabel(tone)}</h3>
              <div class="rows">
                ${items
                  .map(
                    (phrase) => `
                      <div class="row phrase-row">
                        <span>${escapeHtml(phraseKind(phrase))}</span>
                        <strong>${escapeHtml(phrase.text)}</strong>
                        <button class="small-button" type="button" data-action="open-phrase-editor" data-phrase-id="${escapeHtml(phrase.id)}" ${editable ? "" : "disabled"}>Edit</button>
                      </div>
                    `,
                  )
                  .join("")}
              </div>
            </section>
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
    ${showVoiceDialog && voiceDeleteTargetId ? renderDeleteVoiceModal() : ""}
    ${voiceBusy ? renderVoiceBusyModal() : ""}
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
            <p>${isRecording ? formatDuration(recordingDurationSeconds()) : recordedVoice ? `${formatDuration(recordedVoice.seconds)} captured` : "Use a clear 20-30 second sample with minimal background noise."}</p>
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
  const total = Math.max(0, download?.total_bytes ?? voiceBackend?.model.required_bytes ?? 0);
  const downloaded = Math.max(0, Math.min(download?.downloaded_bytes ?? 0, total || Number.MAX_SAFE_INTEGER));
  const percent = total > 0 ? Math.min(100, Math.round((downloaded / total) * 100)) : 0;
  const title = phase === "cancelling" ? "Cancelling download" : "Downloading model pack";
  const eta = download?.eta_seconds === null || download?.eta_seconds === undefined ? "Estimating time left" : `${formatDuration(download.eta_seconds)} left`;
  const progressText = total > 0 ? `${formatBytes(downloaded)} / ${formatBytes(total)}` : formatBytes(downloaded);

  return `
    <div class="modal-backdrop busy-backdrop" role="presentation">
      <section class="modal-panel busy-panel" role="dialog" aria-modal="true" aria-live="polite">
        <p class="eyebrow">VOICE WORKER</p>
        <h2>${escapeHtml(title)}</h2>
        <p>${escapeHtml(download?.message ?? "Downloading model pack.")}</p>
        <div class="loading-bar determinate" aria-label="Model download progress">
          <span style="width: ${percent}%"></span>
        </div>
        <div class="download-meta">
          <span>${escapeHtml(progressText)}</span>
          <span>${escapeHtml(phase === "cancelling" ? "Deleting partial files" : eta)}</span>
        </div>
        <p class="quiet busy-note">This is a ${total > 0 ? formatBytes(total) : "large"} first-time download and can take several minutes depending on your connection.</p>
        ${
          phase === "downloading"
            ? `<div class="modal-actions"><button class="danger" type="button" data-action="cancel-model-download">Cancel download</button></div>`
            : ""
        }
      </section>
    </div>
  `;
}

function renderClipPreview(profile: VoiceProfileSummary): string {
  const clips = profile.clips.slice(0, 2);
  if (clips.length === 0) {
    return "";
  }

  return `
    <div class="clip-preview">
      ${clips
        .map(
          (clip) => `
            <div>
              <span>${escapeHtml(clipLabel(clip))}</span>
              <audio controls preload="none" src="${attr(clipAssetUrl(clip.path))}"></audio>
            </div>
          `,
        )
        .join("")}
    </div>
  `;
}

function renderHistory(): string {
  if (history.length === 0) {
    return `
      <section class="panel">
        <h2>History</h2>
        <p class="empty">No saved sessions yet. Stop a session and the local summary lands here.</p>
      </section>
    `;
  }

  return `
    <section class="panel">
      <h2>History</h2>
      <div class="history-list">
        ${history
          .map(
            (item) => `
              <article class="history-item">
                <div>
                  <h3>${escapeHtml(item.goal)}</h3>
                  <p>${escapeHtml(new Date(item.started_at).toLocaleString())}</p>
                </div>
                <dl>
                  <div><dt>Focus</dt><dd>${formatDuration(item.focus_seconds)}</dd></div>
                  <div><dt>Drift</dt><dd>${formatDuration(item.distracted_seconds)}</dd></div>
                  <div><dt>Nudges</dt><dd>${item.nudges}</dd></div>
                </dl>
                <p class="hint-line">${escapeHtml(item.top_hints.join(" / ") || "No app hints")}</p>
              </article>
            `,
          )
          .join("")}
      </div>
    </section>
  `;
}

function bindEvents(): void {
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => {
    button.addEventListener("click", () => {
      const leavingWebcam = settingsView === "webcam";
      activeTab = button.dataset.tab as Tab;
      settingsView = "home";
      ruleEditor = null;
      render();
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
      handleRuleAction(button);
    });
  });

  document.querySelectorAll<HTMLFormElement>("[data-rule-form]").forEach((form) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      saveRuleForm(form);
    });
  });

  document.getElementById("phrase-editor")?.addEventListener("submit", (event) => {
    event.preventDefault();
    void savePhraseForm(event.currentTarget as HTMLFormElement);
  });

  bindDraftField("preflight-goal", "goal");
  bindDraftField("likely-targets", "likelyTargets");

  document.getElementById("preflight-duration")?.addEventListener("input", (event) => {
    draft.duration_minutes = Number((event.target as HTMLInputElement).value || 25);
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
    render();
  });

  document.getElementById("webcam-strictness")?.addEventListener("change", (event) => {
    webcamSettings.strict = (event.target as HTMLSelectElement).value === "on";
    webcamHitStreak = 0;
    paintWebcamDom();
  });

  document.getElementById("webcam-reaction")?.addEventListener("change", (event) => {
    webcamSettings.reaction = (event.target as HTMLSelectElement).value as WebcamReaction;
  });

  document.getElementById("voice-name")?.addEventListener("input", (event) => {
    if (recordedVoice) {
      recordedVoice.name = (event.target as HTMLInputElement).value;
    }
  });
}

function bindDraftField(id: string, key: "goal" | "likelyTargets"): void {
  document.getElementById(id)?.addEventListener("input", (event) => {
    draft[key] = (event.target as HTMLTextAreaElement).value;
  });
}

async function startSession(): Promise<void> {
  await invokeSnapshot("start_session", {
    input: {
      goal: draft.goal,
      duration_minutes: draft.duration_minutes,
      strictness: draft.strictness,
      rules: cloneRules(preflightRules),
    },
  });
}

async function runAction(action: string, source?: HTMLElement): Promise<void> {
  if (action === "open-preflight") {
    preflightRules = cloneRules(permanentRules);
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

  if (action === "apply-likely-targets") {
    const added = addLikelyTargetsToPreflight();
    showFeedback(added ? `${added} focus rules added` : "No new targets");
    return;
  }

  if (action === "confirm-start-session") {
    addLikelyTargetsToPreflight();
    preflightOpen = false;
    ruleEditor = null;
    await startSession();
    return;
  }

  if (action === "open-voice-settings") {
    settingsView = "voice";
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
    render();
    if (leavingWebcam) {
      await stopWebcamPreview();
    }
    return;
  }

  if (action === "select-voice") {
    const voiceId = source?.dataset.voiceId;
    const voice = voiceId ? availableVoices().find((item) => item.id === voiceId) : null;
    if (voiceId && voice?.selectable) {
      draft.voiceId = voiceId;
      await updateSelectedVoice(voiceId);
      showFeedback(`${voice.name} selected`);
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

  if (action === "record-voice") {
    if (!modelInstalled()) {
      showFeedback("Download voice pack first");
      return;
    }
    openRecordingDialog();
    return;
  }

  if (action === "delete-model-files") {
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
      openPhraseEditor(source?.dataset.phraseId || null);
    }
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

  if (action === "pin-window" || action === "unpin-window") {
    const enabled = action === "pin-window";
    if (await invokeSnapshot("set_status_window_on_top", { enabled })) {
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

  const markTarget = action.startsWith("mark-") ? markTargetLabel(snapshot?.current ?? null) : "";
  if (await invokeSnapshot(command) && action.startsWith("mark-")) {
    showFeedback(`${action === "mark-relevant" ? "Relevant" : "Distracting"}: ${markTarget}`);
  }
}

function handleRuleAction(button: HTMLButtonElement): void {
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
      classification: scope === "preflight" ? "focused" : "distracting",
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
    rules.splice(index, 1);
    if (ruleEditor?.scope === scope) {
      ruleEditor = null;
    }
    render();
  }
}

function saveRuleForm(form: HTMLFormElement): void {
  const scope = form.dataset.ruleForm as RuleScope | undefined;
  if (!scope || ruleEditor?.scope !== scope) {
    return;
  }

  const rule: AppRule = {
    target: formValue(form, "target") as RuleTarget,
    classification: formValue(form, "classification") as RuleClassification,
    value: formValue(form, "value").trim(),
  };

  if (!rule.value) {
    showFeedback("Rule needs text");
    return;
  }

  const rules = rulesForScope(scope);
  if (ruleEditor.index === null) {
    rules.unshift(rule);
  } else {
    rules[ruleEditor.index] = rule;
  }
  ruleEditor = null;
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

  phraseEditor = null;
  render();
  const profileId = selectedGeneratedProfileId();
  if (profileId) {
    const generated = await generateProfileAudio(profileId, wasEdit ? "Phrase saved. Audio regenerated." : "Phrase added. Audio generated.");
    if (!generated) {
      customPhrases = previousCustomPhrases;
      phraseOverrides = previousPhraseOverrides;
      phraseEditor = editor;
      render();
    }
  } else {
    showFeedback(wasEdit ? "Phrase saved" : "Phrase added");
  }
}

async function invokeSnapshot(command: string, args?: Record<string, unknown>): Promise<boolean> {
  try {
    snapshot = await invoke<SessionSnapshot>(command, args);
    settings = snapshot.settings;
    history = snapshot.history;
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
    render();
    reconcilePolling();
    return false;
  }

  render();
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
  if (await invokeSnapshot("set_status_window_enabled", { enabled })) {
    showFeedback(enabled ? "Side window enabled" : "Side window disabled");
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
      voiceBusy = null;
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
      voiceBusy = null;
      await refreshVoiceStatus(false);
      showFeedback("Model pack ready");
    } else if (download.phase === "idle") {
      stopDownloadPolling();
      voiceBusy = null;
      await refreshVoiceStatus(false);
      showFeedback("Download cancelled");
    } else if (download.phase === "error") {
      stopDownloadPolling();
      voiceBusy = null;
      voiceError = download.message;
      await refreshVoiceStatus(false);
    }
  } catch (error) {
    stopDownloadPolling();
    voiceBusy = null;
    voiceError = errorText(error);
  }
  render();
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
      recordingTickId = window.setInterval(render, 500);
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
    await updateSelectedVoice(draft.voiceId);
    await clearVoiceRecording();
    resetRecordedVoice();
    recordingState = "idle";
    showFeedback("Voice generated");
  });
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
    draft.voiceId = voiceOptionId(generated.profile_id);
    await updateSelectedVoice(draft.voiceId);
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
    if (draft.voiceId === voiceOptionId(profileId)) {
      draft.voiceId = "starter-female";
      await updateSelectedVoice(draft.voiceId);
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

  const shouldPollRam = appView === "main" && activeTab === "settings" && settingsView === "voice";
  if (shouldPollRam && voiceRamPollId === null) {
    voiceRamPollId = window.setInterval(() => {
      void refreshVoiceStatus(true);
    }, 3000);
  } else if (!shouldPollRam && voiceRamPollId !== null) {
    window.clearInterval(voiceRamPollId);
    voiceRamPollId = null;
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

function renderFeedback(): string {
  return feedbackMessage ? `<div class="feedback-toast" role="status">${escapeHtml(feedbackMessage)}</div>` : "";
}

function showFeedback(message: string): void {
  feedbackMessage = message;
  render();

  if (feedbackTimer !== null) {
    window.clearTimeout(feedbackTimer);
  }

  feedbackTimer = window.setTimeout(() => {
    feedbackMessage = "";
    feedbackTimer = null;
    render();
  }, 1400);
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
    wrap?.classList.add("is-live");
  }
  const chip = document.getElementById("webcam-chip");
  if (chip) {
    chip.textContent = webcamStatusChip(frame, alerting);
  }
  const mode = document.getElementById("webcam-mode");
  if (mode) {
    mode.textContent = webcamModeLabel();
  }
  const title = document.getElementById("webcam-title");
  if (title) {
    title.textContent = webcamTitle(frame, alerting);
  }
  const copy = document.getElementById("webcam-copy");
  if (copy) {
    copy.textContent = webcamCopy(frame, alerting);
  }
  const slab = document.querySelector(".webcam-stage");
  if (slab) {
    slab.className = `state-slab webcam-stage ${webcamStateClass(frame, alerting)}`;
  }
  const liveError = document.getElementById("webcam-live-error");
  if (liveError) {
    liveError.textContent = webcamError;
    liveError.hidden = !webcamError;
  }
  const metrics = document.querySelector(".webcam-metrics");
  if (metrics) {
    metrics.outerHTML = renderWebcamMetrics(frame);
  }
}

function webcamModeLabel(): string {
  if (!webcamSettings.phoneUse) {
    return "PREVIEW";
  }
  return webcamSettings.strict ? "STRICT" : "WATCH";
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

function webcamStatusChip(frame: WebcamFrame | null, alerting: boolean): string {
  if (alerting) {
    return "PHONE";
  }
  if (!frame) {
    return "STARTING";
  }
  return frame.face_in_frame ? "LIVE" : "NO FACE";
}

function webcamTitle(frame: WebcamFrame | null, alerting: boolean): string {
  if (alerting) {
    return "On the phone";
  }
  if (!frame) {
    return "Opening camera";
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
    return "Preview only. Turn on phone checks to fire an alarm or clip.";
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

function webcamPresentLabel(frame: WebcamFrame | null): string {
  if (frame?.present) {
    return "YES";
  }
  if (frame?.face_in_frame || frame?.person_in_frame) {
    return "RAW";
  }
  return "NO";
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

  if (current.status === "paused" || current.status === "snoozed") {
    return "state-muted";
  }

  return `state-${current.focus?.state ?? "neutral"}`;
}

function nudgeLabel(focus: FocusSnapshot): string {
  if (focus.should_nudge) {
    return "NOW";
  }

  if (focus.next_nudge_in_seconds !== null) {
    return `${focus.next_nudge_in_seconds}S`;
  }

  return "-";
}

async function playNudgeAudioIfNeeded(current: CurrentSession | null): Promise<void> {
  if (!current || current.status !== "active") {
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
  const clips = voice.profile?.clips ?? [];
  if (voice.source !== "custom" || clips.length === 0) {
    return null;
  }

  return clips[(Math.max(1, nudgeCount) - 1) % clips.length];
}

function ruleLabel(rule: AppRule): string {
  return `${ruleTargetLabel(rule.target)} ${rule.value}`;
}

function ruleTargetLabel(target: RuleTarget): string {
  switch (target) {
    case "app_name":
      return "App";
    case "window_title":
      return "Title / site";
    case "process_path":
      return "Path";
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

function rulePill(classification: RuleClassification): string {
  return `<span class="rule-pill rule-${classification}">${ruleClassificationLabel(classification)}</span>`;
}

function strictnessOptions(selected: Strictness): string {
  return (["gentle", "normal", "strict"] as Strictness[])
    .map((value) => `<option value="${value}" ${value === selected ? "selected" : ""}>${label(value)}</option>`)
    .join("");
}

function voiceOptions(selected: string): string {
  return availableVoices()
    .map(
      (voice) =>
        `<option value="${voice.id}" ${voice.id === selected ? "selected" : ""} ${voice.selectable ? "" : "disabled"}>${escapeHtml(voice.name)}</option>`,
    )
    .join("");
}

function ruleTargetOptions(selected: RuleTarget): string {
  return (["app_name", "window_title", "process_path"] as RuleTarget[])
    .map((target) => `<option value="${target}" ${target === selected ? "selected" : ""}>${ruleTargetLabel(target)}</option>`)
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

function addLikelyTargetsToPreflight(): number {
  let added = 0;
  for (const value of parseTargets(draft.likelyTargets)) {
    const exists = preflightRules.some(
      (rule) => rule.target === "window_title" && rule.value.toLowerCase() === value.toLowerCase(),
    );
    if (!exists) {
      preflightRules.unshift({ target: "window_title", value, classification: "focused" });
      added += 1;
    }
  }
  return added;
}

function parseTargets(value: string): string[] {
  return value
    .split(/[\n,]/)
    .map((part) => part.trim())
    .filter(Boolean)
    .slice(0, 12);
}

function rulesForScope(scope: RuleScope): AppRule[] {
  return scope === "settings" ? permanentRules : preflightRules;
}

function availableVoices(): VoiceOption[] {
  const customVoices =
    voiceBackend?.profiles.map((profile) => {
      const hasClips = profile.clip_count > 0;
      const hasReference = profile.reference_retained && Boolean(profile.reference_path);
      const expected = effectivePhrases().length;
      return {
        id: voiceOptionId(profile.profile_id),
        name: profile.display_name,
        kind: hasClips ? "Generated" : hasReference ? "Reference saved" : "Incomplete",
        detail: hasClips
          ? "Generated phrase clips are ready for playback and sessions."
          : hasReference
            ? "Reference clip is saved. Generate audio before using this voice."
            : "Reference clip is missing. Delete this profile or record again.",
        coverage: hasClips ? `${profile.clip_count}/${expected} clips` : "0 clips",
        source: "custom" as VoiceSource,
        selectable: hasClips,
        profile,
      };
    }) ?? [];

  return [...starterVoices, ...customVoices];
}

function selectedVoice(): VoiceOption {
  return availableVoices().find((voice) => voice.id === draft.voiceId) ?? starterVoices[1];
}

function shortVoiceName(name: string): string {
  return name.replace("Starter ", "").replace("Custom ", "Custom");
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
  return `Clone locked. Need about ${need} free RAM, this PC has ${free} free.`;
}

function canEditPhrases(): boolean {
  return modelInstalled() && voiceBusy === null;
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

function profileIdFromVoiceId(voiceId: string): string | null {
  return voiceId.startsWith("profile:") ? voiceId.slice("profile:".length) : null;
}

function selectedGeneratedProfileId(): string | null {
  const profileId = profileIdFromVoiceId(draft.voiceId);
  const profile = profileId ? profileById(profileId) : null;
  if (!profile?.reference_path || !profile.reference_retained) {
    return null;
  }
  return profile.profile_id;
}

function normalizeStoredVoiceId(value: string): string {
  if (value === "starter-male" || value === "starter-female" || value.startsWith("profile:")) {
    return value;
  }
  return "starter-female";
}

function ensureSelectedVoice(): void {
  const voice = availableVoices().find((item) => item.id === draft.voiceId);
  if (!voice || !voice.selectable) {
    draft.voiceId = "starter-female";
  }
}

function openPhraseEditor(phraseId: string | null): void {
  const phrase = phraseId ? effectivePhrases().find((item) => item.id === phraseId) : null;
  phraseEditor = {
    phraseId,
    tone: phrase?.tone ?? "gentle",
    text: phrase?.text ?? "",
  };
  render();
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

function clipLabel(clip: VoiceClipSummary): string {
  return clip.file_name.replace(/^\d+_/, "").replace(/\.wav$/i, "").replaceAll("_", " ");
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

function markTargetLabel(current: CurrentSession | null): string {
  const sample = current?.focus?.sample;
  if (!sample) {
    return "last window";
  }

  return sample.window_title?.trim() || sample.app_name || "last window";
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

function formatDuration(totalSeconds: number): string {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(safe / 60);
  const seconds = safe % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
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
