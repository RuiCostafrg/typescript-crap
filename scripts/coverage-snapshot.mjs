/** Identical snapshot primitives; callers retain their own freshness/publication policy. */
import { createHash } from "node:crypto";
import * as fs from "node:fs/promises";
import path from "node:path";
import { parseUniqueJson } from "./strict-json.mjs";

/** The scanner's default excluded directory names (ts-crap.json excluded_directories). */
export const DEFAULT_EXCLUDED_DIRECTORIES = Object.freeze([
  "node_modules",
  ".git",
  "target",
  "dist",
  "build",
  "coverage",
]);

/**
 * Read only `excluded_directories` from the scanner policy, so collection snapshots
 * the same directories the scanner selects. An absent default ROOT/ts-crap.json
 * means defaults; an explicit --config must exist. Other keys are the scanner's.
 */
export async function policyExclusions(root, config) {
  const file = config === undefined ? path.join(root, "ts-crap.json") : path.resolve(config);
  let bytes;
  try {
    bytes = await fs.readFile(file);
  } catch (error) {
    // A dangling default link is a present but unreadable policy, as in the scanner.
    const absent =
      error.code === "ENOENT" &&
      config === undefined &&
      (await fs.lstat(file).then(
        () => false,
        (entry) => entry.code === "ENOENT",
      ));
    if (absent) return [...DEFAULT_EXCLUDED_DIRECTORIES];
    throw new Error(`Cannot read project configuration ${file}: ${error.message}`);
  }
  let policy;
  // Decode strictly, as the scanner does: replaced bytes must not change the policy.
  try {
    policy = parseUniqueJson(
      new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes),
    );
  } catch (error) {
    throw new Error(`Invalid project configuration ${file}: ${error.message}`);
  }
  if (policy === null || typeof policy !== "object" || Array.isArray(policy)) {
    throw new Error(
      `Invalid project configuration ${file}: the top-level value must be a JSON object`,
    );
  }
  if (policy.version !== 1)
    throw new Error(
      `Invalid project configuration ${file}: unsupported version ${JSON.stringify(policy.version)}`,
    );
  const names = policy.excluded_directories;
  if (names === undefined) return [...DEFAULT_EXCLUDED_DIRECTORIES];
  if (!Array.isArray(names))
    throw new Error(`Invalid project configuration ${file}: excluded_directories must be an array`);
  const seen = new Set();
  for (const name of names) {
    if (
      typeof name !== "string" ||
      !name ||
      name === "." ||
      name === ".." ||
      /[/\\\0]/.test(name)
    ) {
      throw new Error(
        `Invalid project configuration ${file}: excluded_directories entries must be single directory names: ${JSON.stringify(name)}`,
      );
    }
    if (seen.has(name))
      throw new Error(
        `Invalid project configuration ${file}: duplicate excluded_directories entry: ${JSON.stringify(name)}`,
      );
    seen.add(name);
  }
  return names;
}
export const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");

export function decodeSource(bytes) {
  // Fatal decoding keeps hashed bytes and mapped text consistent. Preserve BOM as
  // one UTF-16 unit because V8 offsets and importer locations include the source.
  return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
}

export async function snapshot(
  root,
  {
    supported,
    sourceText,
    lineCount,
    compareNames,
    excludedDirectories = DEFAULT_EXCLUDED_DIRECTORIES,
  },
) {
  const excluded = new Set(excludedDirectories);
  const files = new Map();
  async function walk(directory) {
    const entries = await fs.readdir(directory, { withFileTypes: true });
    entries.sort((a, b) => compareNames(a.name, b.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory() && !excluded.has(entry.name)) await walk(absolute);
      else if (entry.isFile() && supported(entry.name)) {
        const relative = path.relative(root, absolute).split(path.sep).join("/");
        if (relative.includes("\\") || relative.includes(":")) {
          throw new Error(
            `Source path cannot be represented in schema v1: ${JSON.stringify(relative)}`,
          );
        }
        const bytes = await fs.readFile(absolute);
        const source = decodeSource(bytes);
        files.set(absolute, {
          sha256: digest(bytes),
          lines: {},
          ...(lineCount ? { lineCount: source.split("\n").length } : {}),
          ...(sourceText ? { source } : {}),
        });
      }
    }
  }
  await walk(root);
  return files;
}
