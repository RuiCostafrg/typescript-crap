#!/usr/bin/env node
// Expand the owned-file globs in Node so npm scripts also work on Windows.
import { globSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repository = fileURLToPath(new URL("../", import.meta.url));
const modes = {
  lint: ["oxlint", ["--deny-warnings"]],
  "format:check": ["oxfmt", ["--check"]],
  format: ["oxfmt", ["--write"]],
};
const mode = Object.hasOwn(modes, process.argv[2]) && modes[process.argv[2]];
if (!mode || process.argv.length !== 3) {
  throw new Error("Usage: node scripts/js-tooling.mjs lint|format:check|format");
}
const files = globSync(["scripts/*.mjs", "tests/*.test.mjs", "examples/**/*.mjs"], {
  cwd: repository,
  exclude: ["**/node_modules/**"],
}).sort();
if (files.length === 0) throw new Error("No repository .mjs files found");
const [tool, args] = mode;
const directory = path.join(repository, "tests/tooling/node_modules", tool);
const manifest = JSON.parse(readFileSync(path.join(directory, "package.json"), "utf8"));
const result = spawnSync(
  process.execPath,
  [path.join(directory, manifest.bin[tool]), ...args, ...files],
  {
    cwd: repository,
    stdio: "inherit",
  },
);
if (result.error) throw result.error;
process.exit(result.status ?? 1);
