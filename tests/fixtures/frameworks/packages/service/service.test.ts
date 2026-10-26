import { price } from '@ts-crap-fixture/core';
import { shipping } from './src/shipping';

test('service uses the workspace pricing package', () => {
  expect(price(1)).toBe(4);
  if (!process.env.FIXTURE_PARTIAL) {
    expect(price(-1)).toBe(0);
    expect(price(6)).toBe(18);
    expect(price(11)).toBe(22);
    expect([0, 1, 2, 3].map(shipping)).toEqual([0, 5, 10, 20]);
  }
});
