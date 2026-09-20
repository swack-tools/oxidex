#!/usr/bin/env node

import { createHash } from 'node:crypto';
import { createReadStream } from 'node:fs';
import { mkdir, readFile, readdir, stat, writeFile } from 'node:fs/promises';
import http from 'node:http';
import path from 'node:path';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const requireFromDocs = createRequire(path.join(repo, 'docs/package.json'));

function usage(message) {
  if (message) console.error(message);
  console.error('usage: node tools/docs/release-audit.mjs --dist DIR --inventory FILE --output DIR --representatives FILE');
  process.exit(64);
}

function parseArgs(argv) {
  const args = {};
  for (let index = 0; index < argv.length; index += 2) {
    const flag = argv[index];
    const value = argv[index + 1];
    if (!['--dist', '--inventory', '--output', '--representatives'].includes(flag) || !value) {
      usage(`invalid argument: ${flag ?? '<missing>'}`);
    }
    args[flag.slice(2)] = value;
  }
  for (const required of ['dist', 'inventory', 'output', 'representatives']) {
    if (!args[required]) usage(`missing --${required}`);
  }
  return args;
}

function normalizeRoute(value) {
  let route = new URL(value, 'http://release-audit.invalid/').pathname;
  route = route.replace(/\/index\.html$/, '/').replace(/\.html$/, '');
  if (!route.startsWith('/')) route = `/${route}`;
  if (route.length > 1) route = route.replace(/\/$/, '');
  return route;
}

function normalizeBasePath(value) {
  const basePath = value ?? '/';
  if (typeof basePath !== 'string' || !basePath.startsWith('/') || !basePath.endsWith('/') || basePath.includes('..')) {
    throw new Error(`base_path must start and end with '/' and contain no '..': ${basePath}`);
  }
  return basePath.replace(/\/{2,}/g, '/');
}

function mountedPath(basePath, route) {
  return basePath === '/' ? route : `${basePath}${route.replace(/^\//, '')}`;
}

function unmountPath(basePath, pathname) {
  if (basePath === '/') return pathname;
  if (!pathname.startsWith(basePath)) return null;
  return `/${pathname.slice(basePath.length)}`;
}

async function readRouteList(filename) {
  const text = await readFile(filename, 'utf8');
  try {
    const parsed = JSON.parse(text);
    const values = Array.isArray(parsed) ? parsed : (parsed.routes ?? parsed.rendered_routes);
    if (!Array.isArray(values)) throw new Error('JSON must contain a routes array');
    return [...new Set(values.map(normalizeRoute))].sort();
  } catch (error) {
    if (error instanceof SyntaxError) {
      return [...new Set(text.split(/\r?\n/).map(line => line.trim()).filter(Boolean).map(normalizeRoute))].sort();
    }
    throw error;
  }
}

async function walk(directory, relative = '') {
  const entries = await readdir(path.join(directory, relative), { withFileTypes: true });
  const files = [];
  for (const entry of entries.sort((left, right) => left.name < right.name ? -1 : left.name > right.name ? 1 : 0)) {
    const child = path.join(relative, entry.name);
    if (entry.isDirectory()) files.push(...await walk(directory, child));
    else if (entry.isFile()) files.push(child);
  }
  return files;
}

async function discoverRoutes(dist) {
  return (await walk(dist))
    .filter(filename => filename.endsWith('.html'))
    .map(filename => normalizeRoute(`/${filename.split(path.sep).join('/')}`))
    .sort();
}

async function distAggregateHash(dist) {
  const aggregate = createHash('sha256');
  for (const filename of await walk(dist)) {
    const digest = await sha256File(path.join(dist, filename));
    aggregate.update(`${digest}  ./${filename.split(path.sep).join('/')}\n`);
  }
  return aggregate.digest('hex');
}

function mimeType(filename) {
  const extension = path.extname(filename).toLowerCase();
  return ({
    '.css': 'text/css; charset=utf-8',
    '.html': 'text/html; charset=utf-8',
    '.js': 'text/javascript; charset=utf-8',
    '.json': 'application/json; charset=utf-8',
    '.png': 'image/png',
    '.svg': 'image/svg+xml',
    '.woff': 'font/woff',
    '.woff2': 'font/woff2',
  })[extension] ?? 'application/octet-stream';
}

