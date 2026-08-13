# Supervisor Style Guide: BrainDock x Gumroad x Command Center Mashup

## Purpose

This guide captures the second visual direction we discussed for Supervisor: a stricter mashup of BrainDock's soft productivity-console style, Gumroad's chunky personality, and a dense desktop command-center feel.

This is the more opinionated option, but it has been revised after feedback. It should keep the square borders, tactile shadow, confident colors, and Supervisor bite, while calming down the typography so the cards remain easy to skim. The personality should come from the composition, state colors, borders, copy, and controls, not from making every line bold.

Copy the design grammar, not exact third-party assets. Do not copy BrainDock's exact imagery, Gumroad's exact brand treatment, or any existing product's unique layout. The goal is an ownable Supervisor system.

## Discussion Context

Supervisor is a desktop productivity/supervision product. The user wants a UI with personality, not generic "vibecoded AI slop."

Key decisions from the conversation:

- Generic polished SaaS is the wrong lane.
- Default shadcn, purple gradients, glassmorphism, decorative orbs, and AI-dashboard aesthetics are rejected.
- Neo-brutalism had the most personality in the initial moodboard.
- Gumroad looked great because it is chunky, tactile, confident, and not afraid of being a little weird.
- BrainDock is the closest product-style reference because it already lives near local focus, soft nudges, distraction settings, session reports, and privacy.
- The best Supervisor direction may be a mashup:
  - BrainDock base: warm, soft brutalist productivity console.
  - Gumroad personality: chunky controls, hard borders, confident accent colors.
  - Desktop command center: denser evidence, sharper state, more operational feel.

This file is the second lane: the mashup.

## Latest Feedback

The first mashup mock had the right bones but pushed too hard visually. The user liked:

- square borders
- the hard offset shadow, but wanted it toned down
- the color palette
- the stricter Supervisor personality

The user disliked:

- too much bold text inside cards
- too much visual clutter compared with the pure BrainDock-ish version
- typography that made every element compete for attention

The v2 correction is:

- Keep the square, tactile, high-contrast structure.
- Reduce the shadow to a shorter, softer hard offset.
- Use regular-weight body text inside cards.
- Use medium-weight labels instead of heavy labels.
- Let color blocks, borders, spacing, and state placement create hierarchy.
- Reserve emphasis for the main state, key numbers, and primary actions.
- Keep the eye drawn to important parts instead of shouting everywhere.

## One-Sentence Style Name

Playful Surveillance Console.

Alternative names:

- Strict Soft Brutalist Command Center
- BrainDock x Gumroad Desktop Utility
- Opinionated Local Supervision UI

## Product Feeling

The app should feel like a blunt local supervisor living on the laptop. It should not feel evil, corporate, motivational, or cutesy. It should feel like a serious desktop instrument with a strange little soul.

The emotional target:

- direct
- tactile
- funny in a dry way
- useful
- local-first
- alert
- slightly judgmental
- memorable
- dense enough to be credible

If the pure BrainDock-ish version says "you drifted, come back," this version says "Drifting. Pay attention." It is still more direct than BrainDock, but no longer visually yelling in every card.

## Core Principles

1. Start from BrainDock's warm, local productivity-console base.
2. Add Gumroad-like chunk: square borders, tactile controls, short hard shadows, expressive color.
3. Add desktop-command-center density: evidence logs, state labels, rule controls, local status.
4. Make states clear and memorable: `Drifting`, `Locked in`, `Phone detected`, `Voice ready`.
5. Use a limited palette, not a candy explosion.
6. Prefer flat, bordered panels over soft floating cards.
7. Use hard shadows sparingly and keep them toned down.
8. Use monospace labels for system state and verdicts.
9. Keep the app usable, readable, and skimmable. Personality must not destroy function.
10. Build the real app screen first, not a brand/marketing page.

## Visual DNA

