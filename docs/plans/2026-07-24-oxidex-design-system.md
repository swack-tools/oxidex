# OxiDex "Oxide Terminal" Design System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reusable dark-only Claude Design design-system project ("OxiDex Design System") encoding the Oxide Terminal identity: tokens, 13 components, foundation galleries, and a showcase page.

**Architecture:** A Claude Design project (NOT repo code) structured per the proven design-system layout observed in the btad Design System project: `readme.md` + `tokens/*.css` + `components/core/*.jsx|.d.ts|.prompt.md` + gallery `card.html` pages + `guidelines/foundations/*.html` + `showcase.dc.html`. All writes go through the `mcp__claude-design__*` MCP tools; verification is visual (render → screenshot gate → design-verifier), not unit tests.

**Tech Stack:** Claude Design MCP tools, plain HTML/CSS + React 18.3.1 UMD/Babel (pinned CDN tags) for `.jsx` components, `.dc.html` runtime via `create_support_js`, Playwright MCP for the render gate, `design-verifier` agent for fresh-eyes review.

**Spec:** `docs/plans/specs/2026-07-24-oxidex-design-system-design.md` (approved). One deliberate refinement to the spec: components ship as `.jsx + .d.ts + .prompt.md` triples with gallery `card.html` pages (the canonical design-system convention discovered via the btad project), not one `.dc.html` per component. The spec's delivery section explicitly deferred to the canonical layout. `showcase.dc.html` remains a `.dc.html`.

## Global Constraints

- Dark mode ONLY. Page bg `#0d0f12`; never a light surface.
- Token values verbatim from spec: bg `#0d0f12`, surface `#16191f`, surface-2 `#1d2129`, border `#2a2f3a`, accent `#e8824a`, accent-deep `#b5551e`, text `#e6e9ef`, text-dim `#8b93a3`, green `#7fd88f`, red `#e06c75`, blue `#6cb6ff`.
- Semantic pairing rule everywhere: keys/labels dim, values green, offsets dim mono, brand actions orange.
- Fonts: IBM Plex Mono (headings/data/labels/nav/buttons), Inter (body prose only). Google Fonts CDN. 1–2 fonts per file, no others.
- Type scale: 12 / 13 / 15 / 18 / 24 / 36 / 56 px. Headings = mono, normal weight, letter-spaced (0.02–0.08em), NOT bold.
- Spacing: 4px grid; steps 8 / 12 / 16 / 24 / 40 / 64. Radius ≤ 4px (2px chips). No shadows except the hero glow `0 0 24px rgba(232,130,74,.15)` (max one element per page).
- Text below 13px always uses `--ox-text`, never `--ox-text-dim` (AA floor).
- Never hand-write `_ds_manifest.json`, `_ds_bundle.js`, or `support.js` content (app/server-generated).
- Never share a `serve_url` with the user — only `claude.ai/design/...` links.
- Pass `if_match` etags on every `write_files` after the first write of a path.
- React/Babel CDN tags must be the pinned versions with integrity hashes (React 18.3.1, ReactDOM 18.3.1, @babel/standalone 7.29.0) — copy exact tags from the "React tags" block in Task 3.
- `.jsx` scope rules: never `const styles = {}` (name per-component, e.g. `buttonStyles`); export via `Object.assign(window, { Button })` at file end.
- Verify loop after EVERY write of a renderable file: `render_preview` → open `serve_url` in a fresh Playwright page → screenshot + console + failed requests → fix until clean → `design-verifier` agent for galleries and showcase.

---

### Task 1: Project creation, tokens, fonts, readme skeleton

**Files (in the new Claude Design project):**
- Create: project "OxiDex Design System" via `create_project`
- Create: `tokens/colors.css`, `tokens/typography.css`, `tokens/spacing.css`
- Create: `styles.css` (imports/base reset)
- Create: `readme.md` (skeleton; completed in Task 6)

**Interfaces:**
- Produces: `project_id` used by every later task; token CSS classes/vars (`--ox-*`) consumed by every component and page; `ox-glow` utility; base `a`/`a:hover` styling.

- [ ] **Step 1: Create the project**

Call `mcp__claude-design__create_project` with `{"name": "OxiDex Design System"}`. Record `project_id` and `url`. Do NOT pass `design_system_id` (this project IS a design system). Offer the user the live preview link (`url` + `?embed=1`).

- [ ] **Step 2: Write token files**

First `write_files` may return `needs_project_grant` — the user approves once, then retry the identical call. Content:

