# Support inbox, site assistant and newsletter live in the service, off by default

Superseded by ADR 0020: customer relations and billing moved to the private repo, and this repo no longer has the `customer_relations` module.

The hosted service runs its own support inbox, site assistant and newsletter in the `customer_relations` module, instead of outside tools. Each part is off until its settings are set (docs page Self-hosting > Configuration), so a self-hosted instance runs none of it and still needs only Postgres and SMTP (ADR 0002).

## Rules that follow

- Answers come from the docs only. The service reads them as one text file, `/llms-full.txt` on the docs site, built from the same Markdown as the pages. A docs change reaches the answers within ten minutes, with no new release.
- Claude writes the answers, through the Claude API with the docs as a cached system block and the answer constrained to a JSON shape. What the docs cannot answer, a refusal, or a failing API goes to the operators with a proposed answer. Every answer says it was written by an AI assistant.
- Notices to operators carry a link to the conversation, never its content.
- Support answers and notices go over SMTP like all service mail. The newsletter goes through the Resend API from its own subdomain, never over SMTP, so a complaint about it cannot block a sign-in link.
- Confirm and unsubscribe links in mail point at website pages with one button, so mail scanners that open every link confirm and unsubscribe nobody. Mail clients unsubscribe in one click through `List-Unsubscribe-Post` (RFC 8058).
- The background loop runs every minute, and at once when mail arrives, under one Postgres advisory lock, so with several replicas no mail goes out twice.
- The same loop deletes site assistant questions, raw support mail and feedback screenshots after 90 days, support conversations two years after their last message, and feedback reports two years after their last change, so the privacy policy's periods hold without anyone doing it by hand.
- Feedback reports from the web app's feedback button live in the same module and are always on. They go to the operators, or to the organization's controllers on an instance without operators. Diagnostics take only the fields the Security page lists; a report with any other field is refused.
- The admin area (support, site assistant questions, newsletter issues, feedback reports, one record per organization) answers 404 to everyone but the operators, so it does not show that it exists.
- Onboarding emails go to a new organization's controllers when it stalls at one of its four first steps (first token entry, token sheet, bill, close). The service sends at most one per step, records it before sending, and stops after the first close or 30 days without activity, so they never become a series. The `onboarding-emails` job in the job runner (ADR 0002) checks every hour.

## Why

Customer relations runs on almost no outside tools: one service and one database already hold the customers, and the support inbox has to link each conversation to its organization. Keeping message content in our own database, in the EU, keeps it with processors the privacy policy lists.
