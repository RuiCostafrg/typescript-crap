#!/usr/bin/env node
// Isolated conversion benchmark: node scripts/bench-coverage-lines.mjs 500
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { pathToFileURL } from 'node:url';
import { resolve } from 'node:path';
const { lineHits } = await import(process.argv[3]
  ? pathToFileURL(resolve(process.argv[3])).href
  : new URL('./coverage-lines.mjs', import.meta.url).href);

const size = Number(process.argv[2]);
assert.ok(Number.isSafeInteger(size) && size > 0);
const source = 'x'.repeat(2 * size);
const script = { url: 'file:///dense.mts', functions: [{ ranges: Array.from({ length: size }, (_, index) => ({
  startOffset: index, endOffset: source.length - index, count: 1,
})) }] };
const convert = () => lineHits(script, source);
assert.deepEqual(convert(), { 1: 1 }); // correctness and warmup outside samples
const samples = [];
for (let index = 0; index < 5; index++) {
  const start = performance.now();
  convert();
  samples.push(performance.now() - start);
}
assert.deepEqual(convert(), { 1: 1 });
samples.sort((a, b) => a - b);
console.log(JSON.stringify({ mode: 'dense-ranges', size, source_bytes: source.length,
  samples_ms: samples, median_ms: samples[2], peak_rss_kib: process.resourceUsage().maxRSS }));