`tokens/colors.css`:
```css
/* OxiDex — Oxide Terminal color tokens. Dark only. */
:root {
  --ox-bg: #0d0f12;          /* page background */
  --ox-surface: #16191f;     /* panels, cards */
  --ox-surface-2: #1d2129;   /* raised/hover, table headers */
  --ox-border: #2a2f3a;      /* hairline 1px borders */
  --ox-accent: #e8824a;      /* rust-orange brand anchor */
  --ox-accent-deep: #b5551e; /* pressed states, gradient endpoint */
  --ox-text: #e6e9ef;        /* primary text */
  --ox-text-dim: #8b93a3;    /* labels, offsets — never below 13px */
  --ox-green: #7fd88f;       /* values, pass */
  --ox-red: #e06c75;         /* errors, fail */
  --ox-blue: #6cb6ff;        /* links, info */
  --ox-glow: 0 0 24px rgba(232, 130, 74, 0.15); /* hero only, one per page */
}
/* Semantic pairing rule: keys dim, values green, offsets dim mono, actions orange. */
.ox-key { color: var(--ox-text-dim); }
.ox-val { color: var(--ox-green); }
.ox-offset { color: var(--ox-text-dim); font-family: "IBM Plex Mono", monospace; }
.ox-action { color: var(--ox-accent); }
```

`tokens/typography.css`:
```css
/* Mono is the voice: headings, data, labels, nav, buttons. Inter for prose only. */
:root {
  --ox-font-mono: "IBM Plex Mono", ui-monospace, monospace;
  --ox-font-body: "Inter", system-ui, sans-serif;
  --ox-fs-12: 12px; --ox-fs-13: 13px; --ox-fs-15: 15px; --ox-fs-18: 18px;
  --ox-fs-24: 24px; --ox-fs-36: 36px; --ox-fs-56: 56px;
  --ox-track-heading: 0.04em; /* headings are spaced, not bold */
  --ox-track-label: 0.08em;   /* uppercase labels */
}
.ox-h1 { font-family: var(--ox-font-mono); font-size: var(--ox-fs-56); font-weight: 400; letter-spacing: var(--ox-track-heading); color: var(--ox-text); }
.ox-h2 { font-family: var(--ox-font-mono); font-size: var(--ox-fs-36); font-weight: 400; letter-spacing: var(--ox-track-heading); color: var(--ox-text); }
.ox-h3 { font-family: var(--ox-font-mono); font-size: var(--ox-fs-24); font-weight: 400; letter-spacing: var(--ox-track-heading); color: var(--ox-text); }
.ox-label { font-family: var(--ox-font-mono); font-size: var(--ox-fs-12); letter-spacing: var(--ox-track-label); text-transform: uppercase; color: var(--ox-text); }
.ox-body { font-family: var(--ox-font-body); font-size: var(--ox-fs-15); line-height: 1.6; color: var(--ox-text); }
.ox-mono { font-family: var(--ox-font-mono); font-size: var(--ox-fs-13); }
```

`tokens/spacing.css`:
```css
:root {
  --ox-sp-1: 4px; --ox-sp-2: 8px; --ox-sp-3: 12px; --ox-sp-4: 16px;
  --ox-sp-6: 24px; --ox-sp-10: 40px; --ox-sp-16: 64px;
  --ox-radius: 4px;      /* max radius anywhere */
  --ox-radius-chip: 2px; /* chips */
}
```

`styles.css`:
```css
@import url("https://fonts.googleapis.com/css2?family=IBM+Plex+Mono:wght@400;500&family=Inter:wght@400;500&display=swap");
@import url("./tokens/colors.css");
@import url("./tokens/typography.css");
@import url("./tokens/spacing.css");
* { box-sizing: border-box; }
body { margin: 0; background: var(--ox-bg); color: var(--ox-text); font-family: var(--ox-font-body); }
a { color: var(--ox-blue); text-decoration: none; }
a:hover { color: var(--ox-accent); text-decoration: underline; }
::selection { background: rgba(232, 130, 74, 0.3); }
```

- [ ] **Step 3: Write readme.md skeleton**

Sections (fill fully in Task 6, but write real content now for tokens): `# OxiDex Design System — Oxide Terminal`; identity summary (exact/fast/engineered, "a beautiful terminal, not a website"); dark-only rule; full token tables (colors, type, spacing) copied from the CSS above; the semantic pairing rule; heading rule (spaced not bold); shadow/glow rule; AA floor rule; component list placeholder listing the 13 planned components; "How to consume" (copy `styles.css` + `tokens/` in via `copy_files`, or inline the token block).