| Trait | Direction |
| --- | --- |
| Base surface | Warm off-white / aged paper |
| Panels | Yellow-cream with visible dark borders |
| Borders | 2px near-black borders |
| Radius | Low radius, usually 4-6px |
| Shadows | Short hard offset shadows, partially softened |
| Main accent | Warning yellow |
| Secondary accents | Acid green, cobalt, danger red/orange |
| Texture | Subtle square grid, less busy than the first mashup |
| Typography | Regular sans for card text, monospace for system labels |
| Mood | Blunt desktop supervisor |
| Density | Medium, with command-center detail kept skim-friendly |
| Copy | Direct, sharp, a little funny |

## Design Tokens

Use these tokens as the starting point. The palette should feel tighter and harsher than pure BrainDock-ish, but the typography and shadows should be calmer than the first mashup mock.

```css
:root {
  --paper: #f4ead7;
  --panel: #fff4cf;
  --ink: #101010;
  --muted: #5d564a;
  --line: #101010;
  --acid: #b7ff3c;
  --cobalt: #3267ff;
  --warning: #ffcf31;
  --danger: #ff684f;
  --white-soft: #fff7df;
  --shadow: 4px 4px 0 rgba(17, 16, 14, 0.68);
  --shadow-control: 2px 2px 0 rgba(17, 16, 14, 0.62);

  --radius-shell: 6px;
  --radius-card: 5px;
  --radius-control: 4px;
  --border: 2px solid var(--line);

  --font-sans: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  --font-mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
}
```

## Typography

Typography should be blunt and operational, but not heavy everywhere. The v2 mashup should feel easier to skim than the first mock.

Rules:

- Use regular-weight sans for most card text.
- Use medium weight for state titles, metric numbers, and important controls.
- Use monospace for labels, values, status tags, verdicts, and buttons.
- Uppercase is allowed for small labels and controls, but avoid yelling in body text.
- Keep letter spacing at `0` for most text. Use slight positive spacing only for tiny eyebrow labels.
- Do not use delicate luxury editorial type here.
- Do not use cartoon/comic display fonts.
- Avoid stacking bold body copy, bold row labels, bold metadata, and bold buttons in the same card.

Example hierarchy:

| Role | Size | Weight | Notes |
| --- | --- | --- | --- |
| Screen title | 19-26px | 500-650 | Short, direct |
| State title | 30-48px | 500-650 | Title case preferred; uppercase only for special alerts |
| Verdict label | 11-12px | 500 mono | Example: `VOICE`, `LOCAL`, `PAY ATTENTION` |
| Card heading | 11-12px | 500 mono | Compact eyebrow label |
| Body copy | 13-15px | 400 | Direct, readable, regular |
| Metadata | 10-11px | 400-500 mono | Often uppercase, but not visually heavy |
| Metrics | 19-22px | 500 | Clear but not shouty |

## Layout Rules

Use a dashboard/control-panel layout. This should feel like the user opened Supervisor and can immediately start or inspect a focus session.

Recommended screen structure:

- Main state slab: current watch state and intervention status.
- Evidence log: recent events and why Supervisor is nudging.
- Intervention settings: voice, camera/app checks, rule sensitivity.
- Session metrics: lock percentage, strikes, local CPU/GPU mode, saved voice.
- Timeline or report area: after-session evidence and verdict.

For this mashup, density can be higher than BrainDock-ish, but the card interiors should stay calm:

- More rows.
- More labels.
- More compact control groups.
- More visible state.
- Slightly less friendly whitespace.
- Lighter text weights.
- Thinner row dividers.

But keep it readable. The goal is a real app, not a poster. The first version leaned too much toward poster energy; v2 should feel like a working control panel.

## Component Rules

### State Card

The state card should be the loudest element. It can use warning yellow with hard dark borders and a short shadow. The state surface can be loud; the supporting text inside it should remain regular-weight.

Good state labels:

- `Drifting`
- `Locked in`
- `Phone detected`
- `Voice ready`
- `Camera local`
- `Session saved`
- `Away`

