import { useMutation, useQuery } from "@tanstack/react-query";
import { getRouteApi, Link, useNavigate } from "@tanstack/react-router";
import { useState, type ReactNode, type SubmitEvent } from "react";
import type { CostSource } from "@/api/contract";
import { API_PREFIX, call } from "@/api/client";
import { api } from "@/api/endpoints";
import { Button, buttonClass } from "@/components/ui/button";
import { ErrorBox, Loading } from "@/components/ui/display";
import { Field, FormError, Input, Select } from "@/components/ui/form";
import { costSourceOrder, costSourceText } from "@/lib/cost-source";
import { rememberSignUp, type SignUpChoices } from "@/lib/remember";

const signInRoute = getRouteApi("/sign-in");
const inviteRoute = getRouteApi("/i/$code");

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

/**
 * In mock mode there is no inbox, so the page offers the link it would have
 * sent. Like the service's link page, its button signs the browser in and
 * opens the start page.
 */
function MockLink() {
  const { data } = useQuery({
    queryKey: ["mock-link"],
    queryFn: () => call<{ token: string; email: string } | null>("GET", "/__mock/last-link"),
    enabled: Boolean(import.meta.env.VITE_API_MOCK),
  });
  if (!import.meta.env.VITE_API_MOCK || !data) return null;
  const open = async () => {
    // The service answers with a redirect to the start page; the browser goes there itself.
    await fetch(`${API_PREFIX}/sign-in/link`, { method: "POST", body: new URLSearchParams({ token: data.token }), redirect: "manual", credentials: "include" });
    window.location.assign("/");
  };
  return (
    <div className="mt-4 rounded-md border border-dashed border-line-strong p-3 text-xs text-muted">
      Example data: no email is sent.{" "}
      <button type="button" onClick={() => void open()} className="font-medium text-accent underline">
        Open the sign-in link
      </button>
    </div>
  );
}

function CheckYourEmail({ email }: { email: string }) {
  return (
    <Frame title="Check your email" lead={<>We sent a sign-in link to <b className="text-ink">{email}</b>. It works once, for 15 minutes.</>}>
      <Link to="/sign-in" className="text-sm text-accent hover:underline">
        Use another email
      </Link>
      <MockLink />
    </Frame>
  );
}

/**
 * Other ways to sign in, such as the Enterprise plan's identity provider.
 * The open edition has none. Each button is a plain link: the service takes
 * it from there and comes back to the start page.
 */
function Providers() {
  const providers = useQuery({ queryKey: ["sign-in-providers"], queryFn: api.signInProviders, staleTime: Infinity, retry: false });
  if (!providers.data?.length) return null;
  return (
    <div className="mt-4 space-y-2">
      <p className="text-center text-xs text-muted">or</p>
      {providers.data.map((p) => (
        <a key={p.id} href={p.url} className={buttonClass("secondary", "md", "w-full")}>
          {p.label}
        </a>
      ))}
    </div>
  );
}

export function SignInPage(): ReactNode {
  const { sent } = signInRoute.useSearch();
  const navigate = useNavigate();
  const [email, setEmail] = useState("");
  const signIn = useMutation({
    mutationFn: () => api.signIn(email),
    onSuccess: () => navigate({ to: "/sign-in", search: { sent: email } }),
  });
  if (sent) return <CheckYourEmail email={sent} />;
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
      <Providers />
      <p className="mt-5 text-center text-sm text-muted">
        New to Token Controller?{" "}
        <Link to="/signup" className="font-medium text-accent hover:underline">
          Create an organization
        </Link>
      </p>
    </Frame>
  );
}

