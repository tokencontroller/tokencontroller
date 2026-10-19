---
title: "Check and submit"
description: "Check your sessions, fix the exceptions, submit."
---

Before you submit, you see every session since your last submit and how it was split, in the CLI, the Claude Code mod's pane or the web app.

## Check {#review}

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

## Submit {#submit}

```text
tc submit
submitted · 5 sessions · $212.40 · 95% on features
```

A submit covers every message of yours since your last submit. Submit whenever you like; before moving the cutoff, your org manager may ask everyone to. If your organization approves submissions and a people manager [rejects](/account/submit#approval) yours, it comes back to this list with a reason.
