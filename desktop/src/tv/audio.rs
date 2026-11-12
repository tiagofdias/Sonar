//! TV sound for the call: the channel's AAC (including HE-AAC) is decoded with
//! the decoder built into Windows, turned into 48 kHz stereo and encoded as
//! Opus at a music bitrate. Browsers cannot receive AAC over WebRTC, so this is
//! the one part of a TV share that is converted rather than forwarded as-is.
use opus::{Application, Bitrate, Channels, Encoder};
use std::mem::ManuallyDrop;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

/// Windows' AAC decoder (CLSID_CMSAACDecMFT).
const AAC_DECODER: GUID = GUID::from_u128(0x32d186a7_218f_4c75_8876_dd77273a8999);
pub const RATE: u32 = 48_000;
pub const FRAME: usize = 960; // 20 ms per channel
const BITRATE: i32 = 160_000;

fn ctx<T>(what: &str, result: windows::core::Result<T>) -> Result<T, String> { result.map_err(|e| format!("{what}: {e}")) }

pub struct AacDecoder { transform: IMFTransform, buffer: u32, float: bool, pub rate: u32, pub channels: u32 }

impl AacDecoder {
    /// For ADTS frames (what MPEG-TS carries) at this sample rate and channel count.
    pub fn new(sample_rate: u32, channels: u8) -> Result<Self, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            ctx("Media Foundation", MFStartup(MF_VERSION, MFSTARTUP_LITE))?;
            let transform: IMFTransform = ctx("The Windows AAC decoder is unavailable", CoCreateInstance(&AAC_DECODER, None, CLSCTX_INPROC_SERVER))?;
            let input = ctx("Audio type", MFCreateMediaType())?;
            ctx("Audio type", input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio))?;
            ctx("Audio type", input.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_AAC))?;
            ctx("Audio type", input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, sample_rate))?;
            ctx("Audio type", input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels.max(1) as u32))?;
            ctx("Audio type", input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16))?;
            ctx("Audio type", input.SetUINT32(&MF_MT_AAC_PAYLOAD_TYPE, 1))?; // ADTS
            ctx("Audio type", input.SetUINT32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, 0xFE))?;
            // HEAACWAVEINFO after WAVEFORMATEX: payload type 1 (ADTS), profile 0xFE, the rest zero.
            let info: [u8; 12] = [1, 0, 0xFE, 0, 0, 0, 0, 0, 0, 0, 0, 0];
            ctx("Audio type", input.SetBlob(&MF_MT_USER_DATA, &info))?;
            ctx("This channel's sound format is not supported", transform.SetInputType(0, &input, 0))?;
            let mut decoder = Self { transform, buffer: 0, float: false, rate: sample_rate, channels: channels as u32 };
            decoder.choose_output()?;
            ctx("Audio decoder", decoder.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
            ctx("Audio decoder", decoder.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
            Ok(decoder)
        }
    }
    fn choose_output(&mut self) -> Result<(), String> {
        unsafe {
            let mut fallback = None;
            for index in 0.. {
                let Ok(kind) = self.transform.GetOutputAvailableType(0, index) else { break };
                let subtype = ctx("Audio output", kind.GetGUID(&MF_MT_SUBTYPE))?;
                if subtype == MFAudioFormat_Float { fallback = Some((kind, true)); break; }
                if subtype == MFAudioFormat_PCM && kind.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE) == Ok(16) && fallback.is_none() { fallback = Some((kind, false)); }
            }
            let (kind, float) = fallback.ok_or("The AAC decoder offered no usable output")?;
            ctx("Audio output", self.transform.SetOutputType(0, &kind, 0))?;
            self.float = float;
            self.rate = kind.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap_or(self.rate);
            self.channels = kind.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap_or(self.channels).max(1);
            self.buffer = ctx("Audio output", self.transform.GetOutputStreamInfo(0))?.cbSize.max(64 * 1024);
            Ok(())
        }
    }
    /// Decodes one ADTS frame, appending interleaved samples at `rate` / `channels`.
    pub fn decode(&mut self, frame: &[u8], out: &mut Vec<f32>) -> Result<(), String> {
        unsafe {
            let buffer = ctx("Audio input", MFCreateMemoryBuffer(frame.len() as u32))?;
            let (mut data, mut length) = (std::ptr::null_mut(), 0u32);
            ctx("Audio input", buffer.Lock(&mut data, Some(&mut length), None))?;
            std::ptr::copy_nonoverlapping(frame.as_ptr(), data, frame.len());
            let _ = buffer.Unlock();
            ctx("Audio input", buffer.SetCurrentLength(frame.len() as u32))?;
            let sample = ctx("Audio input", MFCreateSample())?;
            ctx("Audio input", sample.AddBuffer(&buffer))?;
            if let Err(error) = self.transform.ProcessInput(0, &sample, 0) {
                // A damaged frame is skipped rather than stopping the sound.
                if error.code() != MF_E_NOTACCEPTING { return Ok(()); }
            }
            loop {
                let target = ctx("Audio output", MFCreateMemoryBuffer(self.buffer))?;
                let holder = ctx("Audio output", MFCreateSample())?;
                ctx("Audio output", holder.AddBuffer(&target))?;
                let mut output = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(Some(holder)), ..Default::default() }];
                let mut status = 0;
                let result = self.transform.ProcessOutput(0, &mut output, &mut status);
                drop(ManuallyDrop::take(&mut output[0].pSample));
                drop(ManuallyDrop::take(&mut output[0].pEvents));
                match result {
                    Ok(()) => {
                        let (mut bytes, mut size) = (std::ptr::null_mut(), 0u32);
                        ctx("Audio output", target.Lock(&mut bytes, None, Some(&mut size)))?;
                        let raw = std::slice::from_raw_parts(bytes, size as usize);
                        if self.float { out.extend(raw.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))); }
                        else { out.extend(raw.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)); }
                        let _ = target.Unlock();
                    }
                    Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE => self.choose_output()?,
                    Err(error) => return Err(format!("Audio decoder: {error}")),
                }
            }
        }
    }
}