Good small status chips:

- `VOICE`
- `LOCAL`
- `LIVE`
- `STRICT`
- `SAVED`
- `CPU`

Tone:

- blunt
- concise
- slightly dry
- never corporate
- never wellness-y

### Evidence Log

This is where the command-center part comes in. The UI should show proof, not vibes.

Rows can include:

- Focus window held
- Phone motion detected
- Away from desk
- Browser drift
- Voice nudge played
- Saved voice used
- Supervisor verdict

Use monospace values on the right:

- `42:08`
- `2 HITS`
- `PAY ATTENTION`
- `LOCAL`
- `VOICE 03`

Row labels should usually be regular weight. Use a thin, partially transparent divider rather than a heavy black rule between every row.

### Intervention Settings

Controls should be blocky and tactile.

Use:

- squared toggle with visible knob
- short hard-shadow buttons
- bordered sliders or segmented controls
- monospace labels
- chunky selected states

Buttons should feel pressed into the UI, not floating above it. Button text can be medium-weight monospace; it does not need to be ultra-bold.

### Mini Metrics

Use small metric boxes to make the app feel like a real instrument.

Examples:

- `91%` locked
- `4` strikes
- `CPU` local
- `2` phone hits
- `03` saved voice
- `12m` clean streak

Metric numbers can be medium-weight and large enough to scan. Labels should stay light monospace so the metric boxes do not become noisy.

## Color Usage

The palette should be loud but disciplined.

Use accents like this:

- Warning yellow: primary state/alert surfaces.
- Danger red/orange: repeated drift, sharp nudges, primary intervention.
- Acid green: enabled state, live watch, "locked in."
- Cobalt: secondary action or tool control.
- Off-white: base surface.
- Black: borders, shadows, type.

Rules:

- Never use more than two loud accents in one card unless the UI state truly requires it.
- Use black borders to keep the palette grounded.
- Prefer flat fills over gradients.
- Avoid purple entirely unless a future brand decision explicitly requires it.

## Texture

Use a subtle square grid. The first mashup's dot-grid energy can get busy; the v2 should borrow BrainDock's calmer skimmability while still feeling authored.

```css
background:
  linear-gradient(rgba(17, 16, 14, 0.045) 1px, transparent 1px),
  linear-gradient(90deg, rgba(17, 16, 14, 0.045) 1px, transparent 1px),
  var(--paper);
background-size: 18px 18px;
```

Keep it behind the shell, not inside every component.

## Motion

Motion should be tactile and snappy:

- Button depresses 2-3px on press.
- Toggle moves with a short snap.
- State card changes with a quick flash/fill swap.
- Evidence row slides in or appears with a short pop.

Avoid:

- Soft drifting animations.
- Ambient glow.
- Long easing curves.
- Decorative parallax.

## Copy Tone

Copy should feel like a blunt local utility, not a motivational coach.

Good:

- `DRIFTING`
- `LOCK IN`
- `PAY ATTENTION`
- `Phone motion detected.`
- `Voice nudge gets sharper after repeated drift.`
- `No wellness fog.`
- `Session saved locally.`

Bad:

- "Unlock peak productivity."
- "Your AI companion empowers focus."
- "You are on a beautiful journey of mindful attention."
- "Harness next-generation agentic productivity."

This UI can be funny, but it should stay useful. Dry, short copy is better than jokes everywhere.

The v2 type direction means copy should read cleanly too. Avoid making every noun phrase uppercase. Let `Drifting`, `Voice`, `Lock in`, and `Edit rules` coexist with ordinary sentence-case explanatory text.

## UI Library Recommendation

For implementation, use headless or lightly styled components:

- Base UI + Tailwind/CSS is the preferred foundation.
- React Aria is good if accessibility and complex controls matter.
- Mantine is acceptable if fully reskinned.
- shadcn is acceptable only as a scaffold. Do not keep default shadcn theme, spacing, shadows, or button style.

