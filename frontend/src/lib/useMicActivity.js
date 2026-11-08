import { useEffect, useState } from "react";

const SPEAKING_RMS = 0.01;
const HOLD_MS = 300;

// True while the local microphone is picking up sound. The audio is only
// measured here: it is never connected to an output or stored.
export function useMicActivity(getStream, enabled) {
  const [active, setActive] = useState(false);
  useEffect(() => {
    if (!enabled || !getStream) {
      setActive(false);
      return undefined;
    }
    let context, source, analyser, current = null, until = 0;
    const samples = new Float32Array(1024);
    const timer = setInterval(() => {
      const stream = getStream();
      if (stream !== current) {
        source?.disconnect();
        source = analyser = undefined;
        current = stream;
        if (stream) {
          context ||= new AudioContext();
          context.resume().catch(() => {});
          source = context.createMediaStreamSource(stream);
          analyser = context.createAnalyser();
          analyser.fftSize = samples.length;
          source.connect(analyser);
        }
      }
      if (!analyser) return;
      analyser.getFloatTimeDomainData(samples);
      const rms = Math.sqrt(samples.reduce((sum, value) => sum + value * value, 0) / samples.length);
      const now = performance.now();
      if (rms > SPEAKING_RMS) until = now + HOLD_MS;
      setActive(now < until);
    }, 100);
    return () => {
      clearInterval(timer);
      source?.disconnect();
      context?.close().catch(() => {});
      setActive(false);
    };
  }, [enabled, getStream]);
  return active;
}
