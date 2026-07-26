# OxiDex Design System — Oxide Terminal

Precision developer-tool aesthetic for OxiDex (the Rust ExifTool). Exact, fast,
engineered — a beautiful terminal, not a website. **Dark mode only**: page
background is always `#0d0f12`; there are no light surfaces in this system.

## Voice rules (apply everywhere)

| # | DO | DON'T |
|---|---|---|
| 1 | Keys/labels dim, values green, offsets dim mono, actions orange (**semantic pairing**) | Color text decoratively or use orange for non-actions |
| 2 | Headings in IBM Plex Mono, weight 400, letter-spaced 0.04em | Bold anything — mono + tracking IS the emphasis |
| 3 | Elevation via surface steps (bg → surface → surface-2) + 1px `--ox-border` hairlines | Drop shadows — except ONE `var(--ox-glow)` hero per page |
| 4 | Radius ≤ 4px (2px on chips) | Pills, rounded corners > 4px |
| 5 | `--ox-text-dim` only at 13px+; smaller text uses `--ox-text` | Dim text below 13px (fails AA) |

## Color tokens (`tokens/colors.css`)

| Token | Value | Role |
|---|---|---|
| `--ox-bg` | `#0d0f12` | Page background (near-black, cool) |
| `--ox-surface` | `#16191f` | Panels, cards |
| `--ox-surface-2` | `#1d2129` | Raised/hover surfaces, table headers |
| `--ox-border` | `#2a2f3a` | Hairline borders (1px, everywhere) |
| `--ox-accent` | `#e8824a` | Rust-orange brand anchor |
| `--ox-accent-deep` | `#b5551e` | Pressed states, gradient endpoint |
| `--ox-text` | `#e6e9ef` | Primary text |
| `--ox-text-dim` | `#8b93a3` | Secondary text, labels, offsets |
| `--ox-green` | `#7fd88f` | Values, pass/success |
| `--ox-red` | `#e06c75` | Errors, fail |
| `--ox-blue` | `#6cb6ff` | Links, info |

Helper classes: `.ox-key` `.ox-val` `.ox-offset` `.ox-action`. One extra shade
exists only inside TerminalBlock bodies: `#0a0c0e`.

## Typography (`tokens/typography.css`)

- **IBM Plex Mono** — headings, data, labels, nav, buttons. The signature voice.
- **Inter** — body prose only (paragraphs longer than ~2 lines).
- Scale: 12 / 13 / 15 / 18 / 24 / 36 / 56 px (`--ox-fs-*`).
- Classes: `.ox-h1` (56) `.ox-h2` (36) `.ox-h3` (24) `.ox-label` (12 caps)
  `.ox-body` (15 Inter) `.ox-mono` (13).
- Fonts load from Google Fonts (see `styles.css` @import).

## Spacing & shape (`tokens/spacing.css`)

4px grid: `--ox-sp-1..16` = 4 / 8 / 12 / 16 / 24 / 40 / 64. Radius:
`--ox-radius` 4px, `--ox-radius-chip` 2px.

## Foundations galleries

- `guidelines/foundations/colors.html` — swatches + semantic pairing demo
- `guidelines/foundations/type.html` — scale specimens + the DON'T-bold rule
- `guidelines/foundations/spacing.html` — scale bars, radii, the one-glow rule

## Components (`components/core/`)

Each is `Name.jsx` (exposes `window.Name`) + `Name.d.ts` (props) +
`Name.prompt.md` (usage voice). Galleries: `core-a.card.html`,
`core-b.card.html`, `core-c.card.html`. Full-system demo: `showcase.dc.html`.

| Component | Purpose | Key props | Gallery |
|---|---|---|---|
| Button | Mono caps action; primary orange fill / secondary border / ghost dim | `variant, disabled, onClick` | core-a |
| TagChip | `0x0110 · Model` chip; dim hex + toned label | `hex, label, tone` | core-a |
| Badge | PASS/FAIL/WIP rects; supported/partial/unsupported dots | `status, label` | core-a |
| Callout | NOTE/WARN/FAIL strip with signal left rule | `kind, children` | core-a |
| Card | Stepped surface; legend-style `title`; one `hero` glow per page | `title, hero` | core-b |
| DataTable | Dense mono rows; keys dim / values green / offsets gutter | `columns[{key,label,kind}], rows` | core-b |
| StatTile | One big 56px mono number + dim caps label + ▲/▼ delta | `value, label, delta` | core-b |
| BenchmarkBar | Orange = us, gray = them; labeled bar ends | `items[{label,value,unit,highlight}], max` | core-b |
| HexViewer | Offset gutter, byte grid, ASCII column, one orange span | `bytes, baseOffset, highlight` | core-c |
| TerminalBlock | $ green, command orange, output dim; COPY affordance | `lines[{prompt,text}], title` | core-c |
| ScanLine | 2px track, orange shimmer sweep; determinate adds fill + % | `progress, label` | core-c |
| NavBar | 56px bar; oxi/dex logo; 2px cursor underline on active | `items[{label,active}], logoText` | core-c |
| SidebarNav | 220px docs nav; active = orange text + left rule | `sections[{title,items}]` | core-c |

## Logo

`assets/logo-mark.svg` — flat dark-background variant (surface circle, orange
line art, oxide cog). The original white-filled circle version lives in the
oxidex repo (`docs/public/logo.svg`) for light contexts like the GitHub
README. In chrome-level UI (NavBar), the mark is just the 8px orange square +
`oxi`/`dex` wordmark — the full SVG is for heroes and covers.

## How to consume

1. Copy `styles.css` + `tokens/` into your project with `copy_files` (or
   inline the three `:root` blocks into a `.dc.html` helmet).
2. Load fonts: IBM Plex Mono 400/500 + Inter 400/500 from Google Fonts.
3. Import components with the pinned React tags, then
   `<script type="text/babel" src="./Button.jsx"></script>` (components
   attach to `window`):

```html
<script src="https://unpkg.com/react@18.3.1/umd/react.development.js" integrity="sha384-hD6/rw4ppMLGNu3tX5cjIb+uRZ7UkRJ6BPkLpg4hAu/6onKUg4lLsHAs9EBPT82L" crossorigin="anonymous"></script>
<script src="https://unpkg.com/react-dom@18.3.1/umd/react-dom.development.js" integrity="sha384-u6aeetuaXnQ38mYT8rp6sbXaQe3NL9t+IBXmnYxwkUI2Hw4bsp2Wvmx4yRQF1uAm" crossorigin="anonymous"></script>
<script src="https://unpkg.com/@babel/standalone@7.29.0/babel.min.js" integrity="sha384-m08KidiNqLdpJqLq95G/LEi8Qvjl/xUYll3QILypMoQ65QorJ9Lvtp2RXYGBFj1y" crossorigin="anonymous"></script>
```
