---
title: "CI agents"
description: "How cost from agents that run without a person is placed and who answers for it."
---

Some agents run without a person: a PR review bot, a nightly dependency update, a cloud agent. Their cost is placed the same way, with one difference: a person still answers for it.

## Who answers for it {#who}

The agent manager of a CI agent is the person who set it up, usually a tech lead. It is the person who created the service token the agent reports with.

## Where it lands {#where}

- A ticket given to the GitHub Action, or set by an earlier step of the job, wins over everything else.
- A review of a pull request lands on the ticket of that pull request.
- CI work that matches no ticket lands on the project.
- The [GitHub Action](/collect/github-action) names its project, and that wins over matching by repository.

<div class="note"><b>Example.</b> A review bot reviews a pull request for KD-214. The cost shows on KD-214 as review cost, and sits with the tech lead who runs the bot. The ticket's assignee is not charged for it.</div>

## No token sheet {#automatic}

CI agents report automatically. Their token entries arrive submitted and never go on a token sheet.
