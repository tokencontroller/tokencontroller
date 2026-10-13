// Records the demo video: one month at Kestrel Digital, the docs' example
// organization, from token entries to the closed cost sheet. No voice; Jan
// records that later. Screens come from a running service with seed data.
//
//   TC_WEB_URL=http://localhost:8080 \
//   TC_SIGN_IN_LINK_CMD="docker exec tc tokencontroller sign-in-link {email}" \
//   TC_CHROMIUM_PATH=/opt/pw-browsers/chromium \
//   node demo/tour.mjs out/
//
// Writes one MP4 per scene into the output folder and demo-tour.mp4 next to
// them (needs ffmpeg on the PATH).
import { execSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { chromium } from "@playwright/test";

const baseURL = process.env.TC_WEB_URL ?? "http://localhost:8080";
const linkCommand = process.env.TC_SIGN_IN_LINK_CMD;
if (!linkCommand)
  throw new Error("set TC_SIGN_IN_LINK_CMD, see the comment at the top");
const out = resolve(process.argv[2] ?? "demo/out");
mkdirSync(out, { recursive: true });
const size = { width: 1600, height: 900 };
const month = (offset) => {
  const date = new Date();
  date.setUTCDate(1);
  date.setUTCMonth(date.getUTCMonth() + offset);
  return date.toISOString().slice(0, 7);
};
const monthName = (ym) =>
  new Date(`${ym}-01T00:00:00Z`).toLocaleString("en-GB", {
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
const closedMonth = month(-1);
const openMonth = month(0);

const people = {
  mara: "m.ortner@kestrel.example",
  priya: "p.shah@kestrel.example",
  nadia: "n.berger@kestrel.example",
};

// A title card between scenes, in the brand's look.
const card = (
  kicker,
  title,
  line,
) => `<!doctype html><html><head><meta charset="utf-8"><style>
  html,body{margin:0;height:100%;background:#F6F3EC;color:#1C2030;font-family:Inter,-apple-system,"Segoe UI",Helvetica,Arial,sans-serif}
  main{height:100%;display:flex;flex-direction:column;justify-content:center;padding:0 160px;box-sizing:border-box}
  .k{font-size:22px;letter-spacing:.12em;text-transform:uppercase;color:#2563CF;font-weight:600;margin-bottom:22px;opacity:0;animation:up .6s .1s forwards}
  h1{font-size:72px;line-height:1.08;margin:0 0 26px;font-weight:700;letter-spacing:-.02em;max-width:18ch;opacity:0;animation:up .6s .25s forwards}
  p{font-size:30px;line-height:1.4;margin:0;color:#4A5064;max-width:34ch;opacity:0;animation:up .6s .45s forwards}
  .dots{position:absolute;right:160px;bottom:120px;display:flex;gap:14px}
  .dots i{width:22px;height:22px;border-radius:7px;display:block;box-shadow:inset 0 1px 0 rgba(255,255,255,.6),0 1px 2px rgba(28,32,48,.25)}
  @keyframes up{from{opacity:0;transform:translateY(14px)}to{opacity:1;transform:none}}
</style></head><body><main>
  <div class="k">${kicker}</div><h1>${title}</h1><p>${line}</p>
  <div class="dots">${["#EE4545", "#F77A22", "#F7C125", "#3FBF68", "#22B5AE", "#2F80E8", "#7E57E8"].map((c) => `<i style="background:${c}"></i>`).join("")}</div>
</main></body></html>`;

const browser = await chromium.launch({
  executablePath: process.env.TC_CHROMIUM_PATH || undefined,
});
let scene = 0;
const scenes = [];

// Records a scene through Chromium's screencast (frames with timestamps),
// which works with any Chromium build, then ffmpeg makes a video of it.
async function record(name, seconds, run) {
  scene += 1;
  const stem = `${String(scene).padStart(2, "0")}-${name}`;
  const frames = join(out, `${stem}-frames`);
  mkdirSync(frames, { recursive: true });
  const context = await browser.newContext({
    baseURL,
    viewport: size,
    deviceScaleFactor: 1,
    colorScheme: "light",
  });
  const page = await context.newPage();
  const cdp = await context.newCDPSession(page);
  const stamps = [];
  cdp.on("Page.screencastFrame", ({ data, metadata, sessionId }) => {
    const file = join(frames, `${String(stamps.length).padStart(5, "0")}.jpg`);
    writeFileSync(file, Buffer.from(data, "base64"));
    stamps.push({ file, at: metadata.timestamp });
    cdp.send("Page.screencastFrameAck", { sessionId }).catch(() => {});
  });
  await cdp.send("Page.startScreencast", {
    format: "jpeg",
    quality: 85,
    maxWidth: size.width,
    maxHeight: size.height,
    everyNthFrame: 1,
  });
  const started = Date.now() / 1000;
  await run(page);
  {
    const ended = Date.now() / 1000;
    await cdp.send("Page.stopScreencast").catch(() => {});
    await context.close();
    if (stamps.length === 0) throw new Error(`${stem}: no frames`);
    // A frame lasts until the next one; the last one until the scene ends.
    const lines = stamps.map((frame, i) => {
      const next =
        i + 1 < stamps.length
          ? stamps[i + 1].at
          : Math.max(frame.at + 0.1, started + (ended - started));
      return `file '${frame.file}'\nduration ${Math.max(0.02, next - frame.at).toFixed(3)}`;
    });
    lines.push(`file '${stamps.at(-1).file}'`);
    const list = join(out, `${stem}.txt`);
    writeFileSync(list, lines.join("\n"));
    const file = join(out, `${stem}.mp4`);
    execSync(
      `ffmpeg -y -loglevel error -f concat -safe 0 -i "${list}" -vf "fps=30,scale=${size.width}:${size.height}:force_original_aspect_ratio=decrease,pad=${size.width}:${size.height}:(ow-iw)/2:(oh-ih)/2,format=yuv420p" -c:v libx264 -preset medium -crf 20 "${file}"`,
      { stdio: "inherit" },
    );
    scenes.push(file);
    console.log(`${file} (about ${seconds}s, ${stamps.length} frames)`);
  }
}

const pause = (page, ms) => page.waitForTimeout(ms);

async function signIn(page, who) {
  const link = execSync(linkCommand.replace("{email}", people[who]), {
    encoding: "utf8",
  })
    .trim()
    .split("\n")
    .at(-1);
  await page.goto(link);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("heading", { level: 1, name: /^Hello/ }).waitFor();
  await page.waitForLoadState("networkidle");
}

// Moves the pointer there like a hand would, then clicks.
async function click(page, locator) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.boundingBox();
  if (box)
    await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, {
      steps: 25,
    });
  await pause(page, 500);
  await locator.click();
}

async function scroll(page, by, steps = 8) {
  for (let i = 0; i < steps; i += 1) {
    await page.mouse.wheel(0, by / steps);
    await pause(page, 90);
  }
}

async function show(page, path, ms) {
  await page.goto(path);
  await page.waitForLoadState("networkidle");
  await pause(page, ms);
}

async function titleCard(name, seconds, kicker, title, line) {
  await record(name, seconds, async (page) => {
    await page.setContent(card(kicker, title, line));
    await pause(page, seconds * 1000);
  });
}

// 1. Opening.
await titleCard(
  "title",
  6,
  "Token Controller",
  "One month at Kestrel Digital",
  "A small agency, four people with AI agents, one closed cost sheet at the end.",
);

// 2. Record: Mara, an agent manager, sees her own token entries, adds one by hand and submits her token sheet.
await titleCard(
  "record-card",
  5,
  "Step 1 of 4 · Record",
  "Every session becomes a token entry",
  "Mara runs Claude Code on the storefront. Her sessions arrive on their own.",
);
await record("record", 55, async (page) => {
  await signIn(page, "mara");
  await pause(page, 3500);
  await show(page, "/token-entries", 2500);
  await scroll(page, 500);
  await pause(page, 1500);
  const row = page
    .getByRole("table", { name: "Token entries" })
    .locator("tbody tr")
    .first();
  await click(page, row);
  await page.getByRole("dialog", { name: "Token entry" }).waitFor();
  await pause(page, 4000);
  await click(
    page,
    page
      .getByRole("dialog", { name: "Token entry" })
      .getByRole("button", { name: "Close" }),
  );
  await pause(page, 800);
  // A session from a laptop that sent nothing: typed in by hand.
  await click(page, page.getByRole("button", { name: "Add", exact: true }));
  const add = page.getByRole("dialog", { name: "Add a token entry" });
  await add
    .getByLabel("Model")
    .pressSequentially("claude-sonnet-5", { delay: 40 });
  await add.getByLabel("Input").pressSequentially("41200", { delay: 60 });
  await add.getByLabel("Output").pressSequentially("6100", { delay: 60 });
  await add
    .getByLabel("Reported cost in US dollars")
    .pressSequentially("4.18", { delay: 80 });
  await add
    .getByLabel("Summary")
    .pressSequentially("Traced the declined card redirect", { delay: 35 });
  await pause(page, 800);
  await click(page, add.getByRole("button", { name: "Add token entry" }));
  await pause(page, 1500);
  await show(page, "/review", 3000);
  const ticket = page.getByLabel(/^Ticket for/).first();
  await click(page, ticket);
  await ticket.selectOption("KD-214");
  await pause(page, 2000);
  await click(page, page.getByRole("button", { name: "Submit token sheet" }));
  await pause(page, 1500);
  const done = page.getByRole("button", { name: "Done" });
  if (await done.isVisible().catch(() => false)) await click(page, done);
  await pause(page, 3500);
});

// 3. Plan: Priya, a people manager, sets a token budget and watches her team.
await titleCard(
  "plan-card",
  5,
  "Step 2 of 4 · Plan",
  "A token budget for what matters",
  "Priya manages team web. She plans tokens per ticket and per month.",
);
await record("plan", 35, async (page) => {
  await signIn(page, "priya");
  await pause(page, 3000);
  await scroll(page, 400);
  await pause(page, 2500);
  await show(page, "/budgets", 3500);
  await click(page, page.getByRole("button", { name: "New token budget" }));
  const dialog = page.getByRole("dialog");
  await pause(page, 1500);
  await click(page, dialog.getByRole("button", { name: "Cancel" }));
  await pause(page, 1000);
  await show(page, "/projects/storefront", 4500);
});

// 4. Reconcile: Nadia, the controller, checks the bills and closes the month.
await titleCard(
  "reconcile-card",
  5,
  "Step 3 of 4 · Reconcile",
  "Token entries meet the provider's bill",
  `Nadia is the controller. ${monthName(closedMonth)} is closed; ${monthName(openMonth)} is open.`,
);
await record("reconcile", 50, async (page) => {
  await signIn(page, "nadia");
  await pause(page, 3500);
  await show(page, `/bills?period=${closedMonth}`, 3000);
  await scroll(page, 450);
  await pause(page, 4000);
  await show(page, "/close", 3500);
  await scroll(page, 350);
  await pause(page, 3500);
  await show(page, `/close/${closedMonth}`, 3000);
  await scroll(page, 500);
  await pause(page, 4500);
});

// 5. Learn: reports, rules and the tag map.
await titleCard(
  "learn-card",
  5,
  "Step 4 of 4 · Learn",
  "Plan the next month with what you learned",
  "Cost by person, by ticket and against the token budgets. Rules place the next entries on their own.",
);
await record("learn", 40, async (page) => {
  await signIn(page, "nadia");
  await show(page, "/reports", 3500);
  await scroll(page, 400);
  await pause(page, 3000);
  await click(page, page.getByRole("tab", { name: "Token budget vs actual" }));
  await pause(page, 4500);
  await show(page, "/rules", 4000);
  await show(page, "/tags", 3000);
  await scroll(page, 600);
  await pause(page, 3500);
});

// 6. Closing.
await titleCard(
  "end",
  6,
  "tokencontroller.com",
  "Every token, accounted for.",
  "Solo is free for one person. Add your team when the first month closes.",
);

await browser.close();

// One file from the scenes.
const list = join(out, "scenes.txt");
writeFileSync(list, scenes.map((file) => `file '${file}'`).join("\n"));
const mp4 = join(out, "demo-tour.mp4");
execSync(
  `ffmpeg -y -loglevel error -f concat -safe 0 -i "${list}" -c copy -movflags +faststart "${mp4}"`,
  { stdio: "inherit" },
);
console.log(mp4);
