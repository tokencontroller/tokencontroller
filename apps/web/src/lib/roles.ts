import type { Member, Role } from "@/api/contract";

// Roles are bundles of grants and stack: each holds what the one before it
// holds (docs: Grants and roles).
const rank: Record<Role, number> = { agent_manager: 1, people_manager: 2, org_manager: 3 };

export function hasRole(person: Pick<Member, "role">, role: Role): boolean {
  return rank[person.role] >= rank[role];
}

export const roleLabel: Record<Role, string> = {
  agent_manager: "Agent manager",
  people_manager: "People manager",
  org_manager: "Org manager",
};
