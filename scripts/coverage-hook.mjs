// Preloaded into every Node child via NODE_OPTIONS. Bind TS coverage to the
// exact source returned by Node's loader, not merely the pre/post-run disk file.
// This is an integrity guard for trusted tests, not a sandbox against hostile JS.
import { registerHooks } from 'node:module';
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';

const manifest = JSON.parse(readFileSync(process.env.TS_CRAP_SNAPSHOT, 'utf8'));
registerHooks({
  load(url, context, nextLoad) {
    const result = nextLoad(url, context);
    if (!url.startsWith('file:')) return result;
    const expected = manifest[fileURLToPath(url)];
    if (expected === undefined) return result;
    if (result.source === null || result.source === undefined) {
      throw new Error(`Cannot verify loaded TypeScript source: ${url}`);
    }
    const actual = createHash('sha256').update(result.source).digest('hex');
    if (actual !== expected) {
      throw new Error(`TypeScript source changed before module load: ${url}`);
    }
    return result;
  },
});
