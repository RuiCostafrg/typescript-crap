import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import {
  checkExtraArguments, checkProvider, detectFramework, findPackage, formatCommand, frameworkArguments, frameworkBinary,
  helperVersion, parseArguments, reportName, scannerArguments, shellQuote, UsageError, vitestProvider,
} from '../scripts/coverage-run.mjs';

const helper = fileURLToPath(new URL('../scripts/coverage-run.mjs', import.meta.url));
const repository = fileURLToPath(new URL('../', import.meta.url));
// cargo test passes the scanner built from this checkout; standalone runs skip scans.
const scanner = process.env.TS_CRAP_SCANNER;

async function project(t, files = {}) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap run ü-')));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const [name, contents] of Object.entries(files)) {
    await fs.mkdir(path.dirname(path.join(root, name)), { recursive: true });
    await fs.writeFile(path.join(root, name), contents);
  }
  return root;
}

// A fake framework writes a real reporter file to the requested directory, so the
// whole pipeline runs offline. It records its arguments for assertions.
const FAKE = `
import * as fs from 'node:fs';
import path from 'node:path';
const args = process.argv.slice(2);
fs.writeFileSync('framework-args.json', JSON.stringify(args));
if (process.env.FAKE_EXIT) process.exit(Number(process.env.FAKE_EXIT));
const value = prefix => args.find(arg => arg.startsWith(prefix))?.slice(prefix.length);
const directory = value('--coverage.reportsDirectory=') ?? value('--coverageDirectory=');
const reporter = value('--coverage.reporter=') ?? value('--coverageReporters=');
const count = Number(process.env.FAKE_COUNT ?? 1);
function report() {
  fs.mkdirSync(directory, { recursive: true });
  if (reporter === 'json') {
    fs.writeFileSync(path.join(directory, 'coverage-final.json'), JSON.stringify({ 'src/a.ts': {
      path: 'src/a.ts', statementMap: { 0: { start: { line: 2, column: 2 } } }, s: { 0: count } } }));
  } else fs.writeFileSync(path.join(directory, 'lcov.info'), 'SF:src/a.ts\\nDA:2,' + count + '\\nend_of_record\\n');
}
if (process.env.FAKE_SLEEP) {
  // FAKE_SIGNAL_EXIT=0 models a framework that writes a report during cleanup and succeeds.
  const exit = Number(process.env.FAKE_SIGNAL_EXIT ?? 1);
  for (const signal of ['SIGINT', 'SIGTERM', 'SIGHUP']) {
    process.on(signal, () => {
      fs.writeFileSync('framework-signal', signal);
      if (exit === 0) report();
      process.exit(exit);
    });
  }
  // Ready only after the handlers exist, so a signal can never arrive unhandled.
  fs.writeFileSync('framework-pid', String(process.pid));
  await new Promise(resolve => setTimeout(resolve, Number(process.env.FAKE_SLEEP)));
}
if (!process.env.FAKE_SKIP_REPORT) report();
`;

async function withFramework(t, framework, files = {}) {
  const bin = framework === 'vitest' ? 'vitest.mjs' : 'bin/jest.js';
  return project(t, {
    'src/a.ts': 'export function f() {\n  return 1;\n}\n',
    [`node_modules/${framework}/package.json`]: JSON.stringify({ name: framework, version: '9.9.9', bin: { [framework]: `./${bin}` } }),
    [`node_modules/${framework}/${bin}`]: FAKE,
    ...(framework === 'vitest' ? { 'node_modules/@vitest/coverage-istanbul/package.json': '{}' } : {}),
    ...files,
  });
}

function runHelper(root, args, env = {}, cwd = root) {
  return spawnSync(process.execPath, [helper, '--root', root, ...args], {
    cwd, encoding: 'utf8', env: { ...process.env, ...env }, timeout: 60_000,
  });
}

test('arguments default to auto JSON in the current directory and keep extra framework args', () => {
  assert.deepEqual(parseArguments([]), { framework: 'auto', format: 'json', root: '.', scan: false, extra: [] });
  assert.deepEqual(parseArguments(['--framework', 'jest', '--provider', 'v8', '--format', 'lcov', '--scan',
    '--scanner', 'x', '--', '--runInBand', '--scan']), {
    framework: 'jest', provider: 'v8', format: 'lcov', root: '.', scan: true, scanner: 'x', extra: ['--runInBand', '--scan'],
  });
  for (const argv of [['--framework', 'mocha'], ['--provider', 'nyc'], ['--format', 'istanbul'], ['--root'],
    ['--root', ''], ['--bogus'], ['--scan', '--scan'], ['--root', 'a', '--root', 'b'], ['--scanner', 'x'], ['--root', '--scan']]) {
    assert.throws(() => parseArguments(argv), UsageError, JSON.stringify(argv));
  }
});

