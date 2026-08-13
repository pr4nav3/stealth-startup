# Supervisor Style Guide: Pure BrainDock-ish

## Purpose

This guide captures the "pure BrainDock-ish" visual direction we discussed for Supervisor. It is meant to be handed to a coding/design agent as a full knowledge transfer: what the style is, why it fits the product, what to copy, what to avoid, and how to implement a representative UI.

The target is not a legal or pixel-for-pixel clone of BrainDock. Copy the design grammar: warm off-white surfaces, visible borders, friendly productivity-console structure, soft yellow alert states, rounded controls, subtle grid texture, simple reports, and gentle nudges. Do not copy BrainDock's exact assets, screenshots, illustrations, brand marks, or wording.

## Discussion Context

Supervisor is a desktop productivity/supervision product. The core experience is not "AI dashboard" or "wellness app." It is a local desktop utility that watches focus state, detects drift, reports session evidence, and uses voice/personality nudges.

The UI exploration went like this:

- Generic SaaS UI felt too polished and forgettable.
- "AI slop" was specifically rejected: purple gradients, floating orbs, glassy cards, centered hero sections, generic shadcn defaults, and fake-polished dashboard templates.
- Neo-brutalism had the most personality among the directions explored.
- Gumroad looked great because it has chunky controls, confidence, and visible character.
- BrainDock felt closest to Supervisor's product category: local focus, session reports, distraction settings, away states, privacy framing, and soft nudge energy.
- The user wants to test two lanes:
  - Pure BrainDock-ish: softer, friendlier, closest to BrainDock's current style.
  - Mashup: BrainDock x Gumroad x desktop command center, stricter and more memorable.

This file is the first lane: pure BrainDock-ish.

## One-Sentence Style Name

Soft Brutalist Productivity Console.

## Product Feeling

The app should feel like a friendly local focus tool with a little personality. It should feel direct and practical, but not hostile. It should look like a real desktop utility that happens to be playful.

The emotional target:

- helpful
- warm
- local-first
- simple
- tactile
- slightly opinionated
- not corporate
- not cyberpunk
- not wellness fog

If the mashup says "pay attention," this version says "you drifted, come back."

## Core Principles

1. Use warm utility instead of sterile polish.
2. Make state obvious: away, focused, drifting, session complete.
3. Use visible borders and chunky controls, but keep them rounded and friendly.
4. Use yellow as the main alert color.
5. Keep density moderate: more like a focused desktop companion than a packed analyst terminal.
6. Use small grid texture or dotted texture to make the canvas feel authored.
7. Use simple cards for repeated content, but do not turn every section into a floating SaaS card.
8. Use real product states and realistic session data.
9. Let copy be direct and plain, with soft nudge energy.
10. Build a usable app screen first, not a marketing page.

## Visual DNA

| Trait | Direction |
| --- | --- |
| Base surface | Warm off-white paper |
| Panels | Cream/yellow-tinted panels |
| Borders | 2px dark brown/charcoal borders |
| Radius | Rounded, usually 10-16px |
| Shadows | Soft hard-ish offset shadow, not blurred SaaS shadow |
| Main accent | Warm yellow |
| Secondary accents | Soft green, sky blue, coral red |
| Texture | Subtle square grid |
| Typography | Heavy clean sans for labels and state; monospace for system metadata |
| Mood | Friendly focus companion |
| Density | Moderate |
| Copy | Gentle and useful |

## Design Tokens

Use these tokens as the starting point. Slight adjustments are fine, but do not drift into purple/blue gradient SaaS.

```css
:root {
  --paper: #f7f1df;
  --panel: #fff8df;
  --panel-warm: #fff7da;
  --ink: #1f1d18;
  --muted: #6f6657;
  --line: #322c23;
  --yellow: #ffd84d;
  --green: #9bd66f;
  --blue: #80b7ff;
  --red: #f36b4f;
  --white-soft: #fffdf2;
  --shadow-soft: 0 5px 0 rgba(31, 29, 24, 0.22);

  --radius-shell: 16px;
  --radius-card: 14px;
  --radius-control: 999px;
  --border: 2px solid var(--line);

  --font-sans: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --font-mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
```

## Typography

Use a clean sans-serif that can carry heavy weights. Inter, Geist, Satoshi, or system UI are all acceptable. Do not use overly futuristic or gamer fonts.

Rules:

- State titles are big and heavy, but not hero-scale.
- Use monospace for small system labels: session time, local mode, event types, rule names.
- Keep letter spacing at `0` for normal text. Eyebrow labels may use a tiny positive spacing if needed.
- Avoid huge marketing H1s inside the application.
- Prefer short labels over explanatory paragraphs.

