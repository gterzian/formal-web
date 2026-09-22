use std::cell::Cell;

use crossbeam_channel::Sender;

use ipc_messages::media::MediaPipelineId;

use crate::backend::{MediaBackendEvent, PipelineHandle};

use super::av_sys::{AvPlayer, AvVideoOutput, url_from_string};

// ---------------------------------------------------------------------------
// AvfPipeline
//
// Runs inside the select loop on the graphics backend's own thread.
// Frames are sent as MediaBackendEvent::Frame.
// ---------------------------------------------------------------------------

pub struct AvfPipeline {
    id: MediaPipelineId,
    player: AvPlayer,
    item: super::av_sys::AvPlayerItem,
    video_output: AvVideoOutput,
    event_tx: Sender<MediaBackendEvent>,
    destroyed: Cell<bool>,
    duration_reported: Cell<bool>,
    /// True when the player was playing (rate > 0) and then stopped (rate = 0),
    /// indicating the video reached end of stream.
    eos_sent: Cell<bool>,
}

impl AvfPipeline {
    pub fn new(
        id: MediaPipelineId,
        url_string: String,
        event_tx: Sender<MediaBackendEvent>,
    ) -> Result<Self, String> {
        let Some(ns_url) = url_from_string(&url_string) else {
            return Err(format!("failed to create NSURL from {url_string}"));
        };
        let player = AvPlayer::new(&ns_url);
        let Some(item) = player.current_item() else {
            return Err(String::from("AVPlayer did not create an AVPlayerItem"));
        };
        let video_output = AvVideoOutput::new();
        video_output.suppress_rendering();
        item.add_output(&video_output);
        player.pause();

        log::info!("[avf] p{}: created", id.0);

        Ok(Self {
            id,
            player,
            item,
            video_output,
            event_tx,
            destroyed: Cell::new(false),
            duration_reported: Cell::new(false),
            eos_sent: Cell::new(false),
        })
    }
}

impl PipelineHandle for AvfPipeline {
    fn play(&self) -> Result<(), String> {
        log::info!("[avf] p{}: play", self.id.0);
        self.player.play();
        Ok(())
    }

    fn pause(&self) -> Result<(), String> {
        log::info!("[avf] p{}: pause", self.id.0);
        self.player.pause();
        Ok(())
    }

    fn seek(&self, position_secs: f64) -> Result<(), String> {
        log::info!("[avf] p{}: seek to {position_secs}s", self.id.0);
        self.player.seek(position_secs);
        Ok(())
    }

    fn is_done(&self) -> bool {
        self.eos_sent.get()
    }

    fn sample(&self) {
        if self.destroyed.get() {
            return;
        }

        // Drain run loop so AVFoundation can service URL loading,
        // KVO, and video output timing.
        let rl = objc2_foundation::NSRunLoop::currentRunLoop();
        let until = objc2_foundation::NSDate::dateWithTimeIntervalSinceNow(0.008);
        rl.runUntilDate(&until);

        // Duration check (once).
        if !self.duration_reported.get()
            && self.item.status() == objc2_av_foundation::AVPlayerItemStatus::ReadyToPlay
        {
            let secs = self.item.duration_secs();
            if secs > 0.0 {
                log::info!("[avf] p{}: duration = {secs}s", self.id.0);
            }
            self.duration_reported.set(true);
        }

        // Detect end of stream: rate dropped to 0 after playing.
        if !self.eos_sent.get() && self.duration_reported.get() {
            let rate = self.player.rate();
            if rate == 0.0
                && self.item.status() == objc2_av_foundation::AVPlayerItemStatus::ReadyToPlay
            {
                // The player stopped. Check if we're at or past the duration.
                let current = self.item.current_time_secs();
                let duration = self.item.duration_secs();
                if duration > 0.0 && current >= duration - 0.1 {
                    log::info!(
                        "[avf] p{}: end of stream (t={current:.2}s / d={duration:.2}s)",
                        self.id.0
                    );
                    self.eos_sent.set(true);
                    if let Err(send_error) = self.event_tx.send(MediaBackendEvent::Eos {
                        pipeline_id: self.id,
                    }) {
                        log::error!("[avf] p{}: eos send failed: {send_error}", self.id.0);
                    }
                }
            }
        }

        // Poll for frames.
        let host_secs = super::av_sys::time::host_time_seconds();
        let item_time = self.video_output.item_time_for_host_time(host_secs);
        if self.video_output.has_new_pixel_buffer(item_time)
            && let Some(pixel_buf) = self.video_output.copy_pixel_buffer(item_time)
        {
            // Deliver the pixel buffer itself: the graphics process wraps
            // it as a Metal texture (zero-copy when GPU-backed) instead
            // of the CPU byte conversion.
            let width = super::av_sys::pixel_buffer::pixel_buffer_width(&pixel_buf);
            let height = super::av_sys::pixel_buffer::pixel_buffer_height(&pixel_buf);
            if let Err(send_error) = self.event_tx.send(MediaBackendEvent::PixelBufferFrame(
                crate::backend::PixelBufferVideoFrame {
                    pipeline_id: self.id,
                    width,
                    height,
                    pixel_buffer: pixel_buf,
                },
            )) {
                log::error!(
                    "[avf] p{}: pixel buffer frame send failed: {send_error}",
                    self.id.0
                );
            }
        }
    }

    fn destroy(self) -> Result<(), String> {
        log::info!("[avf] p{}: destroy", self.id.0);
        self.destroyed.set(true);
        self.player.pause();
        self.item.remove_output(&self.video_output);
        self.player.clear_item();
        Ok(())
    }
}
