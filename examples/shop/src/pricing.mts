export function discount(subtotal: number, customer: "regular" | "vip"): number {
  if (customer === "vip") {
    return subtotal * 0.2;
  }
  if (subtotal >= 100) {
    return subtotal * 0.1;
  }
  return 0;
}

export function shipping(country: string, subtotal: number): number {
  if (subtotal >= 100) {
    return 0;
  }
  if (country === "PT") {
    return 5;
  }
  if (country === "ES") {
    return 8;
  }
  return 20;
}

export const total = (subtotal: number, reduction: number, delivery: number): number =>
  subtotal - reduction + delivery;
