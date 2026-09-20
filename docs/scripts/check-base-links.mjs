#!/usr/bin/env node
// Offline link check for a built VitePress site: proves every same-origin
// href/src in the rendered HTML (root-absolute or relative) stays inside the
// site base.
//
// Why: the stable site is served from '/', the preview from a sub-path (see
// DOCS_BASE in .vitepress/config.mts). A root-absolute URL that VitePress does
// not rewrite -- raw <a href="/..."> HTML in Markdown, a hard-coded head link
// -- works on the stable site and 404s on the preview, and nothing else
// notices. Building with a non-root base and running this makes that a failure.
//
//   node scripts/check-base-links.mjs [distDir] [base]
//     distDir defaults to .vitepress/dist, base to $DOCS_BASE or '/'.
//
// Exit 1 if any link escapes the base. Links inside the base whose target is
// not in the build are reported as warnings only: several predate this check
// (Markdown links to .json files VitePress does not copy, and /benchmarks/,
// which deploy-docs.yml adds after the build) and are equally broken on both
// channels, so they are not a base regression.
import fs from 'node:fs'
import path from 'node:path'

const dist = path.resolve(process.argv[2] || '.vitepress/dist')
const base = process.argv[3] || process.env.DOCS_BASE || '/'
if (!base.startsWith('/') || !base.endsWith('/')) {
  console.error(`base must start and end with '/', got '${base}'`)
  process.exit(2)
}
if (!fs.existsSync(path.join(dist, 'index.html'))) {
  console.error(`no index.html under ${dist}; build the site first`)
  process.exit(2)
}

function* htmlFiles(dir) {
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, e.name)
    if (e.isDirectory()) yield* htmlFiles(p)
    else if (e.name.endsWith('.html')) yield p
  }
}

// cleanUrls: /guide/cli-usage -> guide/cli-usage.html; /guide/ -> guide/index.html
function exists(rel) {
  const p = path.join(dist, decodeURIComponent(rel))
  return [p, `${p}.html`, path.join(p, 'index.html')].some(c => fs.existsSync(c) && fs.statSync(c).isFile())
}

const escaping = new Map()
const missing = new Map()
let pages = 0
let links = 0
for (const file of htmlFiles(dist)) {
  pages++
  const html = fs.readFileSync(file, 'utf8')
  const page = '/' + path.relative(dist, file).split(path.sep).join('/')
  // The URL the page is served at (cleanUrls), which relative links resolve against.
  const served = new URL(base + page.slice(1).replace(/(^|\/)index\.html$/, '$1').replace(/\.html$/, ''), 'http://site')
  for (const m of html.matchAll(/\s(?:href|src)="([^"]*)"/g)) {
    const raw = m[1].replace(/&amp;/g, '&')
    if (raw.startsWith('//') || raw.startsWith('#') || /^[a-z][a-z0-9+.-]*:/i.test(raw)) continue // external, anchor
    links++
    const p = new URL(raw, served).pathname
    const bucket = !p.startsWith(base) ? escaping : !exists(p.slice(base.length)) ? missing : null
    if (!bucket) continue
    if (!bucket.has(p)) bucket.set(p, new Set())
    bucket.get(p).add(page)
  }
}

console.log(`checked ${links} same-origin links on ${pages} pages against base '${base}'`)
for (const [p, from] of missing) {
  console.log(`::warning::link target not in build: ${p} (from ${[...from][0]}${from.size > 1 ? ` +${from.size - 1}` : ''})`)
}
if (escaping.size) {
  for (const [p, from] of escaping) {
    console.log(`::error::link escapes base '${base}': ${p} (from ${[...from][0]}${from.size > 1 ? ` +${from.size - 1}` : ''})`)
  }
  console.log(`${escaping.size} link target(s) escape the base`)
  process.exit(1)
}
console.log(`OK: no link escapes the base (${missing.size} pre-existing missing target(s) warned above)`)
