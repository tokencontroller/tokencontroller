---
title: "CLI"
description: "Install tc, connect or stay local, check, submit and export."
---

The CLI, `tc`, reads session files, splits sessions with your rules, and submits. It works with no service connected.

## Install {#install}

```sh
# macOS and Linux
brew tap tokencontroller/tap https://github.com/tokencontroller/tokencontroller
brew install tc

# Windows: tc-x86_64-pc-windows-msvc.zip from the GitHub releases
```

Binaries for macOS, Windows and Linux, on Intel and ARM, are on [GitHub](https://github.com/tokencontroller/tokencontroller/releases).

## Connect, or stay local {#connect}

```sh
tc connect https://app.tokencontroller.com/i/kestrel-4f2a   # your invite link
tc status
```

Without `tc connect` the CLI keeps everything on your machine. It still splits sessions with your personal rules and shows reports, priced with the copy of the price table built into the binary.

## Everyday commands {#daily}

```sh
tc review                   # your sessions since the last submit and where they were put
tc tag 5 feature=KD-231     # set a tag on session 5 by hand
tc submit                   # submit everything since your last submit
tc report --by team         # cost per value of any tag or attribute
tc export --csv             # every message as CSV, for a spreadsheet
```

With no service, the export is enough to report by hand. Every command and setting is in the [CLI reference](/reference/cli).

## Only some folders {#folders}

If you work for several clients, or want one project counted, track its folder:

```sh
tc list                       # the folders your sessions ran in
tc track ~/work/storefront    # count only sessions there and below
```

Without a tracked folder, `tc` counts every session on the machine. The first time you run it at a terminal, it asks whether to track the folder you are in. Sessions the Claude Code mod reports always count. Details are in the [CLI reference](/reference/cli#folders).

## What it reads {#sources}

Session files of Claude Code and Codex, counted the way the open source ccusage project counts them: one message per model call, with its token counts by kind. Every other agent reports by [telemetry](/collect/telemetry) or through the [token entries endpoint](/collect/token-entries). Reading the same files twice changes nothing, so run it as often as you like. There is no background process and no sync between your machines: each machine submits its own messages.
