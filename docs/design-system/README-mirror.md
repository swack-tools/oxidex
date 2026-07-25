# Design-system mirror

This directory is a source mirror of the Claude Design project **"OxiDex
Design System"** (`claude.ai/design/p/5ad8ee91-589a-4594-b5ac-322178778535`),
which is the live, editable home of the Oxide Terminal design system. The
system itself is documented in `readme.md` (mirrored from the project).

- The Claude Design project is the source of truth; this mirror is for
  version control and review. If the two diverge, the project wins.
- `support.js` (the Design Components runtime that `showcase.dc.html` loads)
  is intentionally NOT mirrored: it is a server-generated bundle, recreated in
  the project via the `create_support_js` tool. `showcase.dc.html` therefore
  only renders inside Claude Design, not from this directory.
- Everything else renders standalone: open the `guidelines/` pages or
  `components/core/core-*.card.html` galleries in a browser (they pull React
  and Babel from CDN).

Mirrored 2026-07-24, after the responsive full-width pass on the showcase and
the narrow-viewport fixes to the galleries.
