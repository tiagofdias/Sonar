// The RNNoise build only starts where it recognizes a window or a worker. An
// audio worklet is neither, though it has everything the build needs. This
// must be imported first.
globalThis.WorkerGlobalScope ||= class {};
