export function classify(value: number): string {
  if (value < 0) return "negative";
  if (value === 0) return "zero";
  if (value < 10) return "small";
  return "large";
}
