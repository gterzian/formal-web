use std::cell::Cell;

use crossbeam_channel::Sender;
use objc2::MainThreadMarker;

use ipc_messages::media::MediaPipelineId;

use crate::backend::{BackendEvent, PipelineHandle};

use super::av_sys::{AvPlayer, AvVideoOutput, url_from_string};

// ---------------------------------------------------------------------------
// AvfPipeline
//
// Runs inside the select loop on the main thread.
// Frames are sent as BackendEvent::Frame.
// ---------------------------------------------------------------------------

pub struct AvfPipeline {
    id: MediaPipelineId,
    player: AvPlayer,
    item: super::av_sys::AvPlayerItem,
    video_output: AvVideoOutput,
    event_tx: Sender<BackendEvent>,
    destroyed: Cell<bool>,
    duration_reported: Cell<bool>,
}

impl AvfPipeline {
    pub fn new(
        id: MediaPipelineId,
        url_string: String,
        event_tx: Sender<BackendEvent>,
    ) -> Result<Self, String> {
        let mtm = MainThreadMarker::new()
            .ok_or_else(|| String::from("AvfPipeline must be created on the main thread"))?;

        let Some(ns_url) = url_from_string(&url_string) else {
            return Err(format!("failed to create NSURL from {url_string}"));
        };
        let player = unsafe { AvPlayer::new_on_main(&ns_url, mtm) };
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
        if !self.duration_reported.get() {
            if self.item.status() == objc2_av_foundation::AVPlayerItemStatus::ReadyToPlay {
                let secs = self.item.duration_secs();
                if secs > 0.0 {
                    log::info!("[avf] p{}: duration = {secs}s", self.id.0);
                }
                self.duration_reported.set(true);
            }
        }

        // Poll for frames.
        let host_secs = super::av_sys::time::host_time_seconds();
        let item_time = self.video_output.item_time_for_host_time(host_secs);
        if self.video_output.has_new_pixel_buffer(item_time) {
            if let Some(pixel_buf) = self.video_output.copy_pixel_buffer(item_time) {
                if let Some(lock) = super::av_sys::pixel_buffer::PixelBufferLock::new(pixel_buf) {
                    if let Some(frame) =
                        super::av_sys::pixel_buffer::pixel_buffer_to_frame(self.id, &lock)
                    {
                        let _ = self
                            .event_tx
                            .send(crate::backend::BackendEvent::Frame(frame));
                    }
                }
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
