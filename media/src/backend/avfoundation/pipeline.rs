use std::cell::Cell;

use crossbeam_channel::Sender;
use objc2::MainThreadMarker;
use objc2_foundation::{NSDate, NSRunLoop};

use ipc_messages::media::MediaPipelineId;

use objc2_av_foundation::AVPlayerItemStatus;

use crate::backend::{BackendEvent, PipelineHandle};

use super::av_sys::{
    AvPlayer, AvVideoOutput, PixelBufferLock, host_time_seconds, pixel_buffer_to_frame,
    url_from_string,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const TICK_SECS: f64 = 0.008;

// ---------------------------------------------------------------------------
// AvfPipeline
//
// Runs inside the select loop on the main thread.  tick() drains the run
// loop so AVFoundation can service URL loading, KVO, and video output
// timing.  Frames are sent as BackendEvent::Frame.
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

static FRAME_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

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

    fn tick(&self) {
        if self.destroyed.get() {
            return;
        }

        // Drain run loop so AVFoundation can service URL loading etc.
        let rl = NSRunLoop::currentRunLoop();
        let until = NSDate::dateWithTimeIntervalSinceNow(TICK_SECS);
        rl.runUntilDate(&until);

        // Duration check (once).
        if !self.duration_reported.get() {
            if self.item.status() == AVPlayerItemStatus::ReadyToPlay {
                let secs = self.item.duration_secs();
                if secs > 0.0 {
                    log::info!("[avf] p{}: duration = {secs}s", self.id.0);
                }
                self.duration_reported.set(true);
            }
        }

        // Frame poll.
        let host_secs = host_time_seconds();
        let item_time = self.video_output.item_time_for_host_time(host_secs);

        if self.video_output.has_new_pixel_buffer(item_time) {
            if let Some(pixel_buf) = self.video_output.copy_pixel_buffer(item_time) {
                if let Some(lock) = PixelBufferLock::new(&pixel_buf) {
                    if let Some(frame) = pixel_buffer_to_frame(self.id, &lock) {
                        let c = FRAME_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if c % 30 == 0 {
                            log::debug!(
                                "[avf] p{}: frame #{c} ({}x{})",
                                self.id.0,
                                frame.width,
                                frame.height,
                            );
                        }
                        let _ = self.event_tx.send(BackendEvent::Frame(frame));
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
