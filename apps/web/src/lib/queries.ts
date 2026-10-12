import { QueryClient, useQuery } from "@tanstack/react-query";
import { api } from "@/api/endpoints";
import { ApiError } from "@/api/client";

export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 30_000,
      retry: (count, error) => !(error instanceof ApiError && error.status < 500) && count < 2,
      refetchOnWindowFocus: false,
    },
  },
});

export const meQuery = { queryKey: ["me"], queryFn: api.me };

export function useMe() {
  const { data } = useQuery(meQuery);
  if (!data) throw new Error("useMe is only used inside the signed-in app");
  return data;
}

export function usePeople() {
  return useQuery({ queryKey: ["members"], queryFn: api.members });
}

/** The display name of a person id, falling back to the id. */
export function usePersonName(): (id: string | null) => string {
  const { data } = usePeople();
  return (id) => {
    if (id === null) return "nobody";
    return data?.find((p) => p.id === id)?.name ?? id;
  };
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
