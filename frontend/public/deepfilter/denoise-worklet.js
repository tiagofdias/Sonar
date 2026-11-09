// Runs DeepFilterNet3 on the audio thread: the same engine, compiled from the
// same Rust, as the desktop app. Web Audio delivers 128 samples at a time and
// the model needs 480, so samples are queued in and out.
const FRAME = 480;
const CAPACITY = FRAME * 8;

class Denoise extends AudioWorkletProcessor {
  constructor(options) {
    super();
    this.enabled = true;
    this.input = new Float32Array(FRAME);
    this.filled = 0;
    this.output = new Float32Array(CAPACITY);
    this.read = 0;
    this.available = 0;
    this.primed = false;
    this.port.onmessage = ({ data }) => {
      if (typeof data?.enabled === "boolean") this.enabled = data.enabled;
    };
    try {
      this.exports = new WebAssembly.Instance(options.processorOptions.module, {}).exports;
      this.engine = this.exports.df_create();
      // The first frame is the slowest; get it out of the way before audio flows.
      this.exports.df_process(this.engine);
      this.port.postMessage({ ready: true });
    } catch (error) {
      this.engine = null;
      this.port.postMessage({ failed: String(error?.stack || error) });
    }
  }

  finishFrame() {
    const frame = this.input;
    if (this.engine && this.enabled) {
      const pointer = this.exports.df_frame(this.engine);
      new Float32Array(this.exports.memory.buffer, pointer, FRAME).set(frame);
      this.exports.df_process(this.engine);
      frame.set(new Float32Array(this.exports.memory.buffer, pointer, FRAME));
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
