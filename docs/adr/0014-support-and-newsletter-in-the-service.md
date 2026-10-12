# Support inbox, site assistant and newsletter live in the service, off by default

The hosted service runs its own support inbox, site assistant and newsletter in the `customer_relations` module, instead of outside tools. Each part is off until its settings are set (docs page Self-hosting > Configuration), so a self-hosted instance runs none of it and still needs only Postgres and SMTP (ADR 0002).

## Rules that follow

- Answers come from the docs only. The service reads them as one text file, `/llms-full.txt` on the docs site, built from the same Markdown as the pages. A docs change reaches the answers within ten minutes, with no new release.
- Claude writes the answers, through the Claude API with the docs as a cached system block and the answer constrained to a JSON shape. What the docs cannot answer, a refusal, or a failing API goes to the operators with a proposed answer. Every answer says it was written by an AI assistant.
- Notices to operators carry a link to the conversation, never its content.
- Support answers and notices go over SMTP like all service mail. The newsletter goes through the Resend API from its own subdomain, never over SMTP, so a complaint about it cannot block a sign-in link.
- Confirm and unsubscribe links in mail point at website pages with one button, so mail scanners that open every link confirm and unsubscribe nobody. Mail clients unsubscribe in one click through `List-Unsubscribe-Post` (RFC 8058).
- The background loop runs every minute, and at once when mail arrives, under one Postgres advisory lock, so with several replicas no mail goes out twice.
- The same loop deletes site assistant questions and raw support mail after 90 days, and support conversations two years after their last message, so the privacy policy's periods hold without anyone doing it by hand.

## Why

Customer relations runs on almost no outside tools: one service and one database already hold the customers, and the support inbox has to link each conversation to its organization. Keeping message content in our own database, in the EU, keeps it with processors the privacy policy lists.
