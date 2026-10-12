// The additions slot. A private build puts its own folder here in place of
// this one; the open build adds nothing (README.md in this folder).

import type { Additions } from "@/lib/additions";

export const additions = {
  routes: () => [],
  useMenu: () => [],
  parts: {},
  onResponse: () => undefined,
  onErrorShown: () => undefined,
} satisfies Additions;
