import { useMutation } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useState, type ReactNode } from "react";
import type { Billing, Interval } from "@/api/contract";
import { api } from "@/api/endpoints";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ErrorBox, Loading, Note, PageHeader, Section } from "@/components/ui/display";
import { Field, FormError, Select } from "@/components/ui/form";
import { formatDate } from "@/lib/format";
import { useBilling } from "@/lib/queries";

const billingRoute = getRouteApi("/app/billing");

/** EU member states pay in EUR (docs: Plans and limits > Payment and currency). */
const EU = new Set(["AT", "BE", "BG", "CY", "CZ", "DE", "DK", "EE", "ES", "FI", "FR", "GR", "HR", "HU", "IE", "IT", "LT", "LU", "LV", "MT", "NL", "PL", "PT", "RO", "SE", "SI", "SK"]);

/** Region codes that are not countries. */
const NOT_COUNTRIES = new Set(["EU", "EZ", "UN", "QO", "XA", "XB", "ZZ"]);

/** Every country the browser has a name for, by name. */
function countries(): { code: string; name: string }[] {
  const names = new Intl.DisplayNames(["en"], { type: "region", fallback: "none" });
  const list: { code: string; name: string }[] = [];
  for (let a = 65; a <= 90; a++) {
    for (let b = 65; b <= 90; b++) {
      const code = String.fromCharCode(a, b);
      const name = NOT_COUNTRIES.has(code) ? undefined : names.of(code);
      if (name) list.push({ code, name });
    }
  }
  return list.sort((x, y) => x.name.localeCompare(y.name));
}

/** The country in the browser's language setting, such as AT for de-AT. */
function browserCountry(): string {
  const region = navigator.language.split("-")[1];
  return region && /^[A-Z]{2}$/.test(region) ? region : "";
}

/** The Team plan price for a country (docs: Plans and limits). */
export function teamPrice(country: string, interval: Interval): string {
  const currency = EU.has(country) ? "EUR" : "USD";
  return interval === "year" ? `2,000 ${currency} a year` : `200 ${currency} a month`;
}

function ChooseTeamPlan() {
  const [interval, setInterval] = useState<Interval>("month");
  const [country, setCountry] = useState(browserCountry);
  const [list] = useState(countries);
  const checkout = useMutation({ mutationFn: () => api.checkout(interval, country), onSuccess: ({ url }) => window.location.assign(url) });
  return (
    <form className="rounded-lg border border-line p-4" onSubmit={(e) => { e.preventDefault(); checkout.mutate(); }}>
      <div className="text-sm font-medium">Team plan</div>
      <p className="mt-0.5 text-[13px] text-muted">Up to 150 members. Roles, review, approval and reject, monthly close, ticket sync, hosting in the EU.</p>
      <div className="mt-4 grid gap-4 sm:grid-cols-2">
        <Field label="Billing country" hint="Customers in the EU pay in EUR, everyone else in USD.">
          <Select required value={country} onChange={(e) => setCountry(e.target.value)}>
            <option value="">Pick a country</option>
            {list.map((c) => (
              <option key={c.code} value={c.code}>
                {c.name}
              </option>
            ))}
          </Select>
        </Field>
        <Field label="Billed">
          <Select value={interval} onChange={(e) => setInterval(e.target.value as Interval)}>
            <option value="month">Monthly</option>
            <option value="year">Yearly</option>
          </Select>
        </Field>
      </div>
      <div className="mt-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div className="text-sm" data-testid="team-price">
          {country ? <span className="font-medium">{teamPrice(country, interval)}</span> : <span className="text-muted">Pick a country to see the price.</span>}
          <span className="text-muted"> per organization, paid by card through Stripe.</span>
        </div>
        <Button type="submit" variant="primary" disabled={!country || checkout.isPending}>
          Continue to payment
        </Button>
      </div>
      <FormError error={checkout.error} />
    </form>
  );
}

function TeamPlan({ billing }: { billing: Billing }) {
  const portal = useMutation({ mutationFn: api.billingPortal, onSuccess: ({ url }) => window.location.assign(url) });
  const end = billing.current_period_end ? formatDate(billing.current_period_end) : null;
  return (
    <div className="rounded-lg border border-line p-4">
      {billing.status === "past_due" || billing.status === "unpaid" ? <Note tone="exception">The last payment failed. Update the card under payment and invoices.</Note> : null}
      <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <div className="flex items-center gap-2 text-sm font-medium">
            Team plan <Badge tone="posted">{billing.interval === "year" ? "Yearly" : "Monthly"}</Badge>
          </div>
          <div className="mt-0.5 text-[13px] text-muted">
            {billing.members} of {billing.member_limit} members.
            {end ? (billing.cancel_at_period_end ? ` Ends on ${end}.` : ` Renews on ${end}.`) : null}
          </div>
        </div>
        <Button onClick={() => portal.mutate()} disabled={portal.isPending}>
          Payment and invoices
        </Button>
      </div>
      <FormError error={portal.error} />
    </div>
  );
}

export function BillingPage(): ReactNode {
  const { checkout } = billingRoute.useSearch();
  // Stripe confirms the payment a moment after it sends people back here.
  const billing = useBilling({ untilTeam: checkout === "success" });
  const waiting = checkout === "success" && billing.data?.plan === "solo";
  return (
    <>
      <PageHeader title="Billing" lead="Your plan, and payment through Stripe." />
      {billing.isPending ? (
        <Loading />
      ) : billing.isError ? (
        <ErrorBox error={billing.error} />
      ) : !billing.data ? (
        <Note>Billing is off on this instance. A self-hosted instance has every Team plan feature, free.</Note>
      ) : (
        <>
          {checkout === "success" ? <Note tone="posted">{waiting ? "Payment received. The Team plan switches on as soon as Stripe confirms it." : "Thank you. The Team plan is on."}</Note> : null}
          <Section title="Plan">
            {billing.data.plan === "team" ? (
              <TeamPlan billing={billing.data} />
            ) : (
              <>
                <div className="mb-3 rounded-lg border border-line p-4 text-sm">
                  <span className="font-medium">Solo</span>
                  <span className="text-muted"> · free forever for one person, every feature. No card needed.</span>
                </div>
                <ChooseTeamPlan />
              </>
            )}
          </Section>
          <p className="text-[13px] text-muted">
            More than 150 members, an isolated instance, or OIDC and SCIM: see the{" "}
            <a className="underline" href="https://docs.tokencontroller.com/plans" target="_blank" rel="noreferrer">
              Enterprise plan
            </a>
            .
          </p>
        </>
      )}
    </>
  );
}
