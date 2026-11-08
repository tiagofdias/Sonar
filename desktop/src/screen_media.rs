//! Screen capture and hardware video encoding for screen sharing, all on the
//! graphics card: Windows Graphics Capture hands a texture to the D3D11 video
//! processor (scale and NV12), which hands it to whichever hardware encoder
//! Media Foundation finds. No codec is bundled and frames never reach system RAM.
#![allow(dead_code)]
use std::mem::ManuallyDrop;
use std::time::{Duration, Instant};
use windows::core::{factory, IInspectable, Interface, GUID};
use windows::Foundation::TimeSpan;
use windows::Graphics::Capture::{Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND, RECT, TRUE};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_UNKNOWN;
use windows::Win32::Graphics::Direct3D10::ID3D10Multithread;
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::*;
use windows::Win32::Graphics::Dxgi::*;
use windows::Win32::Graphics::Gdi::HMONITOR;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

#[derive(Clone, Copy, Debug)]
pub enum Source { Monitor(isize), Window(isize) }

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ended { Stopped, TooSlow }

#[derive(Clone, Copy, Debug)]
pub struct Quality { pub width: u32, pub height: u32, pub fps: u32, pub bitrate: u32 }

impl Quality {
    /// The three sizes offered to users, with the bitrates used in the benchmarks.
    pub fn for_height(height: u32) -> Self {
        match height {
            2160 => Self { width: 3840, height: 2160, fps: 30, bitrate: 12_000_000 },
            720 => Self { width: 1280, height: 720, fps: 30, bitrate: 2_500_000 },
            1440 => Self { width: 2560, height: 1440, fps: 30, bitrate: 7_000_000 },
            _ => Self { width: 1920, height: 1080, fps: 30, bitrate: 4_000_000 },
        }
    }
    /// Never upscale: the output keeps the source's shape and is at most its size.
    pub fn fitted(self, source_width: u32, source_height: u32) -> Self {
        let scale = (self.width as f64 / source_width.max(1) as f64).min(self.height as f64 / source_height.max(1) as f64).min(1.0);
        let even = |value: f64| ((value as u32) & !1).max(2);
        Self { width: even(source_width as f64 * scale), height: even(source_height as f64 * scale), ..self }
    }
}

fn err(context: &str, error: windows::core::Error) -> String { format!("{context}: {error}") }
fn ctx<T>(context: &'static str, result: windows::core::Result<T>) -> Result<T, String> { result.map_err(|e| err(context, e)) }

struct Graphics { device: ID3D11Device, context: ID3D11DeviceContext }

fn create_graphics() -> Result<Graphics, String> {
    unsafe {
        let factory: IDXGIFactory1 = ctx("DXGI", CreateDXGIFactory1())?;
        // First real GPU: the one with a hardware encoder.
        let adapter = ctx("No graphics adapter", factory.EnumAdapters1(0))?;
        let (mut device, mut context) = (None, None);
        ctx("Direct3D", D3D11CreateDevice(&adapter, D3D_DRIVER_TYPE_UNKNOWN, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT | D3D11_CREATE_DEVICE_VIDEO_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)))?;
        let (device, context): (ID3D11Device, ID3D11DeviceContext) = (device.ok_or("No device")?, context.ok_or("No context")?);
        // The encoder uses this device from its own thread.
        ctx("Multithread", device.cast::<ID3D10Multithread>())?.SetMultithreadProtected(TRUE);
        Ok(Graphics { device, context })
    }
}

pub struct Capture { pool: Direct3D11CaptureFramePool, session: GraphicsCaptureSession, pub width: u32, pub height: u32 }

