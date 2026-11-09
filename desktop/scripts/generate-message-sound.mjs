// Synthesizes Sonar's new-message sound: two low water drops, in the same family
// as the ringtone. Web and desktop play the same file.
// Run: node desktop/scripts/generate-message-sound.mjs
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const RATE = 24000, LENGTH = 0.9, SIZE = Math.round(RATE * LENGTH), TAU = Math.PI * 2;
const output = resolve(dirname(fileURLToPath(import.meta.url)), "../../frontend/public/sounds/message.wav");
const mix = new Float64Array(SIZE);

function drop(start, frequency, gain, decay) {
  let phase = 0;
  for (let n = Math.round(start * RATE), t = 0; n < SIZE; n++, t += 1 / RATE) {
    phase += (TAU * frequency * (1 - 0.18 * Math.exp(-t * 28))) / RATE;
    mix[n] += gain * Math.min(t / 0.012, 1) * Math.exp(-t / decay) * (Math.sin(phase) + 0.1 * Math.exp(-t * 9) * Math.sin(2 * phase));
  }
}
drop(0, 293.66, 0.75, 0.11);  // D4
drop(0.15, 392, 0.62, 0.17);  // G4

const peak = Math.max(...mix.map(Math.abs));
const samples = Buffer.alloc(SIZE * 2);
for (let n = 0; n < SIZE; n++) {
  const fade = Math.min(1, (SIZE - n) / (0.05 * RATE));
  samples.writeInt16LE(Math.round((mix[n] / peak) * 0.8 * fade * 32767), n * 2);
}
const header = Buffer.alloc(44);
header.write("RIFF", 0); header.writeUInt32LE(36 + samples.length, 4); header.write("WAVEfmt ", 8);
header.writeUInt32LE(16, 16); header.writeUInt16LE(1, 20); header.writeUInt16LE(1, 22);
header.writeUInt32LE(RATE, 24); header.writeUInt32LE(RATE * 2, 28); header.writeUInt16LE(2, 32);
header.writeUInt16LE(16, 34); header.write("data", 36); header.writeUInt32LE(samples.length, 40);
writeFileSync(output, Buffer.concat([header, samples]));
