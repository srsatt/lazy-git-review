const { subtract } = require("../src/math");

test("subtracts two values", () => {
  expect(subtract(7, 4)).toBe(3);
});