Example hierarchy:

| Role | Size | Weight | Notes |
| --- | --- | --- | --- |
| Screen title | 24-32px | 800-900 | Short, plain |
| State title | 36-52px | 900 | "Away", "Focus steady", "Drifting" |
| Card heading | 13-16px | 800 | Compact |
| Body copy | 13-15px | 600-700 | Short |
| Metadata | 10-12px | 700-800 monospace | Uppercase allowed |

## Layout Rules

Use an app-first layout. The first screen should be the working Supervisor dashboard, not a landing-page hero.

Recommended screen structure:

- Left or top area: live state card.
- Middle area: session evidence/report.
- Side area: intervention settings and saved voice/rule controls.
- Bottom or secondary area: timeline, recent nudges, privacy/local status.

For this pure style, use comfortable spacing. The interface can be dense enough to feel useful, but not so dense that it becomes a command terminal.

Grid:

- Use 12-18px gaps for cards.
- Use 14-18px internal padding.
- Use 18px background grid texture at low opacity.
- Keep panel radius around 14-16px.

## Component Rules

### State Card

The state card is the visual anchor. It should be a large yellow panel with a blunt status title.

Good state labels:

- `Away`
- `Focus steady`
- `Drifting`
- `Session done`
- `Back at desk`
- `Phone seen`

Pure BrainDock-ish tone:

- "Gentle nudge energy."
- "You stepped away for 03:12."
- "Soft voice alert is ready."
- "Session report is saved locally."

Avoid:

- Aggressive labels like `PAY ATTENTION` in this pure style.
- Cybersecurity wording like `THREAT DETECTED`.
- Therapy/wellness wording like "honor your attention journey."

### Session Report Card

Use a simple bordered card with stacked rows. It should read like evidence, but softly.

Rows can include:

- Deep work streak
- Phone reaches
- Longest drift
- Away time
- Nudges delivered
- Saved voice used

Use monospace values on the right.

### Distraction Rules Card

Use a rounded toggle and pill buttons. Controls should feel tactile.

Controls:

- Soft voice alert toggle
- Camera/local watch toggle
- App drift detection toggle
- Start session button
- Tune rules button

For pure style, buttons can be pill-shaped.

### Mini Metrics

Use three small boxes under the state card. They should be readable at a glance.

Examples:

- `82%` focus
- `7` nudges
- `1h` session
- `LOCAL` mode
- `2` phone reaches

## Color Usage

Yellow should do the main emotional work. It makes the state visible without making the app feel hostile.

Use accents like this:

- Yellow: primary alert/status slabs.
- Green: enabled state, "on track", success.
- Blue: primary action, informational state.
- Red/coral: warning, repeated drift, destructive action.
- Off-white: base surface.
- Charcoal/brown: text and borders.

Avoid:

- Purple gradients.
- Glassmorphism.
- Neon cyberpunk surfaces.
- Dark navy dashboards.
- Too many accent colors at once.

## Texture

Use a subtle grid background:

```css
background:
  linear-gradient(rgba(31, 29, 24, 0.045) 1px, transparent 1px),
  linear-gradient(90deg, rgba(31, 29, 24, 0.045) 1px, transparent 1px),
  var(--paper);
background-size: 18px 18px;
```

Keep it quiet. It should create a designed-paper feel, not a loud graph-paper gimmick.

## Motion

Motion should be small and functional:

- Toggle knob moves.
- State card snaps from `Focus steady` to `Away`.
- Report row appears after a session ends.
- Button press has a short tactile movement.

Avoid:

- Floating cards.
- Ambient blobs.
- Constant pulsing.
- Decorative idle animation.

## Copy Tone

Copy should be plain, friendly, and specific.

Good:

- "Soft voice alert when posture or app state looks off."
- "You were away for 03:12."
- "Session report saved locally."
- "Tune the rules before the next block."

Bad:

- "Unlock peak performance with AI."
- "Your productivity companion empowers focus."
- "Crush distraction forever."
- "Let us optimize your digital flow."

## UI Library Recommendation

For implementation, use headless or lightly styled components:

- Base UI + Tailwind/CSS is the preferred foundation.
- React Aria is good if accessibility and complex controls matter.
- Mantine is acceptable for a desktop utility if fully themed.
- shadcn is acceptable only as a behavior/scaffold source. Do not keep default shadcn styling.

Do not import a heavy visual kit that forces generic SaaS styling.

## Agent Instructions

If this guide is given to a coding agent, instruct it to:

