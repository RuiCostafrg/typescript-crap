const first = () => 1; // Golden identity fixture: every byte is pinned by src/analysis/tests.rs.
// It starts with a BOM, contains a CRLF region and astral Unicode. Never
// reformat it; changing a byte changes the committed expectations.
import { helper } from "./helper";
export type * from "./types";
export /* c */ type /* d */ * as ns from "./namespace";

// Declarations and generators.
export function declared(a: number, b?: string): number {
  if (a > 0) {
    return a;
  }
  return b ? 1 : 0;
}
function* generated(limit: number) {
  for (let i = 0; i < limit; i++) yield i;
}
async function asynchronous(value: Promise<number>) {
  return await value;
}
async function* asyncGenerated() {
  yield 1;
}
declare function ambient(x: number): void;
function overload(x: string): string;
function overload(x: number): number;
function overload(x: unknown) { return x; }
export default function () { return "default"; }

// Function expressions.
const named = function inner(x: number) { return x; };
const anonymousExpression = function (x: number) { return x * 2; };
const generatorExpression = function* () { yield 1; };
(function () { helper(); })();

// Arrows, including expression bodies, currying and bare parameters.
const expressionBodied = (x: number) => x + 1;
const curried = (x: number) => (y: number) => (z: number) => x + y + z;
const bare = x => x;
const bareCurried = x => y => z => x ?? y ?? z;
const objectBody = () => ({ key: 1 });
const asyncArrow = async (x: number) => { await x; };
const genericArrow = <T,>(value: T): T => value;
const parenthesized = (() => 1);
let assignedLater: () => number;
assignedLater = () => 2;
const holder = { member: (x: number) => x };
holder.member = function () { return 3; };
(() => {})();

// Methods, accessors, fields, decorators and parameter properties.
@sealed
export class Service<T> extends Base implements Contract {
  static instances = 0;
  #secret = 1;
  #priv = () => this.#secret;
  private field = (x: number) => x;
  handler = function () { return 1; };
  static {
    const init = () => Service.instances++;
  }
  constructor(private readonly injected: string = "x") { super(); }
  @logged
  @traced("method")
  method(@inject() dependency: string, fallback = dependency || "none"): string {
    return fallback;
  }
  get value(): number { return this.#secret; }
  set value(next: number) { this.#secret = next; }
  static create<U>(this: void): Service<U> { return new Service<U>(); }
  async *stream() { yield* [1]; }
  #hidden() { return 2; }
  ["computed" + "key"]() { return 3; }
  "quoted key"() { return 4; }
  42() { return 5; }
  overloaded(x: string): string;
  overloaded(x: number): number;
  overloaded(x: unknown): unknown { return x; }
}

export abstract class Shape {
  abstract area(): number;
  protected abstract get label(): string;
  describe(): string { return `${this.label}: ${this.area()}`; }
}

interface Contract<E = {}> {
  method(dependency: string): string;
  <K extends keyof E>(key: K): E[K]
  <K extends keyof E>(key: K, fallback: E[K]): E[K]
}

// Named, anonymous and nested classes.
const Assigned = class Inner {
  run() { return () => class { deep() { return 1; } }; }
};
const AnonymousClass = class {
  run() { return 2; }
};
function factory() {
  class Local {
    method() { return function nested() { return 3; }; }
  }
  return new Local();
}
namespace Space {
  export function inside() { return 4; }
}

// Object pairs and anonymous callbacks.
export const table = {
  plain: () => 1,
  "string key": function () { return 2; },
  [Symbol.iterator]: function* () { yield 3; },
  method() { return 4; },
  get accessor() { return 5; },
  set accessor(v: number) {},
  async asyncMethod() { return 6; },
  nested: { deeper: (x: number) => x },
};
[1, 2, 3].map((n) => n * 2).filter(function (n) { return n > 2; });
setTimeout(() => {
  helper();
}, 0);

// Every decision kind and logical operator.
export function decisions(input: number[], flag?: boolean, text?: string | null): number {
  let total = 0;
  for (const item of input) {
    if (item > 10 && flag) {
      total += item;
    } else if (item < 0 || !flag) {
      continue;
    }
  }
  for (const key in input) total++;
  for (let i = 0; i < 3; i++) {}
  for (;;) { break; }
  while (total > 100)
    total--;
  do {
    total++;
  } while (total < 5);
  try {
    JSON.parse(text ?? "{}");
  } catch (error) {
    throw error;
  } finally {
    total += 0;
  }
  try { total--; } catch { total++; }
  switch (total) {
    case 1:
      break;
    case 2:
    case 3:
      total = flag ? 1 : 2;
      break;
    default:
      total = 0;
  }
  let a: number | undefined;
  a &&= 1;
  a ||= 2;
  a ??= 3;
  a += 4;
  const bits = (total | 1) & 2;
  var legacy = bits;
  outer: for (const x of input) { continue outer; }
  const handlers: { [key: string]: () => number } = {};
  handlers.missing ||= () => 5;
  return total;
}

// Type-only syntax, default parameters and destructuring defaults.
export function typed<T extends object = {}>(
  value: T extends string ? "s" : T extends number ? "n" : never,
  options: { mode?: "a" | "b" } = {},
  { first = 1, second: renamed = 2 }: { first?: number; second?: number } = {},
  [head = 0, ...rest]: number[] = [],
  callback = () => first,
): T extends Array<infer U> ? U : T {
  type Local = T extends string ? 1 : 0;
  interface Nested<E> {
    <K extends keyof E>(key: K): E[K]
    // A comment in the separator gap.
    <K extends keyof E>(key: K, fallback: E[K]): E[K]
  }
  type Shaped<E> = {
    <K extends keyof E>(key: K): E[K][]
    <K extends keyof E>(key: K, fallback: E[K]): E[K][]
  };
  const {
    alpha = 1,
    beta: gamma =
      2,
  } = options as { alpha?: number; beta?: number };
  const cast = value as unknown as T;
  return cast as never;
}
const optionalArrow = (a?: number, b: number = a ?? 0) => a || b;
function optionalDefault(
  a?: number =
    1,
) { return a; }
export default () => 1;
const asserted = (() => 1) as () => number;
class Adjacent {}function afterAdjacent() { return 0; }

// Comments inside bodies are trivia for identity.
function commented() {
  // line comment
  /* block comment */ return /* inline */ 1; /** doc */
}

// Astral Unicode before functions on one line.
/* 😀🦀 */ const 𝒳 = () => "é𝒳"; /* 𝒴 */ const after = () => () => "😀";
function café(ß = "𝄞") { return ß; } const 名前 = function () { return /é/u.test("é"); };

// CRLF region starts below.
function crlf(a: number) {
  const text = `first
second ${a}`;
  return a ? text : "";
}
const crlfArrow = (x: number) =>
  x * 2;
class CrlfClass {
  method() {
    return 1;
  }
}
// CRLF region ends above.
const lastLine = () => 0;
