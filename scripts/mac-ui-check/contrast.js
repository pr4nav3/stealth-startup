// Loads every state from states.js plus a few more, in light and dark, and lists any text or
// icon too faint against what is really behind it. Run in the browser pane like states.js;
// the result is on window.__contrast. Under 3:1 is broken, under 4.5:1 is worth a look.
const source = await (await fetch("/scripts/mac-ui-check/states.js")).text();
const list = source.slice(source.indexOf("const W ="), source.indexOf("const results = [];"));
const { STATES, M, sleep, click, tab } = new Function(`${list}; return { STATES, M, sleep, click, tab };`)();
const button = (d, re) => { const b = [...d.querySelectorAll("button")].find((x) => re.test(x.textContent.trim())); if (!b) throw new Error("missing " + re); b.click(); };
const voice = async (d) => { tab(d, "settings"); await sleep(300); click(d, '[data-action="open-voice-settings"]'); await sleep(800); };
const EXTRA = [
  ["after-stop", M + "?live=1&voice=1", async (d) => { click(d, '[data-action="stop"]'); await sleep(1500); }],
  ["review", M + "?live=1&drift=1", async (d) => { click(d, '[data-action="stop"]'); await sleep(1500); }],
  ["trends-months", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); click(d, '[data-action="open-trends"]'); await sleep(500); click(d, '[data-unit="month"]'); }],
  ["record", M + "?done=1", async (d) => { await voice(d); click(d, '[data-action="record-voice"]'); }],
  ["voice-delete", M + "?done=1&dad=1", async (d) => { await voice(d); click(d, '[data-action="voice-info"][data-voice-id="mum"]'); await sleep(400); click(d, '[data-action="delete-voice-profile"]'); }],
  ["model-delete", M + "?done=1", async (d) => { await voice(d); click(d, '[data-action="open-model-info"]'); await sleep(400); click(d, '[data-action="ask-delete-model"]'); }],
  ["voice-help", M + "?done=1", async (d) => { await voice(d); click(d, '[data-action="open-voice-help"]'); }],
  ["tone-menu", M + "?done=1&tones=Coach", async (d) => { await voice(d); click(d, '[data-tone="Coach"]'); await sleep(500); click(d, '[data-action="toggle-tone-menu"]'); }],
  // The real card needs a ready closing line, which the mock has none of; same markup, same styles.
  ["session-done", M + "?big=1", (d) => { d.body.insertAdjacentHTML("beforeend", `<div class="modal-backdrop session-over-backdrop"><section class="modal-panel session-over-card"><section class="state-slab interrupt-alert moment-round-over"><h2 class="moment-title">Session done.</h2><p class="moment-goal">Thesis intro</p><p class="moment-focused"><b>29 min</b> focused</p><div class="interrupt-voice"><button class="voice-orb" type="button"><span class="voice-orb-core"><svg class="voice-orb-play" viewBox="0 0 12 12"><path d="M2.5 1v10L11 6z" fill="currentColor"/></svg></span></button><div class="interrupt-voice-text"><p class="interrupt-voice-who">Elise said</p><p class="interrupt-voice-line">Nice work.</p></div></div></section><div class="interrupt-actions moment-actions"><button class="primary" type="button">See how it went</button></div></section></div>`); }],
  ["report-delete-after", M + "?big=1&voice=1", async (d) => { tab(d, "history"); await sleep(600); click(d, ".history-item"); await sleep(500); click(d, '[data-action="ask-delete-session"]'); }],
  ["voice-two", M + "?done=1&two=1&tones=1", voice],
  ["voice-generating", M + "?done=1&gen_ms=60000", async (d) => { await voice(d); button(d, /^generate/i); }],
  ["rule-error", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); [...d.querySelectorAll(".settings-rules-panel button")].find((b) => /add/i.test(b.textContent))?.click(); await sleep(300); const i = d.querySelector(".rule-editor input"); i.value = "chrome"; i.dispatchEvent(new Event("input", { bubbles: true })); await sleep(600); }],
  ["rule-preview", M + "?done=1", async (d) => { tab(d, "settings"); await sleep(300); [...d.querySelectorAll(".settings-rules-panel button")].find((b) => /add/i.test(b.textContent))?.click(); await sleep(300); const i = d.querySelector(".rule-editor input"); i.value = "reddit.com"; i.dispatchEvent(new Event("input", { bubbles: true })); await sleep(600); }],
  ["app-edit-distraction", M + "?big=1", async (d) => { tab(d, "history"); await sleep(600); click(d, ".history-item"); await sleep(500); click(d, '[data-action="edit-app"]'); await sleep(300); click(d, '[data-counts="distraction"]'); }],
  ["live-phone", M + "?live=1&phone=1", null],
  ["live-long", M + "?live=1&long=1", null],
];
// color-mix() computes to color(srgb 0.1 0.2 0.3 / 0.4): channels from 0 to 1.
const parse = (text) => { const m = text.replace("srgb", "").match(/[\d.]+(e-?\d+)?/g).map(Number); const k = text.startsWith("color(") ? 255 : 1; return { r: m[0] * k, g: m[1] * k, b: m[2] * k, a: m[3] ?? 1 }; };
const over = (top, under) => ({ r: top.r * top.a + under.r * (1 - top.a), g: top.g * top.a + under.g * (1 - top.a), b: top.b * top.a + under.b * (1 - top.a), a: 1 });
const lum = (c) => { const f = (v) => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4; }; return 0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b); };
const ratio = (a, b) => { const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p); return (x + 0.05) / (y + 0.05); };
const hex = (c) => "#" + [c.r, c.g, c.b].map((v) => Math.round(v).toString(16).padStart(2, "0")).join("");
function backdrop(el, win) {
  const layers = [];
  let image = false;
  for (let at = el; at; at = at.parentElement) {
    const style = win.getComputedStyle(at);
    if (style.backgroundImage !== "none" && at !== at.ownerDocument.documentElement && at !== at.ownerDocument.body && at.tagName !== "SELECT") image = true;
    const color = parse(style.backgroundColor);
    if (color.a > 0) { layers.push(color); if (color.a === 1) break; }
  }
  let base = { r: 255, g: 255, b: 255, a: 1 };
  if (!layers.length || layers[layers.length - 1].a < 1) base = parse(win.getComputedStyle(el.ownerDocument.documentElement).backgroundColor);
  for (const layer of layers.reverse()) base = over(layer, base);
  return { color: base, image };
}
function opacity(el, win) { let o = 1; for (let at = el; at; at = at.parentElement) o *= Number(win.getComputedStyle(at).opacity); return o; }
const label = (el) => { const names = [...el.classList].filter((c) => c !== "__on").slice(0, 3); return el.tagName.toLowerCase() + (names.length ? "." + names.join(".") : ""); };
function scan(d, win, theme, state, out) {
  const modals = [...d.querySelectorAll(".modal-backdrop")];
  const roots = modals.length ? [modals[modals.length - 1]] : [d.body];
  for (const root of roots) for (const el of [root, ...root.querySelectorAll("*")]) {
    const style = win.getComputedStyle(el);
    const rect = el.getBoundingClientRect();
    if (style.display === "none" || style.visibility === "hidden" || rect.width < 2 || rect.height < 2) continue;
    const icon = el.tagName === "svg";
    const field = el.tagName === "INPUT" && el.type !== "checkbox" || el.tagName === "TEXTAREA" || el.tagName === "SELECT";
    const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (!icon && !field && !own) continue;
    if (el.closest("[hidden]") || el.closest("svg") && !icon) continue;
    const o = opacity(el, win);
    if (o < 0.05) continue;
    const { color: bg, image } = backdrop(el, win);
    const raw = parse(style.color);
    const fg = over({ ...raw, a: raw.a * o }, bg);
    const value = ratio(fg, bg);
    const size = parseFloat(style.fontSize);
    const need = icon ? 3 : size >= 24 ? 3 : 4.5;
    if (value >= need) continue;
    const key = `${theme}|${label(el)}|${hex(fg)}|${hex(bg)}`;
    (out[key] ||= { theme, el: label(el), fg: hex(fg), bg: hex(bg), ratio: Math.round(value * 100) / 100, image, disabled: el.disabled === true, text: (el.textContent || el.value || "").trim().slice(0, 40), states: [] }).states.push(state);
  }
}
const found = {};
const missed = [];
const reached = {};
const overflow = [];

