export function price(quantity: number): number {
  if (quantity < 0) {
    return 0;
  }
  if (quantity > 10) {
    return quantity * 2;
  }
  if (quantity > 5) {
    return quantity * 3;
  }
  return quantity * 4;
}
