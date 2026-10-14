use anyhow::{Context, Result};
use openh264::encoder::{BitRate, Encoder, EncoderConfig, FrameRate, RateControlMode, UsageType};
use openh264::formats::{BgraSliceU8, YUVBuffer};
use openh264::OpenH264API;

use crate::backend::Frame;

/// Software H.264 encoder producing Annex B access units.
pub struct H264Encoder {
    encoder: Encoder,
    yuv: YUVBuffer,
    width: usize,
    height: usize,
    /// Scratch buffer used when the source has odd dimensions.
    cropped: Vec<u8>,
}

impl H264Encoder {
    pub fn new(width: u32, height: u32, fps: u32, bitrate_bps: u32) -> Result<Self> {
        // H.264 4:2:0 needs even dimensions.
        let width = (width & !1) as usize;
        let height = (height & !1) as usize;
        let config = EncoderConfig::new()
            .usage_type(UsageType::ScreenContentRealTime)
            .rate_control_mode(RateControlMode::Bitrate)
            .bitrate(BitRate::from_bps(bitrate_bps))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .skip_frames(false)
            // OpenH264 only uses several cores when a frame is split into
            // slices: about 3x faster on a 2304x1438 desktop with 6 threads.
            .num_threads(encoder_threads())
            .max_slice_len(60_000);
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .context("failed to create H.264 encoder")?;
        Ok(Self {
            encoder,
            yuv: YUVBuffer::new(width, height),
            width,
            height,
            cropped: Vec::new(),
        })
    }

    /// Size the encoder was built for (always even).
    pub fn size(&self) -> (u32, u32) {
        (self.width as u32, self.height as u32)
    }

    /// True when `frame` can't be fed to this encoder without rebuilding it.
    pub fn needs_rebuild(&self, frame: &Frame) -> bool {
        (frame.width & !1) as usize != self.width || (frame.height & !1) as usize != self.height
    }

    pub fn encode(&mut self, frame: &Frame, force_keyframe: bool) -> Result<Vec<u8>> {
        let src_w = frame.width as usize;
        let bgra: &[u8] = if src_w == self.width && frame.height as usize == self.height {
            &frame.bgra
        } else {
            // Drop the odd last column/row.
            self.cropped.clear();
            for row in 0..self.height {
                let start = row * src_w * 4;
                self.cropped
                    .extend_from_slice(&frame.bgra[start..start + self.width * 4]);
            }
            &self.cropped
        };
        self.yuv
            .read_bgra8(BgraSliceU8::new(bgra, (self.width, self.height)));
        if force_keyframe {
            self.encoder.force_intra_frame();
        }
        let bitstream = self
            .encoder
            .encode(&self.yuv)
            .context("H.264 encode failed")?;
        Ok(bitstream.to_vec())
    }
}

fn encoder_threads() -> u16 {
    let cpus = std::thread::available_parallelism().map_or(2, |n| n.get());
    (cpus / 2).clamp(2, 8) as u16
}
