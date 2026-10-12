import type { Person, Role } from "@/api/contract";

// Roles stack: each includes everything the one before it can do (docs: Roles).
const rank: Record<Role, number> = { agent_manager: 1, people_manager: 2, controller: 3 };

export function hasRole(person: Pick<Person, "role">, role: Role): boolean {
  return rank[person.role] >= rank[role];
}

export const roleLabel: Record<Role, string> = {
  agent_manager: "Agent manager",
  people_manager: "People manager",
  controller: "Controller",
};
