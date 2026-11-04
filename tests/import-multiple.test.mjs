import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import * as fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

const helper = fileURLToPath(new URL("../scripts/import-coverage.mjs", import.meta.url));
const source = "export function f() {\n  return 1;\n}\n";
const first = "packages/a/src/a.ts";
const second = "packages/b/src/b.ts";
const scanner = process.env.TS_CRAP_SCANNER;
async function project(t) {
  const root = await fs.realpath(await fs.mkdtemp(path.join(os.tmpdir(), "ts-crap projects ü-")));
  t.after(() => fs.rm(root, { recursive: true, force: true }));
  for (const file of [first, second]) {
    await fs.mkdir(path.dirname(path.join(root, file)), { recursive: true });
    await fs.writeFile(path.join(root, file), source);
  }
  return root;
}
function istanbul(file, counts = [1]) {
  return JSON.stringify({
    [file]: {
      path: file,
      statementMap: Object.fromEntries(
        counts.map((_, id) => [id, { start: { line: 2, column: id * 12 + 2 } }]),
      ),
      s: Object.fromEntries(counts.map((count, id) => [id, count])),
    },
  });
}
function lcov(file, count) {
  return `TN:shared-name\nSF:${file}\nDA:2,${count}\nend_of_record\n`;
}
function collect(
  root,
  inputs,
  reports,
  { format = "istanbul", extra = "", output = "native.json" } = {},
) {
  const script = `const fs=require('node:fs'),path=require('node:path');
    for(const [name,text] of Object.entries(${JSON.stringify(reports)})) {
      fs.mkdirSync(path.dirname(name),{recursive:true}); fs.writeFileSync(name,text);
    } ${extra}`;
  return spawnSync(
    process.execPath,
    [
      helper,
      "--root",
      root,
      "--format",
      format,
      ...inputs.flatMap((input) => ["--input", path.join(root, input)]),
      "--output",
      path.join(root, output),
      "--",
      process.execPath,
      "-e",
      script,
    ],
    { cwd: root, encoding: "utf8" },
  );
}
async function native(root, name = "native.json") {
  return JSON.parse(await fs.readFile(path.join(root, name), "utf8"));
}
async function unpublished(root, result, diagnostic) {
  assert.equal(result.status, 2, result.stderr);
  assert.match(result.stderr, diagnostic);
  await assert.rejects(fs.access(path.join(root, "native.json")), { code: "ENOENT" });
}

test("separate project reports merge source-bound counts and scan together", async (t) => {
  const root = await project(t);
  const result = collect(root, ["run/a.json", "run/b.json"], {
    "run/a.json": istanbul(first),
    "run/b.json": istanbul(second, [3]),
  });
  assert.equal(result.status, 0, result.stderr);
  const report = await native(root);
  assert.deepEqual(Object.keys(report.files), [first, second]);
  assert.deepEqual(report.files[first].lines, { 2: 1 });
  assert.deepEqual(report.files[second].lines, { 2: 3 });
  for (const file of Object.values(report.files)) {
    assert.equal(file.sha256, createHash("sha256").update(source).digest("hex"));
  }
  if (scanner) {
    const scan = spawnSync(
      scanner,
      [
        "--root",
        root,
        "--coverage",
        path.join(root, "native.json"),
        "--require-coverage",
        "--format",
        "json",
        "--threshold",
        "100",
      ],
      { encoding: "utf8" },
    );
    assert.equal(scan.status, 0, scan.stderr);
    const json = JSON.parse(scan.stdout);
    assert.equal(json.functions.length, 2);
    assert(json.functions.every((fn) => fn.coverage === 1 && fn.crap === 1));
  }
});

test("overlapping reports add only after each report's conservative line reduction", async (t) => {
  const root = await project(t);
  const result = collect(root, ["run/a.json", "run/b.json"], {
    "run/a.json": istanbul(first, [2, 0]),
    "run/b.json": istanbul(first, [3, 0]),
  });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual((await native(root)).files[first].lines, { 2: 0 });
});

test("report order is deterministic and independent reports can cover a previously zero line", async (t) => {
  const root = await project(t);
  const reports = { "one/a.json": istanbul(first, [0]), "one/b.json": istanbul(first, [3]) };
  const a = collect(root, Object.keys(reports), reports, { output: "a-native.json" });
  const b = collect(
    root,
    ["two/b.json", "two/a.json"],
    { "two/a.json": istanbul(first, [0]), "two/b.json": istanbul(first, [3]) },
    { output: "b-native.json" },
  );
  assert.equal(a.status, 0, a.stderr);
  assert.equal(b.status, 0, b.stderr);
  assert.deepEqual(await native(root, "a-native.json"), await native(root, "b-native.json"));
  assert.deepEqual((await native(root, "a-native.json")).files[first].lines, { 2: 3 });
});

