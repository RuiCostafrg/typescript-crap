#!/usr/bin/env node
// Networked integration suite; deliberately separate from offline cargo tests.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repository = fileURLToPath(new URL('../', import.meta.url));
const scanner = path.resolve(process.argv[2] ?? 'target/debug/ts-crap');
await fs.access(scanner);
const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap frameworks '));
const root = path.join(temporary, 'workspace');

function run(command, args, expected = 0, env = {}) {
  const result = spawnSync(command, args, {
    cwd: root,
    env: { ...process.env, FIXTURE_PARTIAL: '', FIXTURE_FAIL: '', ...env },
    encoding: 'utf8',
    timeout: 120_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  assert.ifError(result.error);
  assert.equal(result.status, expected,
    `${command} ${args.join(' ')}\n${result.stdout}\n${result.stderr}`);
  return result;
}

try {
  await fs.cp(path.join(repository, 'tests/fixtures/frameworks'), root, {
    recursive: true,
    filter: source => !['node_modules', 'coverage'].includes(path.basename(source)),
  });
  // Invoke npm's JS entry point so Windows does not require a shell for npm.cmd.
  const npm = process.env.npm_execpath;
  if (npm) run(process.execPath, [npm, 'ci', '--ignore-scripts', '--no-audit', '--no-fund']);
  else if (process.platform === 'win32') {
    run(process.env.ComSpec ?? 'cmd.exe', ['/d', '/s', '/c', 'npm ci --ignore-scripts --no-audit --no-fund']);
  } else run('npm', ['ci', '--ignore-scripts', '--no-audit', '--no-fund']);

  // Use real reporters first, then change source within the importer's child.
  const mutate = path.join(temporary, 'mutate-after-tests.mjs');
  await fs.writeFile(mutate, `
    import { spawnSync } from 'node:child_process';
    import { appendFileSync } from 'node:fs';
    const result = spawnSync(process.execPath, process.argv.slice(2), { stdio: 'inherit' });
    if (result.error) throw result.error;
    if (result.status === 0) appendFileSync(process.env.FIXTURE_MUTATE_SOURCE, '// changed during collection\\n');
    process.exitCode = result.status ?? 2;
  `);

  for (const [framework, provider] of [['jest', 'babel'], ['vitest', 'istanbul'], ['vitest', 'v8']]) {
    const mutationSource = framework === 'jest'
      ? 'packages/core/src/price.ts' : 'packages/ui/src/panels/Résumé.tsx';
    const selected = ['packages/core/src', `packages/${framework === 'jest' ? 'service' : 'ui'}/src`];
    const expectedPaths = (framework === 'jest'
      ? ['packages/core/src/price.ts', 'packages/service/src/shipping.ts']
      : ['packages/core/src/price.ts', 'packages/ui/src/Badge.tsx',
        'packages/ui/src/panels/OrderPanel.tsx', 'packages/ui/src/panels/AsyncPanel.tsx',
        'packages/ui/src/panels/Résumé.tsx']).sort();
    for (const format of ['istanbul', 'lcov']) {
      const label = `${framework}/${provider}/${format}`;
      const collect = async (name, partial = false, mutation = false, expected = 0, env = {}) => {
        // Every execution owns a new directory: neither raw nor native is reused.
        const directory = await fs.mkdtemp(path.join(root, 'coverage', `${framework}-${name}-`));
        const input = path.join(directory, format === 'istanbul' ? 'coverage-final.json' : 'lcov.info');
        const output = path.join(directory, 'native.json');
        const reporter = format === 'istanbul' ? 'json' : 'lcovonly';
        const command = framework === 'jest'
          ? [path.join(root, 'node_modules/jest/bin/jest.js'), '--config', 'jest.config.cjs', '--runInBand',
            '--coverage', `--coverageReporters=${reporter}`, `--coverageDirectory=${directory}`]
          : [path.join(root, 'node_modules/vitest/vitest.mjs'), 'run', '--config', 'vitest.config.mjs',
            '--coverage', `--coverage.reporter=${reporter}`, `--coverage.reportsDirectory=${directory}`];
        const args = [path.join(repository, 'scripts/import-coverage.mjs'), '--root', root,
          '--format', format, '--input', input, '--output', output, '--', process.execPath,
          ...(mutation ? [mutate] : []), ...command];
        const result = run(process.execPath, args, expected, {
          FIXTURE_PARTIAL: typeof partial === 'string' ? partial : partial ? '1' : '',
          FIXTURE_COVERAGE_PROVIDER: provider,
          FIXTURE_MUTATE_SOURCE: mutationSource,
          ...env,
        });
        return { input, output, args, result };
      };
      const scan = (coverage, expected = 0) => run(scanner, ['--root', root, ...selected,
        '--coverage', coverage, '--require-coverage', '--threshold', '4', '--format', 'json'], expected);
      const source = path.join(root, mutationSource);
      const original = await fs.readFile(source);
      await fs.mkdir(path.join(root, 'coverage'), { recursive: true });

      const full = await collect('full');
      const fullReport = JSON.parse(scan(full.output).stdout);
      assert.deepEqual([...new Set(fullReport.functions.map(f => f.path))].sort(), expectedPaths, label);
      assert.equal(fullReport.files_scanned, expectedPaths.length, label);
      assert.equal(fullReport.files_missing_coverage, 0, label);
      assert.equal(fullReport.functions_missing_coverage, 0, label);
      for (const f of fullReport.functions) {
        assert.equal(f.coverage, 1, `${label}: ${f.name}`);
        assert.equal(f.crap, f.complexity, `${label}: ${f.name}`);
      }
      const price = fullReport.functions.find(f => f.name === 'price');
      assert.equal(price.complexity, 4, label);
      assert.equal(price.crap, 4, label);

      const partial = await collect('partial', true);
      const partialReport = JSON.parse(scan(partial.output, 1).stdout);
      assert.equal(partialReport.files_missing_coverage, 0, label);
      assert.equal(partialReport.functions_missing_coverage, 0, label);
      const uncalled = partialReport.functions.find(f => f.name === (framework === 'jest' ? 'shipping' : 'Badge'));
      assert.equal(uncalled.coverage, 0, label);
      assert.equal(uncalled.crap, uncalled.complexity ** 2 + uncalled.complexity, label);
      assert(partialReport.functions.find(f => f.name === 'price').crap > 4, label);
      const native = JSON.parse(await fs.readFile(partial.output, 'utf8'));
      assert(Object.values(native.files).some(file => Object.values(file.lines).includes(0)), label);

      if (framework === 'vitest') {
        // Reporters may omit default-argument declaration lines from statement
        // evidence. Executing the default is not enough to invent a covered line.
        const gapReport = run(scanner, ['--root', root, 'packages/ui/coverage-gaps',
          '--coverage', full.output, '--require-coverage', '--threshold', '4', '--format', 'json'], 1);
        assert.equal(JSON.parse(gapReport.stdout).functions_missing_coverage, 1, label);
        const complex = fullReport.functions.filter(f => f.path.includes('/panels/'));
        assert(complex.length >= 9, `${label}: nested callbacks must be scored separately`);
        for (const name of ['quantityLabel', 'ItemList', 'OrderPanel', 'loadMessage', 'AsyncPanel', 'Résumé']) {
          assert(complex.some(f => f.name === name), `${label}: missing ${name}`);
        }
        for (const f of partialReport.functions.filter(f => f.path.includes('/panels/'))) {
          assert.equal(f.coverage, 0, `${label}: unexecuted ${f.name}`);
        }

        const branches = await collect('branches', 'branches');
        const branchReport = JSON.parse(scan(branches.output, 1).stdout);
        assert.equal(branchReport.files_missing_coverage, 0, label);
        assert.equal(branchReport.functions_missing_coverage, 0, label);
        const load = branchReport.functions.find(f => f.name === 'loadMessage');
        assert(load.coverage > 0 && load.coverage < 1, `${label}: async failure paths must remain uncovered`);
        assert(load.crap > load.complexity, label);
        const orderFunctions = branchReport.functions.filter(f => f.path.endsWith('/OrderPanel.tsx'));
        assert.equal(orderFunctions.find(f => f.name === 'OrderPanel').coverage, 1, label);
        assert(orderFunctions.some(f => f.coverage === 0), `${label}: SSR must not cover the event handler`);
        assert.equal(branchReport.functions.find(f => f.name === 'Résumé').coverage, 0,
          `${label}: lazy module not imported by partial tests must retain zero-hit evidence`);
        const branchNative = JSON.parse(await fs.readFile(branches.output, 'utf8'));
        const orderSource = await fs.readFile(path.join(root, 'packages/ui/src/panels/OrderPanel.tsx'), 'utf8');
        const callbackLine = orderSource.split('\n').findIndex(line => line.includes('onSelect(order.id)')) + 1;
        assert.equal(branchNative.files['packages/ui/src/panels/OrderPanel.tsx'].lines[callbackLine], 0, label);
        const fullNative = JSON.parse(await fs.readFile(full.output, 'utf8'));
        assert(fullNative.files['packages/ui/src/panels/OrderPanel.tsx'].lines[callbackLine] > 0, label);

        const failed = await collect('failing-tests', false, false, 2, { FIXTURE_FAIL: '1' });
        assert.match(failed.result.stderr, /actual test failure/);
        await assert.rejects(fs.access(failed.output), { code: 'ENOENT' });
      }

      const before = await fs.readFile(full.output);
      const reused = run(process.execPath, full.args, 2);
      assert.match(reused.stderr, /already exists.*fresh path/s);
      assert.deepEqual(await fs.readFile(full.output), before);
      const repeated = await collect('repeated');
      assert.deepEqual(JSON.parse(scan(repeated.output).stdout), fullReport, label);
      assert.deepEqual(await fs.readFile(repeated.output), before, label);

      await fs.appendFile(source, '// stale after collection\n');
      assert.match(scan(full.output, 2).stderr, /SHA256 mismatch/);
      await fs.writeFile(source, original);
      const changed = await collect('mutated', false, true, 2);
      assert.match(changed.result.stderr, /Source changed during coverage collection/);
      await assert.rejects(fs.access(changed.output), { code: 'ENOENT' });
      await fs.writeFile(source, original);
      console.log(`PASS ${label}: full, uncovered, fresh repeat, reuse rejected, stale and changed source rejected`);
    }
  }
} finally {
  await fs.rm(temporary, { recursive: true, force: true });
}
