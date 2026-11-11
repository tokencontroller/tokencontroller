// The demo video: one month at Kestrel Digital, the docs' worked month
// (Concepts > A worked month), recorded from the real service with seed
// data. About two and a half minutes, no sound, slow enough to talk over.
//
//   usage comes in -> the review list -> a sign-in rule -> bills, checked
//   per contract -> a report with "on a plan" and "not priced yet"
//
// Each scene is recorded through Chromium's screencast (Playwright's own
// video does not work with the preinstalled Chromium) and turned into an
// mp4 with ffmpeg; brand title cards go between the scenes and ffmpeg
// joins everything. See README.md in this folder for how to run it.

import { execSync, spawnSync } from "node:child_process";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const here = dirname(fileURLToPath(import.meta.url));
const base = process.env.TC_WEB_URL ?? "http://localhost:8080";
const linkCommand = process.env.TC_SIGN_IN_LINK_CMD;
const out = resolve(process.env.TC_DEMO_OUT ?? join(here, "out"));
const video = resolve(process.env.TC_DEMO_VIDEO ?? join(out, "demo-tour.mp4"));
if (!linkCommand) throw new Error("Set TC_SIGN_IN_LINK_CMD, such as `docker exec tc tokencontroller sign-in-link {email}`.");

// 1440 × 810 CSS pixels at 4/3 gives a 1920 × 1080 video.
const viewport = { width: 1440, height: 810 };
const scale = 4 / 3;
const size = { width: 1920, height: 1080 };
const fps = 30;
const canvas = "0xF8F7F3";

const people = {
  nadia: "n.varga@kestrel.example",
  mara: "m.ortner@kestrel.example",
  tom: "t.lindqvist@kestrel.example",
};

const pause = (seconds) => new Promise((done) => setTimeout(done, seconds * 1000));

function ffmpeg(args) {
  const run = spawnSync("ffmpeg", ["-loglevel", "error", "-y", ...args], { stdio: "inherit" });
  if (run.status !== 0) throw new Error(`ffmpeg failed: ${args.join(" ")}`);
}

// A visible pointer: the screencast shows the page, not the mouse.
const pointer = `
  addEventListener("DOMContentLoaded", () => {
    const dot = document.createElement("div");
    dot.style.cssText = "position:fixed;z-index:2147483647;left:0;top:0;width:18px;height:18px;margin:-9px 0 0 -9px;border-radius:50%;background:rgba(37,99,207,.35);border:2px solid #2563CF;pointer-events:none;transition:transform .12s;transform:translate(-100px,-100px)";
    document.body.appendChild(dot);
    let x = -100, y = -100;
    addEventListener("mousemove", (e) => { x = e.clientX; y = e.clientY; dot.style.transform = "translate(" + x + "px," + y + "px)"; }, true);
    addEventListener("mousedown", () => { dot.style.transform = "translate(" + x + "px," + y + "px) scale(.7)"; }, true);
    addEventListener("mouseup", () => { dot.style.transform = "translate(" + x + "px," + y + "px)"; }, true);
  });
`;

/**
 * A workaround until the service and the web app agree: the review list and
 * the overview ask for sessions with no date range, which the service
 * refuses with "from is required". The tour adds the range from 1 January
 * to the browser's today. Remove it once the screens work without it.
 */
async function withRange(route) {
  const url = new URL(route.request().url());
  if (!url.searchParams.has("from")) url.searchParams.set("from", "2026-01-01");
  if (!url.searchParams.has("to")) url.searchParams.set("to", "2026-04-10");
  await route.continue({ url: url.toString() });
}

/** A signed-in page for one person, living on 10 April 2026 like the seed. */
async function personPage(browser, who) {
  const context = await browser.newContext({ viewport, deviceScaleFactor: scale, baseURL: base });
  await context.clock.setFixedTime(new Date("2026-04-10T10:00:00Z"));
  await context.route(/\/api\/v1\/(sessions|messages)(\?|$)/, withRange);
  await context.addInitScript(pointer);
  const page = await context.newPage();
  const link = execSync(linkCommand.replace("{email}", people[who]), { encoding: "utf8" }).trim().split("\n").at(-1);
  await page.goto(link);
  await page.getByRole("button", { name: "Sign in" }).click();
  await page.getByRole("heading", { level: 1, name: /^Hello/ }).waitFor();
  await page.mouse.move(viewport.width - 200, viewport.height - 160);
  return page;
}

