import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { runInNewContext } from 'node:vm';
import { test } from 'node:test';

test('HTML filters combine literal path/name search and failing status', () => {
  const listeners = new Map();
  const search = { value: '', addEventListener: (event, fn) => listeners.set(event, fn) };
  const failing = { checked: false, addEventListener: (event, fn) => listeners.set(event, fn) };
  const count = { textContent: '' };
  const rows = [
    { cells: [{ textContent: 'src/CAFÉ.ts:1:1' }, { textContent: '<script>unsafe</script>' }], dataset: { failed: 'true' } },
    { cells: [{ textContent: 'src/retry.ts:2:1' }, { textContent: 'retry' }], dataset: { failed: 'false' } },
    { cells: [{ textContent: 'src/café.ts:3:1' }, { textContent: 'other' }], dataset: { failed: 'false' } },
  ];
  const document = {
    querySelector: selector => ({ '#filter': search, '#only-failing': failing, '#visible-count': count })[selector],
    querySelectorAll: () => rows,
  };
  const template = readFileSync(new URL('../src/report-filter.html', import.meta.url), 'utf8');
  runInNewContext(template.slice(template.indexOf('>') + 1, template.lastIndexOf('</script>')), { document });
  assert.equal(count.textContent, '3 of 3 displayed rows visible');
  search.value = 'café'; listeners.get('input')();
  assert.deepEqual(rows.map(row => row.hidden), [false, true, false]);
  failing.checked = true; listeners.get('change')();
  assert.deepEqual(rows.map(row => row.hidden), [false, true, true]);
  search.value = '<script>'; listeners.get('input')();
  assert.deepEqual(rows.map(row => row.hidden), [false, true, true]);
  search.value = 'absent'; listeners.get('input')();
  assert.equal(count.textContent, '0 of 3 displayed rows visible');
  search.value = ''; failing.checked = false; listeners.get('change')();
  assert.equal(count.textContent, '3 of 3 displayed rows visible');
});

test('HTML filters handle empty reports', () => {
  const control = { value: '', checked: false, addEventListener() {} };
  const count = {};
  const document = {
    querySelector: selector => selector === '#visible-count' ? count : control,
    querySelectorAll: () => [],
  };
  const template = readFileSync(new URL('../src/report-filter.html', import.meta.url), 'utf8');
  runInNewContext(template.slice(template.indexOf('>') + 1, template.lastIndexOf('</script>')), { document });
  assert.equal(count.textContent, '0 of 0 displayed rows visible');
});
