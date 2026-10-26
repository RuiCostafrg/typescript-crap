import { describe, expect, test, vi } from 'vitest';
import { renderToStaticMarkup } from 'react-dom/server';
import { ItemList, OrderPanel, quantityLabel } from './src/panels/OrderPanel';
import { AsyncPanel, loadMessage } from './src/panels/AsyncPanel';
import { DefaultPanel } from './coverage-gaps/DefaultPanel';

const partial = process.env.FIXTURE_PARTIAL;
const full = partial === '' || !partial;
const exercised = full || partial === 'branches';

test.runIf(process.env.FIXTURE_FAIL === '1')('deliberate failure verifies importer publication is aborted', () => {
  expect('actual test failure').toBe('a passing test');
});

describe.skipIf(!exercised)('complex TSX rendered through React', () => {
  test('default argument executes even when a reporter omits its line evidence', () => {
    expect(renderToStaticMarkup(<DefaultPanel />)).toBe('<span>Default</span>');
    expect(renderToStaticMarkup(<DefaultPanel label="Provided" />)).toBe('<span>Provided</span>');
  });
  test('generic lists render empty and populated states', () => {
    expect(renderToStaticMarkup(<ItemList items={[]} render={() => null} />)).toContain('No orders');
    expect(renderToStaticMarkup(<ItemList items={[{ id: 'a', value: '<unsafe>' }]}
      render={item => <b>{item.value}</b>} />)).toBe('<ul><li><b>&lt;unsafe&gt;</b></li></ul>');
  });

  test('workspace prices, labels and nested JSX callbacks', () => {
    const html = renderToStaticMarkup(<OrderPanel orders={[
      { id: 'empty', quantity: 0 },
      { id: 'one', quantity: 1, label: ' <Single> ' },
      { id: 'many', quantity: 11, label: 'Bulk' },
    ]} onSelect={vi.fn()} />);
    expect(html).toContain('Untitled: 0');
    expect(html).toContain('&lt;Single&gt;: 4');
    expect(html).toContain('Bulk: 22');
    expect(html).toContain('data-kind="empty"');
    expect(html).toContain('data-kind="single"');
    expect(html).toContain('data-kind="multiple"');
    expect([0, 1, 2].map(quantityLabel)).toEqual(['empty', 'single', 'multiple']);
  });

  test('loading and successful async results render and escape content', async () => {
    expect(renderToStaticMarkup(<AsyncPanel />)).toContain('Loading…');
    const result = await loadMessage(async () => '<ready>');
    expect(result).toEqual({ state: 'ready', value: '<ready>' });
    expect(renderToStaticMarkup(<AsyncPanel result={result} />)).toBe('<output>&lt;ready&gt;</output>');
  });
});

describe.skipIf(!full)('branches absent from the targeted partial run', () => {
  test.each([
    { disabled: false, quantity: 1, selected: true },
    { disabled: true, quantity: 1, selected: false },
    { disabled: false, quantity: 0, selected: false },
    { disabled: false, quantity: -1, selected: false },
  ])('event handler respects disabled=$disabled, quantity=$quantity', ({ disabled, quantity, selected }) => {
    const onSelect = vi.fn();
    // React server rendering does not dispatch events. Invoke the actual handler
    // produced by the component to exercise callback ownership and early returns.
    const panel = OrderPanel({ orders: [{ id: 'chosen', quantity }], disabled, onSelect });
    const list = panel.props.children;
    const button = list.props.render(list.props.items[0]);
    button.props.onClick();
    expect(onSelect.mock.calls).toEqual(selected ? [['chosen']] : []);
    expect(button.props.disabled).toBe(disabled);
  });

  test.each([
    { load: async () => '  ', message: 'Empty response' },
    { load: async () => { throw new Error('<offline>'); }, message: '<offline>' },
    { load: async () => { throw 'network'; }, message: 'Unknown failure' },
  ])('async failure becomes an accessible alert: $message', async ({ load, message }) => {
    const result = await loadMessage(load);
    expect(result).toEqual({ state: 'error', value: message });
    const html = renderToStaticMarkup(<AsyncPanel result={result} />);
    expect(html).toContain('role="alert"');
    expect(html).toContain(message === '<offline>' ? '&lt;offline&gt;' : message);
  });

  test('lazy TSX module has Unicode paths, identifiers, fragments and text', async () => {
    const { Résumé } = await import('./src/panels/Résumé');
    expect(renderToStaticMarkup(<Résumé count={0} />)).toBe('<h2>👋 訪問者</h2><span>0 orders</span>');
    expect(renderToStaticMarkup(<Résumé name="Zoë & 李" count={1} />))
      .toBe('<h2>👋 Zoë &amp; 李</h2><span>1 order</span>');
    expect(renderToStaticMarkup(<Résumé count={-1} />)).toContain('数量エラー');
  });
});