test('framework detection reads filenames and package.json without evaluating config', async t => {
  const vitest = await project(t, { 'vitest.config.ts': 'throw new Error("never evaluated")' });
  assert.equal(detectFramework(vitest), 'vitest');
  for (const name of ['vite.config.ts', 'vite.config.mjs']) {
    assert.equal(detectFramework(await project(t, { [name]: 'throw 1' })), 'vitest', `Vite-only project: ${name}`);
  }
  const viteJest = await project(t, { 'vite.config.js': '', 'jest.config.ts': '' });
  assert.throws(() => detectFramework(viteJest), /Both Vitest and Jest.*vite\.config\.js.*jest\.config\.ts/);
  for (const name of ['jest.config.cjs', 'jest.config.json']) {
    assert.equal(detectFramework(await project(t, { [name]: 'throw 1' })), 'jest');
  }
  assert.equal(detectFramework(await project(t, { 'package.json': '{"jest":{}}' })), 'jest');
  const neither = await project(t, { 'package.json': '{"name":"x"}' });
  assert.throws(() => detectFramework(neither), /No Vitest or Jest/);
  const both = await project(t, { 'vitest.config.mjs': '', 'package.json': '{"jest":{}}' });
  assert.throws(() => detectFramework(both), /Both Vitest and Jest.*vitest\.config\.mjs.*"jest" key/);
  const duplicate = await project(t, { 'package.json': '{"jest":1,"jest":2}' });
  assert.throws(() => detectFramework(duplicate), /Cannot read .*package\.json/);
});

test('reporter and directory flags match each framework, provider and format', () => {
  assert.deepEqual(frameworkArguments('vitest', { format: 'json', directory: '/d', extra: ['-t', 'x'] }),
    ['run', '--coverage', '--coverage.reporter=json', '--coverage.reportsDirectory=/d', '-t', 'x']);
  assert.deepEqual(frameworkArguments('vitest', { provider: 'v8', format: 'lcov', directory: '/d' }),
    ['run', '--coverage', '--coverage.reporter=lcovonly', '--coverage.reportsDirectory=/d', '--coverage.provider=v8']);
  assert.deepEqual(frameworkArguments('jest', { provider: 'babel', format: 'json', directory: '/d' }),
    ['--coverage', '--coverageReporters=json', '--coverageDirectory=/d', '--coverageProvider=babel']);
  assert.deepEqual(frameworkArguments('jest', { provider: 'v8', format: 'lcov', directory: '/d', extra: ['--ci'] }),
    ['--coverage', '--coverageReporters=lcovonly', '--coverageDirectory=/d', '--coverageProvider=v8', '--ci']);
  assert.equal(reportName('json'), 'coverage-final.json');
  assert.throws(() => frameworkArguments('jest', { format: 'json', directory: '/d', extra: ['--coverageDirectory=/x'] }), UsageError);
});

test('providers use each framework\'s own names and other combinations are rejected', () => {
  for (const provider of ['istanbul', 'v8', undefined]) checkProvider('vitest', provider);
  for (const provider of ['babel', 'v8', undefined]) checkProvider('jest', provider);
  assert.throws(() => checkProvider('vitest', 'babel'), /--provider babel is not a Vitest coverage provider; use istanbul or v8/);
  assert.throws(() => checkProvider('jest', 'istanbul'), /--provider istanbul is not a Jest coverage provider; use babel or v8/);
  assert.throws(() => frameworkArguments('jest', { provider: 'istanbul', format: 'json', directory: '/d' }), UsageError);
});

