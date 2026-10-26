import type { ReactNode } from 'react';
import { price } from '@ts-crap-fixture/core';

type Order = { id: string; quantity: number; label?: string };

export function quantityLabel(quantity: number): string {
  switch (quantity) {
    case 0: return 'empty';
    case 1: return 'single';
    default: return 'multiple';
  }
}

export function ItemList<T extends { id: string }>({ items, render }: {
  items: T[];
  render: (item: T) => ReactNode;
}) {
  if (items.length === 0) {
    return <p role="status">No orders</p>;
  }
  return <ul>{items.map(item => <li key={item.id}>{render(item)}</li>)}</ul>;
}

export function OrderPanel({ orders, disabled, onSelect }: {
  orders: Order[];
  disabled?: boolean;
  onSelect: (id: string) => void;
}) {
  const isDisabled = disabled ?? false;
  const renderOrder = (order: Order) => {
    const label = order.label?.trim() || 'Untitled';
    return <button disabled={isDisabled} data-kind={quantityLabel(order.quantity)} onClick={() => {
      if (isDisabled || order.quantity <= 0) {
        return;
      }
      onSelect(order.id);
    }}>{label}: {price(order.quantity)}</button>;
  };
  return <section aria-label="Orders"><ItemList items={orders} render={renderOrder} /></section>;
}
