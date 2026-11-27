const { spawnSync } = require("node:child_process");
const { mkdirSync, readFileSync, writeFileSync } = require("node:fs");
const path = require("node:path");
const { createCoverageMap } = require("istanbul-lib-coverage");
const { coverageThreshold, ...shardConfig } = require("./jest.sharded.config.cjs");

const [directory, ...extra] = process.argv.slice(2);
if (!directory || extra.length) {
  throw new Error("Usage: node jest-sharded-coverage.cjs FRESH_RUN_DIRECTORY");
}

// The root config is the single source of truth for the percentage gate. Omit
// it only while running shards, because each shard has partial coverage.
const thresholds = Object.entries(coverageThreshold?.global ?? {});
const metrics = ["branches", "functions", "lines", "statements"];
if (
  !thresholds.length ||
  Object.keys(coverageThreshold).some((scope) => scope !== "global") ||
  thresholds.some(
    ([metric, value]) => !metrics.includes(metric) || !Number.isFinite(value) || value < 0 || value > 100,
  )
) {
  throw new Error("This recipe requires global Jest coverage percentage thresholds");
}

const runDirectory = path.resolve(directory);
mkdirSync(runDirectory);
const map = createCoverageMap({});
for (const shard of [1, 2]) {
  const shardDirectory = path.join(runDirectory, `shard-${shard}`);
  mkdirSync(shardDirectory);
  const result = spawnSync(
    process.execPath,
    [
      require.resolve("jest/bin/jest"),
      "--config",
      JSON.stringify(shardConfig),
      "--runInBand",
      `--shard=${shard}/2`,
      "--coverage",
      "--coverageReporters=json",
      `--coverageDirectory=${shardDirectory}`,
    ],
    { cwd: __dirname, stdio: "inherit" },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`Jest shard ${shard}/2 failed (${result.status ?? result.signal})`);
  }
  map.merge(JSON.parse(readFileSync(path.join(shardDirectory, "coverage-final.json"), "utf8")));
}

if (!map.files().length) throw new Error("Jest shards produced no coverage");
const summary = map.getCoverageSummary();
const failures = thresholds.flatMap(([metric, minimum]) => {
  const actual = summary[metric].pct;
  return actual < minimum
    ? [`Jest merged coverage threshold for ${metric} (${minimum}%) not met: ${actual}%`]
    : [];
});
if (failures.length) {
  console.error(failures.join("\n"));
  process.exitCode = 1;
} else {
  writeFileSync(path.join(runDirectory, "coverage-final.json"), JSON.stringify(map), { flag: "wx" });
}
