#!/usr/bin/env node
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const repository = fileURLToPath(new URL('../', import.meta.url));
const require = createRequire(new URL('../tests/tooling/package.json', import.meta.url));
const Ajv = require('ajv/dist/2020.js');
const ajv = new Ajv({ allErrors: true, strict: true });
const validators = {};
for (const name of ['config', 'coverage', 'baseline', 'report']) {
  validators[name] = ajv.compile(JSON.parse(await fs.readFile(new URL(`../schemas/${name}-v1.schema.json`, import.meta.url))));
}
function valid(name, document) {
  assert(validators[name](document), `${name}: ${ajv.errorsText(validators[name].errors)}`);
}
function invalid(name, document) {
  assert.equal(validators[name](document), false, `${name}: invalid example accepted`);
}
for (const name of Object.keys(validators)) {
  valid(name, JSON.parse(await fs.readFile(path.join(repository, 'schemas/examples', `${name}.json`))));
}
const scanner = path.resolve(process.argv[2] ?? 'target/debug/ts-crap');
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap contracts '));
const source = 'export function example(flag: boolean) {\n  if (flag) return 1;\n  return 0;\n}\n';
const coverage = { version: 1, files: { 'example.ts': { sha256: createHash('sha256').update(source).digest('hex'), lines: { 2: 1, 3: 1 } } } };
const scan = (extra, expected = 0) => {
  const result = spawnSync(scanner, ['--root', temporary, '--format', 'json', ...extra], { encoding: 'utf8' });
  assert.ifError(result.error);
  assert.equal(result.status, expected, `${result.stdout}\n${result.stderr}`);
  if (expected === 2) { assert.equal(result.stdout, ''); return; }
  const report = JSON.parse(result.stdout);
  valid('report', report);
  for (const f of report.functions) {
    assert(Math.abs(f.crap - (f.complexity ** 2 * (1 - f.coverage) ** 3 + f.complexity)) < 1e-10);
    assert(f.end_line >= f.line);
  }
  assert.equal(report.failed, report.completeness_failed || report.functions.some(f => f.failed));
  return report;
};
try {
  await fs.writeFile(path.join(temporary, 'example.ts'), source);
  await fs.writeFile(path.join(temporary, 'ts-crap.json'), JSON.stringify({ version: 1, threshold: 2, require_coverage: true }));
  const native = path.join(temporary, 'coverage.json');
  const baseline = path.join(temporary, 'baseline.json');
  await fs.writeFile(native, JSON.stringify(coverage));
  const full = scan(['--coverage', native, '--write-baseline', baseline]);
  assert.equal(full.functions[0].crap, 2);
  valid('coverage', coverage);
  valid('baseline', JSON.parse(await fs.readFile(baseline)));
  scan(['--coverage', native, '--baseline', baseline]);
  coverage.files['example.ts'].lines[3] = 0;
  await fs.writeFile(native, JSON.stringify(coverage));
  assert.equal(scan(['--coverage', native], 1).functions[0].coverage, 0.5);
  delete coverage.files['example.ts'].lines[3];
  await fs.writeFile(native, JSON.stringify(coverage));
  assert.equal(scan(['--coverage', native], 1).functions_missing_coverage, 1);
  await fs.writeFile(path.join(temporary, 'ts-crap.json'), '{"version":1,"threshold":2,"threshold":10}');
  scan([], 2);
  for (const threshold of [-1, null, '10', true]) invalid('config', { version: 1, threshold });
  invalid('config', { version: 1, unknown: true });
  invalid('config', { version: 2 });
  invalid('config', { version: 1, require_coverage: null });
  invalid('coverage', { ...coverage, files: { 'example.ts': { sha256: 'bad', lines: {} } } });
  invalid('coverage', { ...coverage, files: { 'example.ts': { sha256: 'a'.repeat(64), lines: { '01': 1 } } } });
  invalid('coverage', { ...coverage, files: { 'example.ts': { sha256: 'a'.repeat(64) + '\n', lines: {} } } });
  invalid('coverage', { ...coverage, files: { 'example.ts': { sha256: 'a'.repeat(64), lines: { '1\n': 1 } } } });
  invalid('baseline', { version: 1, threshold: -1, functions: [] });
  invalid('report', { ...full, functions: [{ ...full.functions[0], coverage: 1.1 }] });
  valid('report', { ...full, future_optional_output: true });
  for (const relative of ['a.ts', '日本語/ü.tsx', 'line\nbreak.ts', '..\n']) {
    valid('coverage', { version: 1, files: { [relative]: { sha256: 'a'.repeat(64), lines: {} } } });
  }
  for (const relative of ['', '/a.ts', 'a/', 'a//b.ts', '../a.ts', 'a/../b.ts', './a.ts', 'C:a.ts', 'a\\b.ts', 'a\n/../b.ts']) {
    invalid('coverage', { version: 1, files: { [relative]: { sha256: 'a'.repeat(64), lines: {} } } });
  }
  console.log('PASS schemas and executable v1 contracts: examples, reports, baselines, coverage, gate invariants, strict inputs and additive outputs');
} finally { await fs.rm(temporary, { recursive: true, force: true }); }
