#!/usr/bin/env node
/**
 * Raw Node TypeScript coverage (no transpiler/source-map remapping).
 * Usage: node scripts/coverage.mjs --root ROOT --output coverage.json --
 *          node --experimental-strip-types test.mjs
 * The child runs in ROOT. Requires a Node version supporting the supplied TS.
 * V8 offsets and JS string indices are UTF-16 code units, NOT UTF-8 bytes.
 * Every non-whitespace segment on a line must be covered to mark it covered.
 */
import { lineHits, mergeLineHits } from './coverage-lines.mjs';
import { snapshot } from './coverage-snapshot.mjs';
import { spawn } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const extensions = new Set(['.ts', '.tsx', '.mts', '.cts']);

async function validateOutput(output) {
  // Overwrite protection is independent of source selection and existence.
  if (extensions.has(path.extname(output).toLowerCase())) {
    throw new Error('Coverage output cannot overwrite TypeScript source');
  }
  const entry = await fs.lstat(output).catch(error => {
    if (error.code === 'ENOENT') return null;
    throw error;
  });
  if (entry && !entry.isFile()) {
    throw new Error('Coverage output must be a regular file, not a symlink or directory');
  }
}

async function publish(output, result) {
  await fs.mkdir(path.dirname(output), { recursive: true });
  // A random private sibling directory keeps staging on the destination filesystem.
  const directory = await fs.mkdtemp(path.join(path.dirname(output), '.ts-crap-coverage-'));
  const staging = path.join(directory, 'report.json');
  let ownsFile = false;
  try {
    const handle = await fs.open(staging, 'wx', 0o600);
    ownsFile = true;
    try {
      await handle.writeFile(JSON.stringify(result, null, 2) + '\n');
    } finally {
      await handle.close();
    }
    await validateOutput(output);
    await fs.rename(staging, output);
    ownsFile = false;
  } finally {
    if (ownsFile) await fs.rm(staging, { force: true });
    // Never recursively remove entries we did not create.
    await fs.rmdir(directory);
  }
}

function argumentsFor(argv) {
  const options = {};
  let i = 0;
  for (; i < argv.length && argv[i] !== '--'; i += 2) {
    if (!['--root', '--output'].includes(argv[i]) || !argv[i + 1] || argv[i + 1] === '--') {
      throw new Error('Usage: node scripts/coverage.mjs --root ROOT --output coverage.json -- node --experimental-strip-types test.mjs');
    }
    if (options[argv[i]]) throw new Error(`Duplicate option ${argv[i]}`);
    options[argv[i]] = argv[i + 1];
  }
  const command = argv.slice(i + 1);
  if (!options['--root'] || !options['--output'] || argv[i] !== '--' || !command.length) {
    throw new Error('Required: --root ROOT --output FILE -- COMMAND [ARGS...]');
  }
  return { root: options['--root'], output: path.resolve(options['--output']), command };
}

async function main() {
  const args = argumentsFor(process.argv.slice(2));
  const root = await fs.realpath(args.root);
  await validateOutput(args.output);
  // Native V8 collection includes declarations and retains pre-execution text;
  // the loader hook enforces load-time hashes. Never reread source after execution.
  const files = await snapshot(root, {
    supported: name => extensions.has(path.extname(name)),
    sourceText: true, lineCount: false, compareNames: (a, b) => a.localeCompare(b),
  });
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-v8-'));
  try {
    const manifest = path.join(temporary, 'sources.json');
    await fs.writeFile(manifest, JSON.stringify(Object.fromEntries(
      [...files].map(([absolute, file]) => [absolute, file.sha256]),
    )));
    const hook = pathToFileURL(path.join(path.dirname(fileURLToPath(import.meta.url)), 'coverage-hook.mjs')).href;
    const child = spawn(args.command[0], args.command.slice(1), {
      cwd: root, stdio: 'inherit', env: {
        ...process.env, NODE_V8_COVERAGE: temporary, TS_CRAP_SNAPSHOT: manifest,
        NODE_OPTIONS: `${process.env.NODE_OPTIONS ?? ''} --import=${hook}`,
      },
    });
    const { code, signal } = await new Promise((resolve, reject) => {
      child.once('error', reject);
      child.once('close', (code, signal) => resolve({ code, signal }));
    });
    if (code !== 0) {
      console.error(`Coverage command failed (${signal ?? code}); no report written`);
      process.exitCode = code ?? 1;
      return;
    }
    const reports = (await fs.readdir(temporary)).filter(name => name.startsWith('coverage-') && name.endsWith('.json'));
    if (!reports.length) throw new Error('Child produced no V8 coverage; run a Node command with coverage enabled');
    for (const name of reports) {
      const report = JSON.parse(await fs.readFile(path.join(temporary, name), 'utf8'));
      for (const script of report.result) {
        // ESM uses file: URLs; V8 records CommonJS scripts as absolute paths.
        const absolute = script.url.startsWith('file:') ? fileURLToPath(script.url)
          : path.isAbsolute(script.url) ? script.url : null;
        if (absolute === null) continue;
        const file = files.get(absolute);
        if (!file) continue; // Only pre-execution TS sources inside root.
        file.lines = mergeLineHits(file.lines, lineHits(script, file.source));
      }
    }
    const result = { version: 1, files: Object.fromEntries([...files].map(([absolute, file]) => [
      path.relative(root, absolute).split(path.sep).join('/'), { sha256: file.sha256, lines: file.lines },
    ])) };
    // Publish only after a successful child and complete conversion.
    await publish(args.output, result);
  } finally {
    await fs.rm(temporary, { recursive: true, force: true });
  }
}

main().catch(error => {
  console.error(`Coverage: ${error.message}`);
  process.exitCode = 1;
});
