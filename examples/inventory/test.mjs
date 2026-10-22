import assert from 'node:assert/strict';
import { Inventory, normalizeSku } from './src/inventory.mts';

assert.equal(normalizeSku('  café-😀 '), 'CAFÉ-😀');
const empty = new Inventory();
assert.deepEqual(empty.reserve('missing', 1), { ok: false, reason: 'unknown-sku' });
const stock = new Inventory([{ sku: ' book ', quantity: 3 }, { sku: 'BOOK', quantity: 2 }]);
assert.deepEqual(stock.reserve('book', 2), { ok: true, remaining: 3 });
if (!process.argv.includes('--partial')) {
  assert.deepEqual(stock.reserve('book', 3), { ok: true, remaining: 0 });
  assert.deepEqual(stock.reserve('book', 1), { ok: false, reason: 'insufficient-stock' });
  for (const quantity of [0, -1, 1.5, NaN]) {
    assert.deepEqual(stock.reserve('book', quantity), { ok: false, reason: 'invalid-quantity' });
    assert.throws(() => stock.restock('book', quantity), RangeError);
  }
  assert.equal(stock.restock('book', 4), 4);
}
console.log('Inventory TypeScript tests passed');
