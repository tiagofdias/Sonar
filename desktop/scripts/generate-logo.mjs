// Writes Sonar's logo (a boat sending sonar waves downward) for both apps, and
// the desktop app's window and taskbar icons.
// Run from the repo: node desktop/scripts/generate-logo.mjs (needs backend deps and Microsoft Edge).
import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const { chromium } = createRequire(resolve(root, "backend/package.json"))("playwright");

const LIGHT = "#d4c1ff", ACCENT = "#a68be7";
const art = `<path d="M9 25H55L47.5 37H16.5Z" fill="${LIGHT}" stroke="${LIGHT}" stroke-width="2" stroke-linejoin="round"/>
<rect x="24" y="14" width="15" height="11" rx="2" fill="${ACCENT}"/>
<path d="M31.5 14V5" stroke="${LIGHT}" stroke-width="3" stroke-linecap="round"/>
<g fill="none" stroke-linecap="round" stroke-width="3.6">
<path d="M24.3 44.4A10 10 0 0 0 39.7 44.4" stroke="${LIGHT}"/>
<path d="M19 48.9A17 17 0 0 0 45 48.9" stroke="${ACCENT}"/>
<path d="M13.6 53.4A24 24 0 0 0 50.4 53.4" stroke="${LIGHT}" opacity=".55"/>
</g>`;
const mark = (size) => `<svg xmlns="http://www.w3.org/2000/svg" width="${size}" height="${size}" viewBox="0 0 64 64">${art}</svg>\n`;
// Icons sit on a dark tile so they stay readable on a light Windows taskbar.
const tile = `<svg xmlns="http://www.w3.org/2000/svg" width="100%" height="100%" viewBox="0 0 64 64"><rect width="64" height="64" rx="14" fill="#1f1b2b"/>${art}</svg>`;

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
