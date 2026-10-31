#!/usr/bin/env node
// Collect Istanbul (Jest/Vitest/nyc) or LCOV with a pre-execution source snapshot.
import { parseUniqueJson } from './strict-json.mjs';
import { protectConfigDestinations, publishReport } from './coverage-publish.mjs';
import { digest, snapshot } from './coverage-snapshot.mjs';
import { spawn } from 'node:child_process';
import * as fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const REMAP_HINT = 'Use a coverage setup that remaps to TypeScript (Vitest istanbul or v8 provider; Jest babel or v8 provider with source maps), and do not import raw NODE_V8_COVERAGE output';
// Tests, mocks, fixtures and tool configuration are rarely meant to be measured.
const notMeasured = relative =>
  /(^|\/)(__tests__|__mocks__|__fixtures__|tests?|mocks|fixtures)\//.test(relative)
  || /\.(test|spec|mocks?|fixtures?)\.[cm]?tsx?$|(^|\/)[^/]+\.config\.[cm]?tsx?$/.test(relative);
// Paths come from the checkout; never let a filename emit terminal controls.
const display = text => text.replace(/[\u0000-\u001f\u007f-\u009f]/g, char => `\\u${char.charCodeAt(0).toString(16).padStart(4, '0')}`);
/** Explain why a source named by coverage is absent from the collected snapshot. */
function snapshotHint(root, absolute) {
  const relative = path.relative(root, absolute);
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    return `it is outside --root ${root}. Pass the project root that contains your sources`;
  }
  return 'it is under --root but in an excluded directory (node_modules, .git, target, dist, build, coverage) or is not a .ts/.tsx/.mts/.cts source. Limit coverage to TypeScript sources outside those directories';
}
const reporterHint = format => format === 'istanbul'
  ? 'Vitest --coverage.reporter=json or Jest --coverageReporters=json'
  : 'Vitest --coverage.reporter=lcovonly or Jest --coverageReporters=lcovonly';
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
  if (!matches.length) {
    throw new Error(`No source map for generated input: ${filename}; the report covers generated JavaScript instead of TypeScript. ${REMAP_HINT}`);
  }
  const url = matches.at(-1)[1];
  if (/^data:application\/json(?:;charset=[^;,]+)?;base64,/i.test(url)) {
    return { map: parseUniqueJson(Buffer.from(url.split(',')[1], 'base64').toString('utf8')), base: path.dirname(filename) };
  }
  const mapPath = resolveSource(path.dirname(filename), decodeURIComponent(url));
  return { map: parseUniqueJson(await fs.readFile(mapPath, 'utf8')), base: path.dirname(mapPath) };
}
async function convert(root, files, records) {
  let unmapped = 0;
  const tests = new Map();
  for (const record of records) {
    const testName = record.testName ?? '';
    if (!tests.has(testName)) tests.set(testName, new Map());
    const test = tests.get(testName);
    const filename = resolveSource(root, record.path);
    let mapper;
    if (files.has(filename) && !record.map) {
      mapper = (line) => [{ file: files.get(filename), line }];
    } else if (!record.map && supported(filename)) {
      // TypeScript itself needs no source map; it is simply not in the snapshot.
      throw new Error(`Unmappable source outside collected TypeScript snapshot: ${record.path}; ${snapshotHint(root, filename)}`);
    } else {
      const { map, base } = await sourceMap(filename, record.map);
      const rows = mappings(map);
      const sources = map.sources.map((name, index) => {
        const absolute = resolveSource(resolveSource(base, map.sourceRoot || '.'), name);
        const file = files.get(absolute);
        if (!file) {
          throw new Error(`Unmappable source outside collected TypeScript snapshot: ${name}; ${snapshotHint(root, absolute)}`);
        }
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
        if (!test.has(file)) test.set(file, new Map());
        const lines = test.get(file);
        // Within one test, an uncovered statement/segment keeps the line uncovered.
        lines.set(line, Math.min(lines.get(line) ?? hit.count, hit.count));
      }
    }
  }
  if (unmapped) throw new Error(`${unmapped} coverage locations could not be source-mapped; configure the test tool to emit remapped TypeScript coverage. ${REMAP_HINT}`);
  // Independent tests add only after their conservative source-map reduction.
  // Istanbul is already an aggregate and remains a single conservative group.
  for (const test of tests.values()) {
    for (const [file, lines] of test) {
      for (const [line, count] of lines) file.lines[line] = integer((file.lines[line] ?? 0) + count);
    }
  }
}
function istanbul(raw) {
  return Object.entries(raw).map(([key, file]) => {
    const hits = Object.entries(file.statementMap).map(([id, location]) => ({
      line: location.start.line, column: location.start.column, count: integer(file.s[id]),
    }));
    // Reporters record default-argument execution in branch metadata rather than
    // statementMap. These are direct initializer hits, not inferred function hits.
    for (const [id, branch] of Object.entries(file.branchMap ?? {})) {
      if (branch.type !== 'default-arg') continue;
      const counts = file.b?.[id];
      if (!Array.isArray(branch.locations) || !branch.locations.length
          || !Array.isArray(counts) || counts.length !== branch.locations.length) {
        throw new Error('Invalid default-argument coverage');
      }
      for (let index = 0; index < counts.length; index++) {
        const location = branch.locations[index];
        hits.push({ line: location.start.line, column: location.start.column, count: integer(counts[index]) });
      }
    }
    return { path: file.path || key, map: file.inputSourceMap, hits };
  });
}
function lcov(raw, root) {
  const tests = new Map();
  let record, testName = '';
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith('TN:')) {
      if (record) throw new Error('LCOV test name inside source record');
      testName = line.slice(3);
    } else if (line.startsWith('SF:')) {
      if (record) throw new Error('LCOV record missing end_of_record');
      record = { path: resolveSource(root, line.slice(3)), testName, hits: new Map() };
    } else if (line.startsWith('DA:')) {
      if (!record) throw new Error('LCOV DA outside record');
      const [number, count] = line.slice(3).split(',');
      const location = integer(Number(number), 1);
      if (record.hits.has(location)) throw new Error('Duplicate LCOV line within source record');
      record.hits.set(location, integer(Number(count)));
    } else if (line === 'end_of_record') {
      if (record) {
        if (!tests.has(testName)) tests.set(testName, new Map());
        const files = tests.get(testName);
        const previous = files.get(record.path);
        if (previous) {
          // Repeated captures of the same test/source add counters before mapping.
          for (const [line, count] of record.hits) previous.hits.set(line, integer((previous.hits.get(line) ?? 0) + count));
        } else files.set(record.path, record);
      }
      record = undefined;
      testName = '';
    }
  }
  if (record) throw new Error('LCOV record missing end_of_record');
  return [...tests.values()].flatMap(files => [...files.values()].map(record => ({
    ...record, hits: [...record.hits].map(([line, count]) => ({ line, count })),
  })));
}
async function main() {
  const argv = process.argv.slice(2), separator = argv.indexOf('--');
  const options = {};
  if (separator < 0 || separator % 2) throw new Error('Expected --root ROOT --format istanbul|lcov --input RAW --output NATIVE -- COMMAND');
  for (let i = 0; i < separator; i += 2) {
    if (!['--root', '--format', '--input', '--output', '--config'].includes(argv[i]) || !argv[i + 1] || options[argv[i]]) throw new Error('Invalid or duplicate import option');
    options[argv[i]] = argv[i + 1];
  }
  if (!options['--root'] || !options['--input'] || !options['--output'] || !['istanbul', 'lcov'].includes(options['--format']) || !argv[separator + 1]) throw new Error('Missing required import options or command');
  const root = await fs.realpath(options['--root']);
  const input = path.resolve(options['--input']), output = path.resolve(options['--output']);
  const protectConfig = () => protectConfigDestinations(root, [input, output], options['--config']);
  await protectConfig();
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
    // Pass termination to the test command and wait for it. A cancelled run never
    // publishes, even when the command handles the signal and exits successfully.
    let cancelled;
    const forwards = ['SIGINT', 'SIGTERM', 'SIGHUP'].map(signal => [signal, () => { cancelled ??= signal; child.kill(signal); }]);
    for (const [signal, forward] of forwards) process.on(signal, forward);
    const stop = () => { for (const [signal, forward] of forwards) process.off(signal, forward); };
    child.on('error', error => { stop(); reject(error); });
    child.on('exit', (code, signal) => {
      stop();
      if (cancelled) reject(new Error(`Coverage collection cancelled by ${cancelled}; no report written`));
      else if (signal) reject(new Error(`Coverage command terminated: ${signal}`));
      else resolve(code);
    });
  });
  if (status !== 0) {
    throw new Error(`Coverage command failed: ${status}; no native coverage was published (any raw report it wrote is not imported). Fix the failing tests or coverage thresholds, then rerun with fresh paths`);
  }
  const format = options['--format'];
  const raw = await fs.readFile(input, 'utf8').catch(error => {
    if (error.code !== 'ENOENT') throw error;
    throw new Error(`Coverage command succeeded but wrote no report at ${input}. Likely causes: coverage is not enabled (pass --coverage), the reporter does not emit ${format === 'istanbul' ? 'Istanbul JSON' : 'LCOV'} (use ${reporterHint(format)}), or the reporter writes elsewhere (set Vitest --coverage.reportsDirectory or Jest --coverageDirectory to the directory containing --input)`);
  });
  const records = format === 'istanbul' ? istanbul(parseUniqueJson(raw)) : lcov(raw, root);
  if (!records.some(record => record.hits.length)) {
    throw new Error(`Empty imported coverage in ${input}. Likely causes: no tests ran, or coverage include does not match your sources (Vitest coverage.include, Jest collectCoverageFrom, for example "src/**/*.ts")`);
  }
  await convert(root, files, records);
  const result = { version: 1, files: {} };
  for (const [absolute, file] of files) {
    if (digest(await fs.readFile(absolute)) !== file.sha256) throw new Error(`Source changed during coverage collection: ${absolute}`);
    // Missing records remain missing evidence, never invented zero-hit coverage.
    if (Object.keys(file.lines).length) result.files[path.relative(root, absolute).split(path.sep).join('/')] = { sha256: file.sha256, lines: file.lines };
  }
  // A fresh destination is required, so never replace an existing entry.
  await publishReport(output, `${JSON.stringify(result, null, 2)}\n`, { validate: protectConfig });
  // Advice only: the scanner decides completeness for the files it selects.
  const absent = [...files.keys()].map(absolute => path.relative(root, absolute).split(path.sep).join('/'))
    .filter(relative => !result.files[relative] && !notMeasured(relative));
  if (absent.length) {
    const shown = absent.slice(0, 10).map(display).join(', ') + (absent.length > 10 ? `, and ${absent.length - 10} more` : '');
    console.error(`Warning: ${absent.length} TypeScript source file${absent.length === 1 ? ' has' : 's have'} no coverage records and will count as missing coverage if scanned: ${shown}. If they should be measured, add them to Vitest coverage.include or Jest collectCoverageFrom`);
  }
}
main().catch(error => { console.error(JSON.stringify(error.message)); process.exitCode = 2; });
