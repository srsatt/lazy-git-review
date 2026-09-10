const { add } = require("../src/math");

test("adds two values", () => {
  expect(add(2, 3)).toBe(5);
});