- [ ] **Step 4: Verify**

`render_preview` on `styles.css` is not renderable — instead write a throwaway check by rendering Task 2's first foundation page (tokens verified there). For this task the gate is: `list_files` shows all 5 files; `read_file` on `styles.css` round-trips content intact.

---

### Task 2: Foundation gallery pages

**Files:**
- Create: `guidelines/foundations/colors.html`
- Create: `guidelines/foundations/type.html`
- Create: `guidelines/foundations/spacing.html`

**Interfaces:**
- Consumes: `styles.css` (relative `<link rel="stylesheet" href="../../styles.css">`).
- Produces: visual proof of tokens; reference pages linked from readme.

- [ ] **Step 1: Write `colors.html`** — plain HTML page (not .dc.html), dark bg, a grid of swatch cards: each swatch is a 96px square of the token color with 1px `--ox-border`, below it the token name in `.ox-mono` and hex in `.ox-key`. Groups: Surfaces (bg/surface/surface-2/border), Brand (accent/accent-deep), Text (text/text-dim), Signals (green/red/blue). Include one "semantic pairing" demo row: `Make` in `.ox-key` → `SONY` in `.ox-val`, `0x010f` in `.ox-offset`, a `WRITE` word in `.ox-action`.

- [ ] **Step 2: Write `type.html`** — every `.ox-h1/h2/h3/label/body/mono` class rendered with its own name as sample text plus size annotation in `.ox-key`; a paragraph of Inter body demonstrating the "prose > 2 lines" rule; a DON'T row showing bold-heading struck out with the rule "headings are spaced, not bold".

- [ ] **Step 3: Write `spacing.html`** — spacing scale as horizontal orange bars of each `--ox-sp-*` width labeled in `.ox-mono`; radius demo (4px card corner vs 2px chip corner); the glow rule demoed on exactly one card with `box-shadow: var(--ox-glow)`.

- [ ] **Step 4: Verify loop (gate + fresh eyes)**

For each page: `render_preview` → open `serve_url` in a NEW Playwright page (`browser_close` old one first) → screenshot 1440×900 + `browser_console_messages` + failed requests. Gate must be clean (no console errors, no 404s — watch for the Google Fonts and relative CSS links resolving). Then dispatch `design-verifier` agent (Agent tool) with fresh `serve_url`s, project_id, paths, and the spec's Foundations section verbatim. Fix `needs_work` findings; re-render.

---

### Task 3: Component cluster A — Button, TagChip, Badge, Callout

**Files:**
- Create: `components/core/Button.jsx`, `Button.d.ts`, `Button.prompt.md`
- Create: `components/core/TagChip.jsx`, `TagChip.d.ts`, `TagChip.prompt.md`
- Create: `components/core/Badge.jsx`, `Badge.d.ts`, `Badge.prompt.md`
- Create: `components/core/Callout.jsx`, `Callout.d.ts`, `Callout.prompt.md`
- Create: `components/core/core-a.card.html` (gallery)

**Interfaces:**
- Consumes: token CSS (gallery links `../../styles.css`; jsx uses `var(--ox-*)` in inline styles).
- Produces: `window.Button`, `window.TagChip`, `window.Badge`, `window.Callout` React components with the exact props in the `.d.ts` blocks below. Later tasks (showcase) import these files via `<script>` tags and use these names.

**React tags** (exact, for every gallery/card/showcase page — the pinned versions with integrity hashes from the Claude Design prompt):
```html
<script src="https://unpkg.com/react@18.3.1/umd/react.development.js" integrity="sha384-hD6/rw4ppMLGNu3tX5cjIb+uRZ7UkRJ6BPkLpg4hAu/6onKUg4lLsHAs9EBPT82L" crossorigin="anonymous"></script>
<script src="https://unpkg.com/react-dom@18.3.1/umd/react-dom.development.js" integrity="sha384-u6aeetuaXnQ38mYT8rp6sbXaQe3NL9t+IBXmnYxwkUI2Hw4bsp2Wvmx4yRQF1uAm" crossorigin="anonymous"></script>
<script src="https://unpkg.com/@babel/standalone@7.29.0/babel.min.js" integrity="sha384-m08KidiNqLdpJqLq95G/LEi8Qvjl/xUYll3QILypMoQ65QorJ9Lvtp2RXYGBFj1y" crossorigin="anonymous"></script>
```

- [ ] **Step 1: Write Button (full exemplar — this exact code)**

