import { invoke } from "@tauri-apps/api/core";
import "./styles.css";

type PermissionState = "granted" | "missing" | "not_required";
type RuleClassification = "focused" | "neutral" | "distracting";
type RuleTarget = "app_name" | "process_path" | "window_title";
type Strictness = "gentle" | "normal" | "strict";
type SessionStatus = "idle" | "active" | "paused" | "snoozed" | "complete";
type FocusState = "focused" | "neutral" | "suspicious" | "distracted" | "idle";
type Tab = "session" | "settings" | "history";
type AppView = "main" | "overlay";

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

const app = getAppRoot();
const appView: AppView = new URLSearchParams(window.location.search).get("view") === "overlay" ? "overlay" : "main";
document.body.classList.toggle("overlay-view", appView === "overlay");

const starterRules: AppRule[] = ["YouTube", "Netflix", "TikTok", "Instagram", "Reddit"].map((value) => ({
  target: "window_title",
  value,
  classification: "distracting",
}));

const draft = {
  goal: "",
  duration_minutes: 25,
  strictness: "normal" as Strictness,
};

let activeTab: Tab = "session";
let snapshot: SessionSnapshot | null = null;
let settings: SessionSettings = {
  default_strictness: "normal",
  voice_profile: "Starter voice",
  status_window_enabled: true,
  status_window_on_top: true,
};
let history: SessionSummary[] = [];
let lastError = "";
let feedbackMessage = "";
let feedbackTimer: number | null = null;
let pollId: number | null = null;

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
  } catch (error) {
    lastError = errorText(error);
  }

  await refreshSnapshot();
}

