import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const collector = fileURLToPath(new URL('../scripts/coverage.mjs', import.meta.url));
const sentinel = 'do not overwrite this file\n';

async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-publication-ü-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'source.mts'), 'export const value: number = 1;\n');
  return root;
}

function run(root, output, code = '', preload) {
  return spawnSync(process.execPath, [
    ...(preload ? ['--require', preload] : []), collector,
    '--root', root, '--output', output, '--', process.execPath, '--input-type=module', '-e', code,
  ], { cwd: root, encoding: 'utf8', timeout: 30_000 });
}

function succeeded(result) {
  assert.ifError(result.error);
  assert.equal(result.status, 0, result.stderr);
}

async function completeReport(output) {
  assert.equal((await fs.lstat(output)).isFile(), true);
  const report = JSON.parse(await fs.readFile(output, 'utf8'));
  assert.equal(report.version, 1);
  assert.match(report.files['source.mts'].sha256, /^[a-f0-9]{64}$/);
  assert.deepEqual(report.files['source.mts'].lines, {});
}

async function noStaging(root) {
  assert.deepEqual((await fs.readdir(root)).filter(name => name.startsWith('.ts-crap-coverage-')), []);
}

for (const kind of ['symlink', 'hardlink']) {
  test(`planted legacy staging ${kind} preserves victim and planted entry`, async t => {
    const root = await fixture(t);
    const output = path.join(root, 'coverage.json');
    const victim = path.join(root, 'victim.txt');
    await fs.writeFile(victim, sentinel);
    succeeded(run(root, output, `
      import * as fs from 'node:fs/promises';
      const staging = ${JSON.stringify(output)} + '.' + process.ppid + '.tmp';
      try {
        await fs.${kind === 'symlink' ? 'symlink' : 'link'}(${JSON.stringify(victim)}, staging);
        await fs.writeFile('planted.json', JSON.stringify({ staging }));
      } catch (error) {
        await fs.writeFile('planted.json', JSON.stringify({ error: error.code }));
      }
    `));
    const planted = JSON.parse(await fs.readFile(path.join(root, 'planted.json'), 'utf8'));
    if (planted.error) {
      assert.ok(['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(planted.error), planted.error);
      t.skip(`${kind} creation unsupported: ${planted.error}`);
      return;
    }
    assert.equal(await fs.readFile(victim, 'utf8'), sentinel);
    assert.equal(await fs.readFile(planted.staging, 'utf8'), sentinel);
    assert.equal((await fs.lstat(planted.staging)).isSymbolicLink(), kind === 'symlink');
    await completeReport(output);
    await noStaging(root);
  });
}

test('replaces an existing report and creates missing output parents', async t => {
  const root = await fixture(t);
  for (const output of [path.join(root, 'coverage.json'), path.join(root, 'new', 'nested', 'coverage.json')]) {
    if (path.dirname(output) === root) await fs.writeFile(output, sentinel);
    succeeded(run(root, output));
    await completeReport(output);
    await noStaging(path.dirname(output));
  }
});

test('replacing a hardlinked report preserves the other link', async t => {
  const root = await fixture(t);
  const victim = path.join(root, 'victim.ts');
  const output = path.join(root, 'coverage.json');
  await fs.writeFile(victim, sentinel);
  try {
    await fs.link(victim, output);
  } catch (error) {
    if (!['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(error.code)) throw error;
    t.skip(`hardlink creation unsupported: ${error.code}`);
    return;
  }
  succeeded(run(root, output));
  assert.equal(await fs.readFile(victim, 'utf8'), sentinel);
  await completeReport(output);
  await noStaging(root);
});

test('protects TypeScript paths regardless of existence, case, declarations or excluded directories', async t => {
  const root = await fixture(t);
  for (const name of ['missing.ts', 'missing.TSX', 'missing.MTS', 'missing.CTS', 'types.d.ts',
    'types.D.CTS', 'node_modules/ignored.ts', 'dist/ignored.TSX', '.hidden/ignored.mts']) {
    const output = path.join(root, name);
    await fs.mkdir(path.dirname(output), { recursive: true });
    for (const existing of [false, true]) {
      if (existing) await fs.writeFile(output, sentinel);
      const result = run(root, output, "throw Error('child must not run')");
      assert.equal(result.status, 1, result.stderr);
      assert.match(result.stderr, /cannot overwrite TypeScript source/);
      assert.doesNotMatch(result.stderr, /child must not run/);
      if (existing) assert.equal(await fs.readFile(output, 'utf8'), sentinel);
      else await assert.rejects(fs.lstat(output), { code: 'ENOENT' });
    }
  }
  await noStaging(root);
});

for (const dangling of [false, true]) {
  test(`rejects ${dangling ? 'dangling' : 'existing'} output symlinks`, async t => {
    const root = await fixture(t);
    const victim = path.join(root, 'victim.txt');
    const output = path.join(root, 'coverage.json');
    if (!dangling) await fs.writeFile(victim, sentinel);
    try {
      await fs.symlink(victim, output);
    } catch (error) {
      if (!['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(error.code)) throw error;
      t.skip(`symlink creation unsupported: ${error.code}`);
      return;
    }
    const result = run(root, output);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, /must be a regular file/);
    assert.equal((await fs.lstat(output)).isSymbolicLink(), true);
    if (!dangling) assert.equal(await fs.readFile(victim, 'utf8'), sentinel);
    else await assert.rejects(fs.lstat(victim), { code: 'ENOENT' });
    await noStaging(root);
  });
}

test('rejects directories before execution and revalidates output after execution', async t => {
  const root = await fixture(t);
  const output = path.join(root, 'coverage.json');
  await fs.mkdir(output);
  const before = run(root, output, "throw Error('child must not run')");
  assert.equal(before.status, 1, before.stderr);
  assert.match(before.stderr, /must be a regular file/);
  assert.doesNotMatch(before.stderr, /child must not run/);
  await fs.rmdir(output);
  const after = run(root, output, "import { mkdir } from 'node:fs/promises'; await mkdir('coverage.json');");
  assert.equal(after.status, 1, after.stderr);
  assert.match(after.stderr, /must be a regular file/);
  assert.equal((await fs.lstat(output)).isDirectory(), true);
  await noStaging(root);
});

test('failed child preserves existing report and creates no staging directory', async t => {
  const root = await fixture(t);
  const output = path.join(root, 'coverage.json');
  await fs.writeFile(output, sentinel);
  const result = run(root, output, 'process.exit(7)');
  assert.equal(result.status, 7, result.stderr);
  assert.equal(await fs.readFile(output, 'utf8'), sentinel);
  await noStaging(root);
});

// Patch built-in exports only in this collector subprocess. No production fault hooks
// or inherited child preloads are needed. Record owned directories to check cleanup.
for (const failure of ['open', 'write', 'sync', 'close', 'rename']) {
  test(`${failure} failure preserves existing output, closes handles and cleans owned directories`, async t => {
    const root = await fixture(t);
    const output = path.join(root, 'coverage.json');
    const preload = path.join(root, 'fault.cjs');
    await fs.writeFile(output, sentinel);
    await fs.writeFile(preload, `
      const fs = require('node:fs/promises');
      const sync = require('node:fs');
      const created = [];
      const originalTemp = fs.mkdtemp;
      fs.mkdtemp = async (...args) => {
        const directory = await originalTemp(...args);
        created.push(directory);
        sync.writeFileSync('owned.json', JSON.stringify(created));
        return directory;
      };
      const fail = () => { throw Error('injected ${failure} failure'); };
      const originalOpen = fs.open;
      fs.open = async (...args) => {
        ${failure === 'open' ? 'fail();' : ''}
        const handle = await originalOpen(...args);
        const originalWrite = handle.writeFile.bind(handle);
        handle.writeFile = async (...values) => {
          ${failure === 'write' ? "await originalWrite('partial report'); fail();" : 'return originalWrite(...values);'}
        };
        ${failure === 'sync' ? 'handle.sync = async () => fail();' : ''}
        const originalClose = handle.close.bind(handle);
        handle.close = async () => {
          await originalClose();
          sync.writeFileSync('closed.txt', 'closed');
          ${failure === 'close' ? 'fail();' : ''}
        };
        return handle;
      };
      ${failure === 'rename' ? 'fs.rename = async () => fail();' : ''}
      require('node:module').syncBuiltinESMExports();
    `);
    const result = run(root, output, '', preload);
    assert.equal(result.status, 1, result.stderr);
    assert.match(result.stderr, new RegExp('injected ' + failure + ' failure'));
    assert.equal(await fs.readFile(output, 'utf8'), sentinel);
    if (failure !== 'open') assert.equal(await fs.readFile(path.join(root, 'closed.txt'), 'utf8'), 'closed');
    const directories = JSON.parse(await fs.readFile(path.join(root, 'owned.json'), 'utf8'));
    assert.equal(directories.length, 2, 'V8 and publication staging directories recorded');
    for (const directory of directories) await assert.rejects(fs.lstat(directory), { code: 'ENOENT' });
    await noStaging(root);
  });
}

for (const kind of ['symlink', 'hardlink']) {
  test(`exclusive staging creation preserves an unowned ${kind} and its victim`, async t => {
    const root = await fixture(t);
    const output = path.join(root, 'coverage.json');
    const victim = path.join(root, 'victim.txt');
    const preload = path.join(root, 'collision.cjs');
    await fs.writeFile(output, sentinel);
    await fs.writeFile(victim, sentinel);
    await fs.writeFile(preload, `
      const fs = require('node:fs/promises');
      const sync = require('node:fs');
      const path = require('node:path');
      const originalTemp = fs.mkdtemp;
      fs.mkdtemp = async (...args) => {
        const directory = await originalTemp(...args);
        if (path.basename(directory).startsWith('.ts-crap-coverage-')) {
          const staging = path.join(directory, 'report.json');
          try {
            sync.${kind === 'symlink' ? 'symlinkSync' : 'linkSync'}(${JSON.stringify(victim)}, staging);
            sync.writeFileSync('collision.json', JSON.stringify({ directory, staging }));
          } catch (error) {
            sync.writeFileSync('collision.json', JSON.stringify({ error: error.code }));
          }
        }
        return directory;
      };
      require('node:module').syncBuiltinESMExports();
    `);
    const result = run(root, output, '', preload);
    const planted = JSON.parse(await fs.readFile(path.join(root, 'collision.json'), 'utf8'));
    if (planted.error) {
      assert.ok(['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(planted.error), planted.error);
      t.skip(`${kind} creation unsupported: ${planted.error}`);
      return;
    }
    assert.equal(result.status, 1, result.stderr);
    assert.equal(await fs.readFile(output, 'utf8'), sentinel);
    assert.equal(await fs.readFile(victim, 'utf8'), sentinel);
    assert.equal(await fs.readFile(planted.staging, 'utf8'), sentinel);
    assert.equal((await fs.lstat(planted.staging)).isSymbolicLink(), kind === 'symlink');
    assert.equal((await fs.lstat(planted.directory)).isDirectory(), true);
  });
}

test('revalidates destination after closing the staged report', async t => {
  const root = await fixture(t);
  const output = path.join(root, 'coverage.json');
  const preload = path.join(root, 'replace-after-write.cjs');
  await fs.writeFile(output, sentinel);
  await fs.writeFile(preload, `
    const fs = require('node:fs/promises');
    const originalOpen = fs.open;
    fs.open = async (...args) => {
      const handle = await originalOpen(...args);
      const originalClose = handle.close.bind(handle);
      handle.close = async () => {
        await originalClose();
        await fs.rename('coverage.json', 'previous.json');
        await fs.mkdir('coverage.json');
      };
      return handle;
    };
    require('node:module').syncBuiltinESMExports();
  `);
  const result = run(root, output, '', preload);
  assert.equal(result.status, 1, result.stderr);
  assert.match(result.stderr, /must be a regular file/);
  assert.equal(await fs.readFile(path.join(root, 'previous.json'), 'utf8'), sentinel);
  assert.equal((await fs.lstat(output)).isDirectory(), true);
  await noStaging(root);
});
