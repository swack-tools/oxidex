import assert from 'node:assert/strict';
import { cp, mkdtemp, mkdir, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const fixture = path.join(repo, 'tools/docs/testdata/release-audit-site');
const audit = path.join(repo, 'tools/docs/release-audit.mjs');

function run(command, args, options = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: repo, ...options });
    let stdout = '';
    let stderr = '';
    child.stdout?.on('data', chunk => { stdout += chunk; });
    child.stderr?.on('data', chunk => { stderr += chunk; });
    child.on('error', reject);
    child.on('close', code => resolve({ code, stdout, stderr }));
  });
}

async function workspace() {
  const root = await mkdtemp(path.join(tmpdir(), 'oxidex-release-audit-'));
  const dist = path.join(root, 'dist');
  const output = path.join(root, 'evidence');
  await cp(fixture, dist, { recursive: true });
  await mkdir(output);
  return { root, dist, output };
}

async function runAudit({ dist, output }, inventory = path.join(fixture, 'inventory.json')) {
  return run(process.execPath, [
    audit,
    '--dist', dist,
    '--inventory', inventory,
    '--output', output,
    '--representatives', path.join(fixture, 'representatives.json'),
  ]);
}

async function rewriteAbsoluteReferences(directory, basePath) {
  for (const relative of ['index.html', 'guide.html']) {
    const filename = path.join(directory, relative);
    const html = await readFile(filename, 'utf8');
    await writeFile(filename, html.replaceAll('href="/', `href="${basePath}`).replaceAll('src="/', `src="${basePath}`));
  }
}

async function shellDistSha256(dist) {
  const result = await run('bash', ['-c', [
    'set -euo pipefail',
    'cd "$1"',
    "find . -type f -print0 | LC_ALL=C sort -z | xargs -0 shasum -a 256 | shasum -a 256 | awk '{print $1}'",
  ].join('\n'), 'release-audit-hash', dist]);
  assert.equal(result.code, 0, result.stderr);
  return result.stdout.trim();
}

test('audits every inventory route and emits the exact visual matrix', async () => {
  const ws = await workspace();
  const inventory = path.join(ws.root, 'inventory.json');
  await writeFile(inventory, JSON.stringify({
    routes: ['/', '/guide'],
    dist_sha256: await shellDistSha256(ws.dist),
  }));
  const result = await runAudit(ws, inventory);
  assert.equal(result.code, 0, result.stderr || result.stdout);
  const crawl = JSON.parse(await readFile(path.join(ws.output, 'crawl.json'), 'utf8'));
  const matrix = JSON.parse(await readFile(path.join(ws.output, 'visual-matrix.json'), 'utf8'));
  const automation = JSON.parse(await readFile(path.join(ws.output, 'automation-manifest.json'), 'utf8'));
  assert.deepEqual(crawl.inventory_routes, ['/', '/guide']);
  assert.deepEqual(crawl.discovered_routes, ['/', '/guide']);
  assert.equal(crawl.status, 'passed');
  assert.equal(matrix.expected_cells, 8);
  assert.equal(matrix.samples.length, 8);
  assert.equal(new Set(matrix.samples.map(sample => sample.cell)).size, 8);
  assert.ok(matrix.samples.every(sample => sample.screenshot_sha256));
  assert.equal(automation.actual_dist_sha256.length, 64);
  assert.equal(automation.representative_routes.length, 2);
});

test('audits a site mounted beneath the configured base path', async () => {
  const ws = await workspace();
  const inventory = path.join(ws.root, 'inventory.json');
  const representatives = path.join(ws.root, 'representatives.json');
  await rewriteAbsoluteReferences(ws.dist, '/preview/');
  await writeFile(inventory, JSON.stringify({
    routes: ['/', '/guide'],
    base_path: '/preview/',
    dist_sha256: await shellDistSha256(ws.dist),
  }));
  await writeFile(representatives, JSON.stringify({ routes: ['/', '/guide'] }));
  const result = await run(process.execPath, [
    audit,
    '--dist', ws.dist,
    '--inventory', inventory,
    '--output', ws.output,
    '--representatives', representatives,
  ]);
  assert.equal(result.code, 0, result.stderr || result.stdout);
  const crawl = JSON.parse(await readFile(path.join(ws.output, 'crawl.json'), 'utf8'));
  const automation = JSON.parse(await readFile(path.join(ws.output, 'automation-manifest.json'), 'utf8'));
  assert.equal(crawl.status, 'passed');
  assert.equal(crawl.base_path, '/preview/');
  assert.ok(crawl.routes.every(route => new URL(route.final_url).pathname.startsWith('/preview/')));
  assert.equal(automation.base_path, '/preview/');
});

