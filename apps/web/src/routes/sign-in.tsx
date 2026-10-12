import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useEffect, useRef, useState, type SubmitEvent, type ReactNode } from "react";
import type { CostSource } from "@/api/contract";
import { call } from "@/api/client";
import { api } from "@/api/endpoints";
import { Button } from "@/components/ui/button";
import { Field, FormError, Input, Select } from "@/components/ui/form";

const signInRoute = getRouteApi("/sign-in");
const signInVerifyRoute = getRouteApi("/sign-in/verify");

function Frame({ title, lead, children }: { title: string; lead?: ReactNode; children: ReactNode }) {
  return (
    <div className="flex min-h-dvh flex-col items-center justify-center bg-sheet px-4 py-10">
      <div className="w-full max-w-sm">
        <div className="mb-8 flex justify-center">
          <img src="/logo-light.svg" width="56" height="56" alt="Token Controller" className="logo-light size-14" />
          <img src="/logo-dark-small.svg" width="56" height="56" alt="Token Controller" className="logo-dark size-14" />
        </div>
        <div className="rounded-lg bg-page p-6 shadow-raised">
          <h1 className="text-lg font-semibold">{title}</h1>
          {lead ? <p className="mt-1 text-sm text-muted">{lead}</p> : null}
          <div className="mt-5">{children}</div>
        </div>
      </div>
    </div>
  );
}

/** Only a path on this site, so a link can never send someone elsewhere. */
export function safeNext(next: string | undefined): string {
  return next?.startsWith("/") && !next.startsWith("//") ? next : "/";
}

/** In mock mode there is no inbox, so the page offers the link it would have sent. */
function MockLink() {
  const { data } = useQuery({
    queryKey: ["mock-link"],
    queryFn: () => call<{ token: string; email: string; next?: string } | null>("GET", "/__mock/last-link"),
    enabled: Boolean(import.meta.env.VITE_API_MOCK),
  });
  if (!import.meta.env.VITE_API_MOCK || !data) return null;
  return (
    <div className="mt-4 rounded-md border border-dashed border-line-strong p-3 text-xs text-muted">
      Example data: no email is sent.{" "}
      <Link to="/sign-in/verify" search={{ token: data.token, ...(data.next ? { next: data.next } : {}) }} className="font-medium text-accent underline">
        Open the sign-in link
      </Link>
    </div>
  );
}

export function SignInPage(): ReactNode {
  const { sent, next } = signInRoute.useSearch();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const signIn = useMutation({
    mutationFn: () => api.signIn(email, next),
    onSuccess: () => navigate({ to: "/sign-in", search: { sent: email, ...(next ? { next } : {}) } }),
  });
  if (sent) {
    return (
      <Frame title="Check your email" lead={<>We sent a sign-in link to <b className="text-ink">{sent}</b>. It works once, for 15 minutes.</>}>
        <Link to="/sign-in" className="text-sm text-accent hover:underline">
          Use another email
        </Link>
        <MockLink />
      </Frame>
    );
  }
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    signIn.mutate();
  };
  return (
    <Frame title="Sign in" lead="We email you a link. No password.">
      <form onSubmit={submit} className="space-y-4">
        <Field label="Work email">
          <Input type="email" required autoComplete="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder="you@company.com" />
        </Field>
        <FormError error={signIn.error} />
        <Button type="submit" variant="primary" className="w-full" disabled={signIn.isPending}>
          Email me a link
        </Button>
      </form>
      <p className="mt-5 text-center text-sm text-muted">
        New to Token Controller?{" "}
        <Link to="/signup" className="font-medium text-accent hover:underline">
          Create an organization
        </Link>
      </p>
    </Frame>
  );
}

