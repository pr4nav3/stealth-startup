# Supervisor UI Guidelines

**Theme:** Watchtower  
**Status:** Current authoritative UI direction  
**Updated:** September 28, 2026

This file consolidates the product and visual decisions made for Supervisor so far. Treat it as the default specification. Do not invent new colors, visual treatments, cards, badges, copy, or controls unless a requirement cannot be satisfied with this system.

---

## 1. Product intent

Supervisor is a desktop focus tool. It should feel like a quiet, capable local supervisor—not a dashboard the user must manage.

The experience should be:

- immediately understandable
- calm enough for repeated daily use
- tactile and memorable
- direct without being hostile
- visually playful without distracting from the task
- local-first and privacy-aware
- consistent across Windows and macOS

The design direction is **restrained neo-brutalism**. Use the useful parts of neo-brutalism—clear structure, confident color, visible borders, graphic shadows, and tactile controls—without importing unnecessary chaos.

### Primary usability rule

Each screen should have one obvious job and one obvious primary action. Remove anything that does not help the user complete that job.

### Product hierarchy

1. Start a session quickly.
2. Understand the current session state.
3. Respond to an intervention quickly.
4. Change advanced behavior only when needed.
5. Review history after the session.

---

## 2. What Supervisor must not look like

Do not use:

- generic AI SaaS styling
- purple gradients
- glassmorphism
- blurred or glowing shadows
- decorative orbs
- random new accent colors
- blue or cobalt accents unless explicitly approved later
- excessive pill-shaped controls
- heavy bold text in every card
- motivational filler or AI-generated-sounding microcopy
- large empty regions created only to make a layout feel minimal
- diagnostic panels on the home screen
- duplicate status tags such as `READY` and `STANDBY`
- default shadcn styling without a complete Supervisor reskin
- per-website brand colors in blocking controls

Do not copy another product's exact layout, artwork, or brand treatment. References such as BrainDock, Gumroad, and Design Prompts inform the grammar, not the final identity.

---

## 3. Theme tokens

The selected tonal option is the original light cream Supervisor palette.

### Light mode

| Token | Value | Use |
| --- | --- | --- |
| Canvas | `#F4EAD7` | Main application background |
| Panel | `#FFF4CF` | Cards, menus, settings groups |
| Panel alternate | `#EADFC9` | Title bars, inactive controls, secondary surfaces |
| Ink | `#101010` | Main text and borders |
| Muted ink | `#5D564A` | Secondary text and metadata |
| Shadow | `rgba(17, 16, 14, 0.68)` | All hard shadows |
| Green | `#B7FF3C` | Primary action, enabled state, positive/locked state |
| Yellow | `#FFCF31` | Attention, active navigation, focus state, quote tile |
| Coral | `#FF684F` | Stop, warning, intervention, destructive action |
| Soft white | `#FFF7DF` | Neutral control fill when needed |
| Grid line | `rgba(16, 16, 16, 0.045)` | Light-mode canvas grid |

### Dark mode

Dark mode must be charcoal gray rather than near-black. The canvas must remain light enough for black hard shadows to be visible.

| Token | Value | Use |
| --- | --- | --- |
| Canvas | `#40403C` | Main application background |
| Panel | `#52514C` | Cards, menus, settings groups |
| Panel alternate | `#484843` | Title bars, inactive controls, secondary surfaces |
| Ink | `#FFF8E9` | Text on neutral dark surfaces |
| Muted ink | `#DDD5C5` | Secondary text on neutral dark surfaces |
| Border | `#151512` | Borders on neutral dark surfaces |
| Shadow | `#0B0B09` | All hard shadows |
| Green | `#B7FF3C` | Same semantic color as light mode |
| Yellow | `#FFCF31` | Same semantic color as light mode |
| Coral | `#FF684F` | Same semantic color as light mode |
| Grid line | `rgba(255, 255, 255, 0.12)` | Dark-mode canvas grid |

### Colored-surface text rule

Text inside green, yellow, or coral surfaces must always be near-black: `#141410`.

This rule does not change in dark mode. The colored tiles retain their color, so switching their text to white reduces readability and is prohibited.

### Semantic color mapping

Color meaning must remain stable across every screen and both themes:

- **Green:** start, continue, enabled, focused, locked in
- **Yellow:** attention, current selection, focus state, editorial highlight
- **Coral:** stop, destructive action, active intervention, serious warning
- **Neutral cream/gray:** structure, settings, secondary actions, information

Never select a color because a card “needs variety.”

### CSS starting point

