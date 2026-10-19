import { QueryClient, useQuery } from "@tanstack/react-query";
import type { Account } from "@/api/contract";
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
    return { person: me.person, credential: me.credential, organization: await api.organization() };
  },
};

export function useMe(): Account {
  const { data } = useQuery(meQuery);
  if (!data) throw new Error("useMe is only used inside the signed-in app");
  return data;
}

/** Runs a call and turns a 404 into null: for a build's additions. */
export async function unlessMissing<T>(load: () => Promise<T>): Promise<T | null> {
  try {
    return await load();
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) return null;
    throw error;
  }
}

export function useMemberLimit() {
  return useQuery({ queryKey: ["member-limit"], queryFn: api.memberLimit });
}

export function usePeople() {
  return useQuery({ queryKey: ["members"], queryFn: api.members });
}

/** The display name of a person, by id or by email. */
export function usePersonName(): (person: string | null | undefined) => string {
  const { data } = usePeople();
  return (person) => {
    if (!person) return "nobody";
    const found = data?.find((p) => p.id === person || p.email === person);
    return found ? displayName(found) : person;
  };
}

export function useTags() {
  return useQuery({ queryKey: ["tags"], queryFn: api.tags });
}

export function useAttributes() {
  return useQuery({ queryKey: ["attributes"], queryFn: api.attributes });
}

/** The keys of one tag or attribute name, for an input's suggestions. */
export function useKeys(name: string): string[] {
  const tags = useTags();
  const attributes = useAttributes();
  const all = [...(tags.data ?? []), ...(attributes.data ?? [])];
  return all.find((n) => n.name === name)?.values.map((v) => v.key) ?? [];
}

/** Everything that changes after a write: the simplest correct refresh. */
export function refreshAll(): Promise<void> {
  return queryClient.invalidateQueries();
}
