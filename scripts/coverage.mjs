#!/usr/bin/env node
/**
 * Raw Node TypeScript coverage (no transpiler/source-map remapping).
 * Usage: node scripts/coverage.mjs --root ROOT --output coverage.json --
 *          node --experimental-strip-types test.mjs
 * The child runs in ROOT. Requires a Node version supporting the supplied TS.
 * V8 offsets and JS string indices are UTF-16 code units, NOT UTF-8 bytes.
 * Every non-whitespace segment on a line must be covered to mark it covered.
 */
import { createHash } from 'node:crypto';
import { spawn } from 'node:child_process';
import * as fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const extensions = new Set(['.ts', '.tsx', '.mts', '.cts']);
const excluded = new Set(['node_modules', '.git', 'target', 'dist', 'build', 'coverage']);

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

async function snapshot(root) {
  const files = new Map();
  async function walk(directory) {
    const entries = await fs.readdir(directory, { withFileTypes: true });
    entries.sort((a, b) => a.name.localeCompare(b.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory() && !excluded.has(entry.name)) await walk(absolute);
      else if (entry.isFile() && extensions.has(path.extname(entry.name))) {
        const bytes = await fs.readFile(absolute);
        // Fatal decoding avoids hashing bytes different from the text being mapped.
        // ignoreBOM preserves the BOM as one UTF-16 code unit in V8 offsets.
        const source = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
        files.set(absolute, { source, sha256: createHash('sha256').update(bytes).digest('hex'), lines: {} });
      }
    }
  }
  await walk(root);
  return files;
}

function lineHits(script, source) {
  // Sweep range boundaries. The smallest enclosing range determines each segment;
  // ties favor zero, so uncovered branches on an otherwise covered line stay zero.
  const events = new Map();
  function event(offset) {
    if (!events.has(offset)) events.set(offset, { add: [], remove: [] });
    return events.get(offset);
  }
  for (const fn of script.functions) {
    for (const range of fn.ranges) {
      const { startOffset: start, endOffset: end, count } = range;
      if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end < start
          || !Number.isSafeInteger(count) || count < 0) {
        throw new Error(`Invalid V8 range for ${script.url}`);
      }
      // Node's type stripper preserves source offsets but can append a sourceURL
      // directive. Ignore that synthetic suffix, which is not an owned source line.
      const clippedEnd = Math.min(end, source.length);
      if (start >= clippedEnd) continue;
      const item = { start, end: clippedEnd, count };
      event(start).add.push(item);
      event(clippedEnd).remove.push(item);
    }
  }
  const starts = [0];
  for (let i = 0; i < source.length; i++) if (source[i] === '\n') starts.push(i + 1);
  const active = new Set();
  const hits = {};
  const offsets = [...events.keys()].sort((a, b) => a - b);
  let line = 0;
  for (let i = 0; i < offsets.length - 1; i++) {
    const start = offsets[i];
    for (const item of events.get(start).remove) active.delete(item);
    for (const item of events.get(start).add) active.add(item);
    let inner;
    for (const item of active) {
      if (!inner || item.end - item.start < inner.end - inner.start
          || (item.end - item.start === inner.end - inner.start && item.count < inner.count)) inner = item;
    }
    if (!inner) continue;
    while (line + 1 < starts.length && starts[line + 1] <= start) line++;
    for (let cursor = start; cursor < offsets[i + 1];) {
      const end = Math.min(offsets[i + 1], starts[line + 1] ?? source.length);
      if (source.slice(cursor, end).trim()) {
        const key = String(line + 1);
        hits[key] = Math.min(hits[key] ?? Infinity, inner.count);
      }
      cursor = end;
      if (cursor < offsets[i + 1]) line++;
    }
  }
  return hits;
}

async function main() {
  const args = argumentsFor(process.argv.slice(2));
  const root = await fs.realpath(args.root);
  const files = await snapshot(root); // Never read source again after execution.
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), 'ts-crap-v8-'));
  try {
    const outputSource = await fs.realpath(args.output).catch(error => {
      if (error.code === 'ENOENT') return args.output;
      throw error;
    });
    if (files.has(outputSource)) throw new Error('Coverage output cannot overwrite TypeScript source');
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
        for (const [line, count] of Object.entries(lineHits(script, file.source))) {
          const sum = (file.lines[line] ?? 0) + count;
          if (!Number.isSafeInteger(sum)) throw new Error('Coverage hit count overflow');
          file.lines[line] = sum;
        }
      }
    }
    const result = { version: 1, files: Object.fromEntries([...files].map(([absolute, file]) => [
      path.relative(root, absolute).split(path.sep).join('/'), { sha256: file.sha256, lines: file.lines },
    ])) };
    await fs.mkdir(path.dirname(args.output), { recursive: true });
    // Publish only after a successful child and complete conversion.
    const staging = `${args.output}.${process.pid}.tmp`;
    try {
      await fs.writeFile(staging, JSON.stringify(result, null, 2) + '\n');
      await fs.rename(staging, args.output);
    } finally {
      await fs.rm(staging, { force: true });
    }
  } finally {
    await fs.rm(temporary, { recursive: true, force: true });
  }
}

main().catch(error => {
  console.error(`Coverage: ${error.message}`);
  process.exitCode = 1;
});
