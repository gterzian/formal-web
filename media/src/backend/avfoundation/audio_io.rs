//! The default audio input and output for WebRTC audio tracks, on one
//! `AVAudioEngine`: a tap on the input node delivers 48 kHz mono 20 ms
//! frames to the capture sink, and a source node renders the frames the
//! WebRTC engine decoded for each remote track, mixed by summing.
//!
//! Capture and playout share the engine's clock, which keeps the echo
//! canceller's far-end reference aligned with the microphone signal.

use std::collections::{HashMap, VecDeque};
use std::ptr::NonNull;
use std::sync::{Arc, Mutex};

use block2::RcBlock;
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::Bool;
use objc2_avf_audio::{
    AVAudioEngine, AVAudioFormat, AVAudioFrameCount, AVAudioPCMBuffer, AVAudioSourceNode,
    AVAudioTime,
};
use objc2_core_audio_types::{AudioBufferList, AudioTimeStamp};

/// `OSStatus`, the render block's result type: 0 is success.
type OSStatus = i32;

/// The sample rate and frame size the WebRTC engine works in.
pub const SAMPLE_RATE: f64 = 48_000.0;
pub const FRAME_SAMPLES: usize = 960;
/// Playout keeps at most this many samples per stream (200 ms) so a stalled
/// consumer never accumulates latency.
const PLAYOUT_QUEUE_LIMIT: usize = FRAME_SAMPLES * 10;

/// Receives one 20 ms frame of captured 48 kHz mono PCM. Called from the
/// audio engine's tap thread.
pub type CaptureSink = Arc<dyn Fn(Vec<i16>) + Send + Sync>;

/// Identifies one remote audio track's playout queue.
pub type PlayoutKey = (u128, u32);

/// Converts the tap's buffers (the device's rate and channel count) into
/// 48 kHz mono 20 ms frames.
struct CaptureConverter {
    sink: CaptureSink,
    source_rate: f64,
    /// Mono samples at the source rate not yet resampled.
    pending: Vec<f32>,
    /// The position within `pending` of the next output sample.
    position: f64,
    /// Resampled samples not yet delivered as a full frame.
    frame: Vec<i16>,
}

impl CaptureConverter {
    fn push(&mut self, channels: &[&[f32]]) {
        if channels.is_empty() {
            return;
        }
        let frames = channels[0].len();
        let scale = 1.0 / channels.len() as f32;
        for index in 0..frames {
            let sum: f32 = channels.iter().map(|channel| channel[index]).sum();
            self.pending.push(sum * scale);
        }
        let step = self.source_rate / SAMPLE_RATE;
        while (self.position as usize) + 1 < self.pending.len() {
            let base = self.position as usize;
            let fraction = (self.position - base as f64) as f32;
            let sample = self.pending[base] * (1.0 - fraction) + self.pending[base + 1] * fraction;
            self.frame
                .push((sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16);
            self.position += step;
            if self.frame.len() == FRAME_SAMPLES {
                let frame = std::mem::replace(&mut self.frame, Vec::with_capacity(FRAME_SAMPLES));
                (self.sink)(frame);
            }
        }
        let consumed = self.position as usize;
        self.pending.drain(..consumed);
        self.position -= consumed as f64;
    }
}

/// The audio device I/O of one graphics process.
pub struct AudioIo {
    /// The engine owns the source node attached to it; the source node and
    /// the input tap hold copies of their blocks.
    engine: Retained<AVAudioEngine>,
    playout: Arc<Mutex<HashMap<PlayoutKey, VecDeque<i16>>>>,
    /// A tap is installed on the input node.
    capturing: bool,
    running: bool,
}

impl AudioIo {
    pub fn new() -> Result<Self, String> {
        // SAFETY: plain AVFAudio object construction on the calling thread;
        // the engine owns the nodes attached to it.
        unsafe {
            let engine = AVAudioEngine::new();
            let format = AVAudioFormat::initStandardFormatWithSampleRate_channels(
                AVAudioFormat::alloc(),
                SAMPLE_RATE,
                1,
            )
            .ok_or_else(|| String::from("48 kHz mono audio format not available"))?;
            let playout: Arc<Mutex<HashMap<PlayoutKey, VecDeque<i16>>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let render_playout = Arc::clone(&playout);
            let render_block = RcBlock::new(
                move |is_silence: NonNull<Bool>,
                      _timestamp: NonNull<AudioTimeStamp>,
                      frame_count: AVAudioFrameCount,
                      buffers: NonNull<AudioBufferList>|
                      -> OSStatus {
                    render(&render_playout, is_silence, frame_count, buffers);
                    0
                },
            );
            let source = AVAudioSourceNode::initWithFormat_renderBlock(
                AVAudioSourceNode::alloc(),
                &format,
                RcBlock::as_ptr(&render_block),
            );
            engine.attachNode(&source);
            engine.connect_to_format(&source, &engine.mainMixerNode(), Some(&format));
            Ok(Self {
                engine,
                playout,
                capturing: false,
                running: false,
            })
        }
    }

