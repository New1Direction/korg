// Renders the trailer: steps index.html's renderAt(t) at 30 fps, pipes JPEG frames to ffmpeg,
// muxes the synthesised audio, and cuts the 8-second REWIND clip.
//   node trailer/audio.mjs && node trailer/render.mjs            # full film + clip
//   FROM=26 TO=34 NAME=preview node trailer/render.mjs            # just a range (preview)
//   CAPTIONS=0 node trailer/render.mjs                            # no burned-in voiceover captions
import { createRequire } from "module"; import { spawn, spawnSync } from "child_process";
import path from "path"; import fs from "fs"; import { fileURLToPath } from "url";
const require = createRequire("/opt/node22/lib/node_modules/"); // global playwright
let chromium; try { ({ chromium } = require("playwright")); } catch { ({ chromium } = await import("playwright")); }
const here = path.dirname(fileURLToPath(import.meta.url)), out = path.join(here, "out");
const FPS = +(process.env.FPS || 30), FROM = +(process.env.FROM || 0), TO = +(process.env.TO || 60), NAME = process.env.NAME || "korg-trailer";
const wav = path.join(out, "korg-trailer.wav");
if (!fs.existsSync(wav)) { console.error("run `node trailer/audio.mjs` first"); process.exit(1); }
const exe = ["/opt/pw-browsers/chromium-1194/chrome-linux/chrome"].find(p => fs.existsSync(p));
const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
page.on("pageerror", e => { console.error("PAGE ERROR", e.message); process.exit(1); });
await page.goto("file://" + here + "/index.html?capture=1" + (process.env.CAPTIONS === "0" ? "&captions=0" : ""));
const file = path.join(out, NAME + ".mp4");
const ff = spawn("ffmpeg", ["-y", "-loglevel", "error", "-f", "image2pipe", "-framerate", String(FPS), "-c:v", "mjpeg", "-i", "-",
  "-ss", String(FROM), "-t", String(TO - FROM), "-i", wav,
  "-c:v", "libx264", "-preset", "slow", "-crf", "19", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-c:a", "aac", "-b:a", "192k", "-shortest", file], { stdio: ["pipe", "inherit", "inherit"] });
const frames = Math.round((TO - FROM) * FPS), t0 = Date.now();
for (let f = 0; f < frames; f++) {
  const t = FROM + f / FPS;
  await page.evaluate((t) => window.renderAt(t), t);
  const jpg = await page.locator("canvas").screenshot({ type: "jpeg", quality: 95 });
  if (!ff.stdin.write(jpg)) await new Promise(r => ff.stdin.once("drain", r));
  if (f % 150 === 0) console.log(`t=${t.toFixed(1)}s  ${((Date.now() - t0) / 1000).toFixed(0)}s elapsed`);
}
ff.stdin.end(); await new Promise(r => ff.on("close", r)); await browser.close();
console.log("wrote", file, (fs.statSync(file).size / 1e6).toFixed(1) + " MB");
if (FROM === 0 && TO === 60) {
  const clip = path.join(out, "korg-rewind-8s.mp4");
  spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-ss", "26", "-t", "8", "-i", file, "-c:v", "libx264", "-preset", "slow", "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart", "-c:a", "aac", "-b:a", "192k", clip]);
  console.log("wrote", clip, (fs.statSync(clip).size / 1e6).toFixed(1) + " MB");
}
