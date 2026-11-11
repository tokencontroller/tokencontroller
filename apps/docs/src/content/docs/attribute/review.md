---
title: "Check and submit"
description: "Go through your review list: sessions since your last submit, the exceptions to fix, the contract question, then submit."
---

Before you submit, you may want to see what your agents did since your last submit and whether each session landed where it belongs. Your review list shows every session since then and how it was split, in the CLI, the Claude Code mod's pane or the web app under _Check and submit_.

## The review list {#review}

```text
tc review
  #  when    cost     tags                              by
  1  09:12   $41.87   feature KD-214                    rule: key in branch
  2  11:46   $12.10   feature KD-214                    rule: key in branch
  3  13:05   $88.30   feature KD-231                    rule: key in PR title
  4  15:20    $9.75   project shop                      rule: repository
  5  16:41   $60.38   no project                        no rule matched
```

Session 5 is organization overhead with your name on it until you assign it.

## Fix what is wrong {#fix}

```sh
tc tag 5 feature=KD-231             # put it on a feature
tc tag 4 purpose=learning           # any other tag
tc split 3 KD-231=60 KD-240=40      # one session, two features
```

The project and the client follow from the feature as default tags. Every fix is a new split that supersedes the rule's; the old one stays in the log. Add `--remember` to make the fix a [rule](/attribute/rules#remember) for next time.

## Which contract {#contract}

Some messages may reach no contract: the agent did not report its sign-in, no rule matched, and the provider has more than one contract or none. Others may come without their provider. In the web app, the review list asks once per agent which contract those sessions were paid under, such as `claude-team` or `anthropic-api`, or lets you add a new one with its provider and whether it is metered or flat. The answer goes on every message of those sessions. With _Remember_ on, it also becomes a rule of yours, so that agent's sessions go under the same contract from then on, earlier ones too. How a message gets its contract otherwise is under [Which contract a message gets](/reference/data-model#contract).

## Submit {#submit}

```text
tc submit
submitted · 5 sessions · $212.40 · 95% on features
```

A submit covers every message of yours since your last submit. Submit whenever you like; before moving the cutoff, your org manager may ask everyone to. If your organization approves submissions and a people manager [rejects](/account/submit#approval) yours, it comes back to your review list with a reason.
