import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
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
  { format = "istanbul", extra = "", output = "native.json", preload } = {},
) {
  const script = `const fs=require('node:fs'),path=require('node:path');
    for(const [name,text] of Object.entries(${JSON.stringify(reports)})) {
      fs.mkdirSync(path.dirname(name),{recursive:true}); fs.writeFileSync(name,text);
    } ${extra}`;
  return spawnSync(
    process.execPath,
    [
      ...(preload ? ["--require", preload] : []),
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

test("fresh directories may use TypeScript suffixes while file inputs cannot", async (t) => {
  for (const suffix of ["ts", "tsx", "mts", "cts"]) {
    const root = await project(t);
    const directory = `run.${suffix}`;
    const result = collect(root, [directory], { [`${directory}/coverage.json`]: istanbul(first) });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual((await native(root)).files[first].lines, { 2: 1 });
  }
  const root = await project(t);
  await unpublished(
    root,
    collect(root, ["raw.ts"], { "raw.ts": istanbul(first) }),
    /file inputs cannot be TypeScript/,
  );
});

async function directoryLinks(t, root) {
  if (process.platform === "android") {
    t.skip("Android/proot cannot resolve directory links; covered on hosted platforms");
    return false;
  }
  await fs.mkdir(path.join(root, "real"));
  try {
    await fs.symlink(path.join(root, "real"), path.join(root, "alias"), "junction");
  } catch (error) {
    if (["EPERM", "EACCES", "ENOSYS", "ENOTSUP"].includes(error.code)) {
      t.skip("directory links unavailable");
      return false;
    }
    throw error;
  }
  return true;
}

test("existing parent aliases are anchored but collector-created redirects are rejected", async (t) => {
  const root = await project(t);
  if (!(await directoryLinks(t, root))) return;
  const result = collect(root, ["alias/raw.json"], { "alias/raw.json": istanbul(first) });
  assert.equal(result.status, 0, result.stderr);
  await fs.rm(path.join(root, "native.json"));
  await fs.unlink(path.join(root, "alias"));
  const old = path.join(root, "real", "old.json");
  await fs.writeFile(old, istanbul(first, [9]));
  await unpublished(
    root,
    collect(
      root,
      ["alias/old.json"],
      {},
      { extra: `fs.symlinkSync(${JSON.stringify(path.join(root, "real"))},'alias','junction');` },
    ),
    /parent changed through a symbolic link/,
  );
  assert.equal(await fs.readFile(old, "utf8"), istanbul(first, [9]));
});

async function waitForFile(filename, child, diagnostics) {
  const deadline = performance.now() + 30_000;
  while (performance.now() < deadline) {
    try {
      await fs.access(filename);
      return;
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
    }
    assert.equal(child.exitCode, null, diagnostics());
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  assert.fail("collector did not become ready within 30 seconds: " + diagnostics());
}

test(
  "concurrent exact or overlapping inputs are reserved until publication",
  { timeout: 120_000 },
  async (t) => {
    for (const [owned, competing] of [
      ["run/a.json", "run/a.json"],
      ["run", "run/a.json"],
      ["run/a.json", "run"],
    ]) {
      const root = await project(t);
      const script = `const fs=require('node:fs');
      fs.mkdirSync('run',{recursive:true});fs.rmSync('run',{recursive:true});
      fs.writeFileSync('ready','yes');
      const timer=setInterval(()=>{if(!fs.existsSync('release'))return;
        clearInterval(timer);fs.mkdirSync('run',{recursive:true});
        fs.writeFileSync('run/a.json',${JSON.stringify(istanbul(first))});
      },25);`;
      const child = spawn(
        process.execPath,
        [
          helper,
          "--root",
          root,
          "--format",
          "istanbul",
          "--input",
          path.join(root, owned),
          "--output",
          path.join(root, "native.json"),
          "--",
          process.execPath,
          "-e",
          script,
        ],
        { cwd: root, stdio: ["ignore", "ignore", "pipe"] },
      );
      let stderr = "";
      child.stderr.on("data", (chunk) => (stderr += chunk));
      let error;
      child.once("error", (e) => (error = e));
      const exited = new Promise((resolve) => child.once("close", resolve));
      t.after(() => {
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
      });
      await waitForFile(path.join(root, "ready"), child, () => String(error ?? stderr));
      assert.ifError(error);
      const rejected = collect(
        root,
        [competing],
        { "second-started": "yes", "run/a.json": istanbul(first, [9]) },
        { output: "other-native.json" },
      );
      assert.equal(rejected.status, 2, rejected.stderr);
      assert.match(rejected.stderr, /reserved|overlap/);
      const reservation = JSON.parse(rejected.stderr.trim()).match(
        /\(reservation (.+\.lock)\)/,
      )?.[1];
      assert.ok(reservation, rejected.stderr);
      assert.equal((await fs.lstat(reservation)).isFile(), true);

      await assert.rejects(fs.access(path.join(root, "second-started")), { code: "ENOENT" });
      const isolated = collect(
        root,
        ["isolated/raw.json"],
        { "isolated/raw.json": istanbul(second, [3]) },
        { output: "isolated-native.json" },
      );
      assert.equal(isolated.status, 0, isolated.stderr);
      await fs.writeFile(path.join(root, "release"), "yes");
      assert.equal(await exited, 0, stderr);
      assert.deepEqual((await native(root)).files[first].lines, { 2: 1 });
      await assert.rejects(fs.access(path.join(root, "other-native.json")), { code: "ENOENT" });
    }
  },
);

test("failed collectors release their input reservations", async (t) => {
  const root = await project(t);
  await unpublished(
    root,
    collect(root, ["run/a.json"], {}, { extra: "process.exit(7);" }),
    /Coverage command failed: 7/,
  );
  const result = collect(root, ["run/a.json"], { "run/a.json": istanbul(first) });
  assert.equal(result.status, 0, result.stderr);
});

test("racing starts cannot launch two overlapping collectors", { timeout: 120_000 }, async (t) => {
  for (const [left, right, raw] of [
    ["run", "run/a.json", "run/a.json"],
    ["run/a.json", "run", "run/a.json"],
    ["raw.json", "raw.json", "raw.json"],
  ]) {
    const root = await project(t);
    const errors = ["", ""];
    const children = [left, right].map((input, index) => {
      const script = `const fs=require('node:fs'),path=require('node:path');
        fs.writeFileSync('ready-${index}','yes');
        const timer=setInterval(()=>{if(!fs.existsSync('go'))return;
          clearInterval(timer);fs.mkdirSync(path.dirname(${JSON.stringify(raw)}),{recursive:true});
          fs.writeFileSync(${JSON.stringify(raw)},${JSON.stringify(istanbul(first, [index + 1]))});
        },25);`;
      const child = spawn(
        process.execPath,
        [
          helper,
          "--root",
          root,
          "--format",
          "istanbul",
          "--input",
          path.join(root, input),
          "--output",
          path.join(root, `native-${index}.json`),
          "--",
          process.execPath,
          "-e",
          script,
        ],
        { cwd: root, stdio: ["ignore", "ignore", "pipe"] },
      );
      child.stderr.on("data", (chunk) => (errors[index] += chunk));
      child.once("error", (error) => (errors[index] += String(error)));
      return child;
    });
    const exits = children.map((child) => new Promise((resolve) => child.once("close", resolve)));
    t.after(() => {
      for (const child of children)
        if (child.exitCode === null && child.signalCode === null) child.kill("SIGTERM");
    });
    let timeout;
    // Neither collector writes a raw report before go. A rejected peer must
    // therefore lose a reservation, rather than merely see a completed report.
    const rejected = await Promise.race([
      ...exits.map((exit, index) => exit.then((code) => ({ index, code }))),
      new Promise((_, reject) => {
        timeout = setTimeout(
          () => reject(new Error("both overlapping collectors started")),
          30_000,
        );
      }),
    ]).finally(() => clearTimeout(timeout));
    assert.equal(rejected.code, 2, errors[rejected.index]);
    assert.match(errors[rejected.index], /reserved|overlap/);
    await fs.writeFile(path.join(root, "go"), "yes");
    const codes = await Promise.all(exits);
    assert(codes.filter((code) => code === 0).length <= 1, JSON.stringify(codes));
    for (const [index, code] of codes.entries()) {
      if (code === 0)
        assert.deepEqual((await native(root, `native-${index}.json`)).files[first].lines, {
          2: index + 1,
        });
      else {
        assert.equal(code, 2, errors[index]);
        await assert.rejects(fs.access(path.join(root, `native-${index}.json`)), {
          code: "ENOENT",
        });
      }
    }
  }
});

test("case-insensitive input spellings do not look like parent redirects", async (t) => {
  const root = await project(t);
  await fs.writeFile(path.join(root, "case-probe"), "yes");
  try {
    await fs.access(path.join(root, "CASE-PROBE"));
  } catch (error) {
    if (error.code === "ENOENT") {
      t.skip("case-sensitive filesystem");
      return;
    }
    throw error;
  }
  const result = collect(root, ["RUN/RAW.JSON"], { "run/raw.json": istanbul(first) });
  assert.equal(result.status, 0, result.stderr);
  assert.deepEqual((await native(root)).files[first].lines, { 2: 1 });
});

for (const mutation of ["replace", "in-place", "during-read"]) {
  test(`descriptor validation rejects ${mutation} changes after discovery`, async (t) => {
    const root = await project(t);
    const preload = path.join(root, "replace-report.cjs");
    const target = path.join(root, "run", "a.json");
    // Vary the length so this check is reliable even with coarse timestamps.
    const replacement = istanbul(first, [10]);
    await fs.writeFile(
      preload,
      `
    const fs=require('node:fs/promises'), original=fs.open;
    fs.open=async(...args)=>{
      if(args[0]===${JSON.stringify(target)}) {
        ${mutation === "replace" ? `await fs.rename(args[0],args[0]+'.old');await fs.writeFile(args[0],${JSON.stringify(replacement)});` : ""}
        ${mutation === "in-place" ? `await fs.writeFile(args[0],${JSON.stringify(replacement)});` : ""}
        const handle=await original(...args);
        ${mutation === "during-read" ? `const read=handle.readFile.bind(handle);handle.readFile=async(...options)=>{const raw=await read(...options);await fs.writeFile(args[0],${JSON.stringify(replacement)});return raw;};` : ""}
        return handle;
      }
      return original(...args);
    };
    require('node:module').syncBuiltinESMExports();
  `,
    );
    await unpublished(
      root,
      collect(root, ["run/a.json"], { "run/a.json": istanbul(first) }, { preload }),
      /changed after discovery/,
    );
  });
}

test("descriptor fallback rejects a symlink replacement before reading its bytes", async (t) => {
  const root = await project(t);
  const target = path.join(root, "run", "a.json");
  const replacement = path.join(root, "replacement.json");
  await fs.writeFile(replacement, istanbul(first, [9]));
  try {
    await fs.symlink(replacement, path.join(root, "probe"));
    await fs.unlink(path.join(root, "probe"));
  } catch (error) {
    if (["EPERM", "EACCES", "ENOSYS", "ENOTSUP"].includes(error.code)) {
      t.skip("file symlinks unavailable");
      return;
    }
    throw error;
  }
  const preload = path.join(root, "swap-link.cjs");
  await fs.writeFile(
    preload,
    `
    const fs=require('node:fs/promises'),constants=require('node:fs').constants,original=fs.open;
    fs.open=async(...args)=>{
      if(args[0]===${JSON.stringify(target)}) {
        await fs.rename(args[0],args[0]+'.old');await fs.symlink(${JSON.stringify(replacement)},args[0]);
        args[1]&=~(constants.O_NOFOLLOW??0);
        const handle=await original(...args),read=handle.readFile.bind(handle);
        handle.readFile=async(...options)=>{await fs.writeFile('replacement-bytes-read','yes');return read(...options);};
        return handle;
      }
      return original(...args);
    };
    require('node:module').syncBuiltinESMExports();
  `,
  );
  await unpublished(
    root,
    collect(root, ["run/a.json"], { "run/a.json": istanbul(first) }, { preload }),
    /changed after discovery/,
  );
  await assert.rejects(fs.access(path.join(root, "replacement-bytes-read")), { code: "ENOENT" });
});

test("unverifiable report identity fails closed", async (t) => {
  const root = await project(t);
  const target = path.join(root, "run", "a.json");
  const preload = path.join(root, "no-identity.cjs");
  await fs.writeFile(
    preload,
    `
    const fs=require('node:fs/promises'),original=fs.lstat;
    fs.lstat=async(...args)=>{const stat=await original(...args);if(args[0]===${JSON.stringify(target)})stat.ino=0n;return stat;};
    require('node:module').syncBuiltinESMExports();
  `,
  );
  await unpublished(
    root,
    collect(root, ["run/a.json"], { "run/a.json": istanbul(first) }, { preload }),
    /Cannot verify coverage input identity/,
  );
});
