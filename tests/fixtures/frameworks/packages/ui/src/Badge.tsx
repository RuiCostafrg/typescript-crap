import { price } from '@ts-crap-fixture/core';

export function Badge({ quantity }: { quantity: number }) {
  const amount = price(quantity);
  return <span data-amount={amount}>Total: {amount}</span>;
}