test('fails closed when rendered routes and the supplied inventory differ', async () => {
  const ws = await workspace();
  const inventory = path.join(ws.root, 'inventory.json');
  await writeFile(inventory, JSON.stringify({ routes: ['/'] }));
  const result = await runAudit(ws, inventory);
  assert.notEqual(result.code, 0);
  const crawl = JSON.parse(await readFile(path.join(ws.output, 'crawl.json'), 'utf8'));
  assert.ok(crawl.errors.some(error => error.kind === 'inventory_mismatch'));
});

test('fails closed when the snapshot manifest does not match the dist', async () => {
  const ws = await workspace();
  const inventory = path.join(ws.root, 'inventory.json');
  await writeFile(inventory, JSON.stringify({ routes: ['/', '/guide'], dist_sha256: '0'.repeat(64) }));
  const result = await runAudit(ws, inventory);
  assert.notEqual(result.code, 0);
  const crawl = JSON.parse(await readFile(path.join(ws.output, 'crawl.json'), 'utf8'));
  assert.ok(crawl.errors.some(error => error.kind === 'dist_hash_mismatch'));
});

test('fails on 404 and 500 responses and failed local assets', async () => {
  const ws = await workspace();
  const index = path.join(ws.dist, 'index.html');
  const html = await readFile(index, 'utf8');
  await writeFile(index, html.replace(
    '</main>',
    '<img src="/missing.png"><img src="/__release_audit_test__/500"><img src="/__release_audit_test__/wrong-content-type.png"></main>',
  ));
  const result = await runAudit(ws);
  assert.notEqual(result.code, 0);
  const crawl = JSON.parse(await readFile(path.join(ws.output, 'crawl.json'), 'utf8'));
  assert.ok(crawl.errors.some(error => error.status === 404));
  assert.ok(crawl.errors.some(error => error.status === 500));
  assert.ok(crawl.errors.some(error => error.kind === 'asset_failure'));
  assert.ok(crawl.errors.some(error => error.kind === 'content_type_failure'));
});

test('fails on missing fragments and browser console, page, and request errors', async () => {
  const ws = await workspace();
  const index = path.join(ws.dist, 'index.html');
  const html = await readFile(index, 'utf8');
  await writeFile(index, html.replace(
    '</main>',
    '<a href="/guide#absent">bad fragment</a><img src="http://127.0.0.1:1/refused.png"><script>console.error("fixture console"); queueMicrotask(() => { throw new Error("fixture page"); });</script></main>',
  ));
  const result = await runAudit(ws);
  assert.notEqual(result.code, 0);
  const crawl = JSON.parse(await readFile(path.join(ws.output, 'crawl.json'), 'utf8'));
  for (const kind of ['fragment_failure', 'console_error', 'page_error', 'request_failure']) {
    assert.ok(crawl.errors.some(error => error.kind === kind), `missing ${kind}: ${JSON.stringify(crawl.errors)}`);
  }
});

test('build-only local deployment terminates and writes a snapshot manifest', { timeout: 180_000 }, async () => {
  const root = await mkdtemp(path.join(tmpdir(), 'oxidex-docs-snapshot-'));
  const output = path.join(root, 'snapshot');
  const result = await run('bash', [
    'tools/docs-local-deploy.sh', '--worktree', '--build-only', '--output', output,
  ]);
  assert.equal(result.code, 0, result.stderr || result.stdout);
  const manifest = JSON.parse(await readFile(path.join(output, 'snapshot-manifest.json'), 'utf8'));
  assert.equal(manifest.status, 'built');
  assert.equal(manifest.source_kind, 'worktree');
  assert.equal(manifest.candidate_sha, null);
  assert.match(manifest.source_head_sha, /^[0-9a-f]{40}$/);
  assert.equal(manifest.source_identity, `${manifest.source_head_sha}-worktree`);
  assert.equal(manifest.base_path, '/');
  assert.ok(manifest.tree_hash);
  assert.ok(Array.isArray(manifest.routes) && manifest.routes.length > 0);
  assert.ok(manifest.dist_sha256);
});
