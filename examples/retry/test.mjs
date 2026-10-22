import assert from 'node:assert/strict';
import { retry } from './src/retry.mts';

assert.equal(await retry(async () => 'ready'), 'ready');
if (!process.argv.includes('--partial')) {
  let calls = 0;
  assert.equal(await retry(async attempt => {
    calls++;
    if (attempt < 3) throw new Error('temporary');
    return 42;
  }, { shouldRetry: error => error.message === 'temporary' }), 42);
  assert.equal(calls, 3);
  const failure = new Error('permanent');
  const failing = async () => { throw failure; };
  await assert.rejects(retry(failing, { attempts: 2 }), error => error === failure);
  calls = 0;
  await assert.rejects(retry(async () => { calls++; throw failure; }, {
    attempts: 5, shouldRetry: () => false,
  }), error => error === failure);
  assert.equal(calls, 1);
  for (const attempts of [0, -1, 1.5, NaN]) {
    await assert.rejects(retry(failing, { attempts }), RangeError);
  }
}
console.log('Retry TypeScript tests passed');