export function VerifyPage(): ReactNode {
  const { token, next } = signInVerifyRoute.useSearch();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const started = useRef(false);
  const verify = useMutation({
    mutationFn: () => api.verify(token),
    onSuccess: async (me) => {
      queryClient.clear();
      queryClient.setQueryData(["me"], me);
      await navigate({ href: safeNext(next) });
    },
  });
  useEffect(() => {
    if (started.current) return;
    started.current = true;
    verify.mutate();
  }, [verify]);
  return (
    <Frame title={verify.isError ? "This link did not work" : "Signing you in"}>
      {verify.isError ? (
        <>
          <FormError error={verify.error} />
          <Link to="/sign-in" className="mt-4 inline-block text-sm text-accent hover:underline">
            Get a new link
          </Link>
        </>
      ) : (
        <p className="text-sm text-muted">One moment.</p>
      )}
    </Frame>
  );
}

const costSources: { value: CostSource; label: string }[] = [
  { value: "console_api_key", label: "Console or API key: usage billed per token" },
  { value: "enterprise_api_rates", label: "Enterprise plan with usage at API rates" },
  { value: "seat_plan", label: "Seats: Claude Team, Max or Pro" },
  { value: "cloud_marketplace", label: "Cloud marketplace: Bedrock, Vertex, Foundry" },
  { value: "none", label: "None yet" },
];

export function SignUpPage(): ReactNode {
  const navigate = useNavigate();
  const [form, setForm] = useState({ organization: "", name: "", email: "", manages: "people" as "agents" | "people", cost_source: "console_api_key" as CostSource, reporting_currency: "USD" });
  const set = <K extends keyof typeof form>(key: K, value: (typeof form)[K]) => setForm((f) => ({ ...f, [key]: value }));
  const signUp = useMutation({
    mutationFn: () => api.signUp(form),
    onSuccess: () => navigate({ to: "/sign-in", search: { sent: form.email } }),
  });
  return (
    <Frame title="Create your organization" lead="Solo is free forever for one person. No card needed.">
      <form
        onSubmit={(e) => {
          e.preventDefault();
          signUp.mutate();
        }}
        className="space-y-4"
      >
        <Field label="Organization">
          <Input required value={form.organization} onChange={(e) => set("organization", e.target.value)} placeholder="Kestrel Digital" />
        </Field>
        <Field label="Your name">
          <Input required autoComplete="name" value={form.name} onChange={(e) => set("name", e.target.value)} />
        </Field>
        <Field label="Work email">
          <Input type="email" required autoComplete="email" value={form.email} onChange={(e) => set("email", e.target.value)} />
        </Field>
        <fieldset>
          <legend className="mb-1 text-[13px] font-medium text-ink-soft">Do you manage agents, or do you manage people?</legend>
          <div className="grid grid-cols-2 gap-2">
            {(["agents", "people"] as const).map((value) => (
              <label key={value} className={`flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2 text-sm ${form.manages === value ? "border-ink bg-sheet" : "border-line-strong"}`}>
                <input type="radio" name="manages" value={value} checked={form.manages === value} onChange={() => set("manages", value)} className="accent-[var(--tc-ink)]" />
                I manage {value}
              </label>
            ))}
          </div>
        </fieldset>
        <Field label="Cost source" hint="How your provider charges you. It decides what the close checks against.">
          <Select value={form.cost_source} onChange={(e) => set("cost_source", e.target.value as CostSource)}>
            {costSources.map((c) => (
              <option key={c.value} value={c.value}>
                {c.label}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Reporting currency" hint="The one currency your organization computes in.">
          <Select value={form.reporting_currency} onChange={(e) => set("reporting_currency", e.target.value)}>
            {["USD", "EUR", "GBP", "CHF"].map((c) => (
              <option key={c}>{c}</option>
            ))}
          </Select>
        </Field>
        <FormError error={signUp.error} />
        <Button type="submit" variant="primary" className="w-full" disabled={signUp.isPending}>
          Create organization
        </Button>
      </form>
      <p className="mt-5 text-center text-sm text-muted">
        Already a member?{" "}
        <Link to="/sign-in" className="font-medium text-accent hover:underline">
          Sign in
        </Link>
      </p>
    </Frame>
  );
}

export { costSources };