function expectedContentType(pathname) {
  const extension = path.extname(pathname).toLowerCase();
  return ({
    '.css': 'text/css', '.html': 'text/html', '.js': 'text/javascript', '.json': 'application/json',
    '.png': 'image/png', '.svg': 'image/svg+xml', '.woff': 'font/woff', '.woff2': 'font/woff2',
  })[extension] ?? null;
}

async function resolveStaticFile(dist, pathname) {
  let decoded;
  try { decoded = decodeURIComponent(pathname); } catch { return { status: 400 }; }
  const relative = decoded.replace(/^\/+/, '');
  const candidates = relative === ''
    ? [{ path: 'index.html', directoryIndex: false }]
    : [
        { path: relative, directoryIndex: false },
        { path: `${relative}.html`, directoryIndex: false },
        { path: path.join(relative, 'index.html'), directoryIndex: true },
      ];
  for (const candidate of candidates) {
    const filename = path.resolve(dist, candidate.path);
    if (filename !== dist && !filename.startsWith(`${dist}${path.sep}`)) return { status: 403 };
    try {
      if ((await stat(filename)).isFile()) return { status: 200, filename, directoryIndex: candidate.directoryIndex };
    } catch (error) {
      if (error.code !== 'ENOENT' && error.code !== 'ENOTDIR') return { status: 500 };
    }
  }
  return { status: 404 };
}

