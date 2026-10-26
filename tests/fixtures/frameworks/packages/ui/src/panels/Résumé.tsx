export function Résumé({ name, count }: { name?: string; count: number }) {
  const greeting = name ?? '訪問者';
  if (count < 0) {
    return <strong aria-label="invalid">数量エラー</strong>;
  }
  return <><h2>👋 {greeting}</h2><span>{count === 1 ? '1 order' : `${count} orders`}</span></>;
}
