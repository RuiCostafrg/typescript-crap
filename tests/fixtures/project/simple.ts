export function identity(value: number): number {
  return value;
}

export const choose = (flag: boolean): number => {
  if (flag) return 1;
  return 0;
};
