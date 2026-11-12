//! Turns received H.264 RTP packets into pictures with the decoder built into Windows.
#![allow(dead_code)]
use std::mem::ManuallyDrop;
use windows::core::GUID;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};

const START: [u8; 4] = [0, 0, 0, 1];

/// Reassembles H.264 access units from RTP payloads (single, STAP-A and FU-A).
#[derive(Default)]
pub struct Depacketizer { unit: Vec<u8> }
impl Depacketizer {
    /// Adds one payload. Returns the finished access unit on the marker packet.
    pub fn push(&mut self, payload: &[u8], marker: bool) -> Option<Vec<u8>> {
        if self.unit.len() > 8 * 1024 * 1024 { self.unit.clear(); }
        match payload.first().map(|byte| byte & 0x1f) {
            Some(1..=23) => { self.unit.extend_from_slice(&START); self.unit.extend_from_slice(payload); }
            Some(24) => {
                let mut rest = &payload[1..];
                while rest.len() > 2 {
                    let size = u16::from_be_bytes([rest[0], rest[1]]) as usize;
                    if size == 0 || size > rest.len() - 2 { break; }
                    self.unit.extend_from_slice(&START);
                    self.unit.extend_from_slice(&rest[2..2 + size]);
                    rest = &rest[2 + size..];
                }
            }
            Some(28) if payload.len() > 2 => {
                if payload[1] & 0x80 != 0 {
                    self.unit.extend_from_slice(&START);
                    self.unit.push((payload[0] & 0xe0) | (payload[1] & 0x1f));
                }
                self.unit.extend_from_slice(&payload[2..]);
            }
            _ => {}
        }
        (marker && !self.unit.is_empty()).then(|| std::mem::take(&mut self.unit))
    }
}

/// BT.709 limited-range NV12 to RGBA, into a reused buffer.
pub fn nv12_to_rgba(y: &[u8], uv: &[u8], stride: usize, width: usize, height: usize, out: &mut Vec<u8>) {
    out.resize(width * height * 4, 255);
    for (row, line) in out.chunks_exact_mut(width * 4).enumerate().take(height) {
        let luma = &y[row * stride..row * stride + width];
        let chroma = &uv[(row / 2) * stride..(row / 2) * stride + (width & !1)];
        for (col, pixel) in line.chunks_exact_mut(4).enumerate() {
            let c = (luma[col] as i32 - 16) * 298;
            let (u, v) = (chroma[col & !1] as i32 - 128, chroma[(col & !1) + 1] as i32 - 128);
            pixel[0] = ((c + 459 * v + 128) >> 8).clamp(0, 255) as u8;
            pixel[1] = ((c - 55 * u - 136 * v + 128) >> 8).clamp(0, 255) as u8;
            pixel[2] = ((c + 541 * u + 128) >> 8).clamp(0, 255) as u8;
        }
    }
}

fn split(value: u64) -> (u32, u32) { ((value >> 32) as u32, value as u32) }
fn ctx<T>(context: &str, result: windows::core::Result<T>) -> Result<T, String> { result.map_err(|e| format!("{context}: {e}")) }

pub struct Decoder { transform: IMFTransform, buffer_size: u32, stride: usize, coded_height: usize, width: usize, height: usize, time: i64 }

