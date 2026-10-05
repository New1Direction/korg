// usage: node shot.mjs t1 t2 ...  -> writes /tmp scratch pngs
import { createRequire } from "module";
const require = createRequire("/opt/node22/lib/node_modules/");
const { chromium } = require("playwright");
import path from "path"; import { fileURLToPath } from "url";
const here = path.dirname(fileURLToPath(import.meta.url));
const out = process.env.OUT || "/tmp";
const b = await chromium.launch({ executablePath: "/opt/pw-browsers/chromium-1194/chrome-linux/chrome", args: ["--no-sandbox"] }).catch(async () => chromium.launch({ args: ["--no-sandbox"] }));
const pg = await b.newPage({ viewport: { width: 1280, height: 720 } });
pg.on("pageerror", e => console.log("PAGEERR", e.message)); pg.on("console", m => { if (m.type() === "error") console.log("CONSOLE", m.text()); });
await pg.goto("file://" + here + "/index.html?capture=1");
for (const t of process.argv.slice(2)) { const t0 = Date.now(); await pg.evaluate((t) => window.renderAt(t), parseFloat(t)); await pg.locator("canvas").screenshot({ path: `${out}/f_${t}.png` }); console.log(t, Date.now() - t0, "ms"); }
await b.close();
