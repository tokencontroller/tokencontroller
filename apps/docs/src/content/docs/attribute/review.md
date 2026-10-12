---
title: "Review and submit"
description: "Check draft token entries, fix the exceptions, submit the token sheet."
---

Before you submit, you see every draft token entry and its tags. You only fix the exceptions.

## Review {#review}

```text
tc review
  #  when    cost     tags                              by
  1  09:12   $41.87   ticket KD-214 · team web          rule: ticket key in branch
  2  11:46   $12.10   ticket KD-214 · team web          rule: ticket key in branch
  3  13:05   $88.30   ticket KD-231 · team web          rule: ticket key in PR title
  4  15:20    $9.75   project storefront · team web     rule: repository
  5  16:41   $60.38   team web                          no ticket, 2 candidates
```

The same list is in the Claude Code mod's pane and in the web app.

_Candidates_ are other tickets the session named, in its branch, a title or your prompts, that it is not placed on. Pick one to place it there.

## Fix what is wrong {#fix}

```sh
tc tag 5 ticket=KD-231              # set the ticket
tc tag 4 purpose=learning           # any other tag
tc split 3 KD-231=60 KD-240=40      # one session, two tickets
```

Project, client and the other inherited tags follow from the ticket. You can set any tag on your organization's list the same way.

If the same fix keeps coming back, turn it into a [rule](/attribute/rules).

## Submit {#submit}

```text
tc submit
submitted token sheet · 5 token entries · $212.40 · 95% on tickets
```

Submit whenever you like. If your organization has a [cadence](/account/cadence), submit at least once per cadence period. Submitted token entries count in reports at once. If a people manager rejects one, it comes back to your review list with a reason.
