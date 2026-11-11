import { expect, test } from "@playwright/test";
import { openFromMenu, signIn } from "../helpers";

// Against the real service, not the mock: the compose image with the
// example organization loaded (`tokencontroller seed`). The mock answers
// what the web app asks; only the service can refuse it, as it did with
// "from is required" on the session and message lists. Run by
// scripts/live-journeys in the image job of CI.

test("the lists of sessions and messages answer what the web app asks", async ({ page }) => {
  await signIn(page, "nadia");
  // The same calls the screens make: no date range, and one.
  for (const path of ["/api/v1/sessions", "/api/v1/messages", "/api/v1/sessions?from=2026-03-01&to=2026-03-31", "/api/v1/messages?from=2026-03-01&to=2026-03-31"]) {
    const answer = await page.request.get(path);
    expect(answer.status(), `${path}: ${await answer.text()}`).toBe(200);
    const body = (await answer.json()) as { sessions?: unknown[]; messages?: unknown[] };
    expect((body.sessions ?? body.messages ?? []).length, path).toBeGreaterThan(0);
  }
});

test("the review list shows the seeded sessions", async ({ page }) => {
  await signIn(page, "nadia");
  await openFromMenu(page, "Check and submit");
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByRole("table").first().locator("tbody tr").first()).toBeVisible();
});

test("the overview shows project overhead from the service", async ({ page }) => {
  await signIn(page, "nadia");
  const overhead = page.getByText("Project overhead", { exact: true }).locator("xpath=../..");
  await expect(overhead).toBeVisible();
  await expect(overhead).not.toContainText("$0.00");
});
