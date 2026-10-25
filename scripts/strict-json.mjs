// JSON.parse owns grammar validation and value creation. This iterative token
// pass only tracks object-member names, before JSON.parse can discard duplicates.
export function parseUniqueJson(text) {
  const frames = [];
  const tokens = /"(?:[^"\\\u0000-\u001f]|\\(?:["\\/bfnrt]|u[0-9a-fA-F]{4}))*"|[{}\[\],:]|[^\s{}\[\],:"]+/gy;
  let offset = 0;
  while (offset < text.length) {
    if (/\s/.test(text[offset])) { offset++; continue; }
    tokens.lastIndex = offset;
    const match = tokens.exec(text);
    if (!match) throw new SyntaxError('Invalid JSON token');
    const token = match[0];
    offset = tokens.lastIndex;
    const frame = frames.at(-1);
    if (token === '{') frames.push({ keys: new Set(), key: true });
    else if (token === '[') frames.push({});
    else if (token === '}' || token === ']') frames.pop();
    else if (token === ',') { if (frame?.keys) frame.key = true; }
    else if (token === ':') { if (frame?.keys) frame.key = false; }
    else if (frame?.keys && frame.key && token.startsWith('"')) {
      const key = JSON.parse(token);
      if (frame.keys.has(key)) throw new SyntaxError(`Duplicate JSON key: ${JSON.stringify(key)}`);
      frame.keys.add(key);
    }
  }
  return JSON.parse(text);
}
