import { spawnSync } from "node:child_process";
import { mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const [runDirectory] = process.argv.slice(2);
if (process.argv.length !== 3 || !runDirectory) {
  throw new Error("Usage: node collect-vitest-shards.mjs NEW_RUN_DIRECTORY");
}

const run = path.resolve(runDirectory);
mkdirSync(run);
const blobs = path.join(run, "blobs");
const vitest = fileURLToPath(new URL("./node_modules/vitest/vitest.mjs", import.meta.url));

function runVitest(args, reportsDirectory, coverageShard = false) {
  const result = spawnSync(
    process.execPath,
    [
      vitest,
      "run",
      "--config",
      "vitest.sharded.config.mjs",
      "--coverage",
      "--coverage.reportsDirectory",
      reportsDirectory,
      ...args,
    ],
    {
      env: { ...process.env, VITEST_COVERAGE_SHARD: coverageShard ? "1" : "0" },
      stdio: "inherit",
    },
  );
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}

for (const shard of [1, 2]) {
  runVitest(
    ["--shard", `${shard}/2`, "--reporter", "blob", "--outputFile", path.join(blobs, `shard-${shard}.json`)],
    path.join(run, `shard-${shard}`),
    true,
  );
}

runVitest(["--merge-reports", blobs], path.join(run, "coverage"));