1. Build one representative Supervisor dashboard screen first.
2. Use the tokens in this guide before inventing new colors.
3. Implement the state card, report card, and rules card before adding secondary UI.
4. Use realistic session data, not lorem ipsum.
5. Verify desktop and mobile screenshots.
6. Check empty, active, alert, disabled, and completed states.
7. Keep the first version close to the embedded HTML reference.
8. Avoid marketing-page layout unless explicitly asked.

## Acceptance Checklist

The UI is on-style if:

- It feels warm, useful, and local.
- The state card is visually dominant.
- Yellow alert/status surfaces are prominent.
- Borders are visible.
- Controls are chunky and rounded.
- The app does not look like default shadcn, Linear, Stripe, or generic AI SaaS.
- The copy feels direct but not mean.
- The layout looks like an app screen, not a landing page.
- The interface can plausibly support focus sessions, voice nudges, saved voices, and session reports.

The UI is off-style if:

- It uses purple/blue gradients or glass effects.
- It has too many soft floating cards.
- It looks like a wellness habit tracker.
- It looks like a cyberpunk surveillance console.
- It uses vague AI copy.
- It removes the chunky/tactile feeling.

## Embedded HTML Reference Page

Copy the following into an `.html` file to preview the pure BrainDock-ish direction. This is intentionally compact: it demonstrates the visual grammar, not a full product.

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Supervisor Pure BrainDock-ish Cards</title>
    <style>
      :root {
        --paper: #f7f1df;
        --panel: #fff8df;
        --panel-warm: #fff7da;
        --ink: #1f1d18;
        --muted: #6f6657;
        --line: #322c23;
        --yellow: #ffd84d;
        --green: #9bd66f;
        --blue: #80b7ff;
        --red: #f36b4f;
        --shadow-soft: 0 5px 0 rgba(31, 29, 24, 0.22);
        font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
        color: var(--ink);
        background: var(--paper);
      }

      * {
        box-sizing: border-box;
      }

      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
        background:
          linear-gradient(rgba(31, 29, 24, 0.045) 1px, transparent 1px),
          linear-gradient(90deg, rgba(31, 29, 24, 0.045) 1px, transparent 1px),
          var(--paper);
        background-size: 18px 18px;
      }

      .surface {
        width: min(760px, 100%);
        display: grid;
        gap: 12px;
        padding: 14px;
        border: 2px solid var(--line);
        border-radius: 16px;
        background:
          linear-gradient(rgba(31, 29, 24, 0.045) 1px, transparent 1px),
          linear-gradient(90deg, rgba(31, 29, 24, 0.045) 1px, transparent 1px),
          var(--paper);
        background-size: 18px 18px;
      }

      .surface-head {
        display: flex;
        justify-content: space-between;
        gap: 10px;
        align-items: flex-start;
        padding: 2px 2px 4px;
      }

      h1 {
        margin: 0;
        font-size: clamp(22px, 3vw, 31px);
        line-height: 1.04;
        letter-spacing: 0;
        font-weight: 900;
      }

      .tag {
        flex: 0 0 auto;
        display: inline-flex;
        align-items: center;
        border: 2px solid currentColor;
        border-radius: 999px;
        padding: 4px 8px;
        font: 700 11px/1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        text-transform: uppercase;
        letter-spacing: 0.04em;
        background: var(--yellow);
      }

      .grid {
        display: grid;
        grid-template-columns: 1.2fr 0.8fr;
        gap: 12px;
      }

      .stack {
        display: grid;
        gap: 12px;
      }

      .card {
        min-width: 0;
        border: 2px solid var(--line);
        border-radius: 14px;
        background: var(--panel);
        box-shadow: var(--shadow-soft);
        padding: 14px;
      }

      .status-card {
        background: var(--yellow);
      }

      .eyebrow {
        margin: 0 0 8px;
        color: var(--muted);
        font: 800 11px/1.2 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        text-transform: uppercase;
        letter-spacing: 0.06em;
      }

      .status-title {
        display: flex;
        align-items: flex-end;
        justify-content: space-between;
        gap: 12px;
        margin: 0 0 12px;
        font-size: clamp(38px, 8vw, 64px);
        line-height: 0.95;
        font-weight: 900;
        letter-spacing: 0;
      }

      .status-title span:last-child {
        flex: 0 0 auto;
        font: 900 13px/1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        border: 2px solid currentColor;
        border-radius: 999px;
        padding: 8px 10px;
        background: rgba(255, 255, 255, 0.38);
        letter-spacing: 0;
      }

      .card-copy {
        margin: 0;
        max-width: 34ch;
        color: var(--muted);
        font-size: 13px;
        line-height: 1.35;
        font-weight: 650;
      }

      .mini-grid {
        display: grid;
        grid-template-columns: repeat(3, minmax(0, 1fr));
        gap: 8px;
        margin-top: 12px;
      }

      .mini,
      .metric {
        min-width: 0;
        border: 2px solid var(--line);
        border-radius: 10px;
        background: rgba(255, 255, 255, 0.48);
        padding: 9px;
      }

      .mini b,
      .metric b {
        display: block;
        font-size: 21px;
        line-height: 1;
        letter-spacing: 0;
        font-weight: 900;
      }

      .mini span,
      .metric span {
        display: block;
        margin-top: 5px;
        color: var(--muted);
        font: 700 10px/1.15 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        text-transform: uppercase;
        letter-spacing: 0.04em;
      }

      .row {
        display: flex;
        justify-content: space-between;
        gap: 10px;
        align-items: center;
        padding: 9px 0;
        border-bottom: 2px solid rgba(50, 44, 35, 0.16);
      }

      .row:last-child {
        border-bottom: 0;
        padding-bottom: 0;
      }

      .row strong {
        font-size: 13px;
        line-height: 1.15;
        font-weight: 850;
      }

      .row small {
        color: var(--muted);
        font: 750 11px/1.1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        text-align: right;
      }

      .control-row {
        display: grid;
        grid-template-columns: 1fr auto;
        gap: 10px;
        align-items: center;
        margin-top: 10px;
      }

      .switch {
        width: 50px;
        height: 29px;
        border: 2px solid var(--line);
        border-radius: 999px;
        background: var(--green);
        position: relative;
        box-shadow: inset 0 -2px 0 rgba(31, 29, 24, 0.2);
      }

      .switch::after {
        content: "";
        position: absolute;
        width: 19px;
        height: 19px;
        right: 4px;
        top: 3px;
        border: 2px solid var(--line);
        border-radius: 50%;
        background: var(--panel);
      }

      .button-row {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 9px;
        margin-top: 12px;
      }

      .fake-button {
        border: 2px solid var(--line);
        border-radius: 999px;
        background: var(--panel);
        padding: 10px 8px;
        text-align: center;
        font-size: 12px;
        line-height: 1;
        font-weight: 900;
      }

      .fake-button.primary {
        background: var(--blue);
      }

      .metric-row {
        display: grid;
        grid-template-columns: repeat(2, minmax(0, 1fr));
        gap: 9px;
        margin-top: 10px;
      }

      @media (max-width: 720px) {
        .grid {
          grid-template-columns: 1fr;
        }
      }

      @media (max-width: 430px) {
        body {
          padding: 14px;
        }

        .surface {
          padding: 11px;
        }

        .surface-head,
        .status-title {
          align-items: flex-start;
          flex-direction: column;
        }

        .mini-grid,
        .metric-row,
        .button-row {
          grid-template-columns: 1fr;
        }
      }
    </style>
  </head>
  <body>
    <main class="surface" aria-label="Supervisor pure BrainDock-ish reference UI">
      <div class="surface-head">
        <h1>Pure BrainDock-ish</h1>
        <span class="tag">Softer</span>
      </div>

      <section class="grid">
        <article class="card status-card">
          <p class="eyebrow">Desk state</p>
          <h2 class="status-title"><span>Away</span><span>03:12</span></h2>
          <p class="card-copy">Gentle nudge energy. Friendly, rounded, yellow-alert, less judgmental.</p>
          <div class="mini-grid" aria-label="Session statistics">
            <div class="mini"><b>82%</b><span>focus</span></div>
            <div class="mini"><b>7</b><span>nudges</span></div>
            <div class="mini"><b>1h</b><span>session</span></div>
          </div>
        </article>

        <div class="stack">
          <article class="card">
            <p class="eyebrow">Session report</p>
            <div class="row"><strong>Deep work streak</strong><small>38 min</small></div>
            <div class="row"><strong>Phone reaches</strong><small>2 times</small></div>
            <div class="row"><strong>Longest drift</strong><small>04:44</small></div>
          </article>

          <article class="card">
            <p class="eyebrow">Distraction rules</p>
            <div class="control-row">
              <p class="card-copy">Soft voice alert when posture or app state looks off.</p>
              <span class="switch" aria-hidden="true"></span>
            </div>
            <div class="button-row">
              <span class="fake-button primary">Start session</span>
              <span class="fake-button">Tune rules</span>
            </div>
          </article>
        </div>
      </section>
    </main>
  </body>
</html>
```