Important: encode the style as tokens first. Do not let every component choose its own random hex values.

## Agent Instructions

If this guide is given to a coding agent, instruct it to:

1. Build one representative Supervisor dashboard screen first.
2. Use this guide's tokens and component rules before inventing new visuals.
3. Start with the live state card, evidence log, and intervention settings.
4. Use realistic session data and real Supervisor states.
5. Make the state language clear and visible, but do not make every row bold.
6. Verify desktop and mobile screenshots.
7. Check active, alert, disabled, completed, empty, and error states.
8. Keep the first implementation close to the embedded v2 HTML reference.
9. Preserve usability. Personality is allowed, but the app must still be easy to operate and skim.
10. Use color and layout before font weight when establishing hierarchy.

## Acceptance Checklist

The UI is on-style if:

- It clearly feels like BrainDock's softer productivity-console language, but stricter.
- The state card is loud and memorable.
- Borders are hard and visible.
- Shadows are offset and graphic, but toned down.
- Buttons are chunky and tactile.
- There is a subtle grid or similarly authored background texture.
- The evidence log makes the product feel real.
- Body text inside cards is regular-weight.
- Row labels are readable without becoming visually heavy.
- Important parts are obvious at a skim.
- Copy is direct and slightly judgmental without becoming hostile.
- It does not look like generic AI SaaS.
- It feels local-first and desktop-like.

The UI is off-style if:

- It becomes a pure Gumroad candy clone.
- It becomes a pure BrainDock clone with no Supervisor bite.
- It uses soft SaaS shadows and pale cards everywhere.
- It uses the first mashup's heavy bold treatment everywhere.
- It makes the hard shadow too thick or too black.
- It turns evidence rows into visual clutter.
- It uses purple gradients, glass, or decorative orbs.
- It becomes cyberpunk, gamer, or hacker-terminal themed.
- It hides useful state behind vague friendliness.

## Embedded HTML Reference Page

Copy the following into an `.html` file to preview the Supervisor mashup v2 direction. This version reflects the latest feedback: square borders and color attitude stay, shadow is toned down, and card interiors use regular-weight text so the UI remains skimmable.

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>Supervisor Mashup V2 Cards</title>
    <style>
      body {
        margin: 0;
        min-height: 100vh;
        display: grid;
        place-items: center;
        padding: 24px;
        background: #f4ead7;
      }

      @media (max-width: 430px) {
        body {
          padding: 14px;
        }
      }
    </style>
  </head>
  <body>