test('extra arguments cannot redirect, replace or disable the private reporter', () => {
  for (const argument of ['--coverage.reporter=text', '--coverage.reportsDirectory', '/x', '--coverage.reports-directory=/x',
    '--coverage.provider=v8', '--coverage=false', '--no-coverage', '--coverage.enabled=false']) {
    if (!argument.startsWith('-')) continue;
    assert.throws(() => checkExtraArguments('vitest', [argument]), /would change the coverage reporter/, argument);
  }
  for (const argument of ['--coverageReporters=text', '--coverage-directory=/x', '--coverageProvider=v8',
    '--collectCoverage=false', '--no-coverage', '--coverage=false', '--CoverageDirectory', '-coverageDirectory=x']) {
    assert.throws(() => checkExtraArguments('jest', [argument]), /would change the coverage reporter/, argument);
  }
  // Values, test filters and unrelated coverage options remain allowed.
  checkExtraArguments('vitest', ['--config', 'vitest.ci.ts', '--coverage.include=src/**', 'coverage', '-t', 'name']);
  checkExtraArguments('jest', ['--runInBand', '--collectCoverageFrom=src/**', '--coverageThreshold={}', 'coverageDirectory']);
});

test('the Vitest provider is always one that is installed', async t => {
  const none = await project(t);
  assert.throws(() => vitestProvider(none, undefined, '5.0.0'), /needs @vitest\/coverage-v8 or @vitest\/coverage-istanbul.*@vitest\/coverage-v8@5\.0\.0/);
  assert.throws(() => vitestProvider(none, 'istanbul'), /--provider istanbul needs @vitest\/coverage-istanbul; install it with npm install --save-dev @vitest\/coverage-istanbul$/);
  const v8 = await project(t, { 'node_modules/@vitest/coverage-v8/package.json': '{}' });
  assert.equal(vitestProvider(v8, undefined), 'v8', 'the only installed provider is selected explicitly');
  assert.equal(vitestProvider(v8, 'v8'), 'v8');
  assert.throws(() => vitestProvider(v8, 'istanbul'), /--provider istanbul needs/);
  const both = await project(t, { 'node_modules/@vitest/coverage-v8/package.json': '{}', 'node_modules/@vitest/coverage-istanbul/package.json': '{}' });
  assert.equal(vitestProvider(both, undefined), undefined, 'with both installed, the configured provider is kept');
  assert.equal(vitestProvider(both, 'istanbul'), 'istanbul');
  assert.equal(reportName('lcov'), 'lcov.info');
});

test('installed frameworks resolve from hoisted ancestors and report clear install hints', async t => {
  const root = await withFramework(t, 'jest', { 'packages/app/package.json': '{}' });
  const nested = path.join(root, 'packages/app');
  assert.equal(frameworkBinary(nested, 'jest').path, path.join(root, 'node_modules/jest/bin/jest.js'));
  assert.equal(frameworkBinary(nested, 'jest').version, '9.9.9');
  assert.equal(findPackage(nested, 'missing-package-for-test'), null);
  assert.throws(() => frameworkBinary(nested, 'vitest'), /Cannot find vitest.*npm install --save-dev vitest/);
  const binless = await project(t, { 'node_modules/jest/package.json': '{"name":"jest"}' });
  assert.throws(() => frameworkBinary(binless, 'jest'), /declares no jest executable/);
  const stringBin = await project(t, { 'node_modules/vitest/package.json': '{"bin":"./cli.mjs"}' });
  assert.equal(frameworkBinary(stringBin, 'vitest').path, path.join(stringBin, 'node_modules/vitest/cli.mjs'));
});

test('scanner command and quoting are copy-paste safe', () => {
  assert.equal(shellQuote('/a/b-c_d.json'), '/a/b-c_d.json');
  assert.equal(shellQuote("/tmp/it's here", 'linux'), `'/tmp/it'\\''s here'`);
  assert.equal(shellQuote('C:\\Users\\a b\\x.json', 'win32'), "'C:\\Users\\a b\\x.json'");
  assert.equal(shellQuote("C:\\it's", 'win32'), "'C:\\it''s'");
  assert.equal(shellQuote('C:\\tmp\\x.json', 'win32'), 'C:\\tmp\\x.json');
  assert.equal(formatCommand(['ts-crap', '--root', 'C:\\Users\\Jane Doe\\app'], 'win32'), "ts-crap --root 'C:\\Users\\Jane Doe\\app'");
  assert.equal(formatCommand(['ts-crap', '--root', '/home/Jane Doe'], 'linux'), "ts-crap --root '/home/Jane Doe'");
  assert.equal(shellQuote('/tmp/a\\b', 'linux'), "'/tmp/a\\b'", 'POSIX shells consume unquoted backslashes');
  assert.equal(shellQuote('C:\\%TEMP%\\x', 'win32'), "'C:\\%TEMP%\\x'");
  assert.equal(shellQuote('C:\\$(x)\\$y', 'win32'), "'C:\\$(x)\\$y'", 'PowerShell keeps single-quoted text literal');
  assert.deepEqual(scannerArguments({ root: '/r', output: '/o.json' }), ['--root', '/r', '--coverage', '/o.json', '--require-coverage']);
  assert.deepEqual(scannerArguments({ root: '/r', output: '/o', config: '/c.json' }).slice(-2), ['--config', '/c.json']);
});

