// Development check: mixes real recorded speech with noise and measures how
// much RNNoise removes. Usage: denoise_probe <speech.wav, 48 kHz mono 16-bit>
use nnnoiseless::DenoiseState;

fn rms(samples: &[f32]) -> f32 { (samples.iter().map(|s| s * s).sum::<f32>() / samples.len().max(1) as f32).sqrt() }
fn db(ratio: f32) -> f32 { 20.0 * ratio.max(1e-6).log10() }

fn denoise(input: &[f32]) -> Vec<f32> {
    let mut state = DenoiseState::new();
    let mut out = vec![0f32; input.len()];
    for (from, to) in input.chunks_exact(480).zip(out.chunks_exact_mut(480)) { state.process_frame(to, from); }
    out
}

fn main() {
    let path = std::env::args().nth(1).expect("wav path");
    let bytes = std::fs::read(path).expect("read");
    let at = bytes.windows(4).position(|w| w == b"data").expect("data chunk") + 8;
    let speech: Vec<f32> = bytes[at..].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]]) as f32 * 0.6).collect();
    let (mut seed, mut fan) = (12345u32, 0f32);
    let mut rand = move || { seed = seed.wrapping_mul(1664525).wrapping_add(1013904223); (seed >> 8) as f32 / 8388608.0 - 1.0 };
    let tail = 48_000 * 3; // Three seconds of noise alone after the speech.
    let total = speech.len() + tail;
    let noise_sets: [(&str, Vec<f32>); 3] = [
        ("fan / air", (0..total).map(|_| { fan += 0.05 * (rand() * 9000.0 - fan); fan * 2.0 }).collect()),
        ("white hiss", (0..total).map(|_| rand() * 1800.0).collect()),
        ("keyboard clicks", (0..total).map(|i| if i % 7000 < 300 { rand() * 9000.0 * (1.0 - (i % 7000) as f32 / 300.0) } else { rand() * 40.0 }).collect()),
    ];
    for (name, noise) in noise_sets {
        let mixed: Vec<f32> = (0..total).map(|i| speech.get(i).copied().unwrap_or(0.0) + noise[i]).collect();
        let cleaned = denoise(&mixed);
        // The start is skipped: the network needs a moment to settle.
        let (noise_before, noise_after) = (rms(&mixed[speech.len() + 48_000..]), rms(&cleaned[speech.len() + 48_000..]));
        let voiced: Vec<usize> = (24_000..speech.len()).step_by(480).filter(|i| i + 480 <= speech.len() && rms(&speech[*i..i + 480]) > 1200.0).collect();
        let level = |signal: &[f32]| rms(&voiced.iter().flat_map(|i| signal[*i..i + 480].iter().copied()).collect::<Vec<_>>());
        println!("{name:16} noise {:6.1} dB  |  speech kept {:5.1} dB  ({} voiced frames)", db(noise_after / noise_before), db(level(&cleaned) / level(&mixed)), voiced.len());
    }
}
