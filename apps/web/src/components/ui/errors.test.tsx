import { cleanup, render } from "@testing-library/react";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ApiError } from "@/api/client";
import { additions } from "@/lib/additions";
import { ErrorBox } from "./display";
import { FormError } from "./form";

// Every error message the app shows reaches a build's additions once, such as
// for a list of the last errors a person saw.
describe("shown errors", () => {
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  test("ErrorBox and FormError tell the additions what they show", () => {
    const shown = vi.spyOn(additions, "onErrorShown");
    render(<ErrorBox error={new ApiError(404, "not_found", "No such project.")} />);
    render(<FormError error={new Error("Give the bill a provider.")} />);
    render(<ErrorBox error="no Error at all" />);
    expect(shown.mock.calls).toEqual([["No such project."], ["Give the bill a provider."], ["Something went wrong."]]);
  });

  test("FormError without an error shows and tells nothing", () => {
    const shown = vi.spyOn(additions, "onErrorShown");
    const { container } = render(<FormError error={null} />);
    expect(container.innerHTML).toBe("");
    expect(shown).not.toHaveBeenCalled();
  });
});
