const first = () => 1; // Golden identity fixture for the TSX grammar: every byte is pinned by
// src/analysis/tests.rs. It starts with a BOM, contains a CRLF region and
// astral Unicode. Never reformat it.
import React from "react";
export type * from "./types";
export type * as ns from "./namespace";

interface Lookup<E> {
  <K extends keyof E>(key: K): E[K][]
  <K extends keyof E>(key: K, fallback: E[K]): E[K][]
}
interface Plain {
  (x: number): string
  <U>(y: U): U
}
type Shaped<E> = {
  <K extends keyof E>(key: K): E[K]
  /* gap */
  <K extends keyof E>(key: K, fallback: E[K]): E[K]
};

type Props = { items: Item[]; title?: string };

export function List({ items, title = "Default" }: Props) {
  return (
    <ul title={title ?? "x"}>
      {items.map((item) => (
        <li key={item.id} onClick={() => item.select()}>
          {item.done ? <s>{item.label}</s> : item.label} 😀𝒳
        </li>
      ))}
      {items.length > 0 && <Footer />}
    </ul>
  );
}

export const Arrow = ({ x }: { x: number }) => <div>{x || "none"}</div>;
export const Generic = <T,>(value: T) => <span>{String(value)}</span>;
const Fragmented = () => <><b>bold</b>{/* jsx comment */}</>;
const bare = x => <i>{x}</i>;
const curried = (a: number) => (b: number) => <b>{a + b}</b>;
const callback = [1].map(function (n) { return <em key={n}>{n}</em>; });

@observer
class Widget extends React.Component<Props> {
  state = { open: false };
  toggle = () => this.setState({ open: !this.state.open });
  handler = function () { return 1; };
  render() {
    if (!this.props) return null;
    return <div onClick={this.toggle}>{this.state.open ? "open" : "closed"}</div>;
  }
  get size(): number { return 1; }
}

const Anonymous = class {
  render() { return <p>{(() => "inline")()}</p>; }
};

const handlers = {
  onClick: () => <a />,
  "on key": function () { return 2; },
  submit() { return 3; },
};

export function decisions(a?: number, b: number = a ?? 0): number {
  let value = a;
  value &&= 1;
  value ||= 2;
  value ??= 3;
  for (const n of [1]) if (n || b) value++;
  while (value > 9)
    value--;
  switch (value) {
    case 1:
      return a && b ? 1 : 0;
    default:
      break;
  }
  try { value++; } catch { value--; }
  return value;
}

/* 😀 */ const Emoji = () => <span>🦀</span>; const Más = () => <b>é</b>;

// CRLF region starts below.
const Multiline = () => (
  <p>
    first line 𝒳
    second line
  </p>
);
function crlf(x: number) {
  return `a
b${x}`;
}
// CRLF region ends above.
export default function () { return <main />; }
