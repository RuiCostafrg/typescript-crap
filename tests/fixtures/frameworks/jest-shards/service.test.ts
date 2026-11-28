import { shipping } from "../packages/service/src/shipping";

test("calculates shipping across zones", () => {
  expect([0, 1, 2, 3].map(shipping)).toEqual([0, 5, 10, 20]);
});
