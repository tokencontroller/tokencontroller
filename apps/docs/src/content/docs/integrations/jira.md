---
title: "Jira Cloud"
description: "Sync tickets from Jira Cloud."
---

Sync tickets from Jira Cloud, with their titles, descriptions, parents and assignees.

## Connect {#connect}

In the web app, open _Settings_, then _Ticket systems_, and add an API token from Jira Cloud with the email of the Atlassian account it belongs to. No Jira Cloud sign-in is needed. Whoever connects a ticket system answers for that data flow.

## Sync {#sync}

Tickets sync every 15 minutes, or on demand. Map each Jira Cloud project to a Token Controller project; the ticket key pattern follows from it. See [Projects and tickets](/account/projects#tickets).
