import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const packaging = fileURLToPath(new URL('../packaging/npm/', import.meta.url));
const launcher = require('../packaging/npm/lib/index.js');
const { LauncherError } = launcher;

test('every supported platform maps to its platform package; others name the fix', () => {
  assert.deepEqual(launcher.PLATFORMS, {
    'linux-x64': 'typescript-crap-linux-x64', 'linux-arm64': 'typescript-crap-linux-arm64',
    'darwin-x64': 'typescript-crap-darwin-x64', 'darwin-arm64': 'typescript-crap-darwin-arm64',
    'win32-x64': 'typescript-crap-win32-x64',
  });
  assert.equal(launcher.platformPackage('linux', 'arm64'), 'typescript-crap-linux-arm64');
  for (const [platform, arch] of [['win32', 'arm64'], ['freebsd', 'x64'], ['linux', 'ia32'], ['android', 'arm64']]) {
    assert.throws(() => launcher.platformPackage(platform, arch),
      error => error instanceof LauncherError && error.message.includes(`${platform}-${arch}`)
        && error.message.includes('linux-x64, linux-arm64, darwin-x64, darwin-arm64, win32-x64'));
  }
});

test('binaryPath resolves the platform executable and checks its version', () => {
  const resolve = request => {
    assert.equal(request, 'typescript-crap-darwin-arm64/package.json');
    return path.join('/m', 'node_modules', 'typescript-crap-darwin-arm64', 'package.json');
  };
  const options = { platform: 'darwin', arch: 'arm64', resolve, readVersion: () => '2.3.0', version: '2.3.0' };
  assert.equal(launcher.binaryPath(options), path.join('/m', 'node_modules', 'typescript-crap-darwin-arm64', 'bin', 'ts-crap'));
  const windows = launcher.binaryPath({ ...options, platform: 'win32', arch: 'x64',
    resolve: () => path.join('/w', 'typescript-crap-win32-x64', 'package.json') });
  assert.equal(windows, path.join('/w', 'typescript-crap-win32-x64', 'bin', 'ts-crap.exe'));
  assert.throws(() => launcher.binaryPath({ ...options, readVersion: () => '2.2.1' }),
    /typescript-crap-darwin-arm64 is version 2\.2\.1 but typescript-crap is 2\.3\.0; reinstall so both are 2\.3\.0/);
  assert.throws(() => launcher.binaryPath({ ...options, resolve: () => { throw new Error('MODULE_NOT_FOUND'); } }),
    error => error instanceof LauncherError && /typescript-crap-darwin-arm64 is not installed/.test(error.message)
      && error.message.includes('--omit=optional'));
});

test('coverage --scan uses the package scanner unless --scanner is given', () => {
  const scanner = () => '/pkg/bin/ts-crap';
  assert.deepEqual(launcher.coverageArguments(['--framework', 'vitest'], scanner), ['--framework', 'vitest']);
  assert.deepEqual(launcher.coverageArguments(['--scan', '--root', 'a'], scanner),
    ['--scan', '--root', 'a', '--scanner', '/pkg/bin/ts-crap']);
  assert.deepEqual(launcher.coverageArguments(['--scan', '--', '--scan', '--config', 'v.ts'], scanner),
    ['--scan', '--scanner', '/pkg/bin/ts-crap', '--', '--scan', '--config', 'v.ts']);
  // Framework arguments after -- are not ours, and an explicit scanner wins.
  assert.deepEqual(launcher.coverageArguments(['--', '--scan'], scanner), ['--', '--scan']);
  assert.deepEqual(launcher.coverageArguments(['--scan', '--scanner', 'x'], () => assert.fail('not resolved')),
    ['--scan', '--scanner', 'x']);
});

test('withoutPnp removes only the Yarn Plug\'n\'Play runtime from NODE_OPTIONS', () => {
  assert.equal(launcher.withoutPnp('--require /p/.pnp.cjs'), '');
  assert.equal(launcher.withoutPnp('--max-old-space-size=4096 --require /p/.pnp.cjs --experimental-loader file:///p/.pnp.loader.mjs'),
    '--max-old-space-size=4096');
  assert.equal(launcher.withoutPnp('-r "/a b/.pnp.cjs" --enable-source-maps'), '--enable-source-maps');
  assert.equal(launcher.withoutPnp('--require=/p/.pnp.js --import ./setup.mjs'), '--import ./setup.mjs');
  assert.equal(launcher.withoutPnp('--require ./setup.cjs'), '--require ./setup.cjs');
  assert.equal(launcher.withoutPnp(''), '');
  assert.equal(launcher.withoutPnp(undefined), undefined);
});

test('coverage refuses Node.js older than 24 before running anything', async t => {
  assert.equal(launcher.nodeMajor({ node: '22.22.2' }), 22);
  const messages = [];
  t.mock.method(console, 'error', message => messages.push(message));
  const previous = process.exitCode;
  t.after(() => { process.exitCode = previous; });
  await launcher.runCoverage(['--scan'], { versions: { node: '23.9.0' } });
  assert.equal(process.exitCode, 2);
  assert.match(messages.join('\n'), /coverage helpers need Node\.js 24 or later \(this is 23\.9\.0\)/);
});