async function startServer(dist, events, basePath) {
  const server = http.createServer(async (request, response) => {
    const url = new URL(request.url, 'http://127.0.0.1');
    const unmounted = unmountPath(basePath, url.pathname);
    if (unmounted === '/__release_audit_test__/500') {
      response.writeHead(500, { 'content-type': 'text/plain' });
      response.end('fixture failure');
      events.push({ method: request.method, path: url.pathname, status: 500 });
      return;
    }
    if (unmounted === '/__release_audit_test__/wrong-content-type.png') {
      response.writeHead(200, { 'content-type': 'text/plain' });
      response.end('not an image');
      events.push({ method: request.method, path: url.pathname, status: 200 });
      return;
    }
    const resolved = unmounted === null ? { status: 404 } : await resolveStaticFile(dist, unmounted);
    if (resolved.status !== 200) {
      events.push({ method: request.method, path: url.pathname, status: resolved.status });
      response.writeHead(resolved.status, { 'content-type': 'text/plain' });
      response.end(http.STATUS_CODES[resolved.status] ?? 'error');
      return;
    }
    if (resolved.directoryIndex && !url.pathname.endsWith('/')) {
      events.push({ method: request.method, path: url.pathname, status: 301 });
      response.writeHead(301, { location: `${url.pathname}/${url.search}` });
      response.end();
      return;
    }
    events.push({ method: request.method, path: url.pathname, status: 200 });
    response.writeHead(200, { 'content-type': mimeType(resolved.filename) });
    createReadStream(resolved.filename).pipe(response);
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const address = server.address();
  return { server, baseUrl: `http://127.0.0.1:${address.port}` };
}

function closeServer(server) {
  return new Promise(resolve => server.close(resolve));
}

function browserListeners(page, route, errors) {
  page.on('console', message => {
    if (message.type() === 'error') errors.push({ kind: 'console_error', route, message: message.text() });
  });
  page.on('pageerror', error => errors.push({ kind: 'page_error', route, message: error.message, stack: error.stack ?? null }));
  page.on('requestfailed', request => errors.push({
    kind: 'request_failure', route, url: request.url(), resource_type: request.resourceType(),
    message: request.failure()?.errorText ?? 'request failed',
  }));
  page.on('response', response => {
    if (response.status() >= 400) errors.push({
      kind: 'response_failure', route, url: response.url(), status: response.status(),
      resource_type: response.request().resourceType(),
    });
  });
}

function relativeScreenshotPath(route, viewport, theme) {
  const slug = route === '/' ? 'home' : route.replace(/^\//, '').replace(/[^a-zA-Z0-9_-]+/g, '-');
  return path.join('screenshots', `${slug}--${viewport}--${theme}.png`);
}

async function sha256File(filename) {
  const bytes = await readFile(filename);
  return createHash('sha256').update(bytes).digest('hex');
}

async function inspectInteractions(page, viewport) {
  const results = {};
  const hamburger = page.locator('.VPNavBarHamburger').first();
  if (viewport === 'mobile' && await hamburger.count() && await hamburger.isVisible()) {
    await hamburger.click();
    results.mobile_menu = (await hamburger.getAttribute('aria-expanded')) === 'true' ? 'passed' : 'failed';
  } else results.mobile_menu = 'not_present';

  const search = page.locator('.DocSearch-Button').first();
  if (await search.count() && await search.isVisible()) {
    await search.click();
    const dialog = page.locator('.VPLocalSearchBox, .DocSearch-Container, .DocSearch-Modal, dialog[open], [role="dialog"]:visible').first();
    results.search = await dialog.waitFor({ state: 'visible', timeout: 3_000 }).then(() => 'passed', () => 'failed');
    await page.keyboard.press('Escape').catch(() => {});
    await dialog.waitFor({ state: 'hidden', timeout: 3_000 }).catch(() => {});
  } else results.search = 'not_present';

  results.code_block = await page.locator('pre code').count() ? 'present' : 'not_present';
  const originalPath = new URL(page.url()).pathname;
  const internalLinks = page.locator('a[href^="/"]');
  let navigated = false;
  for (let index = 0; index < Math.min(await internalLinks.count(), 30); index += 1) {
    const link = internalLinks.nth(index);
    const href = await link.getAttribute('href');
    if (!href || new URL(href, page.url()).pathname === originalPath || !(await link.isVisible())) continue;
    try {
      await link.click();
      await page.waitForURL(url => url.pathname !== originalPath, { timeout: 5_000 });
      await page.waitForLoadState('networkidle', { timeout: 30_000 });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      navigated = true;
      await page.goBack({ waitUntil: 'networkidle', timeout: 30_000 });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
    } catch {}
    break;
  }
  results.internal_navigation = navigated ? 'passed' : await internalLinks.count() ? 'failed' : 'not_present';

  const table = page.locator('table').first();
  results.wide_table = await table.count() ? await table.evaluate(element => {
    let container = element;
    while (container && container !== document.body) {
      const overflow = getComputedStyle(container).overflowX;
      if (container.scrollWidth > container.clientWidth && (overflow === 'auto' || overflow === 'scroll')) break;
      container = container.parentElement;
    }
    if (!container || container === document.body) {
      return element.getBoundingClientRect().right <= document.documentElement.clientWidth ? 'fit' : 'failed';
    }
    container.scrollLeft = 0;
    container.scrollLeft = container.scrollWidth;
    return container.scrollLeft > 0 ? 'scrolled' : 'failed';
  }) : 'not_present';
  return results;
}

async function captureScreenshot(page, filename) {
  const dimensions = await page.evaluate(() => ({
    width: Math.max(document.documentElement.scrollWidth, document.body.scrollWidth),
    height: Math.max(document.documentElement.scrollHeight, document.body.scrollHeight),
  }));
  if (dimensions.height <= 24_000) {
    await page.screenshot({ path: filename, fullPage: true, animations: 'disabled' });
    return { mode: 'full_page', original_dimensions: dimensions, files: [filename] };
  }

  const originalViewport = page.viewportSize();
  const tileHeight = 6_000;
  const tileCount = Math.ceil(dimensions.height / tileHeight);
  if (tileCount > 40) {
    throw new Error(`representative page requires ${tileCount} screenshot tiles; choose a reviewable wide-table representative`);
  }
  const files = [];
  const tiles = [];
  for (let y = 0, index = 0; y < dimensions.height; y += tileHeight, index += 1) {
    const height = Math.min(tileHeight, dimensions.height - y);
    await page.setViewportSize({ width: originalViewport.width, height });
    await page.evaluate(scrollY => new Promise(resolve => {
      window.scrollTo(0, scrollY);
      requestAnimationFrame(() => requestAnimationFrame(resolve));
    }), y);
    const tile = filename.replace(/\.png$/, `--tile-${String(index + 1).padStart(3, '0')}.png`);
    await page.screenshot({ path: tile, fullPage: false, animations: 'disabled' });
    files.push(tile);
    tiles.push({ y, height, path: tile });
  }
  await page.setViewportSize(originalViewport);
  return { mode: 'full_page_tiled', original_dimensions: dimensions, tile_height: tileHeight, files, tiles };
}

async function audit() {
  const args = parseArgs(process.argv.slice(2));
  const dist = path.resolve(args.dist);
  const output = path.resolve(args.output);
  await mkdir(output, { recursive: true });
  if ((await readdir(output)).length) {
    throw new Error(`output directory must be empty: ${output}`);
  }
  await mkdir(path.join(output, 'screenshots'), { recursive: true });
  const inventoryRoutes = await readRouteList(args.inventory);
  const representativeRoutes = await readRouteList(args.representatives);
  const discoveredRoutes = await discoverRoutes(dist);
  let inventoryDocument = {};
  try { inventoryDocument = JSON.parse(await readFile(args.inventory, 'utf8')); } catch {}
  const basePath = normalizeBasePath(inventoryDocument.base_path);
  const actualDistSha256 = await distAggregateHash(dist);
  const errors = [];
  const serverEvents = [];
  const routeResults = [];
  const routeFragments = new Map();
  const referencedUrls = new Set();
  const referencedBy = new Map();
  const browserMeta = {};
  const automationManifest = {
    schema_version: 1,
    generated_at: new Date().toISOString(),
    command: [process.execPath, ...process.argv.slice(1)],
    candidate_sha: inventoryDocument.candidate_sha ?? null,
    source_head_sha: inventoryDocument.source_head_sha ?? null,
    source_kind: inventoryDocument.source_kind ?? null,
    source_identity: inventoryDocument.source_identity ?? null,
    base_path: basePath,
    tree_hash: inventoryDocument.tree_hash ?? null,
    declared_dist_sha256: inventoryDocument.dist_sha256 ?? null,
    actual_dist_sha256: actualDistSha256,
    inventory_sha256: await sha256File(args.inventory),
    representatives_sha256: await sha256File(args.representatives),
    script_sha256: await sha256File(fileURLToPath(import.meta.url)),
    node_version: process.version,
    playwright_version: requireFromDocs('playwright/package.json').version,
    inventory_routes: inventoryRoutes,
    representative_routes: representativeRoutes,
    launch: {
      engine: 'chromium', headless: true, locale: 'en-US', device_scale_factor: 1,
      viewports: [{ name: 'desktop', width: 1440, height: 1000 }, { name: 'mobile', width: 390, height: 844 }],
      themes: ['light', 'dark'],
    },
  };
  await writeFile(path.join(output, 'automation-manifest.json'), `${JSON.stringify(automationManifest, null, 2)}\n`);
  let matrix = { status: 'failed', expected_cells: representativeRoutes.length * 4, samples: [], errors: [] };

  const missingFromInventory = discoveredRoutes.filter(route => !inventoryRoutes.includes(route));
  const missingFromDist = inventoryRoutes.filter(route => !discoveredRoutes.includes(route));
  if (missingFromInventory.length || missingFromDist.length) {
    errors.push({ kind: 'inventory_mismatch', missing_from_inventory: missingFromInventory, missing_from_dist: missingFromDist });
  }
  if (inventoryDocument.dist_sha256 && inventoryDocument.dist_sha256 !== actualDistSha256) {
    errors.push({
      kind: 'dist_hash_mismatch', expected: inventoryDocument.dist_sha256, actual: actualDistSha256,
    });
  }
  const invalidRepresentatives = representativeRoutes.filter(route => !inventoryRoutes.includes(route));
  if (invalidRepresentatives.length) {
    errors.push({ kind: 'representative_not_in_inventory', routes: invalidRepresentatives });
  }

  let server;
  let browser;
  if (!errors.length) {
    try {
      const started = await startServer(dist, serverEvents, basePath);
      server = started.server;
      const baseUrl = started.baseUrl;
      const { chromium } = requireFromDocs('playwright');
      browser = await chromium.launch({ headless: true });
      browserMeta.engine = 'chromium';
      browserMeta.version = browser.version();
      browserMeta.base_url = baseUrl;
      automationManifest.browser_version = browser.version();
      automationManifest.base_url = baseUrl;
      await writeFile(path.join(output, 'automation-manifest.json'), `${JSON.stringify(automationManifest, null, 2)}\n`);

      const context = await browser.newContext({
        viewport: { width: 1440, height: 1000 }, colorScheme: 'light', locale: 'en-US', deviceScaleFactor: 1,
      });
      for (const route of inventoryRoutes) {
        const routeErrors = [];
        const page = await context.newPage();
        browserListeners(page, route, routeErrors);
        try {
          const response = await page.goto(`${baseUrl}${mountedPath(basePath, route)}`, { waitUntil: 'networkidle', timeout: 30_000 });
          if (!response || response.status() >= 400) {
            routeErrors.push({ kind: 'route_failure', route, status: response?.status() ?? null });
          }
          const contentType = response?.headers()['content-type'] ?? null;
          if (response && !contentType?.startsWith('text/html')) {
            routeErrors.push({ kind: 'content_type_failure', route, expected: 'text/html', actual: contentType });
          }
          const heading = await page.locator('h1, h2').first().textContent().catch(() => null);
          const bodyLength = await page.locator('body').innerText().then(text => text.trim().length).catch(() => 0);
          if (!heading?.trim() || bodyLength < 20) {
            routeErrors.push({ kind: 'content_failure', route, message: 'missing heading or substantive body content' });
          }
          routeFragments.set(route, new Set(await page.locator('[id], a[name]').evaluateAll(elements =>
            elements.flatMap(element => [element.id, element.getAttribute('name')]).filter(Boolean),
          )));
          for (const reference of await page.locator('[href], [src]').evaluateAll(elements => elements.flatMap(element => [element.getAttribute('href'), element.getAttribute('src')]).filter(Boolean))) {
            const absolute = new URL(reference, page.url()).href;
            referencedUrls.add(absolute);
            if (!referencedBy.has(absolute)) referencedBy.set(absolute, new Set());
            referencedBy.get(absolute).add(route);
          }
          routeResults.push({ route, status: response?.status() ?? null, content_type: contentType, final_url: page.url(), heading, errors: routeErrors });
        } catch (error) {
          routeErrors.push({ kind: 'route_failure', route, message: error.message });
          routeResults.push({ route, status: null, final_url: page.url(), heading: null, errors: routeErrors });
        } finally {
          errors.push(...routeErrors);
          await page.close();
        }
      }

      for (const reference of [...referencedUrls].sort()) {
        const url = new URL(reference);
        if (url.origin !== new URL(baseUrl).origin) continue;
        const unmounted = unmountPath(basePath, url.pathname);
        if (unmounted === null) {
          errors.push({ kind: 'base_path_escape', url: url.href, source_routes: [...(referencedBy.get(url.href) ?? [])].sort(), base_path: basePath });
          continue;
        }
        const targetRoute = normalizeRoute(unmounted);
        const isRoute = inventoryRoutes.includes(targetRoute);
        if (!isRoute) {
          const response = await context.request.get(url.href).catch(error => ({ status: () => 0, _error: error.message }));
          const statusCode = response.status();
          if (statusCode >= 400 || statusCode === 0) errors.push({
            kind: 'asset_failure', url: url.href, source_routes: [...(referencedBy.get(url.href) ?? [])].sort(),
            status: statusCode, message: response._error,
          });
          const expected = expectedContentType(url.pathname);
          const actual = response.headers?.()['content-type'] ?? null;
          if (statusCode >= 200 && statusCode < 400 && expected && !actual?.startsWith(expected)) {
            errors.push({
              kind: 'content_type_failure', url: url.href, source_routes: [...(referencedBy.get(url.href) ?? [])].sort(),
              expected, actual,
            });
          }
        }
        if (url.hash) {
          const fragment = decodeURIComponent(url.hash.slice(1));
          if (!routeFragments.get(targetRoute)?.has(fragment)) {
            errors.push({ kind: 'fragment_failure', url: url.href, source_routes: [...(referencedBy.get(url.href) ?? [])].sort() });
          }
        }
      }
      await context.close();

      const viewportConfigs = [
        { name: 'desktop', width: 1440, height: 1000 },
        { name: 'mobile', width: 390, height: 844 },
      ];
      for (const route of representativeRoutes) {
        for (const viewport of viewportConfigs) {
          for (const theme of ['light', 'dark']) {
            const cell = `${route}|${viewport.name}|${theme}`;
            const sampleErrors = [];
            const context = await browser.newContext({
              viewport: { width: viewport.width, height: viewport.height }, colorScheme: theme,
              locale: 'en-US', deviceScaleFactor: 1,
            });
            const page = await context.newPage();
            browserListeners(page, route, sampleErrors);
            const screenshotRelative = relativeScreenshotPath(route, viewport.name, theme);
            const screenshot = path.join(output, screenshotRelative);
            let interactions = {};
            let observedTheme = {};
            let screenshotCapture = {};
            try {
              const response = await page.goto(`${baseUrl}${mountedPath(basePath, route)}`, { waitUntil: 'networkidle', timeout: 30_000 });
              if (!response || response.status() >= 400) sampleErrors.push({ kind: 'route_failure', route, status: response?.status() ?? null });
              await page.evaluate(() => document.fonts?.ready);
              observedTheme = await page.evaluate(() => ({
                html_class: document.documentElement.className,
                data_theme: document.documentElement.getAttribute('data-theme'),
                computed_color_scheme: getComputedStyle(document.documentElement).colorScheme,
                prefers_dark: matchMedia('(prefers-color-scheme: dark)').matches,
              }));
              if (observedTheme.prefers_dark !== (theme === 'dark')) sampleErrors.push({ kind: 'theme_failure', route, expected: theme, observed: observedTheme });
              interactions = await inspectInteractions(page, viewport.name);
              for (const [name, result] of Object.entries(interactions)) {
                if (result === 'failed') sampleErrors.push({ kind: 'interaction_failure', route, interaction: name });
              }
              screenshotCapture = await captureScreenshot(page, screenshot);
            } catch (error) {
              sampleErrors.push({ kind: 'sample_failure', route, message: error.message });
            }
            const screenshotFiles = screenshotCapture.files ?? [];
            const screenshotEvidence = [];
            for (const file of screenshotFiles) {
              screenshotEvidence.push({
                path: path.relative(output, file).split(path.sep).join('/'),
                sha256: await sha256File(file),
              });
            }
            const screenshotHash = screenshotEvidence.length === 1
              ? screenshotEvidence[0].sha256
              : screenshotEvidence.length
                ? createHash('sha256').update(screenshotEvidence.map(item => `${item.sha256}  ${item.path}\n`).join('')).digest('hex')
                : null;
            const primaryScreenshot = screenshotEvidence[0]?.path ?? screenshotRelative.split(path.sep).join('/');
            matrix.samples.push({
              cell, route, viewport: viewport.name, dimensions: { width: viewport.width, height: viewport.height },
              theme, observed_theme: observedTheme, screenshot: primaryScreenshot,
              screenshot_sha256: screenshotHash, screenshots: screenshotEvidence,
              screenshot_capture: { ...screenshotCapture, files: undefined, tiles: screenshotCapture.tiles?.map(tile => ({
                ...tile, path: path.relative(output, tile.path).split(path.sep).join('/'),
              })) }, interactions, errors: sampleErrors,
            });
            matrix.errors.push(...sampleErrors.map(error => ({ ...error, cell })));
            await context.close();
          }
        }
      }
      const expectedCells = new Set(representativeRoutes.flatMap(route => viewportConfigs.flatMap(viewport => ['light', 'dark'].map(theme => `${route}|${viewport.name}|${theme}`))));
      const actualCells = new Set(matrix.samples.filter(sample => sample.screenshot_sha256).map(sample => sample.cell));
      const missingCells = [...expectedCells].filter(cell => !actualCells.has(cell));
      if (missingCells.length) matrix.errors.push({ kind: 'missing_matrix_cells', cells: missingCells });
      matrix.status = matrix.errors.length ? 'failed' : 'passed';
      errors.push(...matrix.errors);
    } catch (error) {
      errors.push({ kind: 'audit_failure', message: error.stack ?? error.message });
    } finally {
      if (browser) await browser.close();
      if (server) await closeServer(server);
    }
  }

  const crawl = {
    schema_version: 1,
    generated_at: new Date().toISOString(),
    status: errors.length ? 'failed' : 'passed',
    dist,
    base_path: basePath,
    inventory_routes: inventoryRoutes,
    discovered_routes: discoveredRoutes,
    representative_routes: representativeRoutes,
    browser: browserMeta,
    routes: routeResults,
    referenced_urls: [...referencedUrls].sort(),
    errors,
  };
  await writeFile(path.join(output, 'crawl.json'), `${JSON.stringify(crawl, null, 2)}\n`);
  await writeFile(path.join(output, 'visual-matrix.json'), `${JSON.stringify(matrix, null, 2)}\n`);
  await writeFile(path.join(output, 'server.log'), `${serverEvents.map(event => JSON.stringify(event)).join('\n')}${serverEvents.length ? '\n' : ''}`);
  if (errors.length) {
    console.error(`release audit failed with ${errors.length} error(s); see ${path.join(output, 'crawl.json')}`);
    process.exitCode = 1;
  } else {
    console.log(`release audit passed: ${inventoryRoutes.length} routes, ${matrix.samples.length} visual cells`);
  }
}

await audit();