impl Capture {
    fn new(graphics: &Graphics, source: Source) -> Result<Self, String> {
        unsafe {
            let interop = ctx("Capture is unavailable", factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>())?;
            let item: GraphicsCaptureItem = match source {
                Source::Monitor(handle) => ctx("Could not capture that screen", interop.CreateForMonitor(HMONITOR(handle as _)))?,
                Source::Window(handle) => ctx("Could not capture that window", interop.CreateForWindow(HWND(handle as _)))?,
            };
            let dxgi: IDXGIDevice = ctx("DXGI device", graphics.device.cast())?;
            let inspectable: IInspectable = ctx("Capture device", CreateDirect3D11DeviceFromDXGIDevice(&dxgi))?;
            let device: IDirect3DDevice = ctx("Capture device", inspectable.cast())?;
            let size: SizeInt32 = ctx("Capture size", item.Size())?;
            let pool = ctx("Capture pool", Direct3D11CaptureFramePool::CreateFreeThreaded(&device, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size))?;
            let session = ctx("Capture session", pool.CreateCaptureSession(&item))?;
            ctx("Start capture", session.StartCapture())?;
            Ok(Self { pool, session, width: size.Width as u32, height: size.Height as u32 })
        }
    }
    fn next(&self) -> Option<ID3D11Texture2D> {
        let frame = self.pool.TryGetNextFrame().ok()?;
        let access: IDirect3DDxgiInterfaceAccess = frame.Surface().ok()?.cast().ok()?;
        unsafe { access.GetInterface().ok() }
    }
}
impl Drop for Capture { fn drop(&mut self) { let _ = self.session.Close(); let _ = self.pool.Close(); } }

/// Scales a captured BGRA frame and converts it to NV12 on the GPU.
struct Converter {
    video_context: ID3D11VideoContext,
    processor: ID3D11VideoProcessor,
    input_view: ID3D11VideoProcessorInputView,
    output_view: ID3D11VideoProcessorOutputView,
    input: ID3D11Texture2D,
    output: ID3D11Texture2D,
}

fn texture(device: &ID3D11Device, width: u32, height: u32, format: DXGI_FORMAT, bind: D3D11_BIND_FLAG) -> Result<ID3D11Texture2D, String> {
    let description = D3D11_TEXTURE2D_DESC { Width: width, Height: height, MipLevels: 1, ArraySize: 1, Format: format, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_DEFAULT, BindFlags: bind.0 as u32, ..Default::default() };
    let mut made = None;
    unsafe { ctx("Texture", device.CreateTexture2D(&description, None, Some(&mut made)))?; }
    made.ok_or_else(|| "No texture".to_string())
}

impl Converter {
    fn new(graphics: &Graphics, in_w: u32, in_h: u32, out_w: u32, out_h: u32, fps: u32) -> Result<Self, String> {
        unsafe {
            let video: ID3D11VideoDevice = ctx("This graphics card has no video processor", graphics.device.cast())?;
            let video_context: ID3D11VideoContext = ctx("Video context", graphics.context.cast())?;
            let rate = DXGI_RATIONAL { Numerator: fps, Denominator: 1 };
            let content = D3D11_VIDEO_PROCESSOR_CONTENT_DESC { InputFrameFormat: D3D11_VIDEO_FRAME_FORMAT_PROGRESSIVE, InputFrameRate: rate, InputWidth: in_w, InputHeight: in_h, OutputFrameRate: rate, OutputWidth: out_w, OutputHeight: out_h, Usage: D3D11_VIDEO_USAGE_OPTIMAL_SPEED };
            let enumerator = ctx("Video processor", video.CreateVideoProcessorEnumerator(&content))?;
            let processor = ctx("Video processor", video.CreateVideoProcessor(&enumerator, 0))?;
            let input = texture(&graphics.device, in_w, in_h, DXGI_FORMAT_B8G8R8A8_UNORM, D3D11_BIND_RENDER_TARGET)?;
            let output = texture(&graphics.device, out_w, out_h, DXGI_FORMAT_NV12, D3D11_BIND_RENDER_TARGET)?;
            let input_description = D3D11_VIDEO_PROCESSOR_INPUT_VIEW_DESC { ViewDimension: D3D11_VPIV_DIMENSION_TEXTURE2D, ..Default::default() };
            let mut input_view = None;
            ctx("Video input", video.CreateVideoProcessorInputView(&input, &enumerator, &input_description, Some(&mut input_view)))?;
            let output_description = D3D11_VIDEO_PROCESSOR_OUTPUT_VIEW_DESC { ViewDimension: D3D11_VPOV_DIMENSION_TEXTURE2D, ..Default::default() };
            let mut output_view = None;
            ctx("Video output", video.CreateVideoProcessorOutputView(&output, &enumerator, &output_description, Some(&mut output_view)))?;
            let source = RECT { left: 0, top: 0, right: in_w as i32, bottom: in_h as i32 };
            let target = RECT { left: 0, top: 0, right: out_w as i32, bottom: out_h as i32 };
            video_context.VideoProcessorSetStreamSourceRect(&processor, 0, TRUE, Some(&source));
            video_context.VideoProcessorSetStreamDestRect(&processor, 0, TRUE, Some(&target));
            video_context.VideoProcessorSetOutputTargetRect(&processor, TRUE, Some(&target));
            Ok(Self { video_context, processor, input_view: input_view.ok_or("No input view")?, output_view: output_view.ok_or("No output view")?, input, output })
        }
    }
    fn convert(&self, graphics: &Graphics, frame: &ID3D11Texture2D) -> Result<(), String> {
        unsafe {
            graphics.context.CopySubresourceRegion(&self.input, 0, 0, 0, 0, frame, 0, None);
            let stream = D3D11_VIDEO_PROCESSOR_STREAM { Enable: TRUE, pInputSurface: ManuallyDrop::new(Some(self.input_view.clone())), ..Default::default() };
            let streams = [stream];
            let result = self.video_context.VideoProcessorBlt(&self.processor, &self.output_view, 0, &streams);
            let [mut stream] = streams;
            ManuallyDrop::drop(&mut stream.pInputSurface);
            ctx("Video conversion", result)
        }
    }
}

