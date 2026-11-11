# Demo tour

`tour.mjs` records the demo video from the real service with the seed's example organization, Kestrel Digital, in the worked month of the docs ([Concepts > A worked month](../../docs/src/content/docs/concepts.md#month)). It is about two and a half minutes long and has no sound, so someone can talk over it.

| Scene | Who | What it shows |
| --- | --- | --- |
| Usage comes in | Mara | Her overview, then a new session arrives: a model named `anthropic/claude-sonnet-4-5` with no provider, which shows "not priced yet" and "provider unknown" in her review list. |
| The review list | Tom | "Which contract?" for his Claude Code session (S4). He picks the Anthropic API and sets it. |
| A sign-in rule | Nadia | The Claude Team plan check says a message is wrong. She adds the rule "signed in to the team's Claude organization" (`organization.id ~ ^kestrel-team$` sets contract `claude-team`), and S1 moves under the seat plan. |
| Bills, checked per contract | Nadia | The three checks of March: 270 = 250 + 20, the seat plan's fee 200 + extra usage 50 covering 350, and OpenAI's 300 with its contract from the fallback. |
| Reports | Nadia | 950 in all, 400 of it on a plan, and the line of messages not priced yet; then by person and by feature. |

Before it records, the tour reverses two seed events through the API (Tom's hand-set contract on S4 and the sign-in rule), so the month starts as it did before anyone looked at it. It needs a freshly seeded database and refuses to run on one it already used.

## Run it

Start the service with Postgres and seed it (see the making-launch-assets notes), then from `apps/web`:

```bash
TC_WEB_URL=http://localhost:8080 \
TC_SIGN_IN_LINK_CMD="docker exec tc tokencontroller sign-in-link {email}" \
TC_CHROMIUM_PATH=/opt/pw-browsers/chromium \
pnpm demo
```

| Setting | Meaning | Default |
| --- | --- | --- |
| `TC_WEB_URL` | The running service | `http://localhost:8080` |
| `TC_SIGN_IN_LINK_CMD` | Prints a sign-in link for `{email}` | required |
| `TC_CHROMIUM_PATH` | The Chromium to use | Playwright's own |
| `TC_DEMO_OUT` | Folder for the scene and card clips | `demo/out` |
| `TC_DEMO_VIDEO` | The finished video | `demo/out/demo-tour.mp4` |

To run it again, reset first: stop the service, drop and create the database, seed, start the service.

## How it is made

- The browser lives on 10 April 2026, so the app's default range (last month) is March 2026.
- Each scene is recorded through Chromium's screencast (CDP `Page.startScreencast`) at 1440 × 810 CSS pixels and a device scale of 4/3, so the video is 1920 × 1080. Playwright's own video recording does not work with the preinstalled Chromium.
- A blue dot shows the pointer, since the screencast does not draw the mouse.
- The title cards are drawn in the browser with the app's own font and logo, held for a few seconds with a short fade.
- ffmpeg turns the frames into one clip per scene at 30 frames a second and joins clips and cards into one H.264 file without sound.

Look at the result as tiles before handing it on:

```bash
ffmpeg -i demo/out/demo-tour.mp4 -vf "fps=1/5,scale=400:-1,tile=6x5" tour-tiles.jpg
```

## A workaround to remove

The review list and the overview ask `GET /api/v1/sessions` (and the price table `GET /api/v1/messages`) with no date range. The mock answers; the service refuses with "from is required". Until the two agree, the tour (and `e2e/screenshots.spec.ts`) add `from=2026-01-01&to=2026-04-10` to those requests. Remove `withRange` once the screens work without it.