for (const [name, url, setup, w, h] of [...STATES, ...EXTRA]) {
  const frame = document.createElement("iframe");
  frame.style.cssText = `position:fixed;left:0;top:0;width:${w || 920}px;height:${h || 760}px;border:0;z-index:9999;background:#fff`;
  document.body.appendChild(frame);
  const loaded = new Promise((r) => (frame.onload = r));
  frame.src = url;
  await loaded; await sleep(900);
  const d = frame.contentDocument;
  const s = d.createElement("style"); s.textContent = "*,*::before,*::after{animation:none!important;transition:none!important}"; d.head.appendChild(s);
  try {
    if (setup) await setup(d);
    await sleep(700);
    reached[name] = [...d.querySelectorAll(".modal-panel")].map((m) => m.className).join(" ; ");
    if (d.documentElement.scrollWidth > frame.contentWindow.innerWidth) overflow.push(name);
    for (const theme of ["light", "dark"]) {
      if (theme === "dark") d.documentElement.dataset.theme = "dark"; else delete d.documentElement.dataset.theme;
      await sleep(60);
      scan(d, frame.contentWindow, theme, name, found);
    }
    // Every :hover and :focus-visible look at once, by rewriting those rules to a class.
    const forced = [];
    for (const sheet of d.styleSheets) { try { for (const rule of sheet.cssRules) if (rule.selectorText && /:hover|:focus-visible/.test(rule.selectorText)) forced.push(rule.cssText.replace(/:hover|:focus-visible/g, ".__on")); } catch {} }
    const on = d.createElement("style"); on.textContent = forced.join(" "); d.head.appendChild(on);
    d.querySelectorAll("*").forEach((el) => el.classList.add("__on"));
    for (const theme of ["light", "dark"]) {
      if (theme === "dark") d.documentElement.dataset.theme = "dark"; else delete d.documentElement.dataset.theme;
      await sleep(60);
      scan(d, frame.contentWindow, theme + "-hover", name, found);
    }
  } catch (e) { missed.push(`${name}: ${e}`); }
  frame.remove();
}
window.__contrast = { checked: STATES.length + EXTRA.length, missed, reached, overflow, found: Object.values(found).sort((a, b) => a.ratio - b.ratio) };
return "done";