impl Decoder {
    /// For screen shares: pictures come out at once, in the order they arrive.
    pub fn new() -> Result<Self, String> { Self::create(true) }
    /// For TV, whose channels use B-frames (sent out of display order): Windows puts
    /// the pictures back in display order, at the cost of a few frames of delay.
    pub fn reordering() -> Result<Self, String> { Self::create(false) }
    /// H.265 channels: Windows has a decoder only with the free "HEVC Video
    /// Extensions" (or one from the graphics driver), so it is looked up.
    pub fn hevc() -> Result<Self, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            ctx("Media Foundation", MFStartup(MF_VERSION, MFSTARTUP_LITE))?;
            let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_HEVC };
            let (mut list, mut count) = (std::ptr::null_mut(), 0u32);
            let missing = || "This channel uses H.265. Install the free \"HEVC Video Extensions\" from the Microsoft Store to share it.".to_string();
            if MFTEnumEx(MFT_CATEGORY_VIDEO_DECODER, MFT_ENUM_FLAG_SYNCMFT | MFT_ENUM_FLAG_SORTANDFILTER, Some(&input), None, &mut list, &mut count).is_err() || count == 0 { return Err(missing()); }
            let activates = std::slice::from_raw_parts(list, count as usize);
            let found = activates.iter().flatten().find_map(|a| a.ActivateObject::<IMFTransform>().ok());
            for index in 0..count as usize { std::ptr::drop_in_place(list.add(index)); }
            windows::Win32::System::Com::CoTaskMemFree(Some(list as _));
            Self::with(found.ok_or_else(missing)?, MFVideoFormat_HEVC, false, Some((1920, 1080)))
        }
    }
    fn create(low_latency: bool) -> Result<Self, String> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            ctx("Media Foundation", MFStartup(MF_VERSION, MFSTARTUP_LITE))?;
            let transform: IMFTransform = ctx("The Windows video decoder is unavailable", CoCreateInstance(&CLSID_MSH264DecoderMFT, None, CLSCTX_INPROC_SERVER))?;
            Self::with(transform, MFVideoFormat_H264, low_latency, None)
        }
    }
    /// `size` is a first guess for decoders that need one before seeing the stream; a
    /// different real size arrives as a format change.
    fn with(transform: IMFTransform, format: GUID, low_latency: bool, size: Option<(u32, u32)>) -> Result<Self, String> {
        unsafe {
            if let Ok(attributes) = transform.GetAttributes() { let _ = attributes.SetUINT32(&MF_LOW_LATENCY, low_latency as u32); }
            let input = ctx("Decoder", MFCreateMediaType())?;
            ctx("Decoder", input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            ctx("Decoder", input.SetGUID(&MF_MT_SUBTYPE, &format))?;
            if let Some((width, height)) = size { let _ = input.SetUINT64(&MF_MT_FRAME_SIZE, ((width as u64) << 32) | height as u64); }
            ctx("Decoder", transform.SetInputType(0, &input, 0))?;
            let mut decoder = Self { transform, buffer_size: 0, stride: 0, coded_height: 0, width: 0, height: 0, time: 0 };
            // Some decoders (H.265) only know their output once they have seen the stream.
            let _ = decoder.choose_output();
            // Optional notices; a decoder still waiting to learn its output may refuse them.
            let _ = decoder.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0);
            let _ = decoder.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0);
            Ok(decoder)
        }
    }

    fn choose_output(&mut self) -> Result<(), String> {
        unsafe {
            for index in 0.. {
                let kind = ctx("Decoder output", self.transform.GetOutputAvailableType(0, index))?;
                // Some decoders list formats with details missing until they have seen data.
                if kind.GetGUID(&MF_MT_SUBTYPE).ok() != Some(MFVideoFormat_NV12) { continue; }
                ctx("Decoder output", self.transform.SetOutputType(0, &kind, 0))?;
                if let Ok(size) = kind.GetUINT64(&MF_MT_FRAME_SIZE) {
                    let (width, height) = split(size);
                    self.coded_height = height as usize;
                    self.width = width as usize;
                    self.height = height as usize;
                    // The picture can be smaller than its 16-pixel-aligned buffer.
                    let mut area = MFVideoArea::default();
                    let bytes = std::slice::from_raw_parts_mut(&mut area as *mut _ as *mut u8, std::mem::size_of::<MFVideoArea>());
                    if kind.GetBlob(&MF_MT_MINIMUM_DISPLAY_APERTURE, bytes, None).is_ok() && area.Area.cx > 0 && area.Area.cy > 0 {
                        self.width = area.Area.cx as usize;
                        self.height = area.Area.cy as usize;
                    }
                    self.stride = kind.GetUINT32(&MF_MT_DEFAULT_STRIDE).map_or(width as usize, |value| value as i32 as usize);
                }
                self.buffer_size = ctx("Decoder output", self.transform.GetOutputStreamInfo(0))?.cbSize;
                return Ok(());
            }
            Err("The decoder offered no usable format".into())
        }
    }

    /// Decodes one access unit; `on_picture` receives each finished picture.
    pub fn decode(&mut self, unit: &[u8], on_picture: &mut dyn FnMut(&[u8], &[u8], usize, usize, usize)) -> Result<(), String> {
        unsafe {
            let buffer = ctx("Decoder input", MFCreateMemoryBuffer(unit.len() as u32))?;
            let (mut data, mut length) = (std::ptr::null_mut(), 0u32);
            ctx("Decoder input", buffer.Lock(&mut data, Some(&mut length), None))?;
            std::ptr::copy_nonoverlapping(unit.as_ptr(), data, unit.len());
            let _ = buffer.Unlock();
            ctx("Decoder input", buffer.SetCurrentLength(unit.len() as u32))?;
            let sample = ctx("Decoder input", MFCreateSample())?;
            ctx("Decoder input", sample.AddBuffer(&buffer))?;
            let _ = sample.SetSampleTime(self.time);
            self.time += 333_333;
            ctx("Decoder input", self.transform.ProcessInput(0, &sample, 0))?;
            self.collect(on_picture)
        }
    }

    /// Hands out the pictures still held for reordering, at the end of a stream.
    pub fn drain(&mut self, on_picture: &mut dyn FnMut(&[u8], &[u8], usize, usize, usize)) -> Result<(), String> {
        unsafe { ctx("Decoder drain", self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_DRAIN, 0))?; }
        self.collect(on_picture)
    }

    fn collect(&mut self, on_picture: &mut dyn FnMut(&[u8], &[u8], usize, usize, usize)) -> Result<(), String> {
        unsafe {
            loop {
                let target = ctx("Decoder output", MFCreateMemoryBuffer(self.buffer_size.max(1)))?;
                let picture = ctx("Decoder output", MFCreateSample())?;
                ctx("Decoder output", picture.AddBuffer(&target))?;
                let mut output = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(Some(picture)), ..Default::default() }];
                let mut status = 0;
                let result = self.transform.ProcessOutput(0, &mut output, &mut status);
                drop(ManuallyDrop::take(&mut output[0].pSample));
                drop(ManuallyDrop::take(&mut output[0].pEvents));
                match result {
                    Ok(()) => {
                        let (mut data, mut length) = (std::ptr::null_mut(), 0u32);
                        ctx("Decoder output", target.Lock(&mut data, None, Some(&mut length)))?;
                        let bytes = std::slice::from_raw_parts(data, length as usize);
                        let split_at = self.stride * self.coded_height;
                        if self.stride > 0 && bytes.len() >= split_at + split_at / 2 {
                            on_picture(&bytes[..split_at], &bytes[split_at..], self.stride, self.width, self.height);
                        }
                        let _ = target.Unlock();
                    }
                    Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    Err(error) if error.code() == MF_E_TRANSFORM_STREAM_CHANGE || error.code() == MF_E_TRANSFORM_TYPE_NOT_SET => self.choose_output()?,
                    Err(error) => return Err(format!("Decoder: {error}")),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reassembles_single_stap_and_fragmented_units() {
        let mut d = Depacketizer::default();
        assert_eq!(d.push(&[0x67, 1, 2], false), None);
        let mut stap = vec![0x78u8, 0, 2, 0x68, 9, 0, 1, 0x06];
        assert_eq!(d.push(&stap, false), None);
        stap.clear();
        assert_eq!(d.push(&[0x7c, 0x85, 7, 8], false), None);
        let unit = d.push(&[0x7c, 0x45, 9], true).unwrap();
        assert_eq!(unit, [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 0, 1, 0x68, 9, 0, 0, 0, 1, 0x06, 0, 0, 0, 1, 0x65, 7, 8, 9]);
    }
    /// A white box moves 4 pixels right every frame in a clip with B-frames: decoded
    /// for TV, it must only ever move right (display order), never jump back.
    #[test]
    fn tv_decoding_shows_b_frames_in_display_order() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/box_bframes.ts")).unwrap();
        let mut demuxer = crate::tv::demux::Demuxer::new();
        let mut units = Vec::new();
        demuxer.push(&bytes, &mut units);
        demuxer.finish(&mut units);
        let positions = |mut decoder: Decoder| {
            let mut xs = Vec::new();
            for unit in &units {
                let crate::tv::demux::Unit::Video { data, .. } = unit else { continue };
                decoder.decode(data, &mut |y, _, stride, width, _| { xs.push(y[80 * stride + 8..80 * stride + width].iter().position(|&v| v > 128).map_or(-1, |x| x as i64 + 8)); }).unwrap();
            }
            decoder.drain(&mut |y, _, stride, width, _| { xs.push(y[80 * stride + 8..80 * stride + width].iter().position(|&v| v > 128).map_or(-1, |x| x as i64 + 8)); }).unwrap();
            xs.into_iter().filter(|&x| x >= 0).collect::<Vec<_>>()
        };
        let ordered = positions(Decoder::reordering().unwrap());
        assert!(ordered.len() >= 40, "most of the 50 frames come out: {} {ordered:?}", ordered.len());
        let back = ordered.windows(2).filter(|w| w[1] < w[0] && w[0] - w[1] < 200).count();
        assert_eq!(back, 0, "display order: {ordered:?}");
        assert_eq!(ordered.len(), 50, "draining hands out the frames held for reordering");
    }
    /// The converted copy for browsers: B-frame video decoded in display order,
    /// re-encoded without B-frames, still shows every frame in order.
    #[test]
    fn tv_converted_copy_keeps_every_frame_in_order() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/box_bframes.ts")).unwrap();
        let mut demuxer = crate::tv::demux::Demuxer::new();
        let mut units = Vec::new();
        demuxer.push(&bytes, &mut units);
        demuxer.finish(&mut units);
        let mut decoder = Decoder::reordering().unwrap();
        let mut encoder: Option<crate::screen_media::FrameEncoder> = None;
        let (mut packets, mut count) = (Vec::<Vec<u8>>::new(), 0i64);
        let mut encode = |y: &[u8], uv: &[u8], stride: usize, width: usize, height: usize| {
            let e = encoder.get_or_insert_with(|| crate::screen_media::FrameEncoder::new(width as u32, height as u32, 25).unwrap());
            e.encode(y, uv, stride, count * 400_000, &mut |p, _| packets.push(p.to_vec())).unwrap();
            count += 1;
        };
        for unit in &units { if let crate::tv::demux::Unit::Video { data, .. } = unit { decoder.decode(data, &mut encode).unwrap(); } }
        decoder.drain(&mut encode).unwrap();
        if let Some(e) = encoder.as_mut() { e.flush(&mut |p, _| packets.push(p.to_vec())).unwrap(); }
        assert_eq!(count, 50, "every picture was encoded");
        let mut check = Decoder::new().unwrap();
        let mut xs = Vec::new();
        for packet in &packets { check.decode(packet, &mut |y, _, stride, width, _| xs.push(y[80 * stride + 8..80 * stride + width].iter().position(|&v| v > 128).map_or(-1, |x| x as i64 + 8))).unwrap(); }
        let xs: Vec<i64> = xs.into_iter().filter(|&x| x >= 0).collect();
        assert!(xs.len() >= 45, "the copy plays nearly every frame at once (no reordering delay): {}", xs.len());
        assert_eq!(xs.windows(2).filter(|w| w[1] < w[0] && w[0] - w[1] < 200).count(), 0, "in order: {xs:?}");
    }
    #[test]
    fn hevc_channels_decode_when_windows_has_the_codec() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/tv/h265.ts")).unwrap();
        let mut demuxer = crate::tv::demux::Demuxer::new();
        let mut units = Vec::new();
        demuxer.push(&bytes, &mut units);
        demuxer.finish(&mut units);
        let mut decoder = match Decoder::hevc() {
            Ok(decoder) => decoder,
            Err(message) => { assert!(message.contains("HEVC Video Extensions"), "{message}"); eprintln!("skipped: {message}"); return; }
        };
        let mut pictures = 0;
        for unit in &units { if let crate::tv::demux::Unit::Video { data, .. } = unit { decoder.decode(data, &mut |_, _, _, w, h| { assert_eq!((w, h), (320, 180)); pictures += 1; }).unwrap(); } }
        decoder.drain(&mut |_, _, _, _, _| pictures += 1).unwrap();
        assert_eq!(pictures, 25);
    }
    #[test]
    fn converts_video_black_and_white() {
        let mut rgba = Vec::new();
        nv12_to_rgba(&[16, 235, 16, 235], &[128, 128], 2, 2, 2, &mut rgba);
        assert_eq!(&rgba[..4], &[0, 0, 0, 255]);
        assert!(rgba[4] >= 254 && rgba[5] >= 254 && rgba[6] >= 254);
    }
}
