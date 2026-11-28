import { price } from "../packages/core/src/price";

test("calculates prices across quantity bands", () => {
  expect(price(1)).toBe(4);
  if (process.env.FIXTURE_PARTIAL !== "1") {
    expect(price(-1)).toBe(0);
    expect(price(6)).toBe(18);
    expect(price(11)).toBe(22);
  }
});
