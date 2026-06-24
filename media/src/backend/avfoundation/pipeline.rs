use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crossbeam_channel::Sender;
use objc2_foundation::{NSDate, NSRunLoop};

use ipc_messages::media::{MediaEvent, MediaPipelineId};

use crate::backend::PipelineHandle;

use super::av_sys::{
    AvPlayer, AvVideoOutput, PixelBufferLock, host_time_seconds, pixel_buffer_to_frame,
    url_from_string,
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Frame poll interval — the run loop drains for this long each iteration.
const POLL_SECS: f64 = 0.033; // ≈30 fps

// ---------------------------------------------------------------------------
// Command queue
// ---------------------------------------------------------------------------

enum AvfCommand {
    Play,
    Pause,
    Seek(f64),
    Destroy,
}

// ---------------------------------------------------------------------------
// AvfPipeline
// ---------------------------------------------------------------------------

pub struct AvfPipeline {
    cmd_tx: crossbeam_channel::Sender<AvfCommand>,
    #[allow(dead_code)]
    destroyed: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

static FRAME_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

impl AvfPipeline {
    pub fn new(
        id: MediaPipelineId,
        url_string: String,
        frame_tx: Sender<MediaEvent>,
    ) -> Result<Self, String> {
        let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<AvfCommand>();
        let destroyed = Arc::new(AtomicBool::new(false));
        let destroyed_clone = Arc::clone(&destroyed);

        let thread = std::thread::Builder::new()
            .name("formal-web-avf-worker".into())
            .spawn(move || {
                // SAFETY: All AVPlayer/AVPlayerItem access is serialized on
                // this dedicated thread via the command queue below.
                let ns_url = url_from_string(&url_string).expect("NSURL creation failed");
                let mut player = unsafe { AvPlayer::new(&ns_url) };
                let item = player.current_item().expect("AVPlayer::currentItem");
                let video_output = AvVideoOutput::new();
                video_output.suppress_rendering();
                item.add_output(&video_output);
                player.pause();

                log::info!("[avf] p{}: ready", id.0);

                let mut duration_reported = false;

                loop {
                    // ── Run loop drives ALL timing ──
                    // AVFoundation URL loading, KVO, and video output timing
                    // all need continuous run loop time. Drain for POLL_SECS,
                    // then process commands and poll frames.
                    let rl = NSRunLoop::currentRunLoop();
                    let until = NSDate::dateWithTimeIntervalSinceNow(POLL_SECS);
                    rl.runUntilDate(&until);

                    // ── Drain all pending commands ──
                    loop {
                        match cmd_rx.try_recv() {
                            Ok(cmd) => match cmd {
                                AvfCommand::Play => {
                                    log::info!("[avf] p{}: play", id.0);
                                    player.play();
                                }
                                AvfCommand::Pause => {
                                    log::info!("[avf] p{}: pause", id.0);
                                    player.pause();
                                }
                                AvfCommand::Seek(pos) => {
                                    log::info!("[avf] p{}: seek to {pos}s", id.0);
                                    player.seek(pos);
                                }
                                AvfCommand::Destroy => {
                                    log::info!("[avf] p{}: destroy", id.0);
                                    destroyed_clone.store(true, Ordering::SeqCst);
                                    player.pause();
                                    item.remove_output(&video_output);
                                    player.clear_item();
                                    return;
                                }
                            },
                            Err(crossbeam_channel::TryRecvError::Empty) => break,
                            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                                log::info!("[avf] p{}: cmd channel closed", id.0);
                                return;
                            }
                        }
                    }

                    // ── Duration check (once) ──
                    if !duration_reported {
                        use objc2_av_foundation::AVPlayerItemStatus;
                        let st = item.status();
                        let st_num: i32 = st.0 as i32;
                        log::info!("[avf] p{}: status={st_num}", id.0);
                        if st == AVPlayerItemStatus::ReadyToPlay {
                            let secs = item.duration_secs();
                            if secs > 0.0 {
                                log::info!("[avf] p{}: duration = {secs}s", id.0);
                            }
                            duration_reported = true;
                        }
                    }

                    // ── Frame poll ──
                    let host_secs = host_time_seconds();
                    let item_time = video_output.item_time_for_host_time(host_secs);

                    if video_output.has_new_pixel_buffer(item_time) {
                        if let Some(pixel_buf) = video_output.copy_pixel_buffer(item_time) {
                            if let Some(lock) = PixelBufferLock::new(&pixel_buf) {
                                if let Some(frame) = pixel_buffer_to_frame(id, &lock) {
                                    let c = FRAME_COUNT.fetch_add(1, Ordering::Relaxed);
                                    if c % 30 == 0 {
                                        log::debug!(
                                            "[avf] p{}: frame #{c} ({}x{})",
                                            id.0,
                                            frame.width,
                                            frame.height,
                                        );
                                    }
                                    let _ = frame_tx.send(MediaEvent::Frame(frame));
                                }
                            }
                        }
                    }
                }
            })
            .map_err(|error| format!("failed to spawn AVFoundation thread: {error}"))?;

        Ok(Self {
            cmd_tx,
            destroyed,
            thread: Some(thread),
        })
    }
}

impl PipelineHandle for AvfPipeline {
    fn play(&self) -> Result<(), String> {
        self.cmd_tx
            .send(AvfCommand::Play)
            .map_err(|_| String::from("avf worker disconnected"))
    }
    fn pause(&self) -> Result<(), String> {
        self.cmd_tx
            .send(AvfCommand::Pause)
            .map_err(|_| String::from("avf worker disconnected"))
    }
    fn seek(&self, position_secs: f64) -> Result<(), String> {
        self.cmd_tx
            .send(AvfCommand::Seek(position_secs))
            .map_err(|_| String::from("avf worker disconnected"))
    }
    fn destroy(mut self) -> Result<(), String> {
        let _ = self.cmd_tx.send(AvfCommand::Destroy);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        Ok(())
    }
}
