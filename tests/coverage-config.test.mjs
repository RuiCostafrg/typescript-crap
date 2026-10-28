import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const scripts = fileURLToPath(new URL('../scripts/', import.meta.url));
const policy = '{"version":1,"threshold":0}\n';

/** Create an isolated project with source accepted by both collectors. */
async function fixture(t) {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-config-ü-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  await fs.writeFile(path.join(root, 'a.ts'), 'function f(){\nreturn 1;\n}\n');
  return root;
}

/** Run the actual helper and a child that records execution and emits valid LCOV. */
function collect(root, helper, output, input = path.join(root, 'raw.info'), config, extra = '', cwd = root) {
  const argv = [path.join(scripts, helper === 'native' ? 'coverage.mjs' : 'import-coverage.mjs'),
    '--root', root, '--output', output];
  if (helper === 'import') argv.push('--format', 'lcov', '--input', input);
  if (config !== undefined) argv.push('--config', config);
  argv.push('--', process.execPath, '--input-type=module', '-e', `
    import * as fs from 'node:fs/promises';
    await fs.writeFile('child-ran', 'yes');
    ${helper === 'import' ? `await fs.writeFile(${JSON.stringify(input)}, 'SF:a.ts\\nDA:2,1\\nend_of_record\\n');` : ''}
    ${extra}
  `);
  return spawnSync(process.execPath, argv, { cwd, encoding: 'utf8', timeout: 30_000 });
}

/** Assert rejection happens before the trusted test command executes. */
async function rejectedBeforeChild(root, result) {
  assert.ifError(result.error);
  assert.notEqual(result.status, 0, result.stderr);
  assert.match(result.stderr, /cannot overwrite project configuration/);
  await assert.rejects(fs.lstat(path.join(root, 'child-ran')), { code: 'ENOENT' });
}

for (const helper of ['native', 'import']) {
  for (const existing of [false, true]) {
    test(`${helper} reserves ${existing ? 'existing' : 'absent'} default and explicit configs`, async t => {
      const root = await fixture(t);
      const config = path.join(root, 'ts-crap.json');
      const custom = path.join(root, 'custom-policy.json');
      if (existing) { await fs.writeFile(config, policy); await fs.writeFile(custom, policy); }
      for (const destination of [config, path.join(root, 'TS-CRAP.JSON'), path.join(root, 'unused', '..', 'ts-crap.json'), custom]) {
        // Existing-parent resolution also handles lexically equivalent input.
        await fs.mkdir(path.join(root, 'unused'), { recursive: true });
        const option = destination === custom ? 'custom-policy.json' : undefined;
        await rejectedBeforeChild(root, collect(root, helper, destination, undefined, option));
        if (helper === 'import') {
          await rejectedBeforeChild(root, collect(root, helper, path.join(root, 'native.json'), destination, option));
        }
      }
      // An explicit override never unreserves the default config.
      await rejectedBeforeChild(root, collect(root, helper, config, undefined, 'custom-policy.json'));
      await rejectedBeforeChild(root, collect(root, helper, custom, undefined,
        path.relative(path.dirname(root), custom), '', path.dirname(root)));
      for (const protectedPath of [config, custom]) {
        if (existing) assert.equal(await fs.readFile(protectedPath, 'utf8'), policy);
        else await assert.rejects(fs.lstat(protectedPath), { code: 'ENOENT' });
      }
    });
  }

  test(`${helper} allows ordinary coverage destinations with a declared config`, async t => {
    const root = await fixture(t);
    await fs.writeFile(path.join(root, 'custom-policy.json'), policy);
    const output = path.join(root, 'native.json');
    const result = collect(root, helper, output, undefined, 'custom-policy.json');
    assert.ifError(result.error);
    assert.equal(result.status, 0, result.stderr);
    assert.equal(JSON.parse(await fs.readFile(output, 'utf8')).version, 1);
    assert.equal(await fs.readFile(path.join(root, 'custom-policy.json'), 'utf8'), policy);
  });

  test(`${helper} protects absent config paths through directory aliases and rechecks after child`, async t => {
    const root = await fixture(t);
    const alias = path.join(root, 'alias');
    try { await fs.symlink(root, alias, 'junction'); }
    catch (error) {
      if (!['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(error.code)) throw error;
      t.skip(`directory links unsupported: ${error.code}`); return;
    }
    try { await fs.realpath(alias); }
    catch (error) {
      if (process.platform !== 'android' || error.code !== 'ELOOP') throw error;
      t.skip('Android/proot cannot resolve directory links; exercised on hosted platforms'); return;
    }
    await rejectedBeforeChild(root, collect(root, helper, path.join(alias, 'ts-crap.json')));
    if (helper === 'import') {
      await rejectedBeforeChild(root, collect(root, helper, path.join(root, 'native.json'), path.join(alias, 'ts-crap.json')));
    }
    await fs.writeFile(path.join(root, 'ts-crap.json'), policy);
    const changed = path.join(root, 'changed');
    await fs.mkdir(changed);
    const result = collect(root, helper, path.join(changed, 'ts-crap.json'), undefined, undefined, `
      await fs.rmdir(${JSON.stringify(changed)});
      await fs.symlink(${JSON.stringify(root)}, ${JSON.stringify(changed)}, 'junction');
    `);
    assert.notEqual(result.status, 0, result.stderr);
    assert.match(result.stderr, /cannot overwrite project configuration/);
    assert.equal(await fs.readFile(path.join(root, 'ts-crap.json'), 'utf8'), policy);
  });

  test(`${helper} protects the resolved target of a valid config symlink`, async t => {
    const root = await fixture(t);
    const target = path.join(root, 'policy.json'), config = path.join(root, 'ts-crap.json');
    await fs.writeFile(target, policy);
    try { await fs.symlink(target, config, 'file'); }
    catch (error) {
      if (!['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(error.code)) throw error;
      t.skip(`file links unsupported: ${error.code}`); return;
    }
    try { await fs.realpath(config); }
    catch (error) {
      if (process.platform !== 'android' || error.code !== 'ELOOP') throw error;
      t.skip('Android/proot cannot resolve file links; exercised on hosted platforms'); return;
    }
    await rejectedBeforeChild(root, collect(root, helper, target));
    assert.equal(await fs.readFile(target, 'utf8'), policy);
  });
}

test('native replacement of a hardlinked report preserves the config inode', async t => {
  const root = await fixture(t);
  const config = path.join(root, 'ts-crap.json'), output = path.join(root, 'native.json');
  await fs.writeFile(config, policy);
  try { await fs.link(config, output); }
  catch (error) {
    if (!['EPERM', 'EACCES', 'ENOSYS', 'ENOTSUP'].includes(error.code)) throw error;
    t.skip(`hardlinks unsupported: ${error.code}`); return;
  }
  const result = collect(root, 'native', output);
  assert.equal(result.status, 0, result.stderr);
  assert.equal(await fs.readFile(config, 'utf8'), policy);
  assert.equal(JSON.parse(await fs.readFile(output, 'utf8')).version, 1);
});
