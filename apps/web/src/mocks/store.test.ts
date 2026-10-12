import { describe, expect, test } from "vitest";
import { allocate } from "./store";

// The mocks stand in for the service until its API lands, so they must keep
// the rules the docs promise; otherwise the screens would show wrong numbers.
describe("example organization", () => {
  test("a split always adds up to the whole amount", () => {
    expect(allocate(1000, [3333, 3333, 3334])).toEqual([333, 333, 334]);
    const parts = allocate(41_870_000_001, [6000, 4000]);
    expect(parts.reduce((a, b) => a + b, 0)).toBe(41_870_000_001);
  });
});
