export function shipping(zone: number): number {
  if (zone === 0) {
    return 0;
  }
  if (zone === 1) {
    return 5;
  }
  if (zone === 2) {
    return 10;
  }
  return 20;
}
