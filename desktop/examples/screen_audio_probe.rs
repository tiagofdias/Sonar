// Development check for per-app sound capture. Starts two child processes that
// each play one very quiet tone, then proves capture hears only the chosen one.
#[path = "../src/screen_audio.rs"]
mod screen_audio;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use screen_audio::Sound;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

fn tone(frequency: f32) {
    let device = cpal::default_host().default_output_device().expect("speakers");
    let config = device.default_output_config().expect("config");
    let (rate, channels) = (config.sample_rate().0 as f32, config.channels() as usize);
    let mut n = 0f32;
    let stream = device.build_output_stream(&config.into(), move |data: &mut [f32], _| {
        for frame in data.chunks_mut(channels) { n += 1.0; frame.fill((n / rate * frequency * std::f32::consts::TAU).sin() * 0.004); }
    }, |_| {}, None).expect("stream");
    stream.play().expect("play");
    std::thread::sleep(Duration::from_secs(14));
}

fn power(samples: &[f32], frequency: f32) -> f32 {
    let w = std::f32::consts::TAU * frequency / 48_000.0;
    let (mut re, mut im) = (0.0, 0.0);
    for (i, s) in samples.iter().enumerate() { re += s * (w * i as f32).cos(); im += s * (w * i as f32).sin(); }
    (re * re + im * im).sqrt() / samples.len().max(1) as f32
}

fn capture(sound: Sound, seconds: u64) -> Result<Vec<f32>, String> {
    let stop = AtomicBool::new(false);
    let mut all = Vec::new();
    let started = Instant::now();
    std::thread::scope(|scope| {
        scope.spawn(|| { std::thread::sleep(Duration::from_secs(seconds)); stop.store(true, Ordering::Release); });
        screen_audio::run(sound, &stop, &mut |frame| if started.elapsed() > Duration::from_millis(600) { all.extend_from_slice(frame); })
    })?;
    Ok(all)
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--tone") { tone(args[1].parse().unwrap()); return; }
    let me = std::env::current_exe().unwrap();
    let spawn = |f: &str| Command::new(&me).args(["--tone", f]).stdout(Stdio::null()).spawn().unwrap();
    let (mut a, mut b) = (spawn("440"), spawn("1000"));
    std::thread::sleep(Duration::from_millis(1200));
    let report = |label: &str, result: Result<Vec<f32>, String>| match result {
        Ok(samples) => println!("{label}: samples={} 440Hz={:.5} 1000Hz={:.5}", samples.len(), power(&samples, 440.0), power(&samples, 1000.0)),
        Err(error) => println!("{label}: ERROR {error}"),
    };
    report("only app A (440)", capture(Sound::OnlyProcess(a.id()), 3));
    report("only app B (1000)", capture(Sound::OnlyProcess(b.id()), 3));
    report("everything except app A", capture(Sound::AllExceptProcess(a.id()), 3));
    let _ = a.kill(); let _ = b.kill();
    std::thread::sleep(Duration::from_millis(800));
    report("nothing of ours playing (other apps only)", capture(Sound::AllExceptProcess(std::process::id()), 3));
}