/// Any channel layout to stereo; 5.1 uses the usual ITU downmix.
pub fn to_stereo(samples: &[f32], channels: u32, out: &mut Vec<f32>) {
    match channels {
        1 => for &s in samples { out.extend([s, s]); },
        2 => out.extend_from_slice(samples),
        6 => for f in samples.chunks_exact(6) {
            // L R C LFE Ls Rs
            let (c, k) = (f[2] * 0.7071, 0.7071);
            out.extend([(f[0] + c + f[4] * k).clamp(-1.0, 1.0), (f[1] + c + f[5] * k).clamp(-1.0, 1.0)]);
        },
        n => for f in samples.chunks_exact(n.max(1) as usize) { out.extend([f[0], *f.get(1).unwrap_or(&f[0])]); },
    }
}

/// Stereo resampling to 48 kHz (linear; TV audio is nearly always 48 kHz already).
pub struct Resampler { from: u32, position: f64, last: [f32; 2] }
impl Resampler {
    pub fn new(from: u32) -> Self { Self { from, position: 0.0, last: [0.0; 2] } }
    pub fn push(&mut self, stereo: &[f32], out: &mut Vec<f32>) {
        if self.from == RATE { out.extend_from_slice(stereo); return; }
        let frames = stereo.len() / 2;
        let step = self.from as f64 / RATE as f64;
        let at = |i: isize| -> [f32; 2] { if i < 0 { self.last } else { [stereo[i as usize * 2], stereo[i as usize * 2 + 1]] } };
        while self.position < frames as f64 - 1.0 {
            let i = self.position.floor() as isize;
            let t = (self.position - i as f64) as f32;
            let (a, b) = (at(i), at(i + 1));
            out.extend([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            self.position += step;
        }
        if frames > 0 { self.last = [stereo[(frames - 1) * 2], stereo[(frames - 1) * 2 + 1]]; self.position -= frames as f64; }
    }
}

/// 48 kHz stereo to 20 ms Opus packets at a music bitrate.
pub struct OpusMusic { encoder: Encoder, pending: Vec<f32> }
impl OpusMusic {
    pub fn new() -> Result<Self, String> {
        let mut encoder = Encoder::new(RATE, Channels::Stereo, Application::Audio).map_err(|_| "Opus is unavailable")?;
        encoder.set_bitrate(Bitrate::Bits(BITRATE)).map_err(|_| "Opus could not be set up")?;
        Ok(Self { encoder, pending: Vec::new() })
    }
    pub fn push(&mut self, stereo: &[f32], on_packet: &mut dyn FnMut(Vec<u8>)) {
        self.pending.extend(stereo.iter().map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 }));
        while self.pending.len() >= FRAME * 2 {
            let frame: Vec<f32> = self.pending.drain(..FRAME * 2).collect();
            let mut packet = vec![0u8; 4000];
            if let Ok(length) = self.encoder.encode_float(&frame, &mut packet) { packet.truncate(length); on_packet(packet); }
        }
    }
}