```css
:root {
  --sv-canvas: #f4ead7;
  --sv-panel: #fff4cf;
  --sv-panel-alt: #eadfc9;
  --sv-ink: #101010;
  --sv-muted: #5d564a;
  --sv-border: #101010;
  --sv-shadow: rgba(17, 16, 14, 0.68);
  --sv-green: #b7ff3c;
  --sv-yellow: #ffcf31;
  --sv-coral: #ff684f;
  --sv-colored-ink: #141410;
  --sv-soft-white: #fff7df;
  --sv-grid-line: rgba(16, 16, 16, 0.045);
}

[data-theme="dark"] {
  --sv-canvas: #40403c;
  --sv-panel: #52514c;
  --sv-panel-alt: #484843;
  --sv-ink: #fff8e9;
  --sv-muted: #ddd5c5;
  --sv-border: #151512;
  --sv-shadow: #0b0b09;
  --sv-grid-line: rgba(255, 255, 255, 0.12);
}
```

---

## 4. Borders, corners, and shadows

Supervisor should feel square and structural, but not razor-edged.

### Borders

- Default border: `2px solid` using the theme border token.
- Use thinner `1px` or translucent dividers inside a grouped card.
- Do not outline every row as a separate card.

### Corner radius

- Window shell: `8px`
- Primary cards: `7px`
- Controls and secondary cards: `5–6px`
- Small internal rectangles and swatches: at least `4–5px`
- Circular elements should only be circular when their function is inherently circular, such as a slider thumb or status light.

All internal square tiles must also have slightly rounded corners. Do not place a perfectly sharp square inside an otherwise rounded system.

### Shadows

- Primary card or button: `4px 4px 0 var(--sv-shadow)`
- Secondary card or menu: `3px 3px 0 var(--sv-shadow)`
- Compact control: `2px 2px 0 var(--sv-shadow)`
- Shadows are always neutral near-black.
- Never use blue, green, yellow, or coral shadows.
- Never add a second colored plate or “highlight” underneath a button.
- Never use a blurred SaaS shadow.

### Pressed interaction

On press, move the control `2px` toward its shadow and reduce the shadow by the same amount. Keep this fast and physical.

---

## 5. Background texture

The default Watchtower canvas uses a subtle square grid.

```css
background-color: var(--sv-canvas);
background-image:
  linear-gradient(var(--sv-grid-line) 1px, transparent 1px),
  linear-gradient(90deg, var(--sv-grid-line) 1px, transparent 1px);
background-size: 18px 18px;
```

Rules:

- Light mode uses dark translucent grid lines.
- Dark mode uses soft white grid lines.
- Apply texture to the application canvas, not every card.
- Forms, dense settings groups, buttons, and primary content cards use solid fills.
- Texture must never reduce text readability.

Alternative textures may be introduced only through a fixed semantic mapping:

- square grid: standard application canvas
- horizontal lines: history, timelines, or report-like surfaces
- dots: occasional decorative or empty-state surface
- solid: cards, settings, preflight, and controls

Do not rotate textures randomly, change them per launch, or use multiple background textures on the same screen merely for decoration.

---

## 6. Typography

Use typography for hierarchy, not noise.

### Font families

- Body and large headings: `Inter`, system sans-serif fallback
- Labels, controls, time, compact values, and system state: system monospace

```css
--sv-font-sans: Inter, ui-sans-serif, system-ui, -apple-system,
  BlinkMacSystemFont, "Segoe UI", sans-serif;
--sv-font-mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
```

### Weight rules

- Body text: regular
- Secondary labels: medium
- Large state text and the primary CTA: bold or extra-bold
- Avoid bold body paragraphs.
- Do not make every row label, metadata value, and button bold simultaneously.

### Case rules

- Sentence case for body copy and headings.
- Uppercase monospace may be used for small labels and controls.
- Avoid long uppercase sentences.

### Recommended scale

| Role | Size |
| --- | --- |
| Main product title | `32–36px` |
| State title | `28–36px` |
| Quote | `28–48px`, responsive |
| Card heading | `14–18px` |
| Body copy | `13–15px` |
| Monospace label | `10–11px` |
| Metadata | `11–12px` |

---

## 7. Layout and density

The UI should be carded, but cards must create meaningful groups rather than fragmenting the screen.

### Spacing

- Standard screen padding: `18–24px`
- Standard card gap: `12–16px`
- Card padding: `14–24px`, based on importance
- Small control gap: `8–10px`

### Dead-space rule

Avoid the disconnected “tile islands” effect. Use a coherent layout grid and keep related components close. Empty space is useful only when it improves hierarchy or reading. Do not leave large unused gaps between independently bordered tiles.

Ways to prevent dead space:

- place one dominant card beside one compact supporting column
- make the primary CTA span the available width
- group related settings into one card with internal dividers
- let a small card sit above a larger card in the same column
- align card edges and baselines
- avoid creating a separate card for a single low-value label

### Hierarchy

Use at most three visual scales on one screen:

1. dominant state or task
2. supporting card or information
3. compact metadata or control

---

## 8. Buttons and controls

### Primary buttons

- Use green.
- Use a single complete button surface.
- Do not add an internal divider.
- Do not add an offset colored “highlight” plate.
- Use near-black text in both themes.
- Use one primary action per screen or control group.

### Destructive buttons

