# Demo video

`tour.mjs` records the demo video: one month at Kestrel Digital, the docs' example organization, from token entries to the closed cost sheet. About two and a half minutes, no sound. Jan records the voice over it later.

It needs a running service with seed data (see the skill `running-the-service-in-a-cloud-session`), a command that prints a sign-in link, Chromium and ffmpeg:

```sh
cd apps/web
TC_WEB_URL=http://localhost:8080 \
TC_SIGN_IN_LINK_CMD="docker exec tc tokencontroller sign-in-link {email}" \
TC_CHROMIUM_PATH=/opt/pw-browsers/chromium \
node demo/tour.mjs demo/out
```

Scenes are recorded through Chromium's screencast, one MP4 per scene in `demo/out/`, then joined into `demo/out/demo-tour.mp4`. Title cards between the scenes use the brand colors. Re-run it whenever a screen changes; the periods follow the seed (last month closed, this month open).
