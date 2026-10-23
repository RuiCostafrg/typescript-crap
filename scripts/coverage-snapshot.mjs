/** Identical snapshot primitives; callers retain their own freshness/publication policy. */
import { createHash } from 'node:crypto';
import * as fs from 'node:fs/promises';
import path from 'node:path';

const excluded = new Set(['.git', 'node_modules', 'target', 'dist', 'build', 'coverage']);
export const digest = bytes => createHash('sha256').update(bytes).digest('hex');

export function decodeSource(bytes) {
  // Fatal decoding keeps hashed bytes and mapped text consistent. Preserve BOM as
  // one UTF-16 unit because V8 offsets and importer locations include the source.
  return new TextDecoder('utf-8', { fatal: true, ignoreBOM: true }).decode(bytes);
}

export async function snapshot(root, { supported, sourceText, lineCount, compareNames }) {
  const files = new Map();
  async function walk(directory) {
    const entries = await fs.readdir(directory, { withFileTypes: true });
    entries.sort((a, b) => compareNames(a.name, b.name));
    for (const entry of entries) {
      const absolute = path.join(directory, entry.name);
      if (entry.isDirectory() && !excluded.has(entry.name)) await walk(absolute);
      else if (entry.isFile() && supported(entry.name)) {
        const bytes = await fs.readFile(absolute);
        const source = decodeSource(bytes);
        files.set(absolute, {
          sha256: digest(bytes), lines: {},
          ...(lineCount ? { lineCount: source.split('\n').length } : {}),
          ...(sourceText ? { source } : {}),
        });
      }
    }
  }
  await walk(root);
  return files;
}
