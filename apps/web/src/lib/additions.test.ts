import { describe, expect, test } from "vitest";
import { joinMenu, type MenuEntry } from "./additions";

// A build's menu entries join the menu where they say. What the open build's
// slot adds is checked in src/additions/, which a private build replaces.
describe("additions", () => {
  test("menu entries join their group by title, and a new title starts a group after the others", () => {
    const own = [
      { title: "", items: ["Overview"] },
      { title: "Organization", items: ["People", "Settings"] },
      { title: "Help", items: [] },
    ];
    const plan: MenuEntry = { group: "Organization", label: "Plan", role: "controller", to: "/plan" };
    const help: MenuEntry = { group: "Help", label: "Ask us", role: "agent_manager", Panel: () => null };
    const status: MenuEntry = { group: "Service", label: "Status", role: "agent_manager", to: "/status" };
    const menu = joinMenu(own, [status, plan, help]);
    expect(menu.map((group) => group.title)).toEqual(["", "Organization", "Help", "Service"]);
    expect(menu[1]).toEqual({ title: "Organization", items: ["People", "Settings"], added: [plan] });
    expect(menu[2]?.added).toEqual([help]);
    expect(menu[3]).toEqual({ title: "Service", items: [], added: [status] });
    expect(joinMenu(own, [])).toEqual(own.map((group) => ({ ...group, added: [] })));
  });
});
