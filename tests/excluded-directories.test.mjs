import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { DEFAULT_EXCLUDED_DIRECTORIES, policyExclusions, snapshot } from '../scripts/coverage-snapshot.mjs';

const scripts = fileURLToPath(new URL('../scripts/', import.meta.url));
// cargo test passes the scanner built from this checkout; standalone runs skip scans.
const scanner = process.env.TS_CRAP_SCANNER;
const source = 'export function f() {\n  return 1;\n}\n';

async function project(t, files = {}) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap excluded ü-')));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const [name, contents] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await fs.writeFile(path.join(root, name), contents);
  }
  return root;
}

const policy = names => JSON.stringify({ version: 1, excluded_directories: names });

test('policy exclusions default when absent and replace the list when configured', async t => {
  assert.deepEqual(DEFAULT_EXCLUDED_DIRECTORIES, ['node_modules', '.git', 'target', 'dist', 'build', 'coverage']);
  const root = await project(t);
  assert.deepEqual(await policyExclusions(root), DEFAULT_EXCLUDED_DIRECTORIES);
  await fs.writeFile(path.join(root, 'ts-crap.json'), '{"version":1,"threshold":3}');
  assert.deepEqual(await policyExclusions(root), DEFAULT_EXCLUDED_DIRECTORIES, 'other keys keep defaults');
  await fs.writeFile(path.join(root, 'ts-crap.json'), policy(['node_modules', '.cache']));
  assert.deepEqual(await policyExclusions(root), ['node_modules', '.cache']);
  await fs.writeFile(path.join(root, 'alt.json'), policy([]));
  assert.deepEqual(await policyExclusions(root, path.join(root, 'alt.json')), [], 'an explicit config wins');
  await assert.rejects(policyExclusions(root, path.join(root, 'missing.json')), /Cannot read project configuration/);
});

test('policy exclusions reject what the scanner rejects', async t => {
  const root = await project(t);
  const file = path.join(root, 'ts-crap.json');
  for (const names of [[''], ['.'], ['..'], ['a/b'], ['a\\b'], ['a\u0000b'], [1], 'build']) {
    await fs.writeFile(file, policy(names));
    await assert.rejects(policyExclusions(root), /Invalid project configuration .*excluded_directories/, JSON.stringify(names));
  }
  await fs.writeFile(file, policy(['build', 'build']));
  await assert.rejects(policyExclusions(root), /duplicate excluded_directories entry: "build"/);
  await fs.writeFile(file, '{"version":1,"version":1}');
  await assert.rejects(policyExclusions(root), /Invalid project configuration .*Duplicate JSON key/);
});

test('a dangling default policy link fails instead of using defaults', { skip: process.platform === 'win32' && 'symlinks need privileges' }, async t => {
  const root = await project(t);
  await fs.symlink(path.join(root, 'absent.json'), path.join(root, 'ts-crap.json'));
  await assert.rejects(policyExclusions(root), /Cannot read project configuration/);
});

test('snapshots honor the configured list', async t => {
  const root = await project(t, { 'src/a.ts': source, 'src/build/b.ts': source, 'node_modules/p/c.ts': source });
  const options = { supported: name => name.endsWith('.ts'), sourceText: false, lineCount: false, compareNames: (a, b) => a < b ? -1 : 1 };
  const names = async excludedDirectories => [...(await snapshot(root, { ...options, excludedDirectories })).keys()]
    .map(file => path.relative(root, file).split(path.sep).join('/'));
  assert.deepEqual(await names(undefined), ['src/a.ts']);
  assert.deepEqual(await names(['node_modules']), ['src/a.ts', 'src/build/b.ts']);
  assert.deepEqual(await names([]), ['node_modules/p/c.ts', 'src/a.ts', 'src/build/b.ts']);
});

function importLcov(root, raw, config) {
  const directory = path.join(root, `out-${Math.random().toString(16).slice(2)}`);
  const input = path.join(directory, 'lcov.info'), output = path.join(directory, 'native.json');
  const child = `require('node:fs').mkdirSync(${JSON.stringify(directory)},{recursive:true});`
    + `require('node:fs').writeFileSync(${JSON.stringify(input)}, ${JSON.stringify(raw)});`;
  const result = spawnSync(process.execPath, [path.join(scripts, 'import-coverage.mjs'), '--root', root, '--format', 'lcov',
    '--input', input, '--output', output, ...(config ? ['--config', config] : []), '--', process.execPath, '-e', child],
  { encoding: 'utf8', timeout: 60_000 });
  return { ...result, output };
}

test('imports cover a real build/ directory only when the policy selects it', async t => {
  const root = await project(t, { 'src/build/b.ts': source });
  const raw = 'SF:src/build/b.ts\nDA:2,1\nend_of_record\n';
  const skipped = importLcov(root, raw);
  assert.equal(skipped.status, 2);
  assert.match(skipped.stderr, /excluded directory \(node_modules, \.git, target, dist, build, coverage\)/);
  await fs.writeFile(path.join(root, 'ts-crap.json'), policy(['node_modules']));
  const selected = importLcov(root, raw);
  assert.equal(selected.status, 0, selected.stderr);
  assert.deepEqual(JSON.parse(await fs.readFile(selected.output, 'utf8')).files['src/build/b.ts'].lines, { 2: 1 });
  // An explicit --config is read the same way, and its list is named in diagnostics.
  await fs.writeFile(path.join(root, 'strict.json'), policy(['src']));
  const strict = importLcov(root, raw, path.join(root, 'strict.json'));
  assert.equal(strict.status, 2);
  assert.match(strict.stderr, /excluded directory \(src\)/);
  await fs.writeFile(path.join(root, 'ts-crap.json'), policy(['a/b']));
  const invalid = importLcov(root, raw);
  assert.equal(invalid.status, 2);
  assert.match(invalid.stderr, /Invalid project configuration/);
});

test('native collection follows the policy and the scanner accepts the result', { skip: !scanner && 'TS_CRAP_SCANNER not set' }, async t => {
  const root = await project(t, { 'src/build/b.mts': source, 'test.mjs': "import { f } from './src/build/b.mts'; if (f() !== 1) throw Error('bad');\n" });
  await fs.writeFile(path.join(root, 'ts-crap.json'), JSON.stringify({ version: 1, excluded_directories: ['node_modules'], require_coverage: true }));
  const output = path.join(root, 'out', 'native.json');
  const collected = spawnSync(process.execPath, [path.join(scripts, 'coverage.mjs'), '--root', root, '--output', output,
    '--', process.execPath, 'test.mjs'], { cwd: root, encoding: 'utf8', timeout: 60_000 });
  assert.equal(collected.status, 0, collected.stderr);
  assert(JSON.parse(await fs.readFile(output, 'utf8')).files['src/build/b.mts']);
  const scan = spawnSync(scanner, ['--root', root, '--coverage', output, '--format', 'json'], { encoding: 'utf8' });
  assert.equal(scan.status, 0, scan.stderr);
  const report = JSON.parse(scan.stdout);
  assert.deepEqual(report.functions.map(f => [f.path, f.coverage]), [['src/build/b.mts', 1]]);
});
