// Writes Sonar's logo (a submarine sending a sonar pulse) for both apps, and
// the desktop app's window and taskbar icons.
// Run from the repo: node desktop/scripts/generate-logo.mjs (needs backend deps and Microsoft Edge).
import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(resolve(root, "backend/package.json"))("playwright");

const LIGHT = "#70eeff", ACCENT = "#84dfeb";
// A submarine sending a sonar pulse ahead. The portholes are real holes, so the
// mark works on any background.
const art = `<path fill="${LIGHT}" fill-rule="evenodd" d="M15.5 25H36.5A9.5 9.5 0 0 1 36.5 44H15.5A9.5 9.5 0 0 1 15.5 25ZM17 31.9a2.6 2.6 0 1 0 0 5.2a2.6 2.6 0 1 0 0-5.2ZM26 31.9a2.6 2.6 0 1 0 0 5.2a2.6 2.6 0 1 0 0-5.2ZM35 31.9a2.6 2.6 0 1 0 0 5.2a2.6 2.6 0 1 0 0-5.2Z"/>
<path d="M20 25V19Q20 17 22 17H30Q32 17 32 19V25Z" fill="${ACCENT}"/>
<path d="M26 17V9H33" fill="none" stroke="${LIGHT}" stroke-width="3" stroke-linecap="round" stroke-linejoin="round"/>
<g fill="none" stroke-linecap="round" stroke-width="3.4">
<path d="M51 28A9 9 0 0 1 51 41" stroke="${LIGHT}"/>
<path d="M56.5 23A17 17 0 0 1 56.5 46" stroke="${ACCENT}"/>
</g>`;
const mark = (size) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 64 64">${art}</svg>\n`;
// Icons sit on a dark tile so they stay readable on a light Windows taskbar.
const tile = `<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#112330"/>${art}</svg>`;

writeFileSync(resolve(root, "desktop/assets/sonar-mark.svg"), mark(43));
writeFileSync(resolve(root, "frontend/public/sonar-mark.svg"), mark(64));
writeFileSync(resolve(root, "frontend/public/favicon.svg"), tile.replace('width="100%" height="100%"', 'width="64" height="64"'));

const browser = await chromium.launch({ executablePath: "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe" });
const sizes = [16, 24, 32, 48, 64, 256], images = [];
for (const size of sizes) {
  const page = await browser.newPage({ viewport: { width: size, height: size } });
  await page.setContent(`<!doctype html><style>html,body{margin:0;background:transparent}svg{display:block}</style>${tile}`);
  images.push(await page.screenshot({ omitBackground: true, type: "png" }));
  await page.close();
}
await browser.close();
writeFileSync(resolve(root, "desktop/assets/sonar-icon.png"), images[sizes.indexOf(256)]);

const header = Buffer.alloc(6);
header.writeUInt16LE(1, 2); header.writeUInt16LE(sizes.length, 4);
let offset = 6 + 16 * sizes.length;
const entries = sizes.map((size, n) => {
  const entry = Buffer.alloc(16);
  entry[0] = size === 256 ? 0 : size; entry[1] = size === 256 ? 0 : size;
  entry.writeUInt16LE(1, 4); entry.writeUInt16LE(32, 6);
  entry.writeUInt32LE(images[n].length, 8); entry.writeUInt32LE(offset, 12);
  offset += images[n].length;
  return entry;
});
writeFileSync(resolve(root, "desktop/assets/sonar.ico"), Buffer.concat([header, ...entries, ...images]));
console.log("wrote logo and icons");