/// Windows' built-in software H.264 encoder, for PCs with no hardware encoder.
struct SoftwareEncoder { transform: IMFTransform, buffer_size: u32, width: usize, height: usize, staging: ID3D11Texture2D, frame_ticks: i64 }

/// Lets development checks exercise the software path on a PC that has hardware.
pub static FORCE_SOFTWARE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl SoftwareEncoder {
    fn new(graphics: &Graphics, quality: Quality) -> Result<Self, String> {
        unsafe {
            let transform: IMFTransform = ctx("The Windows video encoder is unavailable", CoCreateInstance(&CLSID_MSH264EncoderMFT, None, CLSCTX_INPROC_SERVER))?;
            if let Ok(api) = transform.cast::<ICodecAPI>() {
                let _ = api.SetValue(&CODECAPI_AVLowLatencyMode, &windows::core::VARIANT::from(true));
                let _ = api.SetValue(&CODECAPI_AVEncCommonRateControlMode, &windows::core::VARIANT::from(eAVEncCommonRateControlMode_CBR.0 as u32));
                let _ = api.SetValue(&CODECAPI_AVEncMPVGOPSize, &windows::core::VARIANT::from(quality.fps * 4));
                let _ = api.SetValue(&CODECAPI_AVEncMPVDefaultBPictureCount, &windows::core::VARIANT::from(0u32));
            }
            let (rate, size) = (pack(quality.fps, 1), pack(quality.width, quality.height));
            let out_type = ctx("Output type", MFCreateMediaType())?;
            ctx("Type", out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            ctx("Type", out_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264))?;
            ctx("Type", out_type.SetUINT32(&MF_MT_AVG_BITRATE, quality.bitrate))?;
            ctx("Type", out_type.SetUINT64(&MF_MT_FRAME_SIZE, size))?;
            ctx("Type", out_type.SetUINT64(&MF_MT_FRAME_RATE, rate))?;
            ctx("Type", out_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)))?;
            ctx("Type", out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32))?;
            let _ = out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Base.0 as u32);
            ctx("The software encoder refused this size", transform.SetOutputType(0, &out_type, 0))?;
            let in_type = ctx("Input type", MFCreateMediaType())?;
            ctx("Type", in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            ctx("Type", in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))?;
            ctx("Type", in_type.SetUINT64(&MF_MT_FRAME_SIZE, size))?;
            ctx("Type", in_type.SetUINT64(&MF_MT_FRAME_RATE, rate))?;
            ctx("Type", in_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)))?;
            ctx("Type", in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32))?;
            ctx("The software encoder refused the input", transform.SetInputType(0, &in_type, 0))?;
            ctx("Encoder start", transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
            ctx("Encoder start", transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
            let buffer_size = ctx("Encoder output", transform.GetOutputStreamInfo(0))?.cbSize.max(quality.width * quality.height);
            let description = D3D11_TEXTURE2D_DESC { Width: quality.width, Height: quality.height, MipLevels: 1, ArraySize: 1, Format: DXGI_FORMAT_NV12, SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 }, Usage: D3D11_USAGE_STAGING, CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32, ..Default::default() };
            let mut staging = None;
            ctx("Staging texture", graphics.device.CreateTexture2D(&description, None, Some(&mut staging)))?;
            Ok(Self { transform, buffer_size, width: quality.width as usize, height: quality.height as usize, staging: staging.ok_or("No staging texture")?, frame_ticks: 10_000_000 / quality.fps as i64 })
        }
    }

    fn encode(&self, graphics: &Graphics, frame: &ID3D11Texture2D, time: i64, on_packet: &mut dyn FnMut(&[u8], bool)) -> Result<(), String> {
        unsafe {
            graphics.context.CopyResource(&self.staging, frame);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            ctx("Frame readback", graphics.context.Map(&self.staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)))?;
            let total = self.width * self.height * 3 / 2;
            let buffer = ctx("Frame buffer", MFCreateMemoryBuffer(total as u32))?;
            let (mut data, mut length) = (std::ptr::null_mut(), 0u32);
            ctx("Frame buffer", buffer.Lock(&mut data, Some(&mut length), None))?;
            let (source, stride) = (mapped.pData as *const u8, mapped.RowPitch as usize);
            for row in 0..self.height { std::ptr::copy_nonoverlapping(source.add(row * stride), data.add(row * self.width), self.width); }
            let (uv_source, uv_target) = (source.add(stride * self.height), data.add(self.width * self.height));
            for row in 0..self.height / 2 { std::ptr::copy_nonoverlapping(uv_source.add(row * stride), uv_target.add(row * self.width), self.width); }
            let _ = buffer.Unlock();
            graphics.context.Unmap(&self.staging, 0);
            ctx("Frame buffer", buffer.SetCurrentLength(total as u32))?;
            let sample = ctx("Frame sample", MFCreateSample())?;
            ctx("Frame sample", sample.AddBuffer(&buffer))?;
            ctx("Frame time", sample.SetSampleTime(time))?;
            ctx("Frame time", sample.SetSampleDuration(self.frame_ticks))?;
            ctx("Encoder input", self.transform.ProcessInput(0, &sample, 0))?;
            loop {
                let target = ctx("Encoder output", MFCreateMemoryBuffer(self.buffer_size))?;
                let picture = ctx("Encoder output", MFCreateSample())?;
                ctx("Encoder output", picture.AddBuffer(&target))?;
                let mut output = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: 0, pSample: ManuallyDrop::new(Some(picture)), ..Default::default() }];
                let mut status = 0;
                let result = self.transform.ProcessOutput(0, &mut output, &mut status);
                let sample = ManuallyDrop::take(&mut output[0].pSample);
                drop(ManuallyDrop::take(&mut output[0].pEvents));
                match result {
                    Ok(()) => {
                        let key = sample.as_ref().and_then(|s| s.GetUINT32(&MFSampleExtension_CleanPoint).ok()).unwrap_or(0) != 0;
                        let (mut bytes, mut size) = (std::ptr::null_mut(), 0u32);
                        ctx("Encoded sample", target.Lock(&mut bytes, None, Some(&mut size)))?;
                        on_packet(std::slice::from_raw_parts(bytes, size as usize), key);
                        let _ = target.Unlock();
                    }
                    Err(error) if error.code() == MF_E_TRANSFORM_NEED_MORE_INPUT => return Ok(()),
                    Err(error) => return Err(format!("Encoder: {error}")),
                }
            }
        }
    }
    fn request_key(&self) {
        if let Ok(api) = self.transform.cast::<ICodecAPI>() { unsafe { let _ = api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &windows::core::VARIANT::from(1u32)); } }
    }
}