test('helper version comes from bundled metadata, else the checkout manifest', async t => {
  const cargo = await fs.readFile(path.join(repository, 'Cargo.toml'), 'utf8');
  assert.equal(helperVersion(), /^version = "([^"]+)"/m.exec(cargo)[1]);
  const bundle = await project(t, { 'helpers/version.json': '{"version":"0.0.1","scannerVersion":"1.2.3"}' });
  assert.equal(helperVersion(path.join(bundle, 'helpers')), '1.2.3');
  const nothing = await project(t, { 'helpers/x': '' });
  assert.equal(helperVersion(path.join(nothing, 'helpers')), null);
  const unversioned = await project(t, { 'Cargo.toml': '[package]\nname = "x"\n[dependencies]\nversion = "9"\n', 'helpers/x': '' });
  assert.equal(helperVersion(path.join(unversioned, 'helpers')), null);
  const arrays = await project(t, { 'Cargo.toml': '[workspace]\nversion = "0"\n[package]\nauthors = ["a"]\nversion = "4.5.6"\n', 'helpers/x': '' });
  assert.equal(helperVersion(path.join(arrays, 'helpers')), '4.5.6');
});

for (const [framework, configName] of [['vitest', 'vitest.config.mts'], ['jest', 'jest.config.js']]) {
  for (const format of ['json', 'lcov']) {
    test(`${framework} ${format}: auto-detected one-step run publishes native coverage and prints the scan`, async t => {
      const root = await withFramework(t, framework, { [configName]: '' });
      const result = runHelper(root, ['--format', format, '--', '--extra-flag']);
      assert.equal(result.status, 0, result.stderr);
      const command = result.stdout.trim().split('\n').at(-1);
      assert.match(command, /^ts-crap --root .+ --coverage .+native-coverage\.json --require-coverage$/);
      const output = /native coverage written to (.+)$/m.exec(result.stderr)[1];
      assert.equal(command, formatCommand(['ts-crap', ...scannerArguments({ root, output })]));
      const native = JSON.parse(await fs.readFile(output, 'utf8'));
      assert.deepEqual(native.files['src/a.ts'].lines, { 2: 1 });
      const args = JSON.parse(await fs.readFile(path.join(root, 'framework-args.json'), 'utf8'));
      assert.equal(args.at(-1), '--extra-flag');
      assert(args.includes(format === 'json' ? (framework === 'vitest' ? '--coverage.reporter=json' : '--coverageReporters=json')
        : (framework === 'vitest' ? '--coverage.reporter=lcovonly' : '--coverageReporters=lcovonly')));
      const provider = framework === 'vitest' ? 'istanbul' : 'configured provider';
      assert.match(result.stderr, new RegExp(`${framework} 9\\.9\\.9 \\(${provider}, ${format}\\)`));
      assert.equal(args.includes('--coverage.provider=istanbul'), framework === 'vitest', 'the only installed Vitest provider is explicit');
      await fs.rm(path.dirname(output), { recursive: true });
    });
  }
}

test('every run uses a new private directory and an explicit output must be fresh', async t => {
  const root = await withFramework(t, 'vitest');
  const output = path.join(root, 'out', 'native.json');
  await fs.mkdir(path.dirname(output));
  const first = runHelper(root, ['--framework', 'vitest', '--output', output]);
  assert.equal(first.status, 0, first.stderr);
  const directory = JSON.parse(await fs.readFile(path.join(root, 'framework-args.json'), 'utf8'))
    .find(arg => arg.startsWith('--coverage.reportsDirectory=')).split('=')[1];
  await assert.rejects(fs.access(directory), { code: 'ENOENT' }, 'run directory is removed when --output is given');
  const before = await fs.readFile(output);
  const reused = runHelper(root, ['--framework', 'vitest', '--output', output], { FAKE_COUNT: '0' });
  assert.equal(reused.status, 2);
  assert.match(reused.stderr, /already exists.*fresh path/s);
  assert.deepEqual(await fs.readFile(output), before);
  const next = runHelper(root, ['--framework', 'vitest']);
  assert.equal(next.status, 0, next.stderr);
  const second = JSON.parse(await fs.readFile(path.join(root, 'framework-args.json'), 'utf8'))
    .find(arg => arg.startsWith('--coverage.reportsDirectory=')).split('=')[1];
  await fs.rm(second, { recursive: true });
  assert.notEqual(second, directory);
});

