import { QueryClient, useQuery } from "@tanstack/react-query";
import type { Account, Billing } from "@/api/contract";
import { api } from "@/api/endpoints";
import { ApiError } from "@/api/client";
import { displayName } from "@/lib/format";

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
    queryFn: async (): Promise<Billing | null> => {
      try {
        return await api.billing();
      } catch (error) {
        if (error instanceof ApiError && error.status === 404) return null;
        throw error;
      }
    },
    staleTime: 5 * 60_000,
    refetchInterval: (query) => (untilTeam && query.state.data?.plan === "solo" ? 3000 : false),
  });
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

export function usePeriods() {
  return useQuery({ queryKey: ["periods"], queryFn: api.periods });
}

/** The oldest period still open: the one the next close is about. */
export function useFirstOpenPeriod(): string | undefined {
  const periods = usePeriods();
  return [...(periods.data ?? [])].reverse().find((p) => p.status === "open")?.id;
}

/** Everything that changes after a write: the simplest correct refresh. */
export function refreshAll(): Promise<void> {
  return queryClient.invalidateQueries();
}
