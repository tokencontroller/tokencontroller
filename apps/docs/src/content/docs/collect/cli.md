---
title: "CLI"
description: "Install tc, connect or stay local, review, submit and export."
---

The CLI, `tc`, reads session files, places token entries with your rules, and submits token sheets. It works with no service connected.

## Install {#install}

```sh
# macOS and Linux
brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller
brew install tc

# Windows
winget install TokenController.tc
```

Binaries for macOS, Windows and Linux, on Intel and ARM, are on [GitHub](https://github.com/tokencontroller/tokencontroller/releases).

## Connect, or stay local {#connect}

```sh
tc connect https://app.tokencontroller.com/i/kestrel-4f2a   # your invite link
tc status
```

Without `tc connect` the CLI keeps everything on your machine. It still places token entries with your personal rules and shows reports.

## Everyday commands {#daily}

```sh
tc review                 # your draft token entries and where they were placed
tc tag 5 ticket=KD-231    # set a tag on entry 5 by hand
tc submit                 # submit your token sheet
tc report --by team       # cost per value of any tag
tc export --csv           # everything as CSV, for a spreadsheet
```

The export is enough to report by hand when your organization has no service. The full list of commands is in the [CLI reference](/reference/cli).

## What it reads {#sources}

Session files of Claude Code and Codex, counted the way the open source ccusage project counts them. Reading the same files twice changes nothing, so you can run it as often as you like. There is no background process and no sync between your machines: each machine submits its own token entries.
