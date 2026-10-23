#!/usr/bin/env node
// Collect Istanbul (Jest/Vitest/nyc) or LCOV with a pre-execution source snapshot.
import { digest, snapshot } from './coverage-snapshot.mjs';
import { spawn } from 'node:child_process';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const supported = name => /\.(?:tsx?|mts|cts)$/.test(name) && !/\.d\.(?:ts|mts|cts)$/.test(name);
function integer(value, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum) throw new Error(`Invalid coverage integer: ${value}`);
  return value;
}
function decode(segment) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const values = [];
  let value = 0, shift = 0;
  for (const char of segment) {
    const digit = alphabet.indexOf(char);
    if (digit < 0 || shift > 30) throw new Error('Invalid source-map VLQ');
    value += (digit & 31) * 2 ** shift;
    if (digit & 32) shift += 5;
    else { values.push(value % 2 ? -Math.floor(value / 2) : value / 2); value = 0; shift = 0; }
  }
  if (shift) throw new Error('Truncated source-map VLQ');
  return values;
}
function mappings(map) {
  if (map.version !== 3 || !Array.isArray(map.sources) || typeof map.mappings !== 'string' || map.sections) {
    throw new Error('Expected a flat source map v3');
  }
  let source = 0, originalLine = 0, originalColumn = 0;
  return map.mappings.split(';').map(line => {
    let column = 0;
    return line.split(',').filter(Boolean).map(segment => {
      const fields = decode(segment);
      if (![1, 4, 5].includes(fields.length)) throw new Error('Invalid source-map segment');
      column = integer(column + fields[0]);
      if (fields.length === 1) return { column };
      source = integer(source + fields[1]);
      originalLine = integer(originalLine + fields[2]);
      originalColumn = integer(originalColumn + fields[3]);
      if (source >= map.sources.length) throw new Error('Source-map source index out of bounds');
      return { column, source, line: originalLine + 1 };
    });
  });
}
function resolveSource(base, name) {
  if (name.startsWith('file:')) return fileURLToPath(name);
  if (/^[a-zA-Z][a-zA-Z+.-]*:\/\//.test(name)) throw new Error(`Unmappable source URL: ${name}`);
  return path.resolve(base, name);
}
async function sourceMap(filename, inline) {
  if (inline) return { map: inline, base: path.dirname(filename) };
  const text = await fs.readFile(filename, 'utf8');
  const matches = [...text.matchAll(/[#@]\s*sourceMappingURL=([^\s]+)/g)];
  if (!matches.length) throw new Error(`No source map for generated input: ${filename}`);
  const url = matches.at(-1)[1];
  if (/^data:application\/json(?:;charset=[^;,]+)?;base64,/i.test(url)) {
    return { map: JSON.parse(Buffer.from(url.split(',')[1], 'base64').toString('utf8')), base: path.dirname(filename) };
  }
  const mapPath = resolveSource(path.dirname(filename), decodeURIComponent(url));
  return { map: JSON.parse(await fs.readFile(mapPath, 'utf8')), base: path.dirname(mapPath) };
}
async function convert(root, files, records) {
  let unmapped = 0;
  for (const record of records) {
    const filename = resolveSource(root, record.path);
    let mapper;
    if (files.has(filename) && !record.map) {
      mapper = (line) => [{ file: files.get(filename), line }];
    } else {
      const { map, base } = await sourceMap(filename, record.map);
      const rows = mappings(map);
      const sources = map.sources.map((name, index) => {
        const absolute = resolveSource(resolveSource(base, map.sourceRoot || '.'), name);
        const file = files.get(absolute);
        if (!file) throw new Error(`Unmappable source outside collected TypeScript snapshot: ${name}`);
        if (typeof map.sourcesContent?.[index] !== 'string' || digest(Buffer.from(map.sourcesContent[index])) !== file.sha256) {
          throw new Error(`Missing or stale source-map content: ${name}`);
        }
        return file;
      });
      mapper = (line, column) => {
        const segments = rows[line - 1] || [];
        if (column !== undefined) {
          const segment = segments.findLast(item => item.column <= column);
          return segment?.source === undefined ? [] : [{ file: sources[segment.source], line: segment.line }];
        }
        return segments.filter(item => item.source !== undefined).map(item => ({ file: sources[item.source], line: item.line }));
      };
    }
    for (const hit of record.hits) {
      integer(hit.line, 1); integer(hit.count);
      if (hit.column !== undefined) integer(hit.column);
      const targets = mapper(hit.line, hit.column);
      if (!targets.length) { unmapped++; continue; }
      for (const { file, line } of targets) {
        if (line > file.lineCount) throw new Error('Mapped line outside source');
        // Conservative overlap: an uncovered statement/segment keeps the line uncovered.
        file.lines[line] = Math.min(file.lines[line] ?? hit.count, hit.count);
      }
    }
  }
  if (unmapped) throw new Error(`${unmapped} coverage locations could not be source-mapped; configure the test tool to emit remapped TypeScript coverage`);
}
function istanbul(raw) {
  return Object.entries(raw).map(([key, file]) => ({ path: file.path || key, map: file.inputSourceMap,
    hits: Object.entries(file.statementMap).map(([id, location]) => ({ line: location.start.line, column: location.start.column, count: integer(file.s[id]) })) }));
}
function lcov(raw) {
  const records = [];
  let record;
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith('SF:')) {
      if (record) throw new Error('LCOV record missing end_of_record');
      record = { path: line.slice(3), hits: [] };
    } else if (line.startsWith('DA:')) {
      if (!record) throw new Error('LCOV DA outside record');
      const [number, count] = line.slice(3).split(',');
      record.hits.push({ line: integer(Number(number), 1), count: integer(Number(count)) });
    } else if (line === 'end_of_record') {
      if (record) records.push(record);
      record = undefined;
    }
  }
  if (record) throw new Error('LCOV record missing end_of_record');
  return records;
}
async function main() {
  const argv = process.argv.slice(2), separator = argv.indexOf('--');
  const options = {};
  if (separator < 0 || separator % 2) throw new Error('Expected --root ROOT --format istanbul|lcov --input RAW --output NATIVE -- COMMAND');
  for (let i = 0; i < separator; i += 2) {
    if (!['--root', '--format', '--input', '--output'].includes(argv[i]) || options[argv[i]]) throw new Error('Invalid or duplicate import option');
    options[argv[i]] = argv[i + 1];
  }
  if (!options['--root'] || !options['--input'] || !options['--output'] || !['istanbul', 'lcov'].includes(options['--format']) || !argv[separator + 1]) throw new Error('Missing required import options or command');
  const root = await fs.realpath(options['--root']);
  const input = path.resolve(options['--input']), output = path.resolve(options['--output']);
  if (input === output || supported(input) || supported(output)) throw new Error('Coverage inputs/outputs must differ and cannot be TypeScript');
  // Import selection excludes declarations; post-command hashes below enforce
  // unchanged source. Fresh input/output destinations remain importer-only policy.
  const files = await snapshot(root, {
    supported, sourceText: false, lineCount: true, compareNames: (a, b) => a < b ? -1 : a > b ? 1 : 0,
  });
  // Do not accept a pre-existing report as fresh evidence or overwrite its aliases.
  for (const destination of [input, output]) {
    try { await fs.lstat(destination); throw new Error(`Coverage destination already exists: ${destination}; use a fresh path`); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  const status = await new Promise((resolve, reject) => {
    const child = spawn(argv[separator + 1], argv.slice(separator + 2), { cwd: root, stdio: 'inherit' });
    child.on('error', reject);
    child.on('exit', (code, signal) => signal ? reject(new Error(`Coverage command terminated: ${signal}`)) : resolve(code));
  });
  if (status !== 0) throw new Error(`Coverage command failed: ${status}`);
  const raw = await fs.readFile(input, 'utf8');
  const records = options['--format'] === 'istanbul' ? istanbul(JSON.parse(raw)) : lcov(raw);
  if (!records.some(record => record.hits.length)) throw new Error('Empty imported coverage');
  await convert(root, files, records);
  const result = { version: 1, files: {} };
  for (const [absolute, file] of files) {
    if (digest(await fs.readFile(absolute)) !== file.sha256) throw new Error(`Source changed during coverage collection: ${absolute}`);
    // Missing records remain missing evidence, never invented zero-hit coverage.
    if (Object.keys(file.lines).length) result.files[path.relative(root, absolute).split(path.sep).join('/')] = { sha256: file.sha256, lines: file.lines };
  }
  await fs.writeFile(output, `${JSON.stringify(result, null, 2)}\n`, { flag: 'wx' });
}
main().catch(error => { console.error(JSON.stringify(error.message)); process.exitCode = 2; });