<div id="supervisor-mashup-v2" aria-label="Supervisor mashup style revision">
  <style>
    #supervisor-mashup-v2 {
      --paper: #f4ead7;
      --panel: #fff4cf;
      --ink: #11100e;
      --muted: #655d50;
      --line: #171410;
      --warning: #ffcf31;
      --danger: #ff684f;
      --acid: #b7ff3c;
      --cobalt: #3267ff;
      --soft: rgba(255, 255, 255, 0.52);
      --shadow: 4px 4px 0 rgba(17, 16, 14, 0.68);
      width: 100%;
      color: var(--ink);
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }

    #supervisor-mashup-v2 * {
      box-sizing: border-box;
    }

    #supervisor-mashup-v2 .surface {
      display: grid;
      gap: 12px;
      padding: 14px;
      border: 2px solid var(--line);
      border-radius: 6px;
      background:
        linear-gradient(rgba(17, 16, 14, 0.045) 1px, transparent 1px),
        linear-gradient(90deg, rgba(17, 16, 14, 0.045) 1px, transparent 1px),
        var(--paper);
      background-size: 18px 18px;
    }

    #supervisor-mashup-v2 .topbar {
      display: flex;
      align-items: flex-start;
      justify-content: space-between;
      gap: 12px;
      padding: 2px 2px 4px;
    }

    #supervisor-mashup-v2 h2 {
      margin: 0;
      font-size: clamp(19px, 2.6vw, 26px);
      line-height: 1.05;
      font-weight: 500;
      letter-spacing: 0;
    }

    #supervisor-mashup-v2 .tag {
      flex: 0 0 auto;
      display: inline-flex;
      align-items: center;
      border: 2px solid var(--line);
      border-radius: 4px;
      background: var(--acid);
      padding: 5px 8px;
      color: var(--ink);
      font: 500 11px/1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      letter-spacing: 0.02em;
      text-transform: uppercase;
      box-shadow: 2px 2px 0 rgba(17, 16, 14, 0.62);
    }

    #supervisor-mashup-v2 .grid {
      display: grid;
      grid-template-columns: minmax(0, 1.08fr) minmax(0, 0.92fr);
      gap: 12px;
      align-items: start;
    }

    #supervisor-mashup-v2 .stack {
      display: grid;
      gap: 12px;
    }

    #supervisor-mashup-v2 .card {
      min-width: 0;
      border: 2px solid var(--line);
      border-radius: 5px;
      background: var(--panel);
      box-shadow: var(--shadow);
      padding: 14px;
    }

    #supervisor-mashup-v2 .status-card {
      background: var(--warning);
    }

    #supervisor-mashup-v2 .eyebrow {
      margin: 0 0 8px;
      color: var(--muted);
      font: 500 11px/1.2 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      letter-spacing: 0.04em;
      text-transform: uppercase;
    }

    #supervisor-mashup-v2 .status-title {
      display: flex;
      align-items: flex-end;
      justify-content: space-between;
      gap: 12px;
      margin: 0 0 11px;
      font-size: clamp(30px, 5vw, 48px);
      line-height: 0.95;
      font-weight: 500;
      letter-spacing: -0.02em;
    }

    #supervisor-mashup-v2 .status-title span:last-child {
      flex: 0 0 auto;
      border: 2px solid var(--line);
      border-radius: 4px;
      background: var(--danger);
      padding: 7px 9px;
      font: 500 12px/1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      letter-spacing: 0.02em;
      text-transform: uppercase;
    }

    #supervisor-mashup-v2 .copy {
      margin: 0;
      max-width: 42ch;
      color: var(--muted);
      font-size: 13px;
      line-height: 1.4;
      font-weight: 400;
    }

    #supervisor-mashup-v2 .mini-grid {
      display: grid;
      grid-template-columns: repeat(3, minmax(0, 1fr));
      gap: 8px;
      margin-top: 12px;
    }

    #supervisor-mashup-v2 .mini {
      min-width: 0;
      border: 2px solid rgba(17, 16, 14, 0.78);
      border-radius: 4px;
      background: rgba(255, 255, 255, 0.45);
      padding: 9px;
    }

    #supervisor-mashup-v2 .mini b,
    #supervisor-mashup-v2 .metric b {
      display: block;
      color: var(--ink);
      font-size: 21px;
      line-height: 1;
      font-weight: 500;
      letter-spacing: -0.02em;
    }

    #supervisor-mashup-v2 .mini span,
    #supervisor-mashup-v2 .metric span {
      display: block;
      margin-top: 5px;
      color: var(--muted);
      font: 400 10px/1.15 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      letter-spacing: 0.03em;
      text-transform: uppercase;
    }

    #supervisor-mashup-v2 .row {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 10px 0;
      border-bottom: 1px solid rgba(17, 16, 14, 0.24);
    }

    #supervisor-mashup-v2 .row:last-child {
      border-bottom: 0;
      padding-bottom: 0;
    }

    #supervisor-mashup-v2 .row strong {
      min-width: 0;
      color: var(--ink);
      font-size: 13px;
      line-height: 1.2;
      font-weight: 400;
    }

    #supervisor-mashup-v2 .row small {
      flex: 0 0 auto;
      color: var(--muted);
      font: 400 11px/1.1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      text-align: right;
    }

    #supervisor-mashup-v2 .metric-row {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 9px;
      margin-top: 11px;
    }

    #supervisor-mashup-v2 .metric {
      border: 2px solid rgba(17, 16, 14, 0.78);
      border-radius: 4px;
      background: var(--soft);
      padding: 10px;
    }

    #supervisor-mashup-v2 .control-row {
      display: grid;
      grid-template-columns: 1fr auto;
      gap: 12px;
      align-items: center;
      margin-top: 1px;
    }

    #supervisor-mashup-v2 .switch {
      width: 50px;
      height: 29px;
      border: 2px solid var(--line);
      border-radius: 4px;
      background: var(--acid);
      box-shadow: 2px 2px 0 rgba(17, 16, 14, 0.62);
      position: relative;
    }

    #supervisor-mashup-v2 .switch::after {
      content: "";
      position: absolute;
      top: 4px;
      right: 5px;
      width: 17px;
      height: 17px;
      border: 2px solid var(--line);
      border-radius: 3px;
      background: var(--ink);
    }

    #supervisor-mashup-v2 .button-row {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 9px;
      margin-top: 12px;
    }

    #supervisor-mashup-v2 .fake-button {
      border: 2px solid var(--line);
      border-radius: 4px;
      background: var(--cobalt);
      color: #fff7df;
      box-shadow: 2px 2px 0 rgba(17, 16, 14, 0.62);
      padding: 10px 8px;
      text-align: center;
      font: 500 12px/1 ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      letter-spacing: 0.01em;
    }

    #supervisor-mashup-v2 .fake-button.primary {
      background: var(--danger);
      color: var(--ink);
    }

    @media (max-width: 760px) {
      #supervisor-mashup-v2 .grid {
        grid-template-columns: 1fr;
      }
    }

    @media (max-width: 430px) {
      #supervisor-mashup-v2 .surface {
        padding: 11px;
      }

      #supervisor-mashup-v2 .topbar,
      #supervisor-mashup-v2 .status-title {
        align-items: flex-start;
        flex-direction: column;
      }

      #supervisor-mashup-v2 .mini-grid,
      #supervisor-mashup-v2 .metric-row,
      #supervisor-mashup-v2 .button-row {
        grid-template-columns: 1fr;
      }
    }
  </style>

  <section class="surface">
    <div class="topbar">
      <h2>Supervisor Mashup V2</h2>
      <span class="tag">Calmer</span>
    </div>

    <div class="grid">
      <article class="card status-card">
        <p class="eyebrow">Local watch</p>
        <h3 class="status-title"><span>Drifting</span><span>Voice</span></h3>
        <p class="copy">Same chunky command-center DNA, but the card text sits at normal weight so the alert color and state label do the work.</p>
        <div class="mini-grid" aria-label="Live supervision statistics">
          <div class="mini"><b>91%</b><span>locked</span></div>
          <div class="mini"><b>4</b><span>strikes</span></div>
          <div class="mini"><b>CPU</b><span>local</span></div>
        </div>
      </article>

      <div class="stack">
        <article class="card">
          <p class="eyebrow">Evidence log</p>
          <div class="row"><strong>Focus window held</strong><small>42:08</small></div>
          <div class="row"><strong>Phone motion detected</strong><small>2 hits</small></div>
          <div class="row"><strong>Supervisor verdict</strong><small>pay attention</small></div>
        </article>

        <article class="card">
          <p class="eyebrow">Intervention settings</p>
          <div class="control-row">
            <p class="copy">Voice nudge gets sharper after repeated drift. Direct, but not visually shouting.</p>
            <span class="switch" aria-hidden="true"></span>
          </div>
          <div class="button-row">
            <span class="fake-button primary">Lock in</span>
            <span class="fake-button">Edit rules</span>
          </div>
        </article>
      </div>
    </div>
  </section>
</div>
  </body>
</html>
```
