// Loads every app state in an iframe (same origin) and measures it. Same code runs in
// WebKit (webkit-eval) and Chromium (browser pane); the outputs are diffed.
const W = typeof STATE_WIDTH !== "undefined" ? STATE_WIDTH : 920;
const H = typeof STATE_HEIGHT !== "undefined" ? STATE_HEIGHT : 760;
const M = "http://127.0.0.1:1420/mock.html";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const click = (d, sel) => { const el = d.querySelector(sel); if (!el) throw new Error("missing " + sel); el.click(); };
const tab = (d, name) => click(d, `[data-tab="${name}"]`);
const STATES = [
  ["home", M, null],
  ["home-empty", M + "?empty=1", null],
  ["home-first", M + "?first=1", null],
  ["onboarding", M, (d) => click(d, '[data-action="onboard-show"]')],
  ["preflight", M, (d) => click(d, '[data-action="open-preflight"]')],
  ["preflight-open-blocks", M, async (d) => { click(d, '[data-action="open-preflight"]'); await sleep(300); d.querySelectorAll(".preflight-panel details, .preflight-panel summary").forEach((e) => { if (e.tagName === "DETAILS") e.open = true; }); }],
  ["live", M + "?live=1", null],
  ["live-drift", M + "?live=1&drift=1", null],
  ["live-allowed", M + "?live=1&allowed=1", null],
  ["live-rules", M + "?live=1", (d) => click(d, '[data-action="open-session-rules"]')],
  ["settings", M + "?done=1", (d) => tab(d, "settings")],
  ["settings-rule-editor", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); [...d.querySelectorAll(".settings-rules-panel button")].find((b) => /add/i.test(b.textContent))?.click(); }],
  ["settings-delete-all", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); [...d.querySelectorAll("button")].find((b) => /delete all history/i.test(b.textContent))?.click(); }],
  ["voice", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); }],
  ["voice-model-popup", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); [...d.querySelectorAll("button")].find((b) => /voice model/i.test(b.textContent))?.click(); }],
  ["voice-memory-popup", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); [...d.querySelectorAll("button")].find((b) => /memory/i.test(b.textContent))?.click(); }],
  ["voice-line-popup", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); [...d.querySelectorAll("button")].find((b) => /edit/i.test(b.textContent))?.click(); }],
  ["voice-tones-popup", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); [...d.querySelectorAll("button")].find((b) => /manage/i.test(b.textContent))?.click(); }],
  ["voice-new-voice", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); [...d.querySelectorAll("button")].find((b) => /new voice/i.test(b.textContent))?.click(); }],
  ["voice-info", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); [...d.querySelectorAll("button")].find((b) => /^info$/i.test(b.textContent.trim()))?.click(); }],
  ["voice-lowram", M + "?done=1&lowram=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); }],
  ["voice-nomodel", M + "?done=1&nomodel=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); }],
  ["voice-offline", M, async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); }],
  ["webcam", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-webcam-settings"]'); }],
  ["history", M + "?big=1", (d) => tab(d, "history")],
  ["history-picker", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); click(d, '[data-action="toggle-week-picker"]'); }],
  ["history-apps", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); [...d.querySelectorAll('[data-action="history-pane"]')].pop()?.click(); }],
  ["history-many", M + "?many=1", (d) => tab(d, "history")],
  ["history-empty", M + "?empty=1", (d) => tab(d, "history")],
  ["report", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); click(d, ".history-item"); }],
  ["report-delete", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); click(d, ".history-item"); await sleep(500); click(d, '[data-action="ask-delete-session"]'); }],
  ["trends", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); click(d, '[data-action="open-trends"]'); }],
  ["overlay-corner", M + "?view=overlay&live=1", null, 228, 400],
  ["overlay-interrupt", M + "?view=overlay&live=1&drift=1", null, 520, 600],
];
const results = [];
for (const [name, url, setup, w, h] of STATES) {
  const frame = document.createElement("iframe");
  frame.style.cssText = `position:fixed;left:0;top:0;width:${w || W}px;height:${h || H}px;border:0;z-index:9999;background:#fff`;
  document.body.appendChild(frame);
  const loaded = new Promise((r) => (frame.onload = r));
  frame.src = url;
  await loaded; await sleep(900);
  const d = frame.contentDocument;
  const s = d.createElement("style"); s.textContent = "*,*::before,*::after{animation:none!important;transition:none!important}"; d.head.appendChild(s);
  try {
    if (setup) await setup(d);
    await sleep(900);
    const sizes = {};
    for (const sel of [".app-shell", ".home-grid", ".modal-panel", ".preflight-panel", ".state-slab", ".interrupt-alert", ".settings-page", ".voice-page", ".webcam-page", ".history-page", ".report-panel", ".overlay-shell", ".cal-grid", ".quote-card", ".onboard-panel", ".history-item", ".voice-card", ".clips-panel", ".trend-card"]) {
      const el = d.querySelector(sel); if (el) { const r = el.getBoundingClientRect(); sizes[sel] = [Math.round(r.width), Math.round(r.height)]; }
    }
    const w2 = frame.contentWindow;
    results.push({ name, viewport: [w2.innerWidth, w2.innerHeight], docH: d.documentElement.scrollHeight, hOverflow: d.documentElement.scrollWidth > w2.innerWidth, modals: d.querySelectorAll(".modal-panel").length, sizes });
  } catch (e) { results.push({ name, error: String(e) }); }
  frame.remove();
}
return JSON.stringify(results);
