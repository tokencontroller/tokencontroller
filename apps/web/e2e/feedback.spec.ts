import { expect, test } from "@playwright/test";
import { mainMenu, openFromMenu, signIn } from "./helpers";

// Docs: Security and privacy > What a feedback report contains. The person
// sees the whole report first; the screenshot is off until they turn it on,
// and they can blur any part of it. The answer is under Feedback.
test("a member sends a bug with a blurred screenshot, and a controller answers it", async ({ page }) => {
  await signIn(page, "mara");
  await page.goto("/review?period=2026-10");
  await expect(page.getByRole("heading", { name: "Review and submit" })).toBeVisible();

  await (await mainMenu(page)).getByRole("button", { name: "Send feedback" }).click();
  const panel = page.getByRole("dialog", { name: "Send feedback" });
  await panel.getByLabel("Bug").check();
  await panel.getByLabel("What happened, or what you would like").fill("The ticket list in the token entry dialog cuts off on my phone.");

  // The screenshot is off by default; turned on, it shows before anything is sent.
  const screenshotSwitch = panel.getByRole("switch", { name: "Add a screenshot" });
  await expect(screenshotSwitch).not.toBeChecked();
  await screenshotSwitch.click();
  const shot = panel.getByTestId("screenshot");
  await expect(shot).toBeVisible();
  const box = await shot.boundingBox();
  if (!box) throw new Error("no screenshot on screen");
  await page.mouse.move(box.x + 10, box.y + 10);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 3, { steps: 4 });
  await page.mouse.up();
  await expect(panel.getByRole("button", { name: "Undo blur" })).toBeEnabled();

  // The six diagnostics, with the path but nothing after `?`.
  await panel.getByText("What goes along").click();
  const diagnostics = panel.getByTestId("diagnostics");
  await expect(diagnostics).toContainText("/review");
  await expect(diagnostics).not.toContainText("period=");
  await expect(panel.getByText("Never what a request contained.")).toBeVisible();

  await panel.getByRole("button", { name: "Send" }).click();
  await expect(panel).toBeHidden();
  await openFromMenu(page, "Your feedback");
  const mine = page.getByRole("list", { name: "Your feedback" });
  await expect(mine.getByRole("listitem").filter({ hasText: "cuts off on my phone" })).toContainText("received");

  // Without operators, the controllers read the reports (self-hosted).
  await signIn(page, "nadia");
  await openFromMenu(page, "Settings");
  await page.getByRole("link", { name: /Feedback reports/ }).click();
  await page.getByRole("table", { name: "Feedback reports" }).getByText("cuts off on my phone").filter({ visible: true }).first().click();
  await expect(page.getByRole("img", { name: "The screenshot sent with the report" })).toBeVisible();
  await page.getByLabel("Answer").fill("Fixed for the next version: the list scrolls now.");
  await page.getByRole("button", { name: "Save answer" }).click();
  await expect(page.getByText("answered").first()).toBeVisible();

  await signIn(page, "mara");
  await openFromMenu(page, "Your feedback");
  await expect(page.getByText("Fixed for the next version")).toBeVisible();
});

test("a member writes to support, and an operator answers what was escalated", async ({ page }) => {
  await signIn(page, "mara");
  await (await mainMenu(page)).getByRole("button", { name: "Support" }).click();
  const form = page.getByRole("dialog", { name: "Write to support" });
  await expect(form).toContainText("The answer comes by email to m.ortner@kestrel.example.");
  await form.getByLabel("Message").fill("How do I move a token entry to another ticket after I submitted it?");
  await form.getByRole("button", { name: "Send" }).click();
  await expect(form.getByTestId("support-sent")).toBeVisible();
  await form.getByRole("button", { name: "Done" }).click();

  // The admin area shows only for operators.
  await signIn(page, "nadia");
  await expect((await mainMenu(page)).getByRole("link", { name: /Admin/ })).toHaveCount(0);
  await page.evaluate(() => fetch("/api/v1/__mock/operator", { method: "POST" }));
  await page.reload();
  await (await mainMenu(page)).getByRole("link", { name: /^Admin/ }).click();
  await expect(page.getByRole("heading", { name: "Admin" })).toBeVisible();

  await page.getByRole("navigation", { name: "Admin" }).getByRole("link", { name: "Support" }).click();
  const list = page.getByRole("table", { name: "Support conversations" });
  await expect(list.locator("tbody tr").first()).toContainText("escalated");
  await expect(list.getByText("How do I move a token entry").filter({ visible: true }).first()).toBeVisible();
  await list.getByText("Enterprise plan for 220 people").filter({ visible: true }).first().click();
  await expect(page.getByText("Escalated: Asks for a price and a call")).toBeVisible();
  await expect(page.getByLabel("Answer")).toHaveValue(/Northwind Studio fits it/);
  await page.getByRole("button", { name: "Send reply" }).click();
  const messages = page.getByRole("list", { name: "Messages" });
  await expect(messages.getByText("Token Controller support")).toBeVisible();
  await expect(messages.getByText("waiting to be sent")).toBeVisible();
});
