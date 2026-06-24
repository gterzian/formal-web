use std::ffi::CString;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crossbeam_channel::Sender;
use objc2::AnyThread;
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_av_foundation::{AVPlayer, AVPlayerItemStatus, AVPlayerItemVideoOutput};
use objc2_core_media::CMTime;
use objc2_core_video::{CVGetCurrentHostTime, CVGetHostClockFrequency, CVPixelBuffer};
use objc2_foundation::{NSDate, NSRunLoop, NSString, NSURL};

use ipc_messages::media::{MediaEvent, MediaPipelineId, VideoFrame};

use crate::backend::PipelineHandle;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CM_TIME_SCALE: i32 = 600;

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
                let mtm = unsafe { MainThreadMarker::new_unchecked() };

                // ── NSURL from string ──
                let c_string = match CString::new(url_string.as_str()) {
                    Ok(cs) => cs,
                    Err(_) => {
                        log::error!("[avf] p{}: URL has null byte", id.0);
                        return;
                    }
                };
                let ns_string_ptr = std::ptr::NonNull::new(c_string.as_ptr() as *mut _);
                let ns_string: Retained<NSString> = (unsafe {
                    ns_string_ptr.and_then(|pointer| NSString::stringWithUTF8String(pointer))
                })
                .expect("NSString::stringWithUTF8String failed");
                let ns_url = NSURL::initWithString(NSURL::alloc(), &ns_string)
                    .unwrap_or_else(|| panic!("NSURL::initWithString failed for {url_string}"));

                // ── AVPlayer + item ──
                let player = unsafe { AVPlayer::playerWithURL(&ns_url, mtm) };
                let Some(item) = (unsafe { player.currentItem() }) else {
                    log::error!("[avf] p{}: no AVPlayerItem", id.0);
                    return;
                };

                // ── AVPlayerItemVideoOutput ──
                let video_output = unsafe {
                    AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
                        AVPlayerItemVideoOutput::alloc(),
                        None,
                    )
                };
                unsafe { item.addOutput(&video_output) };
                unsafe { video_output.setSuppressesPlayerRendering(true) };

                // ── Start paused ──
                unsafe { player.pause() };
                log::info!("[avf] p{}: ready", id.0);

                // ── State ──
                let mut duration_reported = false;

                // ── Combined command + frame polling loop ──
                loop {
                    // ── Fix 3: Poll item status for duration ──
                    if !duration_reported {
                        let status = unsafe { item.status() };
                        if status == AVPlayerItemStatus::ReadyToPlay {
                            let duration = unsafe { item.duration() };
                            let secs = unsafe { duration.seconds() };
                            if secs.is_finite() && secs > 0.0 {
                                log::info!("[avf] p{}: duration = {secs}s", id.0);
                            }
                            duration_reported = true;
                        }
                    }

                    // ── Convert Mach absolute host time to seconds ──
                    // itemTimeForHostTime expects the CoreVideo host time
                    // base (mach_absolute_time), not unix wall clock.
                    let host_ticks = unsafe { CVGetCurrentHostTime() };
                    let freq = unsafe { CVGetHostClockFrequency() };
                    let host_secs = host_ticks as f64 / freq;
                    let item_time = unsafe { video_output.itemTimeForHostTime(host_secs) };

                    let has_new = unsafe { video_output.hasNewPixelBufferForItemTime(item_time) };
                    if has_new {
                        let pixel_buf = unsafe {
                            video_output.copyPixelBufferForItemTime_itemTimeForDisplay(
                                item_time,
                                std::ptr::null_mut(),
                            )
                        };
                        if let Some(ref pb) = pixel_buf {
                            if let Some(frame) = pixel_buffer_to_frame(id, pb) {
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

                    // ── Fix 2: Drain run loop ──
                    // AVPlayerItemVideoOutput's internal timing machinery
                    // needs a run loop to advance. Drain it briefly.
                    let rl = NSRunLoop::currentRunLoop();
                    let until = NSDate::dateWithTimeIntervalSinceNow(0.008);
                    rl.runUntilDate(&until);

                    // Process one command (non-blocking).
                    match cmd_rx.try_recv() {
                        Ok(cmd) => match cmd {
                            AvfCommand::Play => {
                                log::info!("[avf] p{}: play", id.0);
                                unsafe { player.play() };
                            }
                            AvfCommand::Pause => {
                                log::info!("[avf] p{}: pause", id.0);
                                unsafe { player.pause() };
                            }
                            AvfCommand::Seek(pos) => {
                                log::info!("[avf] p{}: seek to {pos}s", id.0);
                                let time = unsafe { CMTime::with_seconds(pos, CM_TIME_SCALE) };
                                unsafe { player.seekToTime(time) };
                            }
                            AvfCommand::Destroy => {
                                log::info!("[avf] p{}: destroy", id.0);
                                destroyed_clone.store(true, Ordering::SeqCst);
                                unsafe {
                                    player.pause();
                                    item.removeOutput(&video_output);
                                    player.replaceCurrentItemWithPlayerItem(None);
                                }
                                return;
                            }
                        },
                        Err(crossbeam_channel::TryRecvError::Empty) => {}
                        Err(crossbeam_channel::TryRecvError::Disconnected) => {
                            log::info!("[avf] p{}: cmd channel closed", id.0);
                            return;
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

// ---------------------------------------------------------------------------
// CVPixelBuffer → VideoFrame
// ---------------------------------------------------------------------------

fn pixel_buffer_to_frame(pipeline_id: MediaPipelineId, buf: &CVPixelBuffer) -> Option<VideoFrame> {
    use objc2_core_video::{
        CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow, CVPixelBufferGetHeight,
        CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags,
        CVPixelBufferUnlockBaseAddress, kCVReturnSuccess,
    };

    unsafe {
        let lock = CVPixelBufferLockFlags::ReadOnly;
        if CVPixelBufferLockBaseAddress(buf, lock) != kCVReturnSuccess {
            log::warn!("[avf] CVPixelBuffer lock failed");
            return None;
        }

        let width = CVPixelBufferGetWidth(buf);
        let height = CVPixelBufferGetHeight(buf);
        let bpr = CVPixelBufferGetBytesPerRow(buf);
        let base = CVPixelBufferGetBaseAddress(buf) as *const u8;

        if width == 0 || height == 0 || base.is_null() {
            let _ = CVPixelBufferUnlockBaseAddress(buf, CVPixelBufferLockFlags::ReadOnly);
            return None;
        }

        let row_bytes = width * 4;
        let mut data = Vec::with_capacity(height * row_bytes);
        for row in 0..height {
            let src = std::slice::from_raw_parts(base.add(row * bpr), row_bytes);
            data.extend_from_slice(src);
        }

        CVPixelBufferUnlockBaseAddress(buf, CVPixelBufferLockFlags::ReadOnly);

        Some(VideoFrame {
            pipeline_id,
            width: width as u32,
            height: height as u32,
            data,
        })
    }
}
