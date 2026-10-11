---
title: "Azure DevOps"
description: "Sync tickets from Azure DevOps."
---

Sync tickets from Azure DevOps, with their titles, descriptions, parents and assignees.

## Connect {#connect}

In the web app, open _Settings_, then _Ticket systems_, and add an API token from Azure DevOps. No Azure DevOps sign-in is needed. Whoever connects a ticket system answers for that data flow.

## Sync {#sync}

Tickets sync every 15 minutes, or on demand. Map each Azure DevOps project to a Token Controller project; the ticket key pattern follows from it. See [Projects and tickets](/account/projects#tickets).