    fn ensure_running(&mut self) -> Result<(), String> {
        if self.running {
            return Ok(());
        }
        // SAFETY: the engine is fully connected; start fails with an NSError
        // (no device, permission denied) which is reported.
        unsafe {
            self.engine.prepare();
            self.engine
                .startAndReturnError()
                .map_err(|error| format!("audio engine start: {error}"))?;
        }
        self.running = true;
        Ok(())
    }

    fn stop_if_idle(&mut self) {
        let idle = !self.capturing
            && self
                .playout
                .lock()
                .map(|queues| queues.is_empty())
                .unwrap_or(true);
        if idle && self.running {
            // SAFETY: stopping a running engine on the thread that started it.
            unsafe { self.engine.stop() };
            self.running = false;
        }
    }

    /// Capture the default audio input, delivering 20 ms frames to `sink`.
    pub fn start_capture(&mut self, sink: CaptureSink) -> Result<(), String> {
        if self.capturing {
            return Ok(());
        }
        // SAFETY: the input node belongs to the engine; the tap's format is
        // the node's own output format, and the block copies every buffer
        // out before returning.
        unsafe {
            let input = self.engine.inputNode();
            let input_format = input.outputFormatForBus(0);
            let channel_count = input_format.channelCount().max(1) as usize;
            let converter = Mutex::new(CaptureConverter {
                sink,
                source_rate: input_format.sampleRate(),
                pending: Vec::new(),
                position: 0.0,
                frame: Vec::with_capacity(FRAME_SAMPLES),
            });
            let tap = RcBlock::new(
                move |buffer: NonNull<AVAudioPCMBuffer>, _when: NonNull<AVAudioTime>| {
                    let buffer = buffer.as_ref();
                    let frames = buffer.frameLength() as usize;
                    let channel_data = buffer.floatChannelData();
                    if channel_data.is_null() || frames == 0 {
                        return;
                    }
                    let channels: Vec<&[f32]> = (0..channel_count)
                        .map(|channel| {
                            let pointer = *channel_data.add(channel);
                            std::slice::from_raw_parts(pointer.as_ptr(), frames)
                        })
                        .collect();
                    if let Ok(mut converter) = converter.lock() {
                        converter.push(&channels);
                    }
                },
            );
            input.installTapOnBus_bufferSize_format_block(
                0,
                FRAME_SAMPLES as AVAudioFrameCount,
                Some(&input_format),
                RcBlock::as_ptr(&tap),
            );
            self.capturing = true;
        }
        if let Err(error) = self.ensure_running() {
            self.stop_capture();
            return Err(error);
        }
        Ok(())
    }

    pub fn stop_capture(&mut self) {
        if std::mem::replace(&mut self.capturing, false) {
            // SAFETY: removing the tap this object installed on bus 0.
            unsafe { self.engine.inputNode().removeTapOnBus(0) };
        }
        self.stop_if_idle();
    }

    /// Queue 48 kHz mono samples of one remote track for the output.
    pub fn push_playout(&mut self, key: PlayoutKey, samples: Vec<i16>) -> Result<(), String> {
        {
            let mut queues = self
                .playout
                .lock()
                .map_err(|_| String::from("playout queue poisoned"))?;
            let queue = queues.entry(key).or_default();
            queue.extend(samples);
            while queue.len() > PLAYOUT_QUEUE_LIMIT {
                queue.pop_front();
            }
        }
        self.ensure_running()
    }

    pub fn stop_playout(&mut self, key: PlayoutKey) {
        if let Ok(mut queues) = self.playout.lock() {
            queues.remove(&key);
        }
        self.stop_if_idle();
    }
}

/// The source node's render callback: mix the queued samples of every
/// stream into the output buffers, as 32-bit float mono.
fn render(
    playout: &Mutex<HashMap<PlayoutKey, VecDeque<i16>>>,
    is_silence: NonNull<Bool>,
    frame_count: AVAudioFrameCount,
    buffers: NonNull<AudioBufferList>,
) {
    let frames = frame_count as usize;
    let mut mixed = vec![0i32; frames];
    let mut any = false;
    if let Ok(mut queues) = playout.try_lock() {
        for queue in queues.values_mut() {
            for slot in mixed.iter_mut() {
                match queue.pop_front() {
                    Some(sample) => {
                        *slot += i32::from(sample);
                        any = true;
                    }
                    None => break,
                }
            }
        }
    }
    // SAFETY: `buffers` and `is_silence` are valid for the duration of the
    // render callback; each buffer holds `frame_count` floats of one channel.
    unsafe {
        let list = buffers.as_ref();
        let buffer_count = list.mNumberBuffers as usize;
        let first = list.mBuffers.as_ptr();
        for index in 0..buffer_count {
            let buffer = &*first.add(index);
            let data = buffer.mData as *mut f32;
            if data.is_null() {
                continue;
            }
            let output = std::slice::from_raw_parts_mut(data, frames);
            for (out, sample) in output.iter_mut().zip(&mixed) {
                *out = (*sample).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as f32
                    / i16::MAX as f32;
            }
        }
        *is_silence.as_ptr() = Bool::new(!any);
    }
}