/** Records one scene through the screencast and writes it as an mp4. */
async function record(page, name, play) {
  const cdp = await page.context().newCDPSession(page);
  const frames = [];
  cdp.on("Page.screencastFrame", ({ data, metadata, sessionId }) => {
    frames.push({ data, at: metadata.timestamp });
    cdp.send("Page.screencastFrameAck", { sessionId }).catch(() => undefined);
  });
  await cdp.send("Page.startScreencast", { format: "jpeg", quality: 92, maxWidth: size.width, maxHeight: size.height });
  await pause(0.5);
  await play(page);
  const end = Date.now() / 1000;
  await cdp.send("Page.stopScreencast");
  await cdp.detach();
  if (!frames.length) throw new Error(`scene ${name}: no frames`);

  const dir = join(out, name);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  const lines = ["ffconcat version 1.0"];
  frames.forEach((frame, index) => {
    const file = `f${String(index).padStart(5, "0")}.jpg`;
    writeFileSync(join(dir, file), Buffer.from(frame.data, "base64"));
    const next = frames[index + 1]?.at ?? end;
    lines.push(`file '${file}'`, `duration ${Math.max(next - frame.at, 1 / fps).toFixed(3)}`);
  });
  lines.push(`file 'f${String(frames.length - 1).padStart(5, "0")}.jpg'`);
  writeFileSync(join(dir, "frames.txt"), `${lines.join("\n")}\n`);
  const file = join(out, `${name}.mp4`);
  ffmpeg([
    "-f", "concat", "-safe", "0", "-i", join(dir, "frames.txt"),
    "-vf", `fps=${fps},scale=${size.width}:${size.height}:force_original_aspect_ratio=decrease,pad=${size.width}:${size.height}:(ow-iw)/2:(oh-ih)/2:color=${canvas},format=yuv420p`,
    "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-r", String(fps), file,
  ]);
  rmSync(dir, { recursive: true, force: true });
  return file;
}

/** A title card in the brand's look, held for a few seconds. */
async function card(page, name, title, lead, seconds = 4) {
  await page.goto("/api/v1/health");
  const bars = ["#ee4545", "#f77a22", "#f7c125", "#3fbf68", "#22b5ae", "#2f80e8", "#7e57e8"]
    .map((color) => `<span style="background:${color}"></span>`).join("");
  await page.setContent(`<!doctype html><html><head><style>
    @font-face { font-family: Inter; font-weight: 400 600; src: url(/fonts/inter-latin.woff2) format("woff2"); }
    html, body { margin: 0; height: 100%; background: #f8f7f3; color: #1c2030; font-family: Inter, sans-serif; }
    main { height: 100%; display: flex; flex-direction: column; justify-content: center; padding: 0 140px; box-sizing: border-box; }
    .brand { display: flex; align-items: center; gap: 14px; font-size: 22px; font-weight: 600; }
    .bars { display: flex; gap: 8px; margin: 56px 0 28px; }
    .bars span { width: 44px; height: 10px; border-radius: 3px; }
    h1 { font-size: 64px; line-height: 1.1; font-weight: 600; margin: 0; letter-spacing: -0.01em; }
    p { font-size: 28px; line-height: 1.4; color: #646a7d; margin: 22px 0 0; max-width: 980px; }
  </style></head><body><main>
    <div class="brand"><img src="/logo-light-small.svg" width="40" height="40" alt="">Token Controller</div>
    <div class="bars">${bars}</div>
    <h1>${title}</h1>${lead ? `<p>${lead}</p>` : ""}
  </main></body></html>`);
  await page.evaluate(() => document.fonts.ready);
  await page.waitForLoadState("networkidle");
  const still = join(out, `${name}.png`);
  await page.screenshot({ path: still });
  const file = join(out, `${name}.mp4`);
  ffmpeg([
    "-loop", "1", "-framerate", String(fps), "-t", String(seconds), "-i", still,
    "-vf", `scale=${size.width}:${size.height},fade=in:st=0:d=0.4:color=${canvas},fade=out:st=${seconds - 0.4}:d=0.4:color=${canvas},format=yuv420p`,
    "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-r", String(fps), file,
  ]);
  return file;
}

/** Moves the pointer to an element in visible steps, then waits a moment. */
async function point(page, locator, wait = 0.6) {
  await locator.scrollIntoViewIfNeeded();
  const box = await locator.boundingBox();
  if (!box) throw new Error(`nothing to point at: ${String(locator)}`);
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2, { steps: 24 });
  await pause(wait);
}

