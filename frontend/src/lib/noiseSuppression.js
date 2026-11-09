import { useVoiceSettingsStore } from "../store/useVoiceSettingsStore";

const passthrough = (stream) => ({ stream, close: () => {} });

// The gzip-compressed engine is downloaded once, on the first call or room. It is
// not named .gz so that no server marks it as already-compressed transfer encoding.
let engine;
const loadEngine = () => {
  engine ||= fetch("/deepfilter/engine.bin")
    .then((response) => {
      if (!response.ok) throw new Error("Noise suppression download failed");
      return new Response(response.body.pipeThrough(new DecompressionStream("gzip"))).arrayBuffer();
    })
    .then((bytes) => WebAssembly.compile(bytes))
    .catch((error) => { engine = undefined; throw error; });
  return engine;
};

// Runs the microphone through DeepFilterNet3, the same engine the desktop app uses.
// Muting still acts on the raw microphone track, so a muted mic sends silence.
export async function denoisedStream(raw) {
  const settings = useVoiceSettingsStore;
  if (!settings.getState().noiseSuppression) return passthrough(raw);
  let context;
  try {
    const module = await loadEngine();
    context = new AudioContext({ sampleRate: 48000, latencyHint: "interactive" });
    await context.audioWorklet.addModule("/deepfilter/denoise-worklet.js");
    const node = new AudioWorkletNode(context, "sonar-denoise", { numberOfInputs: 1, numberOfOutputs: 1, outputChannelCount: [1], processorOptions: { module } });
    await new Promise((resolve, reject) => {
      const timer = setTimeout(() => reject(new Error("Noise suppression timed out")), 8000);
      node.port.onmessage = ({ data }) => {
        clearTimeout(timer);
        if (data?.ready) resolve();
        else reject(new Error("Noise suppression failed"));
      };
    });
    const source = context.createMediaStreamSource(raw);
    const target = context.createMediaStreamDestination();
    source.connect(node);
    node.connect(target);
    await Promise.race([context.resume(), new Promise((resolve) => setTimeout(resolve, 1000))]);
    const stopWatching = settings.subscribe((next, previous) => {
      if (next.noiseSuppression !== previous.noiseSuppression) node.port.postMessage({ enabled: next.noiseSuppression });
    });
    return {
      stream: target.stream,
      close: () => {
        stopWatching();
        source.disconnect();
        node.disconnect();
        target.stream.getTracks().forEach((track) => track.stop());
        context.close().catch(() => {});
      },
    };
  } catch (error) {
    console.warn("Sonar noise suppression fell back to the browser filter", error);
    context?.close().catch(() => {});
    // The engine could not start: fall back to the browser's own filter.
    await Promise.all(raw.getAudioTracks().map((track) => track.applyConstraints({ noiseSuppression: true }).catch(() => {})));
    return passthrough(raw);
  }
}
