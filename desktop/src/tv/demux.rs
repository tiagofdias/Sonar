//! MPEG-TS (the container IPTV channels use) split into video and audio. Video
//! access units come out byte for byte as the provider sent them, so they can be
//! forwarded without re-encoding. Audio comes out as single AAC (ADTS) frames.
use std::collections::HashMap;

pub const PACKET: usize = 188;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoCodec { H264, H265 }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec { AacAdts, AacLatm }

#[derive(Debug, Clone, PartialEq)]
pub enum Unit {
    /// One access unit in Annex-B form (start codes kept), exactly as received.
    Video { codec: VideoCodec, pts: Option<u64>, dts: Option<u64>, key: bool, data: Vec<u8> },
    /// One AAC frame with its ADTS header.
    Audio { codec: AudioCodec, pts: Option<u64>, sample_rate: u32, channels: u8, data: Vec<u8> },
}

struct Pes { data: Vec<u8> }

#[derive(Default)]
pub struct Demuxer {
    pmt: Option<u16>,
    video: Option<(u16, VideoCodec)>,
    audio: Option<(u16, AudioCodec)>,
    open: HashMap<u16, Pes>,
    carry: Vec<u8>,
}

const MAX_PES: usize = 8 * 1024 * 1024;

impl Demuxer {
    pub fn new() -> Self { Self::default() }
    pub fn video_codec(&self) -> Option<VideoCodec> { self.video.map(|(_, c)| c) }

    /// Feed any amount of bytes; complete units are appended to `out`.
    pub fn push(&mut self, bytes: &[u8], out: &mut Vec<Unit>) {
        self.carry.extend_from_slice(bytes);
        let mut at = 0;
        while self.carry.len() - at >= PACKET {
            if self.carry[at] != 0x47 {
                // Lost sync: skip to the next byte that starts two packets in a row.
                at += 1;
                while self.carry.len() - at >= PACKET * 2 && !(self.carry[at] == 0x47 && self.carry[at + PACKET] == 0x47) { at += 1; }
                continue;
            }
            let packet: [u8; PACKET] = self.carry[at..at + PACKET].try_into().unwrap();
            self.packet(&packet, out);
            at += PACKET;
        }
        self.carry.drain(..at);
    }

    /// Emits whatever is still being collected (for example at the end of a file).
    pub fn finish(&mut self, out: &mut Vec<Unit>) {
        for pid in self.open.keys().copied().collect::<Vec<_>>() { self.flush(pid, out); }
    }

    fn packet(&mut self, p: &[u8; PACKET], out: &mut Vec<Unit>) {
        let start = p[1] & 0x40 != 0;
        let pid = u16::from_be_bytes([p[1] & 0x1f, p[2]]);
        let control = (p[3] >> 4) & 0x3;
        if control & 0x1 == 0 { return; }
        let mut offset = 4;
        if control == 0x3 { offset += 1 + p[4] as usize; }
        if offset >= PACKET { return; }
        let payload = &p[offset..];
        if pid == 0 { if start { self.pat(payload); } return; }
        if Some(pid) == self.pmt { if start { self.pmt_table(payload); } return; }
        let wanted = self.video.map(|(v, _)| v) == Some(pid) || self.audio.map(|(a, _)| a) == Some(pid);
        if !wanted { return; }
        if start {
            self.flush(pid, out);
            self.open.insert(pid, Pes { data: payload.to_vec() });
        } else if let Some(pes) = self.open.get_mut(&pid) {
            if pes.data.len() + payload.len() <= MAX_PES { pes.data.extend_from_slice(payload); } else { self.open.remove(&pid); }
        }
    }

