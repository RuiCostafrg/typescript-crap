import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const importer = fileURLToPath(new URL('../scripts/import-coverage.mjs', import.meta.url));

async function project(t, files) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap diagnostics ü-')));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const [name, contents] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await fs.writeFile(path.join(root, name), contents);
  }
  return root;
}

/** Run the importer with a child that optionally writes RAW to the input path. */
function collect(root, { format = 'istanbul', raw, exit = 0 } = {}) {
  const input = path.join(root, 'out', format === 'istanbul' ? 'coverage-final.json' : 'lcov.info');
  const output = path.join(root, 'native.json');
  const child = `
    const fs = require('node:fs');
    ${raw === undefined ? '' : `fs.mkdirSync(${JSON.stringify(path.dirname(input))}, { recursive: true });
    fs.writeFileSync(${JSON.stringify(input)}, ${JSON.stringify(raw)});`}
    process.exit(${exit});`;
  const result = spawnSync(process.execPath, [importer, '--root', root, '--format', format,
    '--input', input, '--output', output, '--', process.execPath, '-e', child], { encoding: 'utf8', timeout: 60_000 });
  return { ...result, input, output };
}

const source = { 'src/a.ts': 'export function f() {\n  return 1;\n}\n' };
const covered = JSON.stringify({ 'src/a.ts': { path: 'src/a.ts', statementMap: { 0: { start: { line: 2, column: 2 } } }, s: { 0: 1 } } });

test('a failing test command names the fix and keeps exit code 2', async t => {
  const root = await project(t, source);
  const result = collect(root, { raw: covered, exit: 1 });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /Coverage command failed: 1; no report was written\. Fix the failing tests or coverage thresholds/);
  await assert.rejects(fs.access(result.output), { code: 'ENOENT' });
});

test('a missing report names coverage, reporter and directory causes for each format', async t => {
  const root = await project(t, source);
  const json = collect(root);
  assert.equal(json.status, 2);
  assert.match(json.stderr, /succeeded but wrote no report at .*coverage-final\.json/);
  assert.match(json.stderr, /coverage is not enabled \(pass --coverage\)/);
  assert.match(json.stderr, /does not emit Istanbul JSON \(use Vitest --coverage\.reporter=json or Jest --coverageReporters=json\)/);
  assert.match(json.stderr, /Vitest --coverage\.reportsDirectory, Jest --coverageDirectory/);
  const lcov = collect(root, { format: 'lcov' });
  assert.equal(lcov.status, 2);
  assert.match(lcov.stderr, /does not emit LCOV \(use Vitest --coverage\.reporter=lcovonly or Jest --coverageReporters=lcovonly\)/);
});

test('an empty report points at coverage include settings', async t => {
  const root = await project(t, source);
  for (const [format, raw] of [['istanbul', '{}'], ['lcov', 'TN:\n']]) {
    const result = collect(root, { format, raw });
    assert.equal(result.status, 2, format);
    assert.match(result.stderr, /Empty imported coverage in .*Likely causes: no tests ran, or coverage include does not match your sources \(Vitest coverage\.include, Jest collectCoverageFrom/, format);
  }
});

test('unremapped generated JavaScript explains how to get TypeScript coverage', async t => {
  const root = await project(t, { ...source, 'dist-js/a.js': 'exports.f = () => 1;\n' });
  const raw = JSON.stringify({ x: { path: 'dist-js/a.js', statementMap: { 0: { start: { line: 1, column: 0 } } }, s: { 0: 1 } } });
  const result = collect(root, { raw });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /No source map for generated input: .*a\.js; the report covers generated JavaScript instead of TypeScript/);
  assert.match(result.stderr, /Vitest istanbul or v8 provider; Jest babel or v8 provider with source maps.*NODE_V8_COVERAGE/);
});

test('sources outside the root name --root', async t => {
  const root = await project(t, source);
  const map = { version: 3, sources: ['../elsewhere/b.ts'], sourcesContent: ['x'], mappings: 'AAAA' };
  const raw = JSON.stringify({ x: { path: 'gen.js', inputSourceMap: map, statementMap: { 0: { start: { line: 1, column: 0 } } }, s: { 0: 1 } } });
  const result = collect(root, { raw });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /Unmappable source outside collected TypeScript snapshot: \.\.\/elsewhere\/b\.ts; it is outside --root .* or is not a TypeScript source/);
});

test('locations without a mapping keep the remapping hint', async t => {
  const root = await project(t, source);
  const map = { version: 3, sources: ['src/a.ts'], sourcesContent: [source['src/a.ts']], mappings: 'AAAA' };
  const raw = JSON.stringify({ x: { path: 'gen.js', inputSourceMap: map, statementMap: { 0: { start: { line: 5, column: 0 } } }, s: { 0: 1 } } });
  const result = collect(root, { raw });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /1 coverage locations could not be source-mapped; configure the test tool.*Vitest istanbul or v8 provider/);
});

test('TypeScript files absent from coverage produce a warning without changing success', async t => {
  const files = { ...source, 'src/b.ts': 'export const b = 1;\n', 'src/c.mts': 'export const c = 1;\n',
    'src/a.test.ts': '', 'src/a.spec.tsx': '', 'src/__mocks__/m.ts': '', 'vitest.config.ts': '', 'types.d.ts': '' };
  const root = await project(t, files);
  const result = collect(root, { raw: covered });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stderr, /Warning: 2 TypeScript source files have no coverage records and will count as missing coverage if scanned: src\/b\.ts, src\/c\.mts\. If they should be measured, add them to Vitest coverage\.include or Jest collectCoverageFrom/);
  assert.deepEqual(Object.keys(JSON.parse(await fs.readFile(result.output, 'utf8')).files), ['src/a.ts']);
});

test('the warning is singular, truncated after ten files, and absent when everything is covered', async t => {
  const one = await project(t, { ...source, 'src/b.ts': 'export const b = 1;\n' });
  assert.match(collect(one, { raw: covered }).stderr, /Warning: 1 TypeScript source file has no coverage records .*: src\/b\.ts\./);
  const many = Object.fromEntries(Array.from({ length: 12 }, (_, i) => [`src/m${String(i).padStart(2, '0')}.ts`, '']));
  const truncated = collect(await project(t, { ...source, ...many }), { raw: covered });
  assert.match(truncated.stderr, /Warning: 12 TypeScript source files .*src\/m09\.ts, and 2 more\./);
  assert.doesNotMatch(truncated.stderr, /m10/);
  const complete = collect(await project(t, source), { raw: covered });
  assert.equal(complete.status, 0, complete.stderr);
  assert.doesNotMatch(complete.stderr, /Warning/);
});