/// Which codec a hardware encoder produces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Codec { Av1, H264 }

const fn fourcc(code: &[u8; 4]) -> GUID {
    GUID::from_values(u32::from_le_bytes(*code), 0x0000, 0x0010, [0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B, 0x71])
}
const AV1_FORMAT: GUID = fourcc(b"AV01");

fn pack(high: u32, low: u32) -> u64 { ((high as u64) << 32) | low as u64 }

/// Finds hardware encoders, best codec first.
fn find_encoder() -> Result<(IMFTransform, Codec, String), String> {
    unsafe {
        // H.264 first: every Sonar viewer can decode it, including through hardware.
        for (format, codec) in [(MFVideoFormat_H264, Codec::H264)] {
            let input = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: MFVideoFormat_NV12 };
            let output = MFT_REGISTER_TYPE_INFO { guidMajorType: MFMediaType_Video, guidSubtype: format };
            let (mut list, mut count) = (std::ptr::null_mut(), 0u32);
            if MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE | MFT_ENUM_FLAG_SORTANDFILTER, Some(&input), Some(&output), &mut list, &mut count).is_err() || count == 0 { continue; }
            let activates = std::slice::from_raw_parts(list, count as usize);
            let activate = activates[0].clone().ok_or("No encoder")?;
            let (mut name, mut length) = (windows::core::PWSTR::null(), 0u32);
            let label = if activate.GetAllocatedString(&MFT_FRIENDLY_NAME_Attribute, &mut name, &mut length).is_ok() { let text = name.to_string().unwrap_or_default(); windows::Win32::System::Com::CoTaskMemFree(Some(name.0 as _)); text } else { String::new() };
            let transform: windows::core::Result<IMFTransform> = activate.ActivateObject();
            for item in activates { drop(item.clone()); }
            for index in 0..count as usize { std::ptr::drop_in_place(list.add(index)); }
            windows::Win32::System::Com::CoTaskMemFree(Some(list as _));
            if let Ok(transform) = transform { return Ok((transform, codec, label)); }
        }
        Err("This PC has no hardware video encoder".into())
    }
}

