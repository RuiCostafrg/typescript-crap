import assert from 'node:assert/strict';
import { test } from 'node:test';
import { lineHits, mergeLineHits } from '../scripts/coverage-lines.mjs';

const range = (startOffset, endOffset, count) => ({ startOffset, endOffset, count });
const hits = (source, ...ranges) => lineHits({ url: 'file:///sample.mts',
  functions: [{ ranges }] }, source);

test('innermost ranges win, including uncovered segments of covered lines', () => {
  assert.deepEqual(hits('aaa\nbbb\nccc', range(0, 11, 5), range(4, 7, 0)),
    { 1: 5, 2: 0, 3: 5 });
  assert.deepEqual(hits('abcdef', range(0, 6, 1), range(2, 3, 0)), { 1: 0 });
});

test('overlaps, equal ranges and adjacent boundaries are conservative', () => {
  const ranges = [range(0, 6, 3), range(0, 6, 0), range(3, 9, 2)];
  assert.deepEqual(hits('abcdefghi', ...ranges), { 1: 0 });
  assert.deepEqual(hits('abcdefghi', ...ranges.toReversed()), { 1: 0 });
  assert.deepEqual(hits('aaa\nbbb', range(0, 4, 1), range(4, 7, 2)), { 1: 1, 2: 2 });
});

test('UTF-16 emoji offsets, BOM and CRLF preserve line ownership', () => {
  const source = '\uFEFF😀a\r\nbbb\r\n';
  assert.equal(source.indexOf('b'), 6);
  assert.deepEqual(hits(source, range(0, source.length, 2), range(6, 9, 0)),
    { 1: 2, 2: 0 });
});

test('whitespace, uncovered gaps, empty ranges and synthetic suffixes', () => {
  assert.deepEqual(hits('   \nabc\n\n', range(0, 9, 1)), { 2: 1 });
  assert.deepEqual(hits('abc\ndef', range(0, 3, 2)), { 1: 2 });
  assert.deepEqual(hits('abc', range(0, 99, 1), range(3, 99, 0), range(1, 1, 0)), { 1: 1 });
  assert.deepEqual(hits('', range(0, 4, 1)), {});
  assert.deepEqual(hits('abc'), {});
});

test('malformed V8 offsets and counts are rejected', () => {
  for (const invalid of [range(-1, 2, 1), range(2, 1, 1), range(0.5, 2, 1),
    range(0, Infinity, 1), range(0, 2, -1), range(0, 2, 0.5),
    range(0, 2, Number.MAX_SAFE_INTEGER + 1)]) {
    assert.throws(() => hits('abc', invalid), /Invalid V8 range/);
  }
});

test('independent processes add evidence without mutating inputs', () => {
  const first = { 1: 0, 2: 3 };
  const second = { 1: 2, 3: 1 };
  assert.deepEqual(mergeLineHits(first, second), { 1: 2, 2: 3, 3: 1 });
  assert.deepEqual(mergeLineHits(first, second), mergeLineHits(second, first));
  assert.deepEqual(first, { 1: 0, 2: 3 });
  assert.deepEqual(second, { 1: 2, 3: 1 });
  assert.throws(() => mergeLineHits({ 1: Number.MAX_SAFE_INTEGER }, { 1: 1 }), /overflow/);
  for (const invalid of [{ 0: 1 }, { '01': 1 }, { 1: -1 }, { 1: 0.5 }]) {
    assert.throws(() => mergeLineHits({}, invalid), /Invalid coverage/);
  }
});
