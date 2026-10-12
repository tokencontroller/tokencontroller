import { QueryClient, useQuery } from "@tanstack/react-query";
import type { Account, Billing } from "@/api/contract";
import { api } from "@/api/endpoints";
import { ApiError } from "@/api/client";
import { displayName } from "@/lib/format";
import { hasRole } from "@/lib/roles";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: (count, error) => !(error instanceof ApiError && error.status < 500) && count < 2,
      refetchOnWindowFocus: false,
    },
  },
});

/** Who is signed in and their organization's settings, read before any page. */
export const meQuery = {
  queryKey: ["me"],
  queryFn: async (): Promise<Account> => {
    const me = await api.me();
    return { person: me.person, organization: await api.organization() };
  },
};

export function useMe(): Account {
  const { data } = useQuery(meQuery);
  if (!data) throw new Error("useMe is only used inside the signed-in app");
  return data;
}

/**
 * The plan and member count, or null where billing is off (self-hosted).
 * `untilTeam` asks again every few seconds until the Team plan shows.
 */
export function useBilling({ untilTeam = false }: { untilTeam?: boolean } = {}) {
  return useQuery({
    queryKey: ["billing"],
    queryFn: (): Promise<Billing | null> => unlessMissing(api.billing),
    staleTime: 5 * 60_000,
    refetchInterval: (query) => (untilTeam && query.state.data?.plan === "solo" ? 3000 : false),
  });
}

/** Runs a call and turns a 404 into null: the service's way to say "not here". */
async function unlessMissing<T>(load: () => Promise<T>): Promise<T | null> {
  try {
    return await load();
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return null;
    throw error;
  }
}

/** The admin area's counts, or null for everyone who is not an operator. */
export function useAdmin() {
  return useQuery({ queryKey: ["admin"], queryFn: () => unlessMissing(api.adminOverview), staleTime: 60_000 });
}

/** Where support answers, or null where support is off (most self-hosted instances). */
export function useSupport() {
  return useQuery({ queryKey: ["support"], queryFn: () => unlessMissing(api.support), staleTime: 5 * 60_000 });
}

export function usePeople() {
  return useQuery({ queryKey: ["members"], queryFn: api.members });
}

/**
 * The display name of a person, by id or by email. A token entry's `person`
 * tag holds the email; agent managers and assignees are ids.
 */
export function usePersonName(): (person: string | null | undefined) => string {
  const { data } = usePeople();
  return (person) => {
    if (!person) return "nobody";
    const found = data?.find((p) => p.id === person || p.email === person);
    return found ? displayName(found) : person;
  };
}

export function useTagNames() {
  return useQuery({ queryKey: ["tag-names"], queryFn: api.tagNames });
}

/** The values a tag name has, most used first. */
export function useTagValues(name: string) {
  return useQuery({ queryKey: ["tag-values", name], queryFn: () => api.tagValues(name), enabled: Boolean(name) });
}

export function useProjects() {
  return useQuery({ queryKey: ["projects"], queryFn: () => api.projects() });
}

export function useTickets() {
  return useQuery({ queryKey: ["tickets"], queryFn: () => api.tickets() });
}

/** Every period up to this month, newest first. Only people managers and controllers may list them. */
export function usePeriods() {
  const { data: me } = useQuery(meQuery);
  return useQuery({ queryKey: ["periods"], queryFn: api.periods, enabled: me ? hasRole(me.person, "people_manager") : false });
}

/** The oldest period still open: the one the next close is about. */
export function useFirstOpenPeriod(): string | undefined {
  const periods = usePeriods();
  return [...(periods.data ?? [])].reverse().find((p) => !p.closed)?.period;
}

/** Everything that changes after a write: the simplest correct refresh. */
export function refreshAll(): Promise<void> {
  return queryClient.invalidateQueries();
}