    fn section(payload: &[u8]) -> Option<&[u8]> {
        let pointer = *payload.first()? as usize;
        let table = payload.get(1 + pointer..)?;
        let length = (u16::from_be_bytes([*table.get(1)? & 0x0f, *table.get(2)?]) as usize).min(table.len().saturating_sub(3));
        table.get(..3 + length)
    }
    fn pat(&mut self, payload: &[u8]) {
        let Some(table) = Self::section(payload) else { return };
        if table.first() != Some(&0x00) || table.len() < 12 { return; }
        for entry in table[8..table.len() - 4].chunks_exact(4) {
            let program = u16::from_be_bytes([entry[0], entry[1]]);
            if program != 0 { self.pmt = Some(u16::from_be_bytes([entry[2] & 0x1f, entry[3]])); return; }
        }
    }
    fn pmt_table(&mut self, payload: &[u8]) {
        let Some(table) = Self::section(payload) else { return };
        if table.first() != Some(&0x02) || table.len() < 16 { return; }
        let info = (u16::from_be_bytes([table[10] & 0x0f, table[11]]) as usize).min(table.len());
        let mut at = 12 + info;
        let end = table.len() - 4;
        while at + 5 <= end {
            let kind = table[at];
            let pid = u16::from_be_bytes([table[at + 1] & 0x1f, table[at + 2]]);
            let es = u16::from_be_bytes([table[at + 3] & 0x0f, table[at + 4]]) as usize;
            match kind {
                0x1b if self.video.is_none() => self.video = Some((pid, VideoCodec::H264)),
                0x24 if self.video.is_none() => self.video = Some((pid, VideoCodec::H265)),
                0x0f if self.audio.is_none() => self.audio = Some((pid, AudioCodec::AacAdts)),
                0x11 if self.audio.is_none() => self.audio = Some((pid, AudioCodec::AacLatm)),
                _ => {}
            }
            at += 5 + es;
        }
    }

    fn flush(&mut self, pid: u16, out: &mut Vec<Unit>) {
        let Some(pes) = self.open.remove(&pid) else { return };
        let d = &pes.data;
        if d.len() < 9 || d[0..3] != [0, 0, 1] { return; }
        let flags = d[7] >> 6;
        let header = 9 + d[8] as usize;
        if header > d.len() { return; }
        let pts = (flags & 0x2 != 0 && d.len() >= 14).then(|| timestamp(&d[9..14]));
        let dts = if flags == 0x3 && d.len() >= 19 { Some(timestamp(&d[14..19])) } else { pts };
        let body = &d[header..];
        if self.video.map(|(v, _)| v) == Some(pid) {
            let codec = self.video.unwrap().1;
            if !body.is_empty() { out.push(Unit::Video { codec, pts, dts, key: is_key(codec, body), data: body.to_vec() }); }
        } else if let Some((_, codec)) = self.audio.filter(|(a, _)| *a == pid) {
            match codec {
                AudioCodec::AacAdts => split_adts(body, pts, out),
                AudioCodec::AacLatm => if !body.is_empty() { out.push(Unit::Audio { codec, pts, sample_rate: 48_000, channels: 2, data: body.to_vec() }) },
            }
        }
    }
}

fn timestamp(b: &[u8]) -> u64 {
    (((b[0] as u64 >> 1) & 0x07) << 30) | ((b[1] as u64) << 22) | (((b[2] as u64) >> 1) << 15) | ((b[3] as u64) << 7) | ((b[4] as u64) >> 1)
}

/// NAL units of an Annex-B access unit (without their start codes).
pub fn nal_units(data: &[u8]) -> impl Iterator<Item = &[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 { starts.push(i + 3); i += 3; } else { i += 1; }
    }
    let ends: Vec<usize> = starts.iter().skip(1).map(|&s| { let mut e = s - 3; while e > 0 && data[e - 1] == 0 { e -= 1; } e }).chain(std::iter::once(data.len())).collect();
    starts.into_iter().zip(ends).filter(|(s, e)| e > s).map(move |(s, e)| &data[s..e])
}

pub fn is_key(codec: VideoCodec, data: &[u8]) -> bool {
    nal_units(data).any(|nal| match codec {
        VideoCodec::H264 => nal.first().is_some_and(|b| b & 0x1f == 5),
        VideoCodec::H265 => nal.first().is_some_and(|b| (16..=21).contains(&((b >> 1) & 0x3f))),
    })
}

const RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];
fn split_adts(mut body: &[u8], pts: Option<u64>, out: &mut Vec<Unit>) {
    let mut index = 0u64;
    while body.len() >= 7 && body[0] == 0xff && body[1] & 0xf0 == 0xf0 {
        let length = (((body[3] & 0x03) as usize) << 11) | ((body[4] as usize) << 3) | ((body[5] as usize) >> 5);
        if length < 7 || length > body.len() { break; }
        let Some(&rate) = RATES.get(((body[2] >> 2) & 0x0f) as usize) else { break };
        let channels = ((body[2] & 0x01) << 2) | (body[3] >> 6);
        // Each AAC frame is 1024 samples; frames after the first get their own time.
        let at = pts.map(|p| p + index * 1024 * 90_000 / rate as u64);
        out.push(Unit::Audio { codec: AudioCodec::AacAdts, pts: at, sample_rate: rate, channels, data: body[..length].to_vec() });
        body = &body[length..];
        index += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> Vec<u8> { std::fs::read(format!("{}/tests/tv/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap() }
    fn demux(bytes: &[u8], chunk: usize) -> (Vec<Unit>, Option<VideoCodec>) {
        let mut demuxer = Demuxer::new();
        let mut out = Vec::new();
        for piece in bytes.chunks(chunk) { demuxer.push(piece, &mut out); }
        demuxer.finish(&mut out);
        (out, demuxer.video_codec())
    }
    #[test]
    fn h264_video_comes_out_byte_for_byte() {
        // Generated with ffmpeg: 1 s of 320x180 H.264 High at 50 fps with AAC, and the
        // same video copied out as a raw Annex-B stream (tests/tv/README.txt).
        let (units, codec) = demux(&fixture("h264_high_50fps.ts"), 1000);
        assert_eq!(codec, Some(VideoCodec::H264));
        let video: Vec<_> = units.iter().filter_map(|u| if let Unit::Video { data, key, .. } = u { Some((data, *key)) } else { None }).collect();
        assert_eq!(video.len(), 50, "one access unit per frame");
        assert!(video[0].1, "starts on a keyframe");
        let joined: Vec<u8> = video.iter().flat_map(|(d, _)| d.iter().copied()).collect();
        assert_eq!(joined, fixture("h264_high_50fps.h264"), "forwarded video is identical to the source");
        let first: Vec<u8> = nal_units(video[0].0).map(|n| n[0] & 0x1f).collect();
        assert!(first.contains(&7) && first.contains(&8) && first.contains(&5), "SPS, PPS and IDR in the first unit: {first:?}");
        let sps = nal_units(video[0].0).find(|n| n[0] & 0x1f == 7).unwrap();
        assert_eq!(sps[1], 100, "High profile");
        let pts: Vec<u64> = units.iter().filter_map(|u| if let Unit::Video { pts, .. } = u { *pts } else { None }).collect();
        assert!(pts.windows(2).filter(|w| w[1] > w[0]).count() > 0);
    }
    #[test]
    fn aac_audio_is_split_into_timed_frames() {
        let (units, _) = demux(&fixture("h264_high_50fps.ts"), 188 * 7);
        let audio: Vec<_> = units.iter().filter_map(|u| if let Unit::Audio { pts, sample_rate, channels, data, .. } = u { Some((*pts, *sample_rate, *channels, data.len())) } else { None }).collect();
        assert!(audio.len() >= 40, "about 47 frames in a second at 48 kHz, got {}", audio.len());
        assert!(audio.iter().all(|(_, rate, channels, len)| *rate == 48_000 && *channels == 2 && *len > 7));
        let times: Vec<u64> = audio.iter().filter_map(|a| a.0).collect();
        assert!(times.windows(2).all(|w| w[1] > w[0]), "each frame has a later time");
        assert_eq!(times[1] - times[0], 1920, "1024 samples at 48 kHz is 1920 ticks of 90 kHz");
    }
    #[test]
    fn h265_channels_are_recognised() {
        let (units, codec) = demux(&fixture("h265.ts"), 4096);
        assert_eq!(codec, Some(VideoCodec::H265));
        assert!(matches!(units.iter().find(|u| matches!(u, Unit::Video { .. })), Some(Unit::Video { key: true, .. })));
    }
    #[test]
    fn garbage_and_lost_sync_do_not_break_it() {
        let mut bytes = vec![0x12; 300];
        bytes.extend(fixture("h264_high_50fps.ts"));
        let (units, _) = demux(&bytes, 777);
        assert_eq!(units.iter().filter(|u| matches!(u, Unit::Video { .. })).count(), 50);
        let (none, _) = demux(&[0x47; 5000], 100);
        assert!(none.is_empty());
    }
}