async function refreshSnapshot(): Promise<void> {
  try {
    snapshot = await invoke<SessionSnapshot>("get_session_snapshot");
    settings = snapshot.settings;
    history = snapshot.history;
    lastError = "";
  } catch (error) {
    lastError = errorText(error);
  }

  render();
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
      ${permissionNotice(current)}

      ${activeTab === "session" ? renderSession(current) : ""}
      ${activeTab === "settings" ? renderSettings(current) : ""}
      ${activeTab === "history" ? renderHistory() : ""}
      ${renderFeedback()}
    </main>
  `;

  bindEvents();
}

function renderSession(current: CurrentSession | null): string {
  return `
    <section class="session-grid">
      <section class="state-slab ${stateClass(current)}" aria-live="polite">
        <div class="state-head">
          <span>${statusChip(current)}</span>
          <span>${current ? escapeHtml(label(current.strictness)) : "READY"}</span>
        </div>
        <h2>${escapeHtml(stateTitle(current))}</h2>
        <p>${escapeHtml(stateCopy(current))}</p>
        ${current ? renderProgress(current) : ""}
      </section>

      <section class="panel start-panel">
        <h2>${current ? "Session controls" : "Start session"}</h2>
        ${current ? renderControls(current) : renderStartForm()}
      </section>

      <section class="panel metrics-panel">
        <h2>Session signal</h2>
        ${renderMetrics(current)}
      </section>

      <section class="panel activity-panel">
        <h2>Current window</h2>
        ${renderActivity(current)}
      </section>

      <section class="panel evidence-panel">
        <h2>Evidence log</h2>
        ${renderEvidence(current)}
      </section>
    </section>
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

function renderStartForm(): string {
  return `
    <form id="start-form" class="start-form">
      <label>
        <span>Goal</span>
        <input id="goal" name="goal" maxlength="120" value="${attr(draft.goal)}" placeholder="Finish the thing" />
      </label>
      <div class="form-row">
        <label>
          <span>Minutes</span>
          <input id="duration" name="duration" type="number" min="5" max="180" step="5" value="${draft.duration_minutes}" />
        </label>
        <label>
          <span>Strictness</span>
          <select id="strictness" name="strictness">
            ${strictnessOptions(draft.strictness)}
          </select>
        </label>
      </div>
      <button class="primary" type="submit">Start watching</button>
    </form>
  `;
}

function renderControls(current: CurrentSession): string {
  const paused = current.status === "paused";
  return `
    <div class="control-grid">
      <button class="primary" data-action="${paused ? "resume" : "pause"}">${paused ? "Resume" : "Pause"}</button>
      <button data-action="snooze">Snooze 5 min</button>
      <button data-action="mark-relevant">Mark relevant</button>
      <button data-action="mark-distracting">Mark distracting</button>
      <button class="danger" data-action="stop">Stop</button>
    </div>
  `;
}

function renderOverlayControls(current: CurrentSession): string {
  const paused = current.status === "paused";
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
        <button class="text-button" data-action="snooze">Mute nudges</button>
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
        ${metric("Drift", "-")}
        ${metric("Nudges", "0")}
        ${metric("Mode", "LOCAL")}
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

function renderEvidence(current: CurrentSession | null): string {
  const rows = current?.evidence ?? [];
  if (rows.length === 0) {
    return `<p class="empty">No evidence yet. Start the session and Supervisor will keep receipts.</p>`;
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
      <div class="rows">
        <div class="row"><span>Capture</span><strong>STANDBY</strong><time>-</time></div>
        <div class="row"><span>Windows note</span><strong>FALLIBLE TITLE</strong><time>OK</time></div>
      </div>
    `;
  }

  const sample = focus.sample;
  return `
    <div class="rows">
      <div class="row"><span>App</span><strong>${escapeHtml(sample.app_name || "-")}</strong><time>${escapeHtml(sample.platform)}</time></div>
      <div class="row"><span>Title</span><strong>${escapeHtml(sample.window_title || "Unavailable")}</strong><time>${formatDuration(focus.seconds_on_current_window)}</time></div>
      <div class="row"><span>Rule</span><strong>${escapeHtml(focus.matched_rule ? ruleLabel(focus.matched_rule) : "NO MATCH")}</strong><time>${escapeHtml(ruleClassificationLabel(focus.classification))}</time></div>
      <div class="row"><span>Nudge</span><strong>${escapeHtml(nudgeLabel(focus))}</strong><time>${sample.idle_seconds === null ? "IDLE ?" : `${sample.idle_seconds.toFixed(1)}S`}</time></div>
    </div>
  `;
}

function renderSettings(current: CurrentSession | null): string {
  const rules = current?.rules ?? starterRules;
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

      <section class="panel settings-top-panel">
        <h2>Voice</h2>
      </section>

      <section class="panel settings-rules-panel">
        <h2>Rule list</h2>
        ${renderRuleList(rules)}
      </section>

      <section class="panel settings-note-panel">
        <h2>Windows signal note</h2>
        <p class="quiet">Foreground app, PID, title, bounds, and idle time use documented Win32 desktop APIs. Titles can still be empty during app switches, custom-title windows, UWP hosting, or protected-process cases.</p>
      </section>
    </section>
  `;
}

function renderRuleList(rules: AppRule[]): string {
  if (rules.length === 0) {
    return `<p class="empty">No rules yet.</p>`;
  }

  return `
    <div class="rule-table">
      <div class="rule-row rule-head">
        <span>Source</span>
        <span>Mode</span>
        <span>Name</span>
      </div>
      ${rules
        .slice(0, 8)
        .map(
          (rule) => `
            <div class="rule-row">
              <span class="rule-source">${escapeHtml(ruleTargetLabel(rule.target))}</span>
              <span>${rulePill(rule.classification)}</span>
              <strong>${escapeHtml(rule.value)}</strong>
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
      activeTab = button.dataset.tab as Tab;
      render();
    });
  });

  document.getElementById("start-form")?.addEventListener("submit", (event) => {
    event.preventDefault();
    void startSession();
  });

  document.getElementById("goal")?.addEventListener("input", (event) => {
    draft.goal = (event.target as HTMLInputElement).value;
  });

  document.getElementById("duration")?.addEventListener("input", (event) => {
    draft.duration_minutes = Number((event.target as HTMLInputElement).value || 25);
  });

  document.getElementById("strictness")?.addEventListener("change", (event) => {
    draft.strictness = (event.target as HTMLSelectElement).value as Strictness;
  });

  document.getElementById("default-strictness")?.addEventListener("change", (event) => {
    void updateDefaultStrictness((event.target as HTMLSelectElement).value as Strictness);
  });

  document.getElementById("status-window-enabled")?.addEventListener("change", (event) => {
    void updateStatusWindowEnabled((event.target as HTMLInputElement).checked);
  });

  document.querySelectorAll<HTMLButtonElement>("[data-action]").forEach((button) => {
    button.addEventListener("click", () => {
      void runAction(button.dataset.action || "");
    });
  });
}

async function startSession(): Promise<void> {
  await invokeSnapshot("start_session", {
    input: {
      goal: draft.goal,
      duration_minutes: draft.duration_minutes,
      strictness: draft.strictness,
    },
  });
}

async function runAction(action: string): Promise<void> {
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

  if (await invokeSnapshot(command) && action.startsWith("mark-")) {
    showFeedback(action === "mark-relevant" ? "Marked relevant" : "Marked distracting");
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

async function updateStatusWindowPin(enabled: boolean): Promise<void> {
  if (await invokeSnapshot("set_status_window_on_top", { enabled })) {
    showFeedback(enabled ? "Window pinned" : "Window unpinned");
  }
}

async function updateStatusWindowEnabled(enabled: boolean): Promise<void> {
  if (await invokeSnapshot("set_status_window_enabled", { enabled })) {
    showFeedback(enabled ? "Side window enabled" : "Side window disabled");
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
    return "Set the target. Start the watch. Keep it local.";
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

function ruleLabel(rule: AppRule): string {
  return `${ruleTargetLabel(rule.target)} ${rule.value}`;
}

function ruleTargetLabel(target: RuleTarget): string {
  switch (target) {
    case "app_name":
      return "App";
    case "window_title":
      return "Website";
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
