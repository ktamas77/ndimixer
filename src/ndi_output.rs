use anyhow::Result;
use grafton_ndi::{BorrowedVideoFrame, PixelFormat, Sender, SenderOptions, NDI};
use image::RgbaImage;

pub struct NdiOutput {
    tx: std::sync::mpsc::SyncSender<Vec<u8>>,
    bgra_buf: Vec<u8>,
    _send_thread: std::thread::JoinHandle<()>,
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
            .clock_video(false)
            .clock_audio(true)
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

        // Bounded channel: 1 frame buffer. If NDI send is busy, render drops the frame.
        let (tx, rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(1);

        let w = width as i32;
        let h = height as i32;
        let fr = frame_rate as i32;
        let name = output_name.to_string();

        let send_thread = std::thread::Builder::new()
            .name(format!("ndi-{}", name))
            .spawn(move || {
                let mut sender = sender;
                while let Ok(bgra_data) = rx.recv() {
                    if let Ok(frame) = BorrowedVideoFrame::try_from_uncompressed(
                        &bgra_data,
                        w,
                        h,
                        PixelFormat::BGRA,
                        fr,
                        1,
                    ) {
                        let token = sender.send_video_async(&frame);
                        drop(token);
                    }
                }
            })
            .expect("Failed to spawn NDI send thread");

        Ok(Self {
            tx,
            bgra_buf: vec![0u8; buf_size],
            _send_thread: send_thread,
        })
    }

    /// Send an RGBA image as NDI BGRA. Non-blocking: if the previous frame
    /// hasn't finished sending, this frame is dropped.
    pub fn send_frame(&mut self, image: &RgbaImage) -> Result<()> {
        let src = image.as_raw();
        let needed = src.len();

        if self.bgra_buf.len() != needed {
            self.bgra_buf.resize(needed, 0);
        }

        // RGBA → BGRA conversion
        let dst = &mut self.bgra_buf;
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            d[0] = s[2]; // B
            d[1] = s[1]; // G
            d[2] = s[0]; // R
            d[3] = s[3]; // A
        }

        // Non-blocking send to NDI thread (drops frame if busy)
        let _ = self.tx.try_send(self.bgra_buf.clone());

        Ok(())
    }
}