pub struct Encoder { transform: IMFTransform, events: IMFMediaEventGenerator, stream_in: u32, stream_out: u32, pub codec: Codec, pub name: String, frame_ticks: i64, needs_input: std::cell::Cell<u32>, _manager: IMFDXGIDeviceManager }

impl Encoder {
    fn new(graphics: &Graphics, quality: Quality) -> Result<Self, String> {
        unsafe {
            let (transform, codec, name) = find_encoder()?;
            let attributes = ctx("Encoder attributes", transform.GetAttributes())?;
            ctx("Encoder unlock", attributes.SetUINT32(&MF_TRANSFORM_ASYNC_UNLOCK, 1))?;
            let _ = attributes.SetUINT32(&MF_LOW_LATENCY, 1);
            let (mut token, mut manager) = (0u32, None);
            ctx("D3D manager", MFCreateDXGIDeviceManager(&mut token, &mut manager))?;
            let manager: IMFDXGIDeviceManager = manager.ok_or("No D3D manager")?;
            ctx("D3D manager", manager.ResetDevice(&graphics.device, token))?;
            ctx("Encoder device", transform.ProcessMessage(MFT_MESSAGE_SET_D3D_MANAGER, manager.as_raw() as usize))?;
            let events: IMFMediaEventGenerator = ctx("Encoder events", transform.cast())?;

            if let Ok(api) = transform.cast::<ICodecAPI>() {
                let set = |key: &GUID, value: u32| { let _ = api.SetValue(key, &windows::core::VARIANT::from(value)); };
                let on = |key: &GUID| { let _ = api.SetValue(key, &windows::core::VARIANT::from(true)); };
                on(&CODECAPI_AVLowLatencyMode);
                set(&CODECAPI_AVEncCommonRateControlMode, eAVEncCommonRateControlMode_CBR.0 as u32);
                set(&CODECAPI_AVEncCommonMeanBitRate, quality.bitrate);
                set(&CODECAPI_AVEncMPVGOPSize, quality.fps * 4);
                set(&CODECAPI_AVEncMPVDefaultBPictureCount, 0);
            }

            let (mut inputs, mut outputs) = (0, 0);
            let (mut stream_in, mut stream_out) = (0u32, 0u32);
            ctx("Encoder streams", transform.GetStreamCount(&mut inputs, &mut outputs))?;
            let (mut in_ids, mut out_ids) = ([0u32; 1], [0u32; 1]);
            if transform.GetStreamIDs(&mut in_ids, &mut out_ids).is_ok() { stream_in = in_ids[0]; stream_out = out_ids[0]; }

            let rate = pack(quality.fps, 1);
            let size = pack(quality.width, quality.height);
            let out_type = ctx("Output type", MFCreateMediaType())?;
            ctx("Type", out_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            ctx("Type", out_type.SetGUID(&MF_MT_SUBTYPE, &if codec == Codec::Av1 { AV1_FORMAT } else { MFVideoFormat_H264 }))?;
            ctx("Type", out_type.SetUINT32(&MF_MT_AVG_BITRATE, quality.bitrate))?;
            ctx("Type", out_type.SetUINT64(&MF_MT_FRAME_SIZE, size))?;
            ctx("Type", out_type.SetUINT64(&MF_MT_FRAME_RATE, rate))?;
            ctx("Type", out_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)))?;
            ctx("Type", out_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32))?;
            if codec == Codec::H264 { let _ = out_type.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_ConstrainedBase.0 as u32); }
            ctx("The encoder refused this size", transform.SetOutputType(stream_out, &out_type, 0))?;

            let in_type = ctx("Input type", MFCreateMediaType())?;
            ctx("Type", in_type.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video))?;
            ctx("Type", in_type.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12))?;
            ctx("Type", in_type.SetUINT64(&MF_MT_FRAME_SIZE, size))?;
            ctx("Type", in_type.SetUINT64(&MF_MT_FRAME_RATE, rate))?;
            ctx("Type", in_type.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1)))?;
            ctx("Type", in_type.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32))?;
            ctx("The encoder refused the input", transform.SetInputType(stream_in, &in_type, 0))?;

            ctx("Encoder start", transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0))?;
            ctx("Encoder start", transform.ProcessMessage(MFT_MESSAGE_NOTIFY_BEGIN_STREAMING, 0))?;
            ctx("Encoder start", transform.ProcessMessage(MFT_MESSAGE_NOTIFY_START_OF_STREAM, 0))?;
            Ok(Self { transform, events, stream_in, stream_out, codec, name, frame_ticks: 10_000_000 / quality.fps as i64, needs_input: std::cell::Cell::new(0), _manager: manager })
        }
    }

    /// Feeds one NV12 texture when the encoder asks, and returns finished packets.
    fn pump(&self, frame: Option<(&ID3D11Texture2D, i64)>, on_packet: &mut dyn FnMut(&[u8], bool)) -> Result<bool, String> {
        unsafe {
            loop {
                let Ok(event) = self.events.GetEvent(MF_EVENT_FLAG_NO_WAIT) else { break };
                let kind = ctx("Encoder event", event.GetType())?;
                if kind == METransformNeedInput.0 as u32 {
                    self.needs_input.set(self.needs_input.get() + 1);
                } else if kind == METransformHaveOutput.0 as u32 {
                    let mut output = [MFT_OUTPUT_DATA_BUFFER { dwStreamID: self.stream_out, ..Default::default() }];
                    let mut status = 0;
                    let result = self.transform.ProcessOutput(0, &mut output, &mut status);
                    let sample = ManuallyDrop::take(&mut output[0].pSample);
                    let _ = ManuallyDrop::take(&mut output[0].pEvents);
                    ctx("Encoder output", result)?;
                    if let Some(sample) = sample {
                        let key = sample.GetUINT32(&MFSampleExtension_CleanPoint).unwrap_or(0) != 0;
                        let buffer = ctx("Encoded sample", sample.ConvertToContiguousBuffer())?;
                        let (mut data, mut length) = (std::ptr::null_mut(), 0u32);
                        ctx("Encoded sample", buffer.Lock(&mut data, None, Some(&mut length)))?;
                        on_packet(std::slice::from_raw_parts(data, length as usize), key);
                        let _ = buffer.Unlock();
                    }
                }
            }
            // The encoder asks for input before we may have a frame ready, so
            // its requests are counted rather than answered on the spot.
            let Some((texture, time)) = frame else { return Ok(false) };
            if self.needs_input.get() == 0 { return Ok(false); }
            self.needs_input.set(self.needs_input.get() - 1);
            let buffer = ctx("Frame buffer", MFCreateDXGISurfaceBuffer(&ID3D11Texture2D::IID, texture, 0, false))?;
            let sample = ctx("Frame sample", MFCreateSample())?;
            ctx("Frame sample", sample.AddBuffer(&buffer))?;
            ctx("Frame time", sample.SetSampleTime(time))?;
            ctx("Frame time", sample.SetSampleDuration(self.frame_ticks))?;
            ctx("Encoder input", self.transform.ProcessInput(self.stream_in, &sample, 0))?;
            Ok(true)
        }
    }
}

