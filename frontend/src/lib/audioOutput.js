export function supportsOutputSelection() {
  return (
    typeof HTMLMediaElement !== "undefined" &&
    typeof HTMLMediaElement.prototype.setSinkId === "function"
  );
}
export async function applyAudioOutput(player, settings) {
  player.volume = Math.max(0, Math.min(1, settings.outputVolume));
  if (typeof player.setSinkId === "function")
    await player.setSinkId(settings.outputDeviceId || "");
  else if (settings.outputDeviceId)
    throw new Error(
      "This browser uses your system speakers. Select System default or change the output in your device settings.",
    );
}
export function speakerTestBlob() {
  // Synthesized local two-note chime. No microphone access or external asset.
  const rate = 24000,
    count = Math.floor(rate * 0.9),
    bytes = new ArrayBuffer(44 + count * 2),
    view = new DataView(bytes);
  const text = (offset, value) =>
    [...value].forEach((char, index) =>
      view.setUint8(offset + index, char.charCodeAt(0)),
    );
  text(0, "RIFF");
  view.setUint32(4, 36 + count * 2, true);
  text(8, "WAVE");
  text(12, "fmt ");
  view.setUint32(16, 16, true);
  view.setUint16(20, 1, true);
  view.setUint16(22, 1, true);
  view.setUint32(24, rate, true);
  view.setUint32(28, rate * 2, true);
  view.setUint16(32, 2, true);
  view.setUint16(34, 16, true);
  text(36, "data");
  view.setUint32(40, count * 2, true);
  for (let i = 0; i < count; i++) {
    const t = i / rate,
      start = t < 0.4 ? 0 : 0.45,
      local = t - start,
      frequency = start === 0 ? 440 : 660;
    const envelope =
      local >= 0 && local < 0.35
        ? Math.min(1, local / 0.025, (0.35 - local) / 0.07)
        : 0;
    view.setInt16(
      44 + i * 2,
      Math.round(
        Math.sin(2 * Math.PI * frequency * local) * envelope * 0.22 * 32767,
      ),
      true,
    );
  }
  return new Blob([bytes], { type: "audio/wav" });
}