async function click(page, locator, wait = 0.6) {
  await point(page, locator, 0.35);
  await locator.click();
  await pause(wait);
}

async function type(page, locator, text) {
  await click(page, locator, 0.2);
  await locator.pressSequentially(text, { delay: 30 });
  await pause(0.4);
}

/** Mara's fifth session: a model name with a provider prefix and no provider. */
const unpriced = {
  agent_harness: "opencode",
  model: "anthropic/claude-sonnet-4-5",
  session_id: "seed-s5",
  started_at: "2026-03-27T15:00:00Z",
  ended_at: "2026-03-27T15:40:00Z",
  tokens: { input: 2_000_000, output: 150_000 },
  summary: "Returns portal: printable labels",
  feature: "KD-2",
  messages: [{ message_id: "1", at: "2026-03-27T15:00:00Z", tokens: { input: 2_000_000, output: 150_000 } }],
};

async function expectOk(answer) {
  if (!answer.ok()) throw new Error(`${answer.url()} answered ${answer.status()}: ${await answer.text()}`);
}

mkdirSync(out, { recursive: true });
const browser = await chromium.launch(process.env.TC_CHROMIUM_PATH ? { executablePath: process.env.TC_CHROMIUM_PATH } : {});
const clips = [];
try {
  const nadia = await personPage(browser, "nadia");
  // The month before anyone looked at it: Tom has not set S4's contract
  // yet, and the sign-in rule for the seat plan is not there yet.
  for (const event of ["con_seed_s4", "rul_seed_team_sign_in"]) {
    const answer = await nadia.request.post("/api/v1/reversals", { data: { event_id: event } });
    if (!answer.ok()) throw new Error(`Cannot reverse ${event} (${answer.status()}): reset the database and seed again before the tour.`);
  }
  const mara = await personPage(browser, "mara");
  const tom = await personPage(browser, "tom");

  clips.push(await card(nadia, "00-title", "One month at Kestrel Digital", "Three contracts, four people, three bills. Where did March's AI agent money go, and do the bills agree?", 5));

  // 1. Usage comes in.
  clips.push(await card(nadia, "01-card", "Usage comes in", "Each agent sends what it used. A session lands in its person's review list."));
  clips.push(await record(mara, "01-usage", async (page) => {
    await pause(2);
    await point(page, page.getByRole("heading", { name: "Your sessions to check" }), 1);
    await expectOk(await page.request.post("/api/v1/token-entries", { data: unpriced }));
    await click(page, page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: "Check and submit" }), 2);
    await page.getByRole("table", { name: "Sessions" }).getByText("Returns portal: printable labels").waitFor();
    await point(page, page.getByRole("table", { name: "Sessions" }).getByText("Returns portal: printable labels"), 1.5);
    await point(page, page.getByText("not priced yet").first(), 2);
    await point(page, page.getByText("provider unknown").first(), 2);
    await point(page, page.getByText("Which contract?"), 2.5);
  }));

  // 2. The review list: Tom says which contract S4 was paid under.
  clips.push(await card(nadia, "02-card", "The review list", "Tom's Claude Code session has no contract yet. He says which one it was paid under."));
  await tom.goto("/sessions");
  await tom.getByText("Which contract?").waitFor();
  clips.push(await record(tom, "02-review-list", async (page) => {
    await pause(2);
    await point(page, page.getByText("Which contract?"), 1.5);
    await point(page, page.getByRole("table", { name: "Sessions" }).getByText("no contract"), 1.5);
    const contract = page.getByLabel("Contract for Claude Code");
    await click(page, contract, 0.3);
    await contract.selectOption("anthropic-api");
    await pause(1);
    await click(page, page.getByRole("checkbox", { name: "Remember" }), 0.8);
    await click(page, page.getByRole("button", { name: "Set", exact: true }), 2.5);
    await point(page, page.getByRole("table", { name: "Sessions" }).getByText("anthropic-api").first(), 3);
  }));

  // 3. A sign-in rule puts the seat plan's sessions under their contract.
  clips.push(await card(nadia, "03-card", "A sign-in rule", "Sessions signed in to the team's Claude organization were paid by the seat plan. One rule says so."));
  await nadia.goto("/bills");
  await nadia.getByText("a message is wrong").first().waitFor();
  clips.push(await record(nadia, "03-sign-in-rule", async (page) => {
    await pause(1.5);
    const team = page.getByRole("group", { name: /^Claude Team plan/ });
    await point(page, team.getByText("a message is wrong"), 2);
    await point(page, page.getByText("Same provider, no contract").first(), 2);
    await click(page, page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: "Rules" }), 1.2);
    await click(page, page.getByRole("button", { name: "Add a rule" }).first(), 0.8);
    const dialog = page.getByRole("dialog", { name: "Add a rule" });
    await type(page, dialog.getByLabel("Rule name"), "signed in to the team's Claude organization");
    await click(page, dialog.getByLabel("Signal"), 0.2);
    await dialog.getByLabel("Signal").fill("");
    await dialog.getByLabel("Signal").pressSequentially("organization.id", { delay: 30 });
    await type(page, dialog.getByLabel("Pattern"), "^kestrel-team$");
    await click(page, dialog.getByLabel("Sets tag name"), 0.2);
    await dialog.getByLabel("Sets tag name").selectOption("contract");
    await pause(0.6);
    await type(page, dialog.getByLabel("Tag value"), "claude-team");
    await pause(1);
    await click(page, dialog.getByRole("button", { name: "Add the rule" }), 1.5);
    await point(page, page.getByRole("table", { name: "Organization rules" }).getByText("contract: claude-team"), 1.5);
    await click(page, page.getByRole("navigation", { name: "Main" }).getByRole("link", { name: "Check and submit" }), 1);
    await click(page, page.getByRole("checkbox", { name: "Everyone I may view" }), 1.5);
    await point(page, page.getByRole("table", { name: "Sessions" }).getByText("claude-team"), 2.5);
  }));

  // 4. Bills, each checked under its contract.
  clips.push(await card(nadia, "04-card", "Bills, checked per contract", "Each bill against the messages under its contract, with the two lines that could hide a mistake beside it."));
  await nadia.goto("/bills");
  await nadia.getByRole("group").first().waitFor();
  clips.push(await record(nadia, "04-bills", async (page) => {
    await pause(1.5);
    const api = page.getByRole("group", { name: /^Anthropic API/ });
    await point(page, api.getByText("$270.00", { exact: true }), 1);
    await point(page, api.getByText("$250.00", { exact: true }), 1);
    await point(page, api.getByText("$20.00", { exact: true }), 2);
    const team = page.getByRole("group", { name: /^Claude Team plan/ });
    await point(page, team.getByText("$200.00", { exact: true }), 1);
    await point(page, team.getByText("$50.00", { exact: true }), 1);
    await point(page, team.getByText("$350.00", { exact: true }), 2);
    await point(page, page.getByText("Contract only from the provider"), 2.5);
  }));

  // 5. A report: cost at the price table, what ran on a plan, what is not priced yet.
  clips.push(await card(nadia, "05-card", "Reports", "Cost of anything you can filter. What ran on a plan is marked, and what is not priced yet stands on its own line."));
  await nadia.goto("/reports");
  await nadia.getByRole("table", { name: "Cost report" }).waitFor();
  clips.push(await record(nadia, "05-report", async (page) => {
    await pause(1.5);
    await point(page, page.getByText("$950.00", { exact: true }).first(), 1.5);
    await point(page, page.getByLabel("By kind of contract").getByText("On a plan"), 2);
    await point(page, page.getByText(/not priced yet/).first(), 2.5);
    const by = page.getByLabel("Group by");
    await click(page, by, 0.3);
    await by.selectOption("person");
    await page.getByRole("table", { name: "Cost report" }).getByText("Mara Ortner").waitFor();
    await pause(2);
    await by.selectOption("feature");
    await page.getByRole("table", { name: "Cost report" }).getByText("KD-1").waitFor();
    await pause(2.5);
  }));

  clips.push(await card(nadia, "06-end", "Token Controller", "docs.tokencontroller.com", 4));
} finally {
  await browser.close();
}

const list = join(out, "clips.txt");
writeFileSync(list, `${clips.map((clip) => `file '${clip}'`).join("\n")}\n`);
mkdirSync(dirname(video), { recursive: true });
ffmpeg(["-f", "concat", "-safe", "0", "-i", list, "-c:v", "libx264", "-preset", "slow", "-crf", "22", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-an", video]);
console.log(`wrote ${video}`);