/** The link in an invite mail: the organization and the address it was sent to. */
export function InvitePage(): ReactNode {
  const { code } = inviteRoute.useParams();
  const navigate = useNavigate();
  const invite = useQuery({ queryKey: ["invite", code], queryFn: () => api.invite(code), retry: false });
  const signIn = useMutation({
    mutationFn: (email: string) => api.signIn(email),
    onSuccess: (_, email) => navigate({ to: "/sign-in", search: { sent: email } }),
  });
  if (invite.isPending) {
    return (
      <Frame title="Your invite">
        <Loading />
      </Frame>
    );
  }
  if (invite.isError) {
    return (
      <Frame title="This invite does not work">
        <ErrorBox error={invite.error} />
        <p className="mt-4 text-sm text-muted">Ask whoever invited you for a new link, or sign in if you joined already.</p>
        <Link to="/sign-in" className="mt-3 inline-block text-sm text-accent hover:underline">
          Sign in
        </Link>
      </Frame>
    );
  }
  return (
    <Frame title={`Join ${invite.data.organization_name}`} lead={<>You were invited as <b className="text-ink">{invite.data.email}</b>. We email you a sign-in link; no password.</>}>
      <FormError error={signIn.error} />
      <Button variant="primary" className="w-full" disabled={signIn.isPending} onClick={() => signIn.mutate(invite.data.email)}>
        Email me a sign-in link
      </Button>
    </Frame>
  );
}

const currencies = ["USD", "EUR", "GBP", "CHF"];

export function SignUpPage(): ReactNode {
  const navigate = useNavigate();
  const [form, setForm] = useState({ organization: "", email: "", name: "" });
  const [choices, setChoices] = useState<SignUpChoices>({ manages: "people", cost_source: "usage_at_api_rates", reporting_currency: "USD" });
  const signUp = useMutation({
    mutationFn: () => api.signUp(form.email.trim(), form.organization.trim(), form.name.trim()),
    onSuccess: async () => {
      // The service takes the names and the email; the rest is set once signed in.
      rememberSignUp(choices);
      await navigate({ to: "/sign-in", search: { sent: form.email.trim() } });
    },
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
          <Input required value={form.organization} onChange={(e) => setForm((f) => ({ ...f, organization: e.target.value }))} placeholder="Kestrel Digital" />
        </Field>
        <Field label="Work email">
          <Input type="email" required autoComplete="email" value={form.email} onChange={(e) => setForm((f) => ({ ...f, email: e.target.value }))} />
        </Field>
        <Field label="Your name" hint="As your colleagues will see it. You can also add it later.">
          <Input autoComplete="name" value={form.name} onChange={(e) => setForm((f) => ({ ...f, name: e.target.value }))} />
        </Field>
        <fieldset>
          <legend className="mb-1 text-[13px] font-medium text-ink-soft">Do you manage agents, or do you manage people?</legend>
          <div className="grid grid-cols-2 gap-2">
            {(["agents", "people"] as const).map((value) => (
              <label key={value} className={`flex cursor-pointer items-center gap-2 rounded-md border px-3 py-2 text-sm ${choices.manages === value ? "border-ink bg-sheet" : "border-line-strong"}`}>
                <input type="radio" name="manages" value={value} checked={choices.manages === value} onChange={() => setChoices((c) => ({ ...c, manages: value }))} className="accent-[var(--tc-ink)]" />
                I manage {value}
              </label>
            ))}
          </div>
        </fieldset>
        <CostSourceField value={choices.cost_source} onChange={(cost_source) => setChoices((c) => ({ ...c, cost_source }))} />
        <Field label="Reporting currency" hint="The one currency your organization computes in.">
          <Select value={choices.reporting_currency} onChange={(e) => setChoices((c) => ({ ...c, reporting_currency: e.target.value }))}>
            {currencies.map((c) => (
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

/** How the provider charges: it decides what the close reconciles against. */
export function CostSourceField({ value, onChange, disabled }: { value: CostSource | null; onChange: (value: CostSource | null) => void; disabled?: boolean }) {
  return (
    <Field label="Cost source" hint="How your provider charges you. It decides what the close checks against.">
      <Select disabled={disabled} value={value ?? ""} onChange={(e) => onChange((e.target.value || null) as CostSource | null)}>
        {costSourceOrder.map((source) => (
          <option key={source} value={source}>
            {costSourceText[source].name}
          </option>
        ))}
        <option value="">None yet</option>
      </Select>
    </Field>
  );
}

export { currencies };