test('project configuration is protected and failures keep exit codes without publishing', async t => {
  const root = await withFramework(t, 'jest');
  const policy = '{"version":1}\n';
  await fs.writeFile(path.join(root, 'ts-crap.json'), policy);
  const config = runHelper(root, ['--framework', 'jest', '--output', path.join(root, 'ts-crap.json')]);
  assert.equal(config.status, 2);
  assert.match(config.stderr, /cannot overwrite project configuration/);
  assert.equal(await fs.readFile(path.join(root, 'ts-crap.json'), 'utf8'), policy);
  const custom = path.join(root, 'policy.json');
  const alternate = runHelper(root, ['--framework', 'jest', '--config', custom, '--output', custom]);
  assert.equal(alternate.status, 2);
  assert.match(alternate.stderr, /cannot overwrite project configuration/);

  const failed = runHelper(root, ['--framework', 'jest'], { FAKE_EXIT: '1' });
  assert.equal(failed.status, 2);
  assert.match(failed.stderr, /no coverage published; reporter output \(if any\) is in /);
  assert.equal(failed.stdout, '');
});

test('missing frameworks, providers, roots and ambiguous projects fail before tests run', async t => {
  const empty = await project(t, { 'vitest.config.ts': '' });
  const missing = runHelper(empty, []);
  assert.equal(missing.status, 2);
  assert.match(missing.stderr, /Cannot find vitest installed/);
  const vitest = await withFramework(t, 'vitest');
  const provider = runHelper(vitest, ['--framework', 'vitest', '--provider', 'v8']);
  assert.equal(provider.status, 2);
  assert.match(provider.stderr, /--provider v8 needs @vitest\/coverage-v8.*@vitest\/coverage-v8@9\.9\.9/);
  await assert.rejects(fs.access(path.join(vitest, 'framework-args.json')), { code: 'ENOENT' });
  await fs.mkdir(path.join(vitest, 'node_modules/@vitest/coverage-v8'), { recursive: true });
  await fs.writeFile(path.join(vitest, 'node_modules/@vitest/coverage-v8/package.json'), '{}');
  const installed = runHelper(vitest, ['--framework', 'vitest', '--provider', 'v8']);
  assert.equal(installed.status, 0, installed.stderr);
  const noProvider = await withFramework(t, 'vitest');
  await fs.rm(path.join(noProvider, 'node_modules/@vitest'), { recursive: true });
  const prompt = runHelper(noProvider, ['--framework', 'vitest']);
  assert.equal(prompt.status, 2);
  assert.match(prompt.stderr, /Vitest coverage needs @vitest\/coverage-v8 or @vitest\/coverage-istanbul/);
  await assert.rejects(fs.access(path.join(noProvider, 'framework-args.json')), { code: 'ENOENT' });
  const both = await project(t, { 'vitest.config.ts': '', 'jest.config.js': '' });
  assert.match(runHelper(both, []).stderr, /pass --framework vitest or --framework jest/);
  const root = runHelper(path.join(empty, 'absent'), [], {}, empty);
  assert.equal(root.status, 2);
  assert.match(root.stderr, /Cannot resolve --root/);
  const usage = runHelper(empty, ['--format', 'xml']);
  assert.equal(usage.status, 2);
  assert.match(usage.stderr, /Invalid --format "xml"; expected json\|lcov/);
});