`Button.jsx`:
```jsx
const buttonStyles = {
  base: {
    fontFamily: "var(--ox-font-mono)",
    fontSize: "var(--ox-fs-13)",
    letterSpacing: "var(--ox-track-label)",
    textTransform: "uppercase",
    padding: "8px 16px",
    borderRadius: "var(--ox-radius)",
    border: "1px solid transparent",
    cursor: "pointer",
    background: "none",
  },
  primary: {
    background: "var(--ox-accent)",
    color: "#0d0f12",
    borderColor: "var(--ox-accent)",
  },
  secondary: {
    color: "var(--ox-accent)",
    borderColor: "var(--ox-accent)",
  },
  ghost: {
    color: "var(--ox-text-dim)",
    borderColor: "transparent",
  },
};

function Button({ variant = "primary", children, onClick, disabled }) {
  const [pressed, setPressed] = React.useState(false);
  const style = {
    ...buttonStyles.base,
    ...buttonStyles[variant],
    ...(pressed && variant === "primary"
      ? { background: "var(--ox-accent-deep)", borderColor: "var(--ox-accent-deep)" }
      : {}),
    ...(disabled ? { opacity: 0.4, cursor: "not-allowed" } : {}),
  };
  return (
    <button
      style={style}
      onClick={disabled ? undefined : onClick}
      onMouseDown={() => setPressed(true)}
      onMouseUp={() => setPressed(false)}
      onMouseLeave={() => setPressed(false)}
    >
      {children}
    </button>
  );
}

Object.assign(window, { Button });
```

`Button.d.ts`:
```ts
interface ButtonProps {
  /** primary = orange fill/black text; secondary = orange border+text; ghost = dim text */
  variant?: "primary" | "secondary" | "ghost";
  children: React.ReactNode;
  onClick?: () => void;
  disabled?: boolean;
}
declare function Button(props: ButtonProps): JSX.Element;
```

`Button.prompt.md`:
```md
# Button
Mono uppercase label, 1px border, 4px radius. One primary button per view max.
- primary: orange fill, near-black text. The main action.
- secondary: transparent, orange border + text. Peer actions.
- ghost: dim text, no border. Tertiary/dismiss.
Pressed primary darkens to --ox-accent-deep. Never bold; the mono + tracking IS the emphasis.
```

- [ ] **Step 2: Write TagChip** — same file pattern (`tagChipStyles`, `Object.assign(window, { TagChip })`). Props: `hex?: string` (e.g. `"0x0110"`), `label: string`, `tone?: "neutral" | "group" | "value" | "error"` (default neutral). Render: inline-flex, 2px radius, 1px `--ox-border` border, `padding: 2px 8px`, gap 6px, font `.ox-mono` at 12px BUT color floor: 12px text uses `--ox-text` (AA rule) with only the hex prefix at 13px+ allowed dim — simplest compliant form: chip font-size 13px, hex prefix `--ox-text-dim`, label `--ox-text`. Tones set label color: group → `--ox-accent`, value → `--ox-green`, error → `--ox-red`; tone also tints border to a 35%-alpha version of the same color (e.g. `rgba(232,130,74,.35)`).

- [ ] **Step 3: Write Badge** — props `status: "pass" | "fail" | "wip" | "supported" | "partial" | "unsupported"`, `label?: string` (defaults to status uppercased). PASS/FAIL/WIP: 2px-radius rect, mono 12px uppercase in `--ox-text`, background `rgba(green|red|dim, .12)`, 1px border in the signal color, text colored green/red/dim→`--ox-text-dim` respectively (but WIP label text at 12px uses `--ox-text`; only the dot may be dim). supported/partial/unsupported: leading dot (8px circle) green / half-opacity green / `--ox-border`, label in `--ox-text`.

- [ ] **Step 4: Write Callout** — props `kind: "note" | "warn" | "fail"`, `children`. Full-width strip: `background: var(--ox-surface)`, `border: 1px solid var(--ox-border)`, `border-left: 2px solid <signal>` (note → `--ox-blue`, warn → `--ox-accent`, fail → `--ox-red`), padding 12px 16px, leading mono uppercase label (`NOTE`/`WARN`/`FAIL`) in the signal color at 12px+tracking followed by body text in `.ox-body` at 15px.

