# OxiDex Design System — "Oxide Terminal" — Design Spec

**Date:** 2026-07-24
**Status:** Approved for planning
**Deliverable:** A Claude Design project ("OxiDex Design System") — not code in this repo.

## Purpose

A reusable visual identity for OxiDex, usable as the design system for future
Claude Design work: landing pages, docs themes, dashboards, and decks about the
project. Derived from the repo's existing branding (VitePress rust-orange
palette `#dd7732`/`#c96628`/`#b5551e`, cog-document logo) — **evolved**, not
strictly matched, and not a free reimagining.

## Direction: Oxide Terminal

Precision developer-tool aesthetic. Near-black surfaces, rust-orange accents,
monospace-forward type, hex-dump and tag-table motifs. Feel: exact, fast,
engineered — a beautiful terminal, not a website. **Dark mode only.**

## Foundations

### Color tokens (CSS custom properties)

| Token | Value | Role |
|---|---|---|
| `--ox-bg` | `#0d0f12` | Page background (near-black, slightly cool) |
| `--ox-surface` | `#16191f` | Panels, cards |
| `--ox-surface-2` | `#1d2129` | Raised/hover surfaces, table header rows |
| `--ox-border` | `#2a2f3a` | Hairline borders (1px, everywhere) |
| `--ox-accent` | `#e8824a` | Rust-orange brand anchor (evolved from `#dd7732` for dark-bg contrast) |
| `--ox-accent-deep` | `#b5551e` | Pressed states; gradient endpoint back to original brand |
| `--ox-text` | `#e6e9ef` | Primary text |
| `--ox-text-dim` | `#8b93a3` | Secondary text, labels, hex offsets |
| `--ox-green` | `#7fd88f` | Values, pass/success (the "data" color) |
| `--ox-red` | `#e06c75` | Errors, fail states |
| `--ox-blue` | `#6cb6ff` | Links, info |

**Semantic pairing rule** (borrowed from terminal output, applied to every
component): keys/labels are dim, values are green, offsets are dim mono, brand
actions are orange.

### Typography

- **IBM Plex Mono** — headings, data, labels, nav, buttons. The signature voice.
- **Inter** — body prose only (paragraphs longer than ~2 lines).
- Scale: 12 / 13 / 15 / 18 / 24 / 36 / 56 px.
- Headings: mono at normal weight with letter-spacing — not bold. Terminal
  text isn't bold, it's spaced.
- Fonts load via Google Fonts CDN links in each file.

### Spacing & shape

- 4px base grid; common steps 8 / 12 / 16 / 24 / 40 / 64.
- Border radius: 4px max (2px on chips). Crisp, not rounded.
- No drop shadows — elevation via surface color steps + 1px borders.
  One exception: faint orange glow `box-shadow: 0 0 24px rgba(232,130,74,.15)`
  reserved for the single hero element on a page.

### Accessibility floor

`--ox-text-dim` (#8b93a3) on `--ox-surface` (#16191f) is ~4.6:1 — AA for the
small mono text it's used on. Any text below 13px uses `--ox-text` instead.

## Component kit (13 components)

Each is a documented component with variants + usage notes:

1. **Button** — mono uppercase label, 1px border, 4px radius. Variants:
   primary (orange fill, black text), secondary (border only, orange text),
   ghost (dim text).
2. **Tag chip** — signature component: small mono chip like `0x0110 · Model`,
   dim hex prefix + bright name. Variants: neutral, orange (group name),
   green (value present), red (error).
3. **Card / Panel** — surface + hairline border; optional titled header
   rendered like a box-drawing frame (`┌─ TITLE ─` style, via CSS, not
   literal glyphs).
4. **Data table** — dense mono rows, `surface-2` header, dim key column /
   green value column, optional hex-offset gutter column, hover row highlight.
5. **Hex viewer block** — offset gutter, byte pairs, ASCII column, orange
   highlight span for "the bytes that matter."
6. **Terminal block** — CLI sample with `$` prompt lines, orange command
   highlighting, copy affordance.
7. **Stat tile** — big mono number (e.g. `9.7×`), dim label, optional
   green/red delta.
8. **Benchmark bar** — horizontal comparison bars (oxidex orange vs. ExifTool
   dim gray), value labels at bar ends; follows dataviz skill rules.
9. **Nav bar** — slim top bar: logo mark, mono links, orange 2px active
   underline (cursor-like).
10. **Sidebar nav** — docs-style section list, dim items, active item orange
    with left rule.
11. **Badge / status** — `PASS` / `FAIL` / `WIP` mono badges; format-support
    states (green dot / half dot / dim dot).
12. **Progress / scan line** — thin progress bar with animated scan shimmer;
    doubles as a section-divider motif.
13. **Callout** — info/warn/error strip with left border color and mono label
    prefix (`NOTE`, `WARN`, `FAIL`).

### Non-component pieces

- **Logo treatment** — reuse the existing cog-document mark as a flat
  single-color orange glyph on dark backgrounds; the white-fill circle version
  remains for light contexts (e.g. README).
- **Showcase page** — `showcase.dc.html`: a fake "oxidex inspect" report page
  exercising every component — nav, hero stat row, tag table, hex viewer,
  benchmark section, callouts, footer.

## Delivery

- New Claude Design project **"OxiDex Design System"**, structured per Claude
  Design's design-system conventions (fetch exact canonical layout via
  `get_claude_design_prompt` before writing).
- README / design-system guide: token reference, semantic pairing rule,
  do/don't notes.
- One `.dc.html` per component (13) + `showcase.dc.html`; each file
  self-contained (carries the shared token CSS block) per canonical-HTML rules.

## Verification

After every write: `render_preview` → open in Chrome (fresh page) →
screenshot + console + failed-request check → fix until clean → fresh-eyes
pass with the design-verifier agent against this spec. Explicit dark-bg
contrast check per the accessibility floor above.

## Out of scope (YAGNI)

Light mode; deck-stage theme; form controls beyond buttons; modals; charts
beyond the benchmark bar; animation beyond the scan shimmer; redesigning the
actual VitePress docs site.
