use anyhow::Result;
use grafton_ndi::{BorrowedVideoFrame, PixelFormat, Sender, SenderOptions, NDI};
use image::RgbaImage;

pub struct NdiOutput {
    sender: Sender,
    frame_rate: u32,
    bgra_buf: Vec<u8>,
}

impl NdiOutput {
    pub fn new(
        ndi: &NDI,
        output_name: &str,
        width: u32,
        height: u32,
        frame_rate: u32,
    ) -> Result<Self> {
        let opts = SenderOptions::builder(output_name)
            .clock_video(true)
            .clock_audio(false)
            .build();
        let sender = Sender::new(ndi, &opts)?;

        tracing::info!(
            "NDI output '{}' created ({}x{}@{}fps)",
            output_name,
            width,
            height,
            frame_rate
        );

        let buf_size = (width * height * 4) as usize;

        Ok(Self {
            sender,
            frame_rate,
            bgra_buf: vec![0u8; buf_size],
        })
    }

    /// Send an RGBA image as an NDI BGRA video frame using a reusable buffer.
    pub fn send_frame(&mut self, image: &RgbaImage) -> Result<()> {
        let (w, h) = image.dimensions();
        let src = image.as_raw();
        let needed = src.len();

        // Resize buffer if needed (only on resolution change)
        if self.bgra_buf.len() != needed {
            self.bgra_buf.resize(needed, 0);
        }

        // RGBA → BGRA conversion into reusable buffer
        let dst = &mut self.bgra_buf;
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2]; // B
            d[1] = s[1]; // G
            d[2] = s[0]; // R
            d[3] = s[3]; // A
        }

        let frame = BorrowedVideoFrame::try_from_uncompressed(
            &self.bgra_buf,
            w as i32,
            h as i32,
            PixelFormat::BGRA,
            self.frame_rate as i32,
            1,
        )?;

        // send_video_async requires &mut self on sender — use sync send via borrowed frame
        // The sync send_video takes &VideoFrame (owned). Use async+drop for zero-copy.
        let token = self.sender.send_video_async(&frame);
        drop(token); // Flushes immediately, releases buffer

        Ok(())
    }
}
