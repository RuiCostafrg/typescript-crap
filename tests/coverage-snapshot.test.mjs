import assert from 'node:assert/strict';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { test } from 'node:test';
import { decodeSource, digest, snapshot } from '../scripts/coverage-snapshot.mjs';

test('snapshot policies preserve BOM/hash bytes, declaration selection and excluded directories', async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-snapshot-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  const source = '\uFEFFexport const value = "😀";\r\n';
  for (const name of ['a.mts', 'types.d.ts', 'types.d.cts', 'types.d.mts', 'view.tsx', 'skip.TS', '.ts', '.mts']) {
    await fs.writeFile(path.join(root, name), source);
  }
  for (const directory of ['.git', 'node_modules', 'target', 'dist', 'build', 'coverage']) {
    await fs.mkdir(path.join(root, directory));
    await fs.writeFile(path.join(root, directory, 'ignored.ts'), source);
  }
  const compareNames = (a, b) => a.localeCompare(b);
  const policies = [
    { supported: name => ['.ts', '.tsx', '.mts', '.cts'].includes(path.extname(name)), sourceText: true, lineCount: false, compareNames },
    { supported: name => /\.(?:tsx?|mts|cts)$/.test(name) && !/\.d\.(?:ts|mts|cts)$/.test(name), sourceText: false, lineCount: true, compareNames },
  ];
  const native = await snapshot(root, policies[0]);
  const imported = await snapshot(root, policies[1]);
  assert.equal(native.size, 5);
  assert.equal(imported.size, 4);
  assert.equal(native.has(path.join(root, '.ts')), false);
  assert.equal(imported.has(path.join(root, '.ts')), true);
  for (const [name, file] of native) {
    assert.equal(file.source, source);
    assert.equal(file.sha256, digest(Buffer.from(source)));
    assert.equal('lineCount' in file, false);
    assert.deepEqual(file.lines, {});
    if (imported.has(name)) {
      assert.equal(imported.get(name).sha256, file.sha256);
      assert.equal('source' in imported.get(name), false);
      assert.equal(imported.get(name).lineCount, 2);
    }
  }
  assert.equal(decodeSource(Buffer.from(source)), source);
  assert.throws(() => decodeSource(Buffer.from([0xff])), TypeError);
  await fs.writeFile(path.join(root, 'invalid.ts'), Buffer.from([0xff]));
  for (const policy of policies) {
    await assert.rejects(snapshot(root, policy), TypeError);
  }
});

test('snapshot rejects schema-incompatible Unix filenames', { skip: process.platform === 'win32' }, async t => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-paths-'));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const name of ['src\\a.ts', 'a:b.ts']) {
    await fs.writeFile(path.join(root, name), 'const f = () => 1;');
    await assert.rejects(snapshot(root, { supported: () => true, compareNames: () => 0 }), /cannot be represented/);
    await fs.unlink(path.join(root, name));
  }
});