test('--scan checks the exact scanner version before running tests', async t => {
  const root = await withFramework(t, 'jest');
  const absent = runHelper(root, ['--framework', 'jest', '--scan', '--scanner', path.join(root, 'no-scanner')]);
  assert.equal(absent.status, 2);
  assert.match(absent.stderr, /Scanner not found: .*pass --scanner PATH/);
  const fake = path.join(root, 'old-scanner.mjs');
  await fs.writeFile(fake, '#!/usr/bin/env node\nconsole.log("ts-crap 0.0.1");\n', { mode: 0o755 });
  if (process.platform !== 'win32') {
    const mismatch = runHelper(root, ['--framework', 'jest', '--scan', '--scanner', fake]);
    assert.equal(mismatch.status, 2);
    assert.match(mismatch.stderr, /version mismatch: helpers are .* reports "ts-crap 0\.0\.1"/);
  }
  await assert.rejects(fs.access(path.join(root, 'framework-args.json')), { code: 'ENOENT' });
});

test('--scan runs the matching scanner and returns its gate exit code', { skip: !scanner && 'TS_CRAP_SCANNER not set' }, async t => {
  const root = await withFramework(t, 'vitest');
  const pass = runHelper(root, ['--framework', 'vitest', '--scan', '--scanner', scanner]);
  assert.equal(pass.status, 0, pass.stderr);
  assert.match(pass.stdout, /src\/a\.ts/);
  const uncovered = runHelper(root, ['--framework', 'vitest', '--scan', '--scanner', scanner], { FAKE_COUNT: '0' });
  assert.equal(uncovered.status, 0, 'complexity 1 with zero coverage stays under the default threshold');
  await fs.writeFile(path.join(root, 'ts-crap.json'), '{"version":1,"threshold":1}\n');
  const gate = runHelper(root, ['--framework', 'vitest', '--scan', '--scanner', scanner], { FAKE_COUNT: '0' });
  assert.equal(gate.status, 1, gate.stderr);
  await fs.rm(path.join(root, 'ts-crap.json'));
  await fs.writeFile(path.join(root, 'src/b.ts'), 'export function g() {\n  return 2;\n}\n');
  const incomplete = runHelper(root, ['--framework', 'vitest', '--scan', '--scanner', scanner]);
  assert.equal(incomplete.status, 1, 'files without coverage records fail --require-coverage');
});

test('an explicit output parent is created before tests run', async t => {
  const root = await withFramework(t, 'jest');
  const output = path.join(root, 'new', 'nested', 'native.json');
  const result = runHelper(root, ['--framework', 'jest', '--output', output]);
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual(JSON.parse(await fs.readFile(output, 'utf8')).files['src/a.ts'].lines, { 2: 1 });
});

test('extra arguments that would redirect the reporter fail before tests run', async t => {
  const root = await withFramework(t, 'vitest');
  const result = runHelper(root, ['--framework', 'vitest', '--', '--coverage.reportsDirectory=coverage']);
  assert.equal(result.status, 2);
  assert.match(result.stderr, /would change the coverage reporter, directory or provider/);
  await assert.rejects(fs.access(path.join(root, 'framework-args.json')), { code: 'ENOENT' });
});

for (const [signal, cleanupExit] of [['SIGINT', '1'], ['SIGTERM', '1'], ['SIGHUP', '1'], ['SIGINT', '0'], ['SIGTERM', '0']]) {
  test(`${signal} reaches the framework, which stops, and nothing is published (cleanup exit ${cleanupExit})`, { skip: process.platform === 'win32' && 'POSIX signals' }, async t => {
    const root = await withFramework(t, 'jest');
    const output = path.join(root, 'native.json');
    const child = spawn(process.execPath, [helper, '--root', root, '--framework', 'jest', '--output', output], {
      cwd: root, env: { ...process.env, FAKE_SLEEP: '5000', FAKE_SIGNAL_EXIT: cleanupExit }, stdio: 'ignore',
    });
    const exited = new Promise(resolve => child.on('exit', code => resolve(code)));
    // Wait until the framework is running inside the importer, then cancel the wrapper only.
    for (let i = 0; i < 400; i++) {
      try { await fs.access(path.join(root, 'framework-pid')); break; }
      catch { await new Promise(resolve => setTimeout(resolve, 25)); }
    }
    const pid = Number(await fs.readFile(path.join(root, 'framework-pid'), 'utf8'));
    child.kill(signal);
    assert.equal(await exited, 2);
    assert.equal(await fs.readFile(path.join(root, 'framework-signal'), 'utf8'), signal);
    assert.throws(() => process.kill(pid, 0), { code: 'ESRCH' }, 'the framework process has stopped');
    await assert.rejects(fs.access(output), { code: 'ENOENT' });
  });
}
