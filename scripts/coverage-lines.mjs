// Min-heap ordered by the existing coverage rule, not by interval boundaries.
// Expired entries are discarded lazily when they reach the root. Each range is
// inserted once and removed at most once: O(R log R) heap work, O(R) storage.
function precedes(a, b) {
  const difference = (a.end - a.start) - (b.end - b.start);
  return difference < 0 || (difference === 0 && a.count < b.count);
}

function insert(heap, range) {
  let index = heap.length;
  heap.push(range);
  while (index > 0) {
    const parent = Math.floor((index - 1) / 2);
    if (!precedes(range, heap[parent])) break;
    heap[index] = heap[parent];
    index = parent;
  }
  heap[index] = range;
}

function removeFirst(heap) {
  const last = heap.pop();
  if (!heap.length) return;
  let index = 0;
  while (index * 2 + 1 < heap.length) {
    let child = index * 2 + 1;
    if (child + 1 < heap.length && precedes(heap[child + 1], heap[child])) child++;
    if (!precedes(heap[child], last)) break;
    heap[index] = heap[child];
    index = child;
  }
  heap[index] = last;
}

/** Pure V8 UTF-16 range conversion; no process or filesystem access. */
export function lineHits(script, source) {
  // Sweep range boundaries. The smallest enclosing range determines each segment;
  // ties favor zero, so uncovered branches on an otherwise covered line stay zero.
  const events = new Map();
  function event(offset) {
    if (!events.has(offset)) events.set(offset, []);
    return events.get(offset);
  }
  for (const fn of script.functions) {
    for (const range of fn.ranges) {
      const { startOffset: start, endOffset: end, count } = range;
      if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end < start
          || !Number.isSafeInteger(count) || count < 0) {
        throw new Error(`Invalid V8 range for ${script.url}`);
      }
      // Node's type stripper preserves source offsets but can append a sourceURL
      // directive. Ignore that synthetic suffix, which is not an owned source line.
      const clippedEnd = Math.min(end, source.length);
      if (start >= clippedEnd) continue;
      const item = { start, end: clippedEnd, count };
      event(start).push(item);
      event(clippedEnd); // End boundaries still split segments, even with no additions.
    }
  }
  const starts = [0];
  for (let i = 0; i < source.length; i++) if (source[i] === '\n') starts.push(i + 1);
  const active = [];
  const hits = {};
  const offsets = [...events.keys()].sort((a, b) => a - b);
  let line = 0;
  for (let i = 0; i < offsets.length - 1; i++) {
    const start = offsets[i];
    for (const item of events.get(start)) insert(active, item);
    while (active.length && active[0].end <= start) removeFirst(active);
    const inner = active[0];
    if (!inner) continue;
    while (line + 1 < starts.length && starts[line + 1] <= start) line++;
    for (let cursor = start; cursor < offsets[i + 1];) {
      const end = Math.min(offsets[i + 1], starts[line + 1] ?? source.length);
      if (source.slice(cursor, end).trim()) {
        const key = String(line + 1);
        hits[key] = Math.min(hits[key] ?? Infinity, inner.count);
      }
      cursor = end;
      if (cursor < offsets[i + 1]) line++;
    }
  }
  return hits;
}


/** Merge independent process/script evidence without mutating either input. */
export function mergeLineHits(previous, incoming) {
  const result = { ...previous };
  for (const [line, count] of Object.entries(incoming)) {
    const existing = result[line] ?? 0;
    if (!/^[1-9][0-9]*$/.test(line) || !Number.isSafeInteger(count) || count < 0
        || !Number.isSafeInteger(existing) || existing < 0) {
      throw new Error('Invalid coverage line or hit count');
    }
    const sum = existing + count;
    if (!Number.isSafeInteger(sum)) throw new Error('Coverage hit count overflow');
    result[line] = sum;
  }
  return result;
}