impl Encoder {
    /// A viewer who just joined cannot start until the next full picture.
    fn request_key(&self) {
        if let Ok(api) = self.transform.cast::<ICodecAPI>() { unsafe { let _ = api.SetValue(&CODECAPI_AVEncVideoForceKeyFrame, &windows::core::VARIANT::from(1u32)); } }
    }
}

impl Drop for Encoder {
    fn drop(&mut self) { unsafe { let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_OF_STREAM, 0); let _ = self.transform.ProcessMessage(MFT_MESSAGE_NOTIFY_END_STREAMING, 0); let _ = self.transform.ProcessMessage(MFT_MESSAGE_COMMAND_FLUSH, 0); } }
}

/// Whether this PC can encode video on its graphics card.
pub fn has_hardware_encoder() -> bool { !FORCE_SOFTWARE.load(std::sync::atomic::Ordering::Relaxed) && startup().is_ok() && find_encoder().is_ok() }

pub fn startup() -> Result<(), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        ctx("Media Foundation", MFStartup(MF_VERSION, MFSTARTUP_LITE))
    }
}

/// Captures and encodes until `keep_going` says stop. Runs on its own thread.
pub fn run(source: Source, requested: Quality, force_key: &std::sync::atomic::AtomicBool, keep_going: &mut dyn FnMut() -> bool, on_ready: &mut dyn FnMut(Quality, Codec, &str), on_packet: &mut dyn FnMut(&[u8], bool, Duration)) -> Result<Ended, String> {
    startup()?;
    let graphics = create_graphics()?;
    let capture = Capture::new(&graphics, source)?;
    let quality = requested.fitted(capture.width, capture.height);
    // Hardware when the PC has it; otherwise the encoder built into Windows.
    let hardware = if FORCE_SOFTWARE.load(std::sync::atomic::Ordering::Relaxed) { None } else { Encoder::new(&graphics, quality).ok() };
    let software = if hardware.is_none() { Some(SoftwareEncoder::new(&graphics, quality)?) } else { None };
    let converter = Converter::new(&graphics, capture.width, capture.height, quality.width, quality.height, quality.fps)?;
    match &hardware { Some(encoder) => on_ready(quality, encoder.codec, &encoder.name), None => on_ready(quality, Codec::H264, "Windows software encoder") }
    let interval = Duration::from_secs(1) / quality.fps;
    let started = Instant::now();
    let (mut next, mut have_frame) = (Instant::now(), false);
    let mut waiting_frame = false;
    // Frames that should exist against frames that came out, checked every few seconds.
    let (mut produced, mut window_start, mut slow_windows) = (0u32, Instant::now() + Duration::from_secs(2), 0u32);
    while keep_going() {
        if Instant::now() >= window_start + Duration::from_secs(4) {
            if window_start <= Instant::now() && produced < quality.fps * 4 * 7 / 10 && have_frame { slow_windows += 1; } else { slow_windows = 0; }
            if slow_windows >= 2 { return Ok(Ended::TooSlow); }
            produced = 0;
            window_start = Instant::now();
        }
        if let Some(frame) = capture.next() {
            // The window was resized: keep the last size rather than stretch.
            let mut description = D3D11_TEXTURE2D_DESC::default();
            unsafe { frame.GetDesc(&mut description); }
            if description.Width == capture.width && description.Height == capture.height {
                converter.convert(&graphics, &frame)?;
                have_frame = true;
            }
        }
        let now = Instant::now();
        // Still screens produce no new captures, so the last picture repeats.
        if have_frame && now >= next { waiting_frame = true; next += interval; if next < now { next = now + interval; } }
        let time = started.elapsed().as_nanos() as i64 / 100;
        let frame = waiting_frame.then_some((&converter.output, time));
        if force_key.swap(false, std::sync::atomic::Ordering::AcqRel) {
            if let Some(encoder) = &hardware { encoder.request_key(); }
            if let Some(encoder) = &software { encoder.request_key(); }
        }
        let mut packets = |bytes: &[u8], key: bool| { produced += 1; on_packet(bytes, key, interval) };
        if let Some(encoder) = &hardware {
            if encoder.pump(frame, &mut packets)? { waiting_frame = false; }
        } else if let (Some(encoder), Some((texture, time))) = (&software, frame) {
            encoder.encode(&graphics, texture, time, &mut packets)?;
            waiting_frame = false;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(Ended::Stopped)
}

// Keep the type name in one place for callers that format durations.
#[allow(unused)]
fn _span(value: TimeSpan) -> TimeSpan { value }
