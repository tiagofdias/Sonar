// Runs RNNoise on the audio thread. Web Audio delivers 128 samples at a time and
// RNNoise needs 480, so samples are queued in and out; this adds 10 ms of delay.
import "./worklet-shim.js";
import { Rnnoise } from "./rnnoise.js";

const FRAME = 480;
const CAPACITY = FRAME * 8;

class Denoise extends AudioWorkletProcessor {
  constructor() {
    super();
    this.enabled = true;
    this.state = null;
    this.input = new Float32Array(FRAME);
    this.filled = 0;
    this.output = new Float32Array(CAPACITY);
    this.read = 0;
    this.available = 0;
    this.primed = false;
    this.port.onmessage = ({ data }) => {
      if (typeof data?.enabled === "boolean") this.enabled = data.enabled;
    };
    Rnnoise.load()
      .then((rnnoise) => {
        this.state = rnnoise.createDenoiseState();
        this.port.postMessage({ ready: true });
      })
      .catch((error) => this.port.postMessage({ failed: String(error?.stack || error) }));
  }

  finishFrame() {
    const frame = this.input;
    if (this.state && this.enabled) {
      for (let i = 0; i < FRAME; i++) frame[i] *= 32768;
      this.state.processFrame(frame);
      for (let i = 0; i < FRAME; i++) frame[i] = Math.max(-1, Math.min(1, frame[i] / 32768));
    }
    for (let i = 0; i < FRAME; i++) {
      this.output[(this.read + this.available) % CAPACITY] = frame[i];
      if (this.available < CAPACITY) this.available++;
      else this.read = (this.read + 1) % CAPACITY;
    }
    this.filled = 0;
  }

  process(inputs, outputs) {
    const source = inputs[0]?.[0];
    const target = outputs[0]?.[0];
    if (!target) return true;
    const length = target.length;
    for (let i = 0; i < length; i++) {
      this.input[this.filled++] = source ? source[i] : 0;
      if (this.filled === FRAME) this.finishFrame();
    }
    // One frame is held back so a burst of quanta never runs the output dry.
    if (!this.primed && this.available >= FRAME) this.primed = true;
    for (let i = 0; i < length; i++) {
      if (this.primed && this.available > 0) {
        target[i] = this.output[this.read];
        this.read = (this.read + 1) % CAPACITY;
        this.available--;
      } else target[i] = 0;
    }
    return true;
  }
}

registerProcessor("sonar-denoise", Denoise);