- Use coral for `Stop`, destructive confirmation, or a serious intervention.
- Use near-black text.

### Secondary buttons

- Use the neutral panel fill.
- Keep the same border, radius, and shadow system.

### Pills

Avoid pills by default. A value such as `30 min` should use a compact, slightly rounded rectangle rather than a capsule. Pills are reserved for genuine toggles, filters, or compact status where the capsule shape communicates function.

### Switches

- Use compact rounded rectangles with a visibly separate knob.
- Maintain a clear on/off position in addition to color.
- Use green only for the enabled state.

### Focus and accessibility

- Provide visible keyboard focus.
- Preserve at least a roughly `44px` target for important controls.
- Do not rely on color alone; include text, shape, position, or an explicit mark.
- Verify contrast against the actual surface in both themes.

---

## 9. Copy tone

Copy should be short, literal, and calm. Prefer a concrete state, object, or action over a slogan. Buttons should start with a verb when possible; state labels should describe the current state without extra explanation.

Avoid:

- “Unlock your peak productivity.”
- “Your AI focus companion.”
- “No session running. Your next chapter awaits.”
- “One clear start.”
- “Harness agentic intelligence.”
- any fake-friendly sentence added only to occupy empty space

Editorial content may be more expressive. Interface copy should remain functional.

---

## 10. Motion

Use motion to explain state changes and make controls feel tactile.

Allowed:

- `80–160ms` button press
- `120–180ms` disclosure expansion
- short slider snapping motion
- quick position or size transition when it helps explain a window-state change
- restrained fill change when a state changes

Avoid:

- looping decorative motion
- ambient floating elements
- long easing curves
- glowing pulses
- parallax
- animation that delays a critical action

Respect reduced-motion preferences.

---

## 11. Cross-platform requirements

Supervisor must behave consistently on Windows and macOS.

- Keep application content independent from native title-bar differences.
- Abstract window placement and sizing behind the desktop framework.
- Store floating-window positions and restore them within the current display's usable bounds.
- Recalculate placement when monitor configuration, resolution, scale, or work area changes.
- Center modal or intervention windows on the active display.
- Ensure always-on-top behavior does not block access to critical actions.
- Do not depend on browser-specific DOM injection unless an extension is explicitly part of the product.
- Test keyboard focus, multi-monitor behavior, sleep/wake, and full-screen applications on both operating systems.

---

## 12. Agent implementation rules

When implementing or revising Supervisor UI:

1. Use the theme tokens. Do not introduce one-off hex values.
2. Preserve semantic color meanings across light and dark mode.
3. Keep colored-tile text near-black in both modes.
4. Use the Supervisor canvas colors: `#F4EAD7` and `#40403C`.
5. Use white grid lines in dark mode.
6. Keep black shadows visibly distinct from the dark canvas.
7. Round every internal rectangular surface slightly.
8. Use one neutral hard-shadow system everywhere.
9. Keep the primary action visually obvious.
10. Remove duplicate state, metadata, and explanation before adding layout complexity.
11. Prefer one grouped card with dividers over multiple disconnected tiles.
12. Do not add new badges, tags, icons, or copy simply to fill space.
13. Use realistic Supervisor states and labels.
14. Implement collapsed and expanded disclosure states where progressive disclosure is used.
15. Verify light mode, dark mode, narrow layouts, keyboard navigation, and reduced motion.
16. Keep Windows and macOS window behavior behind one cross-platform interface.
17. If a behavior is not specified, choose the simpler reversible implementation and flag the decision instead of inventing a new visual system.

---

## 13. Acceptance checklist

### Global

- [ ] The UI feels like Watchtower: warm, tactile, direct, and restrained.
- [ ] Light canvas is `#F4EAD7`.
- [ ] Dark canvas is `#40403C`.
- [ ] Dark-mode grid lines are white and subtle.
- [ ] Black shadows remain visible in dark mode.
- [ ] Green, yellow, and coral tiles always use near-black text.
- [ ] Borders are mostly `2px` and near-black.
- [ ] Cards and internal tiles have `5–8px` radii.
- [ ] Shadows are hard, neutral, and unblurred.
- [ ] No random blue shadow or button underlay exists.
- [ ] No gradients, glass, glow, or decorative orb exists.
- [ ] The layout does not contain accidental dead zones between tile islands.

### Layout and accessibility

- [ ] Related settings are grouped with internal dividers.
- [ ] Keyboard focus is visible.
- [ ] Important controls have adequate target size.
- [ ] Meaning never depends on color alone.
- [ ] Both light and dark themes pass contrast review.

---

## 14. Scope of this document

This document defines the reusable visual and interaction system. It deliberately does not define the content or layout of individual screens. Use `SUPERVISOR_SCREEN_SPECIFICATIONS.md` for approved screen-level decisions.

Earlier BrainDock-ish and mashup style documents are historical references and may contain superseded choices such as cobalt accents, lighter canvases, denser diagnostics, or older shadow rules.