- [ ] **Step 5: Write gallery `core-a.card.html`** — plain HTML page: `<link>` to `../../styles.css`, the React tags block, `<script src="./Button.jsx" type="text/babel">` etc. for all four, then one `text/babel` script rendering a demo grid into `#root`: each component name as `.ox-h3`, every variant/tone/status side by side in `display:flex; gap:16px` rows, dark bg. Include disabled button and a long-label chip (`0x9286 · UserComment`) to check truncation behavior (chips don't wrap; `white-space: nowrap`).

- [ ] **Step 6: Verify loop** — gate on `core-a.card.html` (console clean is critical here: Babel errors surface in console), then `design-verifier` with the four `.prompt.md` contents + spec component list as the brief. Fix and re-render until done.

---

### Task 4: Component cluster B — Card, DataTable, StatTile, BenchmarkBar

**Files:**
- Create: `components/core/Card.jsx`, `Card.d.ts`, `Card.prompt.md`
- Create: `components/core/DataTable.jsx`, `DataTable.d.ts`, `DataTable.prompt.md`
- Create: `components/core/StatTile.jsx`, `StatTile.d.ts`, `StatTile.prompt.md`
- Create: `components/core/BenchmarkBar.jsx`, `BenchmarkBar.d.ts`, `BenchmarkBar.prompt.md`
- Create: `components/core/core-b.card.html` (gallery)

**Interfaces:**
- Consumes: token CSS; React tags block from Task 3 (copy verbatim).
- Produces: `window.Card`, `window.DataTable`, `window.StatTile`, `window.BenchmarkBar`.

- [ ] **Step 1: Card.jsx** — props `title?: string`, `hero?: boolean`, `children`. Container: `background: var(--ox-surface)`, `border: 1px solid var(--ox-border)`, `border-radius: var(--ox-radius)`, padding 24px; `hero` adds `box-shadow: var(--ox-glow)`. Titled variant renders the box-drawing-style header via CSS (no literal glyphs): a flex row placed with negative margin so it overlaps the top border — `<div style="display:flex; align-items:center; gap:8px; margin:-36px 0 16px">` containing a 16px horizontal 1px line (`background: var(--ox-border)`), the title as mono 12px uppercase tracked `--ox-text-dim` on a `background: var(--ox-surface)` pill with 4px side padding (so it "interrupts" the border), then a flex-1 1px line. Card body is `children`.

- [ ] **Step 2: DataTable.jsx** — props `columns: {key, label, kind?: "offset" | "key" | "value" | "text"}[]`, `rows: object[]`. `<table>` full-width, `border-collapse: collapse`, `font: 13px var(--ox-font-mono)`. Header row: `background: var(--ox-surface-2)`, cells mono 12px uppercase tracked `--ox-text` (12px floor), `padding: 8px 12px`, `border-bottom: 1px solid var(--ox-border)`. Body cells `padding: 6px 12px; border-bottom: 1px solid var(--ox-border)`; kind→class: offset → `.ox-offset`, key → `.ox-key`, value → `.ox-val`, text → default `--ox-text`. Row hover: `background: var(--ox-surface-2)` via `onMouseEnter/Leave` state or a `<style>` tag emitted once by the component (`.ox-dt tbody tr:hover{background:var(--ox-surface-2)}` — emit a `<style>` element inside the component root; simplest reliable approach with inline-style React).

- [ ] **Step 3: StatTile.jsx** — props `value: string` (preformatted, e.g. `"9.7×"`), `label: string`, `delta?: {text: string, good: boolean}`. Column layout: value mono 56px normal-weight `--ox-text`; label mono 13px uppercase tracked `--ox-text-dim` (13px, not 12px, so dim passes the AA floor). Delta: mono 13px, `--ox-green` if good else `--ox-red`, prefixed `▲`/`▼`.

- [ ] **Step 4: BenchmarkBar.jsx** — props `items: {label: string, value: number, unit: string, highlight?: boolean}[]`, `max?: number` (default = max item value). Each row: label column fixed 140px mono 13px `--ox-text`; bar track flex-1 `background: var(--ox-surface-2)` height 20px; fill width `${value/max*100}%`, `background: var(--ox-accent)` when highlight else `var(--ox-border)`; value+unit label mono 13px right of bar (`--ox-text`). Dataviz rules honored: labeled ends, no gridlines needed at this size, one highlighted series (oxidex) vs dim comparison (ExifTool). Bars animate width via CSS transition 400ms gated on `@media (prefers-reduced-motion: no-preference)` (initial render sets final width when reduced motion).

- [ ] **Step 5: Write `.d.ts` + `.prompt.md` for all four** — d.ts mirrors the exact props above; prompt.md one-paragraph usage voice like Button's (Card: "surfaces are stepped, not shadowed; one hero glow per page"; DataTable: "the workhorse — keys dim, values green, offsets in the gutter"; StatTile: "one big mono number, no icon, no sparkline"; BenchmarkBar: "orange is us, gray is them; always label bar ends").

- [ ] **Step 6: Gallery `core-b.card.html`** — same skeleton as Task 3 Step 5. Demos: titled card + hero card side by side; DataTable with EXIF-flavored fixture rows (columns Offset/Tag/Value: `0x010f · Make · SONY`, `0x0110 · Model · ILCE-7M4`, `0x829a · ExposureTime · 1/250`, `0x829d · FNumber · f/2.8`, `0x8827 · ISO · 100`); stat row of 3 tiles (`9.7×` faster / `32,677` tags / `140+` formats, one with green delta `▲ 2.1× vs v1.1`); BenchmarkBar with `oxidex 0.41s` (highlight) vs `ExifTool 3.98s`.

- [ ] **Step 7: Verify loop** — gate + design-verifier as Task 3 Step 6.

---

### Task 5: Component cluster C+D — HexViewer, TerminalBlock, ScanLine, NavBar, SidebarNav

**Files:**
- Create: `components/core/HexViewer.jsx`, `HexViewer.d.ts`, `HexViewer.prompt.md`
- Create: `components/core/TerminalBlock.jsx`, `TerminalBlock.d.ts`, `TerminalBlock.prompt.md`
- Create: `components/core/ScanLine.jsx`, `ScanLine.d.ts`, `ScanLine.prompt.md`
- Create: `components/core/NavBar.jsx`, `NavBar.d.ts`, `NavBar.prompt.md`
- Create: `components/core/SidebarNav.jsx`, `SidebarNav.d.ts`, `SidebarNav.prompt.md`
- Create: `components/core/core-c.card.html` (gallery)

**Interfaces:**
- Consumes: token CSS; React tags block (Task 3, verbatim).
- Produces: `window.HexViewer`, `window.TerminalBlock`, `window.ScanLine`, `window.NavBar`, `window.SidebarNav`.

- [ ] **Step 1: HexViewer.jsx** — props `bytes: number[]`, `baseOffset?: number` (default 0), `highlight?: {start: number, end: number, label?: string}`. Renders rows of 16 bytes inside a `--ox-surface` panel (padding 16px, 1px border, radius 4px, overflow-x auto). Per row: offset gutter (`(baseOffset + row*16).toString(16).padStart(8,"0")`) in `.ox-offset` 13px; 16 byte pairs (`b.toString(16).padStart(2,"0")`) mono 13px `--ox-text` with an extra 8px gap after byte 8; ASCII column (printable 0x20–0x7e else `·`) in `--ox-text-dim` 13px. Bytes with index in `[highlight.start, highlight.end)` get `background: rgba(232,130,74,.2); color: var(--ox-accent)` (both hex pair and ASCII char). If `highlight.label`, a mono 12px `--ox-accent` caption row (`└─ ${label}`) under the panel — 12px is fine because it's accent-colored, not dim. Layout: each row is one flex line — gutter (fixed 80px), 16 hex-pair `<span>`s (each 22px wide, centered, with 8px extra margin after the 8th), 16px gap, then the 16 ASCII chars. Fixed-width spans, not table/grid, so the highlight background hugs each byte.

- [ ] **Step 2: TerminalBlock.jsx** — props `lines: {prompt?: boolean, text: string}[]`, `title?: string` (default `"oxidex"`). Panel: `background: #0a0c0e` (one step darker than bg — the only place this shade appears; define local const), 1px `--ox-border`, radius 4px. Title bar: 28px, `--ox-surface` bg, centered title in mono 13px `--ox-text-dim` (13px so dim passes the AA floor). Body padding 16px, mono 13px, line-height 1.7. Prompt lines: leading `$ ` in `--ox-green`, first word after prompt (the command) in `--ox-accent`, rest `--ox-text`. Non-prompt lines (output): `--ox-text-dim` at 13px. Copy affordance: absolute top-right `COPY` mono 12px `--ox-accent` button (accent ≥ AA on this bg), `navigator.clipboard.writeText(all prompt lines joined)`, flips to `COPIED` for 1.5s.

- [ ] **Step 3: ScanLine.jsx** — props `progress?: number` (0–1; omitted = indeterminate divider mode), `label?: string`. Track: height 2px full-width `--ox-surface-2`. Determinate: fill `width: progress*100%`, `background: var(--ox-accent)`. Both modes: a 48px-wide shimmer gradient (`linear-gradient(90deg, transparent, rgba(232,130,74,.6), transparent)`) sweeping left→right on a 2.4s loop — `@media (prefers-reduced-motion: no-preference)` only; static fill otherwise. Label: mono 13px `--ox-text-dim` above, right-aligned percentage in `--ox-green` when determinate.

- [ ] **Step 4: NavBar.jsx** — props `items: {label: string, active?: boolean}[]`, `logoText?: string` (default `"oxidex"`). 56px bar, `background: var(--ox-surface)`, `border-bottom: 1px solid var(--ox-border)`, flex, padding 0 24px, gap 24px. Logo: mono 15px, `oxi` in `--ox-text` + `dex` in `--ox-accent`, preceded by an 8px orange square (the only logo mark needed here; the full SVG mark is showcase-only). Items: mono 13px uppercase tracked; inactive `--ox-text-dim`, active `--ox-text` with 2px `--ox-accent` bottom border (the cursor underline) offset to sit on the bar's border line.

- [ ] **Step 5: SidebarNav.jsx** — props `sections: {title: string, items: {label: string, active?: boolean}[]}[]`. 220px column. Section title: mono 12px uppercase tracked `--ox-text` (floor), margin-top 24px. Items: mono 13px, padding 6px 12px, `--ox-text-dim` default; active: `--ox-accent` text + 2px `--ox-accent` left border + `background: var(--ox-surface)`; hover: `--ox-text` (style-tag approach as DataTable).

- [ ] **Step 6: `.prompt.md` files** — HexViewer: "decorative or real — the orange span is 'the bytes that matter'; never highlight more than one span"; TerminalBlock: "prompt green, command orange, output dim — same pairing rule as everything else"; ScanLine: "doubles as a section divider; one animated instance per view"; NavBar: "the underline is a cursor, 2px, never a pill"; SidebarNav: "docs-style; active items get the left rule".

- [ ] **Step 7: Gallery `core-c.card.html`** — HexViewer with a real JPEG SOI/APP1 header fixture: bytes `[0xff,0xd8,0xff,0xe1,0x24,0x8a,0x45,0x78,0x69,0x66,0x00,0x00,0x4d,0x4d,0x00,0x2a, ...]` (pad to 48 bytes with plausible TIFF header bytes), highlight `{start: 6, end: 12, label: "Exif identifier"}`; TerminalBlock with `$ oxidex -Make -Model photo.jpg` + two output lines; determinate (0.62) and indeterminate ScanLines; NavBar with `DOCS / TAGS / BENCHMARKS (active) / GITHUB`; SidebarNav with two sections (`FORMATS`: JPEG active, PNG, TIFF; `GUIDES`: Writing, Batch).

- [ ] **Step 8: Verify loop** — gate + design-verifier. HexViewer alignment is the likely needs_work: confirm byte columns align vertically across rows in the screenshot (monospace grid), ASCII column doesn't wrap.

---

### Task 6: Logo asset, readme completion

**Files:**
- Create: `assets/logo-mark.svg` (recolor of existing repo logo, NOT hand-drawn)
- Modify: `readme.md` (complete)

**Interfaces:**
- Consumes: `/Users/allen/git/oxidex/docs/public/logo.svg` (repo file) as the source artwork.
- Produces: flat dark-bg logo variant used by the showcase; finished readme (the file the design-system loader surfaces).

- [ ] **Step 1: Create the flat logo variant** — take the repo `docs/public/logo.svg` geometry and recolor for dark: background circle fill `#16191f` stroke `#2a2f3a`; document rect fill `none` stroke `#e8824a` stroke-width 2; mountain path + sun circle fill `#e8824a` opacity .7/1; metadata lines stroke `#e8824a`; cog: outer circle fill `#b5551e`, inner white → `#0d0f12`, teeth `#e8824a`. This is recoloring existing paths (allowed), not drawing new SVG art. Write via `write_files` as `assets/logo-mark.svg`.

- [ ] **Step 2: Complete readme.md** — replace the component-list placeholder with the real manifest: for each of the 13 components, one line: name, one-sentence purpose, props summary, gallery link (`components/core/core-a.card.html` etc.). Add: foundations links, logo usage (flat mark on dark; original circle version stays in the repo for light contexts), the four voice rules as a DO/DON'T table (spaced-not-bold; keys-dim-values-green; one glow per page; radius ≤ 4px), and the React tags block (so consumers copy the pinned tags). Pass `if_match` with readme's current etag.

- [ ] **Step 3: Verify** — `read_file` round-trip; render `assets/logo-mark.svg` via `render_preview` and screenshot it on the gate pass (SVG renders standalone) — confirm it reads at 32px and 120px (screenshot both by wrapping in a quick zoomed browser view or just visually check clarity).

---

### Task 7: Showcase page — "oxidex inspect" report

**Files:**
- Create: `support.js` at project root via `create_support_js` (never hand-written)
- Create: `showcase.dc.html`

**Interfaces:**
- Consumes: ALL 13 components via `<x-import component="Button" from="./components/core/Button.jsx" hint-size="...">` etc.; `assets/logo-mark.svg`; token CSS inlined into `<helmet>` (dc files carry their own token block per canonical-HTML self-containment — copy the three token files' `:root` blocks into `<helmet><style>`).
- Produces: the deliverable page proving the system works together.

- [ ] **Step 1: `create_support_js`** at project root (path `support.js`).

- [ ] **Step 2: Write `showcase.dc.html`** in exact `.dc.html` shape (DOCTYPE → head with `<script src="./support.js"></script>` → `<x-dc>` → `<helmet data-dc-atomics>` with the full token `:root` block + Google Fonts `<link>` + `a`/`a:hover` styles → markup → logic script). Page structure top to bottom (single `<x-dc>`, `data-screen-label` on each section):
  1. NavBar (x-import) — items DOCS / TAGS / BENCHMARKS / GITHUB, active INSPECT.
  2. Hero: flat logo mark 96px + `.ox-h1` `oxidex inspect` + one-line `.ox-body` dim subtitle + primary Button `RUN INSPECT` + secondary `EXPORT JSON`. Hero card is THE glow element.
  3. Stat row: three StatTiles (`9.7×`, `32,677`, `140+`).
  4. Two-column: SidebarNav (formats) | main column with: TagChip row (`EXIF` group chip, `0x010f Make` neutral, `0x0110 Model` value, `0xa005 InteropIFD` error) → DataTable (the Task 4 EXIF fixture + 3 more rows: `0x9003 · DateTimeOriginal · 2026:07:24 02:33:00`, `0x920a · FocalLength · 85mm`, `0xa002 · ImageWidth · 7008`) → HexViewer (Task 5 fixture) → Callout kind=note ("MakerNotes parsed via Sony 0x9050 cipher") → TerminalBlock.
  5. Benchmark section: `.ox-h2` `BENCHMARKS` + BenchmarkBar (oxidex vs ExifTool) + ScanLine determinate 1.0 labeled `PARSE COMPLETE` + Badges row (PASS, WIP, supported/partial/unsupported dots).
  6. Footer: mono 13px dim, `GPL-3.0 · oxidex.net · github.com/swack-tools/oxidex` as styled links.
  Every `x-import` gets `hint-size`; every non-void element explicitly closed; double-quoted attributes; no `<script src>` inside template body.

- [ ] **Step 3: Gate** — render_preview → fresh Playwright page → screenshot full page (scroll capture or sectional screenshots) + console + failed requests. Expect first-round issues: x-import path resolution and Babel-in-dc interplay; if `x-import` of `.jsx` fails structurally after 3 rounds, fall back to rendering showcase as a plain `.html` page with the Task 3 React-tags pattern (same visual result; note the change in the summary and readme).

- [ ] **Step 4: Fresh eyes** — design-verifier with the spec verbatim + instruction to check: every one of the 13 components appears; pairing rule holds (keys dim/values green/actions orange); exactly one glow; no bold headings; no light surfaces. Fix until `done`.

- [ ] **Step 5: Deliver** — share `write_files` url with `?file=showcase.dc.html` (URL-encoded) + the done-round screenshot. Ask the user to mark the project as a design system in the Claude Design UI if they want it in `list_design_systems` (the `_ds_manifest.json`/`_ds_bundle.js` compilation is app-side, not ours).

---

### Task 8: Final sweep

- [ ] **Step 1:** `list_files depth -1` — confirm expected file inventory, no strays.
- [ ] **Step 2:** `list_comments queued_for_claude: true` — handle any queued user feedback per the /design skill rules.
- [ ] **Step 3:** Re-render galleries once more after any Task 7 component fixes (a component edited during showcase debugging may have regressed its gallery).
- [ ] **Step 4:** Final summary to user: project link, showcase link, screenshot, the one-line aesthetic-assumption note, and the repo-side artifacts (spec + plan on branch `claude/oxidex-design-system-spec`).
