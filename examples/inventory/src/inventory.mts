export type Reservation =
  | { ok: true; remaining: number }
  | { ok: false; reason: 'invalid-quantity' | 'unknown-sku' | 'insufficient-stock' };

export interface StockItem {
  sku: string;
  quantity: number;
}

export function normalizeSku(value: string): string {
  return value.trim().toUpperCase();
}

/** In-memory stock service with explicit business failures and normalized keys. */
export class Inventory {
  private readonly stock = new Map<string, number>();

  constructor(items: readonly StockItem[] = []) {
    for (const item of items) {
      this.restock(item.sku, item.quantity);
    }
  }

  restock(sku: string, quantity: number): number {
    if (!Number.isInteger(quantity) || quantity <= 0) {
      throw new RangeError('Quantity must be a positive integer');
    }
    const key = normalizeSku(sku);
    const next = (this.stock.get(key) ?? 0) + quantity;
    this.stock.set(key, next);
    return next;
  }

  reserve(sku: string, quantity: number): Reservation {
    if (!Number.isInteger(quantity) || quantity <= 0) {
      return { ok: false, reason: 'invalid-quantity' };
    }
    const key = normalizeSku(sku);
    const available = this.stock.get(key);
    if (available === undefined) {
      return { ok: false, reason: 'unknown-sku' };
    }
    if (available < quantity) {
      return { ok: false, reason: 'insufficient-stock' };
    }
    const remaining = available - quantity;
    this.stock.set(key, remaining);
    return { ok: true, remaining };
  }
}
