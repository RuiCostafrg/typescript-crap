import assert from 'node:assert/strict';
import { discount, shipping, total } from './src/pricing.mts';

assert.equal(discount(200, 'vip'), 40);
assert.equal(discount(200, 'regular'), 20);
assert.equal(discount(50, 'regular'), 0);
assert.equal(shipping('PT', 150), 0);
assert.equal(shipping('PT', 50), 5);
assert.equal(shipping('ES', 50), 8);
assert.equal(shipping('US', 50), 20);
assert.equal(total(50, 10, 5), 45);
console.log('Shop TypeScript tests passed');
