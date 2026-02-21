use anyhow::Result;
use grafton_ndi::{PixelFormat, SenderOptions, Sender, VideoFrame, NDI};
use image::RgbaImage;

pub struct NdiOutput {
    sender: Sender,
    frame_rate: u32,
}

impl NdiOutput {
    pub fn new(ndi: &NDI, output_name: &str, width: u32, height: u32, frame_rate: u32) -> Result<Self> {
        let opts = SenderOptions::builder(output_name)
            .clock_video(true)
            .clock_audio(false)
            .build();
        let sender = Sender::new(ndi, &opts)?;

        tracing::info!("NDI output '{}' created ({}x{}@{}fps)", output_name, width, height, frame_rate);

        Ok(Self {
            sender,
            frame_rate,
        })
    }

    /// Send an RGBA image as an NDI BGRA video frame.
    pub fn send_frame(&self, image: &RgbaImage) -> Result<()> {
        let (w, h) = image.dimensions();

        // Convert RGBA to BGRA
        let mut bgra_data: Vec<u8> = image.as_raw().clone();
        for pixel in bgra_data.chunks_exact_mut(4) {
            pixel.swap(0, 2); // Swap R and B
        }

        let frame = VideoFrame {
            width: w as i32,
            height: h as i32,
            pixel_format: PixelFormat::BGRA,
            frame_rate_n: self.frame_rate as i32,
            frame_rate_d: 1,
            picture_aspect_ratio: 0.0, // auto
            scan_type: grafton_ndi::ScanType::Progressive,
            timecode: 0,
            data: bgra_data,
            line_stride_or_size: grafton_ndi::LineStrideOrSize::LineStrideBytes(
                PixelFormat::BGRA.line_stride(w as i32),
            ),
            metadata: None,
            timestamp: 0,
        };

        self.sender.send_video(&frame);
        Ok(())
    }

}
