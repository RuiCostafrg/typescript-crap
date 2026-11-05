#!/usr/bin/env node
// Collect Istanbul (Jest/Vitest/nyc) or LCOV with a pre-execution source snapshot.
import { parseUniqueJson } from "./strict-json.mjs";
import { coverageLocation, protectConfigDestinations, publishReport } from "./coverage-publish.mjs";
import {
  DEFAULT_EXCLUDED_DIRECTORIES,
  digest,
  policyExclusions,
  snapshot,
} from "./coverage-snapshot.mjs";
import { spawn } from "node:child_process";
import { constants as fsConstants, existsSync, lstatSync } from "node:fs";
import * as fs from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";

const REMAP_HINT =
  "Use a coverage setup that remaps to TypeScript (Vitest istanbul or v8 provider; Jest babel or v8 provider with source maps), and do not import raw NODE_V8_COVERAGE output";
// Tests, mocks, fixtures and tool configuration are rarely meant to be measured.
const notMeasured = (relative) =>
  /(^|\/)(__tests__|__mocks__|__fixtures__|tests?|mocks|fixtures)\//.test(relative) ||
  /\.(tests?|specs?|mocks?|fixtures?)\.[cm]?tsx?$|(^|\/)[^/]*\.config\.[cm]?tsx?$/.test(relative);
// Paths come from the checkout; never let a filename emit terminal controls.
const display = (text) =>
  text.replace(
    // oxlint-disable-next-line no-control-regex -- Escape filename control bytes before terminal output.
    /[\u0000-\u001f\u007f-\u009f]/g,
    (char) => `\\u${char.charCodeAt(0).toString(16).padStart(4, "0")}`,
  );
// The policy's excluded directory names, set once collection starts.
let excludedDirectories = [...DEFAULT_EXCLUDED_DIRECTORIES];
/** Explain why a source named by coverage is absent from the collected snapshot. */
function snapshotHint(root, absolute) {
  const relative = path.relative(root, absolute);
  if (relative === ".." || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    return `it is outside --root ${root}. Pass the project root that contains your sources`;
  }
  // The snapshot never follows symbolic links, to files or directories.
  let current = root;
  for (const part of relative.split(path.sep)) {
    current = path.join(current, part);
    let entry;
    try {
      entry = lstatSync(current);
    } catch {
      break;
    }
    if (entry.isSymbolicLink()) {
      return `it is reached through the symbolic link ${path.relative(root, current) || current}, which collection does not follow. Configure coverage to report the real source path`;
    }
  }
  if (!existsSync(absolute)) {
    return "it does not exist under --root (moved, renamed or deleted since the report was generated). Recollect coverage from the current sources";
  }
  const excluded = excludedDirectories.length ? excludedDirectories.join(", ") : "none configured";
  return `it is under --root but in an excluded directory (${excluded}), is a declaration file (.d.ts/.d.mts/.d.cts), or is not a .ts/.tsx/.mts/.cts source. Limit coverage to non-declaration TypeScript sources outside those directories`;
}
const reporterHint = (format) =>
  format === "istanbul"
    ? "Vitest --coverage.reporter=json or Jest --coverageReporters=json"
    : "Vitest --coverage.reporter=lcovonly or Jest --coverageReporters=lcovonly";