/// The whole chain for one channel: ADTS frames in, Opus packets and 48 kHz stereo PCM out.
pub struct TvAudio { decoder: AacDecoder, resampler: Resampler, opus: OpusMusic, scratch: Vec<f32>, stereo: Vec<f32> }
impl TvAudio {
    pub fn new(sample_rate: u32, channels: u8) -> Result<Self, String> {
        let decoder = AacDecoder::new(sample_rate, channels)?;
        let resampler = Resampler::new(decoder.rate);
        Ok(Self { decoder, resampler, opus: OpusMusic::new()?, scratch: Vec::new(), stereo: Vec::new() })
    }
    /// `on_pcm` gets the 48 kHz stereo sound (for playing it here); `on_packet` the Opus for the call.
    pub fn push(&mut self, adts: &[u8], on_pcm: &mut dyn FnMut(&[f32]), on_packet: &mut dyn FnMut(Vec<u8>)) -> Result<(), String> {
        self.scratch.clear();
        self.decoder.decode(adts, &mut self.scratch)?;
        if self.decoder.rate != self.resampler.from { self.resampler = Resampler::new(self.decoder.rate); }
        let mut stereo = Vec::with_capacity(self.scratch.len());
        to_stereo(&self.scratch, self.decoder.channels, &mut stereo);
        self.stereo.clear();
        self.resampler.push(&stereo, &mut self.stereo);
        if !self.stereo.is_empty() { on_pcm(&self.stereo); }
        self.opus.push(&self.stereo, on_packet);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tv::demux::{Demuxer, Unit};
    fn crossings(samples: &[f32]) -> usize { samples.windows(2).filter(|w| w[0] < 0.0 && w[1] >= 0.0).count() }
    #[test]
    fn aac_from_a_channel_becomes_stereo_opus_with_the_same_tone() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/h264_high_50fps.ts")).unwrap();
        let mut demuxer = Demuxer::new();
        let mut units = Vec::new();
        demuxer.push(&bytes, &mut units);
        demuxer.finish(&mut units);
        let mut audio = None;
        let (mut pcm, mut packets) = (Vec::new(), Vec::new());
        for unit in &units {
            let Unit::Audio { data, sample_rate, channels, .. } = unit else { continue };
            let chain = audio.get_or_insert_with(|| TvAudio::new(*sample_rate, *channels).unwrap());
            chain.push(data, &mut |s| pcm.extend_from_slice(s), &mut |p| packets.push(p)).unwrap();
        }
        let seconds = pcm.len() as f64 / 2.0 / RATE as f64;
        assert!(seconds > 0.8, "about a second of sound: {seconds}");
        let left: Vec<f32> = pcm.iter().step_by(2).copied().collect();
        let tone = crossings(&left[RATE as usize / 10..]) as f64 / (left.len() as f64 / RATE as f64 - 0.1);
        assert!((tone - 440.0).abs() < 15.0, "the 440 Hz test tone survives decoding: {tone}");
        assert!(packets.len() >= 40, "20 ms Opus packets: {}", packets.len());
        let mut decoder = opus::Decoder::new(RATE, Channels::Stereo).unwrap();
        let mut back = Vec::new();
        for packet in &packets { let mut out = vec![0f32; FRAME * 2]; let n = decoder.decode_float(packet, &mut out, false).unwrap(); back.extend_from_slice(&out[..n * 2]); }
        let left: Vec<f32> = back.iter().step_by(2).copied().collect();
        let tone = crossings(&left[RATE as usize / 10..]) as f64 / (left.len() as f64 / RATE as f64 - 0.1);
        assert!((tone - 440.0).abs() < 15.0, "and survives Opus too: {tone}");
        let average = packets.iter().map(Vec::len).sum::<usize>() as f64 / packets.len() as f64;
        assert!(average > 150.0, "a music bitrate, not a voice one: {average} bytes per packet");
    }
    #[test]
    fn channel_layouts_and_rates_become_48k_stereo() {
        let mut out = Vec::new();
        to_stereo(&[0.5, -0.5], 1, &mut out);
        assert_eq!(out, [0.5, 0.5, -0.5, -0.5]);
        out.clear();
        to_stereo(&[0.1, 0.2, 0.4, 0.9, 0.0, 0.0], 6, &mut out);
        assert!((out[0] - (0.1 + 0.4 * 0.7071)).abs() < 1e-4 && (out[1] - (0.2 + 0.4 * 0.7071)).abs() < 1e-4, "{out:?}");
        let mut resampler = Resampler::new(44_100);
        let input: Vec<f32> = (0..44_100 * 2).map(|i| (i / 2) as f32 / 44_100.0).collect();
        let mut resampled = Vec::new();
        resampler.push(&input, &mut resampled);
        assert!((resampled.len() as i64 / 2 - 48_000).abs() < 5, "{}", resampled.len() / 2);
    }
}
