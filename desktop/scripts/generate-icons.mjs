// Copies the exact Lucide icons used by the web client (frontend/node_modules/
// lucide-react, ISC license) into SVG files for the native desktop renderer, so
// both apps show identical artwork. Run: node desktop/scripts/generate-icons.mjs
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";
const root = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const source = resolve(root, "frontend/node_modules/lucide-react/dist/esm/icons");
const target = resolve(root, "desktop/assets/icons");
const names = ["message-circle", "users", "shield-check", "chevron-down", "chevron-left", "arrow-up-right", "search", "lock-keyhole", "log-out", "phone", "phone-off", "phone-incoming", "pin", "ellipsis", "timer", "headphones", "headphone-off", "check", "reply", "trash-2", "send", "x", "smile-plus", "mic", "mic-off", "plus", "hash", "volume-2", "key-round", "user-round", "copy", "monitor", "lock", "lock-open", "arrow-left", "audio-lines", "rotate-ccw", "circle-check", "circle-x", "server", "refresh-cw", "user-minus", "ban", "crown", "shield", "pencil", "speaker", "play", "square", "eye", "eye-off", "info"];
mkdirSync(target, { recursive: true });
const escape = (value) => String(value).replaceAll("&", "&amp;").replaceAll('"', "&quot;").replaceAll("<", "&lt;");
for (const name of names) {
  const text = readFileSync(resolve(source, `${name}.js`), "utf8");
  const literal = /createLucideIcon\("[^"]+", (\[[\s\S]*\])\);/.exec(text)?.[1];
  if (!literal) throw new Error(`Unrecognized icon module: ${name}`);
  const nodes = Function(`"use strict"; return ${literal};`)();
  const body = nodes.map(([tag, attrs]) => `<${tag} ${Object.entries(attrs).filter(([key]) => key !== "key").map(([key, value]) => `${key}="${escape(value)}"`).join(" ")}/>`).join("");
  writeFileSync(resolve(target, `${name}.svg`), `<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${body}</svg>\n`);
}
writeFileSync(resolve(target, "LICENSE.txt"), readFileSync(resolve(root, "frontend/node_modules/lucide-react/LICENSE")));
console.log(`wrote ${names.length} icons`);