const supported = (name) => /\.(?:tsx?|mts|cts)$/.test(name) && !/\.d\.(?:ts|mts|cts)$/.test(name);
function integer(value, minimum = 0) {
  if (!Number.isSafeInteger(value) || value < minimum)
    throw new Error(`Invalid coverage integer: ${value}`);
  return value;
}
function decode(segment) {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const values = [];
  let value = 0,
    shift = 0;
  for (const char of segment) {
    const digit = alphabet.indexOf(char);
    if (digit < 0 || shift > 30) throw new Error("Invalid source-map VLQ");
    value += (digit & 31) * 2 ** shift;
    if (digit & 32) shift += 5;
    else {
      values.push(value % 2 ? -Math.floor(value / 2) : value / 2);
      value = 0;
      shift = 0;
    }
  }
  if (shift) throw new Error("Truncated source-map VLQ");
  return values;
}
function mappings(map) {
  if (
    map.version !== 3 ||
    !Array.isArray(map.sources) ||
    typeof map.mappings !== "string" ||
    map.sections
  ) {
    throw new Error("Expected a flat source map v3");
  }
  let source = 0,
    originalLine = 0,
    originalColumn = 0;
  return map.mappings.split(";").map((line) => {
    let column = 0;
    return line
      .split(",")
      .filter(Boolean)
      .map((segment) => {
        const fields = decode(segment);
        if (![1, 4, 5].includes(fields.length)) throw new Error("Invalid source-map segment");
        column = integer(column + fields[0]);
        if (fields.length === 1) return { column };
        source = integer(source + fields[1]);
        originalLine = integer(originalLine + fields[2]);
        originalColumn = integer(originalColumn + fields[3]);
        if (source >= map.sources.length) throw new Error("Source-map source index out of bounds");
        return { column, source, line: originalLine + 1 };
      });
  });
}
function resolveSource(base, name) {
  if (name.startsWith("file:")) return fileURLToPath(name);
  if (/^[a-zA-Z][a-zA-Z+.-]*:\/\//.test(name)) throw new Error(`Unmappable source URL: ${name}`);
  return path.resolve(base, name);
}
async function sourceMap(filename, inline) {
  if (inline) return { map: inline, base: path.dirname(filename) };
  const text = await fs.readFile(filename, "utf8");
  const matches = [...text.matchAll(/[#@]\s*sourceMappingURL=([^\s]+)/g)];
  if (!matches.length) {
    throw Object.assign(
      new Error(
        `No source map for generated input: ${filename}; the report covers generated JavaScript instead of TypeScript. ${REMAP_HINT}`,
      ),
      { missingSourceMap: true },
    );
  }
  const url = matches.at(-1)[1];
  if (/^data:application\/json(?:;charset=[^;,]+)?;base64,/i.test(url)) {
    return {
      map: parseUniqueJson(Buffer.from(url.split(",")[1], "base64").toString("utf8")),
      base: path.dirname(filename),
    };
  }
  const mapPath = resolveSource(path.dirname(filename), decodeURIComponent(url));
  return { map: parseUniqueJson(await fs.readFile(mapPath, "utf8")), base: path.dirname(mapPath) };
}
async function convert(root, files, records) {
  let unmapped = 0;
  const tests = new Map();
  for (const record of records) {
    const testName = record.testName ?? "";
    if (!tests.has(testName)) tests.set(testName, new Map());
    const test = tests.get(testName);
    const filename = resolveSource(root, record.path);
    let mapper;
    if (files.has(filename) && !record.map) {
      mapper = (line) => [{ file: files.get(filename), line }];
    } else {
      const { map, base } = await sourceMap(filename, record.map).catch((error) => {
        // Generated TypeScript (for example under build/) still remaps through its
        // map. Without one, a TypeScript record is simply outside the snapshot.
        // Only a missing generated file itself; a missing sidecar map keeps its own error.
        const sourceMissing =
          error.code === "ENOENT" && path.resolve(error.path ?? "") === filename;
        // Declaration files are TypeScript too, just never collected.
        const typescript = supported(filename) || /\.d\.[cm]?ts$/.test(filename);
        if (!record.map && typescript && (sourceMissing || error.missingSourceMap)) {
          throw new Error(
            `Unmappable source outside collected TypeScript snapshot: ${record.path}; ${snapshotHint(root, filename)}`,
          );
        }
        throw error;
      });
      const rows = mappings(map);
      const sources = map.sources.map((name, index) => {
        const absolute = resolveSource(resolveSource(base, map.sourceRoot || "."), name);
        const file = files.get(absolute);
        if (!file) {
          throw new Error(
            `Unmappable source outside collected TypeScript snapshot: ${name}; ${snapshotHint(root, absolute)}`,
          );
        }
        if (
          typeof map.sourcesContent?.[index] !== "string" ||
          digest(Buffer.from(map.sourcesContent[index])) !== file.sha256
        ) {
          throw new Error(`Missing or stale source-map content: ${name}`);
        }
        return file;
      });
      mapper = (line, column) => {
        const segments = rows[line - 1] || [];
        if (column !== undefined) {
          const segment = segments.findLast((item) => item.column <= column);
          return segment?.source === undefined
            ? []
            : [{ file: sources[segment.source], line: segment.line }];
        }
        return segments
          .filter((item) => item.source !== undefined)
          .map((item) => ({ file: sources[item.source], line: item.line }));
      };
    }
    for (const hit of record.hits) {
      integer(hit.line, 1);
      integer(hit.count);
      if (hit.column !== undefined) integer(hit.column);
      const targets = mapper(hit.line, hit.column);
      if (!targets.length) {
        unmapped++;
        continue;
      }
      for (const { file, line } of targets) {
        if (line > file.lineCount) throw new Error("Mapped line outside source");
        if (!test.has(file)) test.set(file, new Map());
        const lines = test.get(file);
        // Within one test, an uncovered statement/segment keeps the line uncovered.
        lines.set(line, Math.min(lines.get(line) ?? hit.count, hit.count));
      }
    }
  }
  if (unmapped)
    throw new Error(
      `${unmapped} coverage locations could not be source-mapped; configure the test tool to emit remapped TypeScript coverage. ${REMAP_HINT}`,
    );
  // Independent tests add only after their conservative source-map reduction.
  // Istanbul is already an aggregate and remains a single conservative group.
  for (const test of tests.values()) {
    for (const [file, lines] of test) {
      for (const [line, count] of lines)
        file.lines[line] = integer((file.lines[line] ?? 0) + count);
    }
  }
}
function istanbul(raw) {
  return Object.entries(raw).map(([key, file]) => {
    const hits = Object.entries(file.statementMap).map(([id, location]) => ({
      line: location.start.line,
      column: location.start.column,
      count: integer(file.s[id]),
    }));
    // Reporters record default-argument execution in branch metadata rather than
    // statementMap. These are direct initializer hits, not inferred function hits.
    for (const [id, branch] of Object.entries(file.branchMap ?? {})) {
      if (branch.type !== "default-arg") continue;
      const counts = file.b?.[id];
      if (
        !Array.isArray(branch.locations) ||
        !branch.locations.length ||
        !Array.isArray(counts) ||
        counts.length !== branch.locations.length
      ) {
        throw new Error("Invalid default-argument coverage");
      }
      for (let index = 0; index < counts.length; index++) {
        const location = branch.locations[index];
        hits.push({
          line: location.start.line,
          column: location.start.column,
          count: integer(counts[index]),
        });
      }
    }
    return { path: file.path || key, map: file.inputSourceMap, hits };
  });
}
function lcov(raw, root) {
  const tests = new Map();
  let record,
    testName = "";
  for (const line of raw.split(/\r?\n/)) {
    if (line.startsWith("TN:")) {
      if (record) throw new Error("LCOV test name inside source record");
      testName = line.slice(3);
    } else if (line.startsWith("SF:")) {
      if (record) throw new Error("LCOV record missing end_of_record");
      record = { path: resolveSource(root, line.slice(3)), testName, hits: new Map() };
    } else if (line.startsWith("DA:")) {
      if (!record) throw new Error("LCOV DA outside record");
      const [number, count] = line.slice(3).split(",");
      const location = integer(Number(number), 1);
      if (record.hits.has(location)) throw new Error("Duplicate LCOV line within source record");
      record.hits.set(location, integer(Number(count)));
    } else if (line === "end_of_record") {
      if (record) {
        if (!tests.has(testName)) tests.set(testName, new Map());
        const files = tests.get(testName);
        const previous = files.get(record.path);
        if (previous) {
          // Repeated captures of the same test/source add counters before mapping.
          for (const [line, count] of record.hits)
            previous.hits.set(line, integer((previous.hits.get(line) ?? 0) + count));
        } else files.set(record.path, record);
      }
      record = undefined;
      testName = "";
    }
  }
  if (record) throw new Error("LCOV record missing end_of_record");
  return [...tests.values()].flatMap((files) =>
    [...files.values()].map((record) => ({
      ...record,
      hits: [...record.hits].map(([line, count]) => ({ line, count })),
    })),
  );
}
// Cooperative reservations live outside report directories, which reporters may
// delete on startup. Publish complete metadata atomically, then check overlaps.
async function reserveInputs(inputs) {
  const owner = process.getuid?.() ?? digest(Buffer.from(os.userInfo().username));
  const directory = path.join(await fs.realpath(os.tmpdir()), `ts-crap-import-locks-${owner}`);
  await fs.mkdir(directory, { mode: 0o700 }).catch((error) => {
    if (error.code !== "EEXIST") throw error;
  });
  const info = await fs.lstat(directory);
  if (!info.isDirectory() || (process.getuid && info.uid !== process.getuid())) {
    throw new Error(`Unsafe coverage reservation directory: ${directory}`);
  }
  // Fold case conservatively so case-insensitive volumes cannot bypass a claim.
  const keys = [...new Set(inputs.map((input) => input.toLowerCase()))].sort();
  const staging = await fs.mkdtemp(path.join(directory, "lease-"));
  const metadata = path.join(staging, "reservation");
  const owned = [];
  let identity;
  const release = async () => {
    try {
      for (const filename of owned) {
        const current = await fs.lstat(filename, { bigint: true }).catch((error) => {
          if (error.code !== "ENOENT") throw error;
        });
        if (current && current.dev === identity.dev && current.ino === identity.ino) {
          await fs.unlink(filename);
        }
      }
    } finally {
      await fs.rm(metadata, { force: true });
      await fs.rmdir(staging);
    }
  };
  const contains = (parent, child) =>
    parent === child || child.startsWith(parent.endsWith(path.sep) ? parent : parent + path.sep);
  try {
    await fs.writeFile(metadata, JSON.stringify({ token: staging, inputs: keys }), {
      flag: "wx",
      mode: 0o600,
    });
    identity = await fs.stat(metadata, { bigint: true });
    for (const key of keys) {
      const filename = path.join(directory, `${digest(Buffer.from(key))}.lock`);
      try {
        await fs.link(metadata, filename);
      } catch (error) {
        if (error.code !== "EEXIST") throw error;
        throw new Error(
          `Coverage input is reserved by another active or abandoned import: ${key} (reservation ${filename}); use a unique private run directory, or remove the reservation after confirming its collector has stopped`,
        );
      }
      owned.push(filename);
    }
    for (const name of await fs.readdir(directory)) {
      if (!name.endsWith(".lock")) continue;
      const filename = path.join(directory, name);
      const text = await fs.readFile(filename, "utf8").catch((error) => {
        if (error.code !== "ENOENT") throw error;
      });
      if (text === undefined) continue;
      const claim = parseUniqueJson(text);
      if (claim.token === staging) continue;
      if (!Array.isArray(claim.inputs) || claim.inputs.some((input) => typeof input !== "string")) {
        throw new Error(`Invalid coverage reservation: ${filename}`);
      }
      if (
        claim.inputs.some((other) =>
          keys.some((key) => contains(key, other) || contains(other, key)),
        )
      ) {
        throw new Error(
          `Coverage inputs overlap another active or abandoned import (reservation ${filename}); use a unique private run directory, or remove the reservation after confirming its collector has stopped`,
        );
      }
    }
    return release;
  } catch (error) {
    await release();
    throw error;
  }
}

async function requireFresh(destinations) {
  for (const destination of destinations) {
    try {
      await fs.lstat(destination);
      throw new Error(`Coverage destination already exists: ${destination}; use a fresh path`);
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
  }
}

// Discover reports only after the collection command has created fresh inputs.
async function reportInputs(inputs, format, locations) {
  const reports = [];
  const paths = new Set();
  const identities = new Set();
  async function visit(filename, explicit = false) {
    const stat = await fs.lstat(filename, { bigint: true }).catch((error) => {
      if (error.code !== "ENOENT") throw error;
      throw new Error(
        `Coverage command succeeded but wrote no report at ${filename}. Likely causes: coverage is not enabled (pass --coverage), the reporter does not emit ${format === "istanbul" ? "Istanbul JSON" : "LCOV"} (use ${reporterHint(format)}), or the reporter writes elsewhere (set Vitest --coverage.reportsDirectory or Jest --coverageDirectory to the directory containing --input)`,
      );
    });
    if (stat.isSymbolicLink())
      throw new Error(`Coverage input cannot be a symbolic link: ${filename}`);
    if (stat.isDirectory()) {
      for (const name of (await fs.readdir(filename)).sort())
        await visit(path.join(filename, name));
      return;
    }
    if (!stat.isFile())
      throw new Error(`Coverage input must be a regular file or directory: ${filename}`);
    if (explicit && supported(filename))
      throw new Error("Coverage file inputs cannot be TypeScript");
    const matches =
      format === "istanbul" ? /\.json$/i.test(filename) : /\.(?:info|lcov)$/i.test(filename);
    if (!explicit && !matches) return;
    const canonical = await fs.realpath(filename);
    const identity = `${stat.dev}:${stat.ino}`;
    if (paths.has(canonical) || (stat.ino !== 0n && identities.has(identity))) {
      throw new Error(`Duplicate coverage input: ${filename}`);
    }
    paths.add(canonical);
    if (stat.ino !== 0n) identities.add(identity);
    reports.push({ filename, stat });
  }
  for (const [index, input] of inputs.entries()) {
    // Existing aliases were already resolved. Walk the captured route to reject
    // new parent links; resolving that route again also normalizes filesystem case.
    const expected = locations[index];
    let parent = path.parse(expected).root;
    for (const part of path
      .relative(parent, path.dirname(expected))
      .split(path.sep)
      .filter(Boolean)) {
      parent = path.join(parent, part);
      const entry = await fs.lstat(parent).catch((error) => {
        if (error.code !== "ENOENT") throw error;
      });
      if (!entry) break;
      if (entry.isSymbolicLink())
        throw new Error(`Coverage input parent changed through a symbolic link: ${input}`);
    }
    if ((await coverageLocation(input)) !== (await coverageLocation(expected))) {
      throw new Error(`Coverage input parent changed through a symbolic link: ${input}`);
    }
    const before = reports.length;
    await visit(locations[index], true);
    if (reports.length === before)
      throw new Error(`No ${format} report files in coverage input: ${input}`);
  }
  return reports;
}
// Bind reads to the discovered file, not a path a writer can replace. On systems
// without O_NOFOLLOW, path and descriptor identity are both checked before bytes
// are read; collectors must still finish all writers before returning.
async function readReport({ filename, stat: expected }) {
  if (expected.ino === 0n) throw new Error(`Cannot verify coverage input identity: ${filename}`);
  const changed = () => new Error(`Coverage input changed after discovery: ${filename}`);
  const handle = await fs
    .open(
      filename,
      fsConstants.O_RDONLY | (fsConstants.O_NOFOLLOW ?? 0) | (fsConstants.O_NONBLOCK ?? 0),
    )
    .catch((error) => {
      if (["ENOENT", "ELOOP"].includes(error.code)) throw changed();
      throw error;
    });
  const matches = (actual) =>
    actual.isFile() &&
    actual.dev === expected.dev &&
    actual.ino === expected.ino &&
    actual.size === expected.size &&
    actual.mtimeNs === expected.mtimeNs &&
    actual.ctimeNs === expected.ctimeNs;
  try {
    if (
      !matches(await handle.stat({ bigint: true })) ||
      !matches(await fs.lstat(filename, { bigint: true }))
    )
      throw changed();
    const raw = await handle.readFile("utf8");
    if (!matches(await handle.stat({ bigint: true }))) throw changed();
    return raw;
  } finally {
    await handle.close();
  }
}

async function main() {
  const argv = process.argv.slice(2),
    separator = argv.indexOf("--");
  const options = {};
  const rawInputs = [];
  if (separator < 0 || separator % 2)
    throw new Error(
      "Expected --root ROOT --format istanbul|lcov --input RAW [--input RAW...] --output NATIVE -- COMMAND",
    );
  for (let i = 0; i < separator; i += 2) {
    if (
      !["--root", "--format", "--input", "--output", "--config"].includes(argv[i]) ||
      !argv[i + 1] ||
      (argv[i] !== "--input" && options[argv[i]])
    )
      throw new Error("Invalid or duplicate import option");
    if (argv[i] === "--input") rawInputs.push(argv[i + 1]);
    else options[argv[i]] = argv[i + 1];
  }
  if (
    !options["--root"] ||
    !rawInputs.length ||
    !options["--output"] ||
    !["istanbul", "lcov"].includes(options["--format"]) ||
    !argv[separator + 1]
  )
    throw new Error("Missing required import options or command");
  const root = await fs.realpath(options["--root"]);
  const inputs = rawInputs.map((input) => path.resolve(input));
  const output = path.resolve(options["--output"]);
  if (new Set(inputs).size !== inputs.length) throw new Error("Duplicate coverage input");
  const protectConfig = () =>
    protectConfigDestinations(root, [...inputs, output], options["--config"]);
  await protectConfig();
  if (inputs.includes(output) || supported(output))
    throw new Error("Coverage inputs/outputs must differ and cannot be TypeScript");
  // Import selection excludes declarations; post-command hashes below enforce
  // unchanged source. Fresh input/output destinations remain importer-only policy.
  excludedDirectories = await policyExclusions(root, options["--config"]);
  const files = await snapshot(root, {
    supported,
    sourceText: false,
    lineCount: true,
    compareNames: (a, b) => (a < b ? -1 : a > b ? 1 : 0),
    excludedDirectories,
  });
  // Canonicalize existing aliases once, then reject collector-created redirects.
  const locations = await Promise.all(inputs.map(coverageLocation));
  await requireFresh([...inputs, output]);
  const release = await reserveInputs(locations);
  const result = { version: 1, files: {} };
  try {
    // A competing collector may have finished between the initial check and claim.
    await requireFresh([...inputs, output]);
    const status = await new Promise((resolve, reject) => {
      const child = spawn(argv[separator + 1], argv.slice(separator + 2), {
        cwd: root,
        stdio: "inherit",
      });
      // Pass termination to the test command and wait for it. A cancelled run never
      // publishes, even when the command handles the signal and exits successfully.
      let cancelled;
      const forwards = ["SIGINT", "SIGTERM", "SIGHUP"].map((signal) => [
        signal,
        () => {
          cancelled ??= signal;
          child.kill(signal);
        },
      ]);
      for (const [signal, forward] of forwards) process.on(signal, forward);
      const stop = () => {
        for (const [signal, forward] of forwards) process.off(signal, forward);
      };
      child.on("error", (error) => {
        stop();
        reject(error);
      });
      child.on("exit", (code, signal) => {
        stop();
        if (cancelled)
          reject(new Error(`Coverage collection cancelled by ${cancelled}; no report written`));
        else if (signal) reject(new Error(`Coverage command terminated: ${signal}`));
        else resolve(code);
      });
    });
    if (status !== 0) {
      throw new Error(
        `Coverage command failed: ${status}; no native coverage was published (any raw report it wrote is not imported). Fix the failing tests or coverage thresholds, then rerun with fresh paths`,
      );
    }
    const format = options["--format"];
    const reports = await reportInputs(inputs, format, locations);
    for (const report of reports) {
      const input = report.filename;
      try {
        const raw = await readReport(report);
        const records = format === "istanbul" ? istanbul(parseUniqueJson(raw)) : lcov(raw, root);
        if (!records.some((record) => record.hits.length)) {
          throw new Error(
            `Empty imported coverage in ${input}. Likely causes: no tests ran, or coverage include does not match your sources (Vitest coverage.include, Jest collectCoverageFrom, for example "src/**/*.ts")`,
          );
        }
        // Reduce overlaps independently in each report before adding its line counts.
        await convert(root, files, records);
      } catch (error) {
        // Preserve existing single-file diagnostics; name the failing batch member.
        if (inputs.length === 1 && input === locations[0]) throw error;
        throw new Error(`Could not import coverage from ${input}: ${error.message}`);
      }
    }
    for (const [absolute, file] of files) {
      if (digest(await fs.readFile(absolute)) !== file.sha256)
        throw new Error(`Source changed during coverage collection: ${absolute}`);
      // Missing records remain missing evidence, never invented zero-hit coverage.
      if (Object.keys(file.lines).length)
        result.files[path.relative(root, absolute).split(path.sep).join("/")] = {
          sha256: file.sha256,
          lines: file.lines,
        };
    }
    // A fresh destination is required, so never replace an existing entry.
    await publishReport(output, `${JSON.stringify(result, null, 2)}\n`, {
      validate: protectConfig,
    });
  } finally {
    await release();
  }
  // Advice only: the scanner decides completeness for the files it selects.
  const absent = [...files.keys()]
    .map((absolute) => path.relative(root, absolute).split(path.sep).join("/"))
    .filter((relative) => !result.files[relative] && !notMeasured(relative));
  if (absent.length) {
    const shown =
      absent.slice(0, 10).map(display).join(", ") +
      (absent.length > 10 ? `, and ${absent.length - 10} more` : "");
    console.error(
      `Warning: ${absent.length} TypeScript source file${absent.length === 1 ? " has" : "s have"} no coverage records and will count as missing coverage if scanned: ${shown}. If they should be measured, add them to Vitest coverage.include or Jest collectCoverageFrom`,
    );
  }
}
main().catch((error) => {
  console.error(JSON.stringify(error.message));
  process.exitCode = 2;
});