for (const format of ["istanbul", "lcov"]) {
  test(`${format} directory inputs find nested reports and isolate identical LCOV test names`, async (t) => {
    const root = await project(t);
    const ext = format === "istanbul" ? "json" : "info";
    const report = format === "istanbul" ? (n) => istanbul(first, [n]) : (n) => lcov(first, n);
    const result = collect(
      root,
      ["run"],
      {
        [`run/project-a/report.${ext}`]: report(2),
        [`run/project-b/report.${ext}`]: report(3),
        "run/ignored.txt": "not coverage",
      },
      { format },
    );
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual((await native(root)).files[first].lines, { 2: 5 });
  });
}

test("stale inputs and duplicate arguments fail before starting the collector", async (t) => {
  for (const stale of ["file", "directory", "output", "duplicate"]) {
    const root = await project(t);
    if (stale === "file") await fs.writeFile(path.join(root, "run"), "old raw");
    if (stale === "directory") await fs.mkdir(path.join(root, "run"));
    if (stale === "output") await fs.writeFile(path.join(root, "native.json"), "old output");
    const result = collect(root, stale === "duplicate" ? ["run", "run"] : ["run"], {
      started: "yes",
    });
    assert.equal(result.status, 2, result.stderr);
    assert.match(result.stderr, /already exists|Duplicate coverage input/);
    await assert.rejects(fs.access(path.join(root, "started")), { code: "ENOENT" });
    if (stale === "output")
      assert.equal(await fs.readFile(path.join(root, "native.json"), "utf8"), "old output");
  }
});

test("overlapping input selections never count the same report twice", async (t) => {
  const root = await project(t);
  await unpublished(
    root,
    collect(root, ["run", "run/a.json"], { "run/a.json": istanbul(first) }),
    /Duplicate coverage input/,
  );
});

test("every input is validated before publication", async (t) => {
  for (const bad of [
    "missing",
    "empty-directory",
    "empty-report",
    "malformed",
    "overflow",
    "source-change",
    "failed-command",
  ]) {
    const root = await project(t);
    const reports = { "run/a.json": istanbul(first) };
    let extra = "";
    let diagnostic;
    if (bad === "missing") diagnostic = /wrote no report/;
    if (bad === "empty-directory") {
      extra = "fs.mkdirSync('run/b.json');";
      diagnostic = /No istanbul report files/;
    }
    if (bad === "empty-report") {
      reports["run/b.json"] = "{}";
      diagnostic = /Empty imported coverage/;
    }
    if (bad === "malformed") {
      reports["run/b.json"] = "{";
      diagnostic = /JSON|Expected|property/i;
    }
    if (bad === "overflow") {
      reports["run/a.json"] = istanbul(first, [Number.MAX_SAFE_INTEGER]);
      reports["run/b.json"] = istanbul(first, [1]);
      diagnostic = /Invalid coverage integer/;
    }
    if (bad === "source-change") {
      reports["run/b.json"] = istanbul(second);
      extra = `fs.appendFileSync(${JSON.stringify(first)},'// changed');`;
      diagnostic = /Source changed/;
    }
    if (bad === "failed-command") {
      extra = "process.exit(7);";
      diagnostic = /Coverage command failed: 7/;
    }
    await unpublished(
      root,
      collect(root, ["run/a.json", "run/b.json"], reports, { extra }),
      diagnostic,
    );
  }
});

test("directory imports reject symbolic links to old evidence", async (t) => {
  const root = await project(t);
  try {
    await fs.symlink("missing", path.join(root, "probe"));
    await fs.unlink(path.join(root, "probe"));
  } catch (error) {
    if (["EPERM", "EACCES", "ENOTSUP"].includes(error.code)) {
      t.skip("symbolic links unavailable");
      return;
    }
    throw error;
  }
  await fs.writeFile(path.join(root, "old.json"), istanbul(first));
  await unpublished(
    root,
    collect(
      root,
      ["run"],
      {},
      { extra: "fs.mkdirSync('run'); fs.symlinkSync('../old.json','run/link.json');" },
    ),
    /symbolic link/,
  );
});

test("hard-linked copies of one report cannot double its counts", async (t) => {
  const root = await project(t);
  const result = collect(
    root,
    ["run"],
    { "run/a.json": istanbul(first) },
    { extra: "fs.linkSync('run/a.json','run/b.json');" },
  );
  await unpublished(root, result, /Duplicate coverage input/);
});
