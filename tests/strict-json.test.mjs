import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseUniqueJson } from '../scripts/strict-json.mjs';

test('duplicate decoded keys are rejected at every object depth', () => {
  for (const text of [
    '{"a":0,"a":1}', '{"a":1,"\\u0061":0}', '{"s":{"0":0,"0":1}}',
    '{"s":{"0":1,"0":0}}', '[{"statementMap":{"0":{},"0":{}}}]',
    '{"__proto__":{},"__proto__":{}}', '{"a.ts":{},"a.ts":{}}',
    '{"a":[{}, {"x":0,"x":1}]}',
  ]) assert.throws(() => parseUniqueJson(text), /Duplicate JSON key/);
});

test('valid values agree with JSON.parse and malformed grammar is rejected', () => {
  for (const text of [
    'null', 'true', '-1.25e+3', '"string"', '[]', '{}',
    '{"a":{"x":1},"b":{"x":2},"array":[1,2,{"x":3}]}',
    '{"quote\\\"colon:":"{},:\\\"", "\\u0062": [false,null,"\\\\"]}',
    '{"__proto__":{},"constructor":42}', ' {"😀":1,"é":2}\n',
  ]) assert.deepEqual(parseUniqueJson(text), JSON.parse(text));
  for (const text of ['', '{', '[}', '{"a" 1}', '{"a":1,}', '[1,]', '01', 'undefined', '"\\x01"', '"\n"', '{}{}']) {
    assert.throws(() => parseUniqueJson(text), SyntaxError);
  }
});

test('generated nested JSON keeps keys isolated across siblings', () => {
  let state = 12345;
  const next = () => (state = (state * 1664525 + 1013904223) >>> 0);
  for (let i = 0; i < 200; i++) {
    const value = { a: [next(), { a: String.fromCodePoint(32 + next() % 1000) }], b: { a: next() % 2 === 0 } };
    assert.deepEqual(parseUniqueJson(JSON.stringify(value)), value);
  }
  const text = '['.repeat(20000) + '0' + ']'.repeat(20000);
  assert.ok(parseUniqueJson(text));
});