test('run settles with the exit code or terminating signal; finish mirrors it', async t => {
  assert.deepEqual(await launcher.run(process.execPath, ['-e', 'process.exit(7)'], { stdio: 'ignore' }), { code: 7 });
  if (process.platform !== 'win32') {
    assert.deepEqual(await launcher.run(process.execPath, ['-e', "process.kill(process.pid, 'SIGTERM')"], { stdio: 'ignore' }),
      { signal: 'SIGTERM' });
  }
  await assert.rejects(launcher.run(path.join(os.tmpdir(), 'no-such-ts-crap-binary'), [], { stdio: 'ignore' }), { code: 'ENOENT' });
  const previous = process.exitCode;
  t.after(() => { process.exitCode = previous; });
  launcher.finish({ code: 3 });
  assert.equal(process.exitCode, 3);
  process.exitCode = previous;
  // Listeners are removed after the child exits, so nothing leaks between runs.
  for (const signal of launcher.FORWARDED) assert.equal(process.listenerCount(signal), 0);
});

/** An installed layout: the real launcher files, a stand-in scanner and stand-in helpers. */
async function installed(t, { platformVersion = '9.8.7' } = {}) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), 'typescript-crap launcher-')));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const main = path.join(root, 'node_modules', 'typescript-crap');
  for (const relative of ['bin/ts-crap.js', 'bin/typescript-crap.js', 'lib/index.js']) {
    await fs.mkdir(path.dirname(path.join(main, relative)), { recursive: true });
    await fs.copyFile(path.join(packaging, relative), path.join(main, relative));
  }
  await fs.writeFile(path.join(main, 'package.json'), JSON.stringify({ name: 'typescript-crap', version: '9.8.7' }));
  await fs.mkdir(path.join(main, 'coverage'));
  await fs.writeFile(path.join(main, 'coverage', 'coverage-run.mjs'),
    "console.log(JSON.stringify({ args: process.argv.slice(2), printed: process.env.TS_CRAP_SCANNER_COMMAND, options: process.env.NODE_OPTIONS ?? null }));\n");
  const name = launcher.PLATFORMS[`${process.platform}-${process.arch}`];
  const platform = path.join(root, 'node_modules', name);
  await fs.mkdir(path.join(platform, 'bin'), { recursive: true });
  await fs.writeFile(path.join(platform, 'package.json'), JSON.stringify({ name, version: platformVersion }));
  const scanner = path.join(platform, 'bin', 'ts-crap');
  await fs.writeFile(scanner, '#!/bin/sh\nprintf "%s|" "$@"\nexit "${TS_CRAP_FAKE_STATUS:-0}"\n', { mode: 0o755 });
  const pnp = path.join(root, '.pnp.cjs');
  await fs.writeFile(pnp, '');
  return { main, scanner, pnp };
}

const posixHost = process.platform !== 'win32' && launcher.PLATFORMS[`${process.platform}-${process.arch}`];

test('the bins pass arguments and exit statuses through the native scanner', { skip: !posixHost && 'needs a supported POSIX host' }, async t => {
  const { main } = await installed(t);
  for (const bin of ['ts-crap.js', 'typescript-crap.js']) {
    const result = spawnSync(process.execPath, [path.join(main, 'bin', bin), '--root', 'a b', '--format', 'json'],
      { encoding: 'utf8', env: { ...process.env, TS_CRAP_FAKE_STATUS: '1' } });
    assert.equal(result.stdout, '--root|a b|--format|json|');
    assert.equal(result.status, 1);
  }
  // `ts-crap coverage` is not special: only the typescript-crap bin has subcommands.
  assert.equal(spawnSync(process.execPath, [path.join(main, 'bin', 'ts-crap.js'), 'coverage'], { encoding: 'utf8' }).stdout, 'coverage|');
});

test('a scanner without permission bits stops with exit 2 and the fix', { skip: !posixHost && 'needs a supported POSIX host' }, async t => {
  const { main, scanner } = await installed(t);
  await fs.chmod(scanner, 0o644);
  const result = spawnSync(process.execPath, [path.join(main, 'bin', 'ts-crap.js'), '--version'], { encoding: 'utf8' });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /EACCES; the executable lost its permission bits, so reinstall typescript-crap on this machine/);
});

test('a mismatched platform package stops with exit 2 and the fix', { skip: !posixHost && 'needs a supported POSIX host' }, async t => {
  const { main } = await installed(t, { platformVersion: '1.0.0' });
  const result = spawnSync(process.execPath, [path.join(main, 'bin', 'ts-crap.js'), '--version'], { encoding: 'utf8' });
  assert.equal(result.status, 2);
  assert.match(result.stderr, /is version 1\.0\.0 but typescript-crap is 9\.8\.7/);
  assert.equal(result.stdout, '');
});

test('typescript-crap coverage runs the bundled helper with the package scanner', { skip: !posixHost && 'needs a supported POSIX host' }, async t => {
  const { main, scanner, pnp } = await installed(t);
  const bin = path.join(main, 'bin', 'typescript-crap.js');
  const run = (args, env = {}) => {
    const result = spawnSync(process.execPath, [bin, 'coverage', ...args], { encoding: 'utf8', env: { ...process.env, ...env } });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  // The launcher itself loads the PnP runtime, as under `yarn run`; its child must not.
  const inherited = process.env.NODE_OPTIONS ?? '';
  const scan = run(['--scan', '--framework', 'jest', '--', '--runInBand'],
    { NODE_OPTIONS: `${inherited} --require "${pnp}" --enable-source-maps` });
  assert.deepEqual(scan.args, ['--scan', '--framework', 'jest', '--scanner', scanner, '--', '--runInBand']);
  assert.deepEqual(JSON.parse(scan.printed), ['npx', 'typescript-crap@9.8.7']);
  assert.equal(scan.options, `${inherited} --enable-source-maps`.trim());
  const print = run(['--root', 'x'], { TS_CRAP_SCANNER_COMMAND: '["ts-crap"]' });
  assert.deepEqual(print.args, ['--root', 'x']);
  assert.equal(print.printed, '["ts-crap"]');
});
