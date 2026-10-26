import { expect, test } from 'vitest';
import { price } from '@ts-crap-fixture/core';
import { Badge } from './src/Badge';

test('TSX component uses the workspace pricing package', () => {
  expect(price(1)).toBe(4);
  if (!process.env.FIXTURE_PARTIAL) {
    expect(price(-1)).toBe(0);
    expect(price(6)).toBe(18);
    expect(price(11)).toBe(22);
    const badge = Badge({ quantity: 1 });
    expect(badge.type).toBe('span');
    expect(badge.props['data-amount']).toBe(4);
  }
});
