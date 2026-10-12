import { renderHook } from "@testing-library/react";
import { describe, expect, test } from "vitest";
import { additions } from "@/lib/additions";
import { addedMocks } from "./mocks";

// The open build's slot must add nothing, so the open web app stays as it is.
// A private build's folder replaces this file along with the rest.
describe("the open build's additions", () => {
  test("add no pages, menu entries, page parts or mock answers", () => {
    expect(additions.routes({ app: {} as never, requireRole: () => () => Promise.resolve() })).toEqual([]);
    expect(renderHook(() => additions.useMenu()).result.current).toEqual([]);
    expect(Object.keys(additions.parts)).toEqual([]);
    expect(addedMocks({} as never)).toEqual([]);
  });
});
