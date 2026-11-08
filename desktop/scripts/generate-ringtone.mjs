// Synthesizes Sonar's original incoming-call ringtone: low, round water drops
// over slow waves. Web and desktop play the same file.
// Run: node desktop/scripts/generate-ringtone.mjs
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RATE = 24000, LOOP = 4.8, SIZE = Math.round(RATE * LOOP), TAU = Math.PI * 2;
const output = resolve(dirname(fileURLToPath(import.meta.url)), "../../frontend/public/sounds/ringtone.wav");
const mix = new Float64Array(SIZE);
// Everything wraps around the loop, so tails spill into its start with no seam.
const add = (time, value) => { mix[((Math.round(time * RATE) % SIZE) + SIZE) % SIZE] += value; };

// One round "bloop": the pitch slides up a little as a drop does, with no hard edge.
function bloop(start, frequency, gain, decay, attack = 0.02) {
  const length = Math.min(decay * 6, LOOP - 0.05);
  let phase = 0;
  for (let n = 0; n < length * RATE; n++) {
    const t = n / RATE;
    phase += (TAU * frequency * (1 - 0.18 * Math.exp(-t * 28))) / RATE;
    const warmth = 0.1 * Math.exp(-t * 9) * Math.sin(2 * phase);
    const end = Math.min(1, (length - t) / 0.08);
    add(start + t, gain * Math.min(t / attack, 1) * Math.exp(-t / decay) * end * (Math.sin(phase) + warmth));
  }
}

function drop(start, frequency, gain, decay = 0.5) {
  bloop(start, frequency, gain, decay);
  bloop(start + 0.36, frequency, gain * 0.22, decay * 1.1, 0.05);
  bloop(start + 0.78, frequency, gain * 0.1, decay * 1.2, 0.08);
}

drop(0, 220, 0.8);          // A3
drop(0.48, 329.63, 0.62);   // E4
drop(0.96, 277.18, 0.68);   // C#4
drop(1.62, 220, 0.9, 0.85); // A3, the long landing
bloop(1.62, 164.81, 0.32, 0.9, 0.06); // E3 under it

// Slow waves: soft low noise that swells twice and is silent at the loop point.
let seed = 20240611, low = 0, lower = 0;
for (let n = 0; n < SIZE; n++) {
  seed = (seed * 1664525 + 1013904223) >>> 0;
  low += 0.06 * (seed / 2 ** 31 - 1 - low);
  lower += 0.06 * (low - lower);
  mix[n] += 0.75 * Math.sin((TAU * n) / SIZE) ** 2 * lower;
}

// A few deep bubbles in the quiet part of the loop.
for (const [start, from, to] of [[3.05, 170, 300], [3.32, 200, 340], [3.71, 160, 280], [4.02, 190, 320]]) {
  let phase = 0;
  const length = 0.09;
  for (let n = 0; n < length * RATE; n++) {
    const t = n / RATE / length;
    phase += (TAU * (from + (to - from) * t * t)) / RATE;
    add(start + n / RATE, 0.16 * Math.sin(Math.PI * t) ** 2 * Math.sin(phase));
  }
}

const peak = mix.reduce((most, value) => Math.max(most, Math.abs(value)), 0);
const TARGET = 0.5; // -6 dB: clearly audible without being harsh on headphones.
const samples = Buffer.alloc(SIZE * 2);
mix.forEach((value, n) => samples.writeInt16LE(Math.round((value / peak) * TARGET * 32767), n * 2));

const header = Buffer.alloc(44);
header.write("RIFF", 0); header.writeUInt32LE(36 + samples.length, 4); header.write("WAVEfmt ", 8);
header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(1, 22);
header.writeUInt32LE(RATE, 24); header.writeUInt32LE(RATE * 2, 28); header.writeUInt16LE(2, 32);
header.writeUInt16LE(16, 34); header.write("data", 36); header.writeUInt32LE(samples.length, 40);
mkdirSync(dirname(output), { recursive: true });
writeFileSync(output, Buffer.concat([header, samples]));

const rms = Math.sqrt(mix.reduce((sum, value) => sum + (value / peak * TARGET) ** 2, 0) / SIZE);
const seam = Math.abs(mix[0] - mix[SIZE - 1]) / peak * TARGET;
console.log({ seconds: LOOP, bytes: 44 + samples.length, rms: +rms.toFixed(3), seam: +seam.toFixed(4) });
