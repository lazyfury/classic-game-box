//! Audio output: a `cpal` stream fed by an SPSC ring buffer.
//!
//! The libretro core produces interleaved **int16 stereo** samples through its
//! `audio_sample_batch` callback. The app drains them from `cgb-libretro` and
//! pushes them here with [`AudioOutput::push_interleaved`]; the realtime output
//! callback pops them and converts to whatever sample format the device wants.
//!
//! The device is opened at the core's sample rate (44100 / 48000 / 65536 /
//! 131072, from `retro_get_system_av_info`). Getting that wrong is audible as
//! wrong pitch, which is why the rate is a parameter of [`AudioOutput::new`]
//! and not read from the device default.
//!
//! **The audio callback never locks.** The ring buffer is the only shared
//! state, and it is lock-free. (The producer side *is* behind a `Mutex` in
//! [`AudioOutput`], but that lock is taken on the app thread, never in the
//! callback — the old Electron front end was bitten by locking in the audio
//! callback, which is why this is called out.)

use std::sync::Mutex;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};

/// Something that went wrong opening the output device.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no audio output device")]
    NoDevice,

    #[error("audio device config failed: {0}")]
    Config(String),

    #[error("audio device supports {0:?}, which this build does not convert")]
    UnsupportedFormat(cpal::SampleFormat),

    #[error("could not build the audio stream: {0}")]
    Build(String),
}

/// A live output stream plus its producer end.
pub struct AudioOutput {
    /// Dropped last: stops the callback that reads the consumer.
    _stream: cpal::Stream,
    producer: Mutex<HeapProd<i16>>,
    sample_rate: u32,
}

impl AudioOutput {
    /// Open the default output device at `sample_rate` Hz, stereo.
    pub fn new(sample_rate: u32) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(AudioError::NoDevice)?;

        let supported = device
            .default_output_config()
            .map_err(|error| AudioError::Config(error.to_string()))?;

        let mut config: cpal::StreamConfig = supported.clone().into();
        config.channels = 2;
        config.sample_rate = cpal::SampleRate(sample_rate);
        config.buffer_size = cpal::BufferSize::Default;

        // Ring buffer capacity: about one second of interleaved stereo. Small
        // enough to keep latency low, large enough to ride out a scheduling
        // hiccup. The value is in *samples*, not frames.
        let capacity = (sample_rate as usize).saturating_mul(2).max(4096);
        let (producer, consumer) = HeapRb::<i16>::new(capacity).split();

        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => build_stream::<f32>(&device, &config, consumer),
            cpal::SampleFormat::I16 => build_stream::<i16>(&device, &config, consumer),
            cpal::SampleFormat::U16 => build_stream::<u16>(&device, &config, consumer),
            other => return Err(AudioError::UnsupportedFormat(other)),
        }?;

        stream
            .play()
            .map_err(|error| AudioError::Build(error.to_string()))?;

        Ok(Self {
            _stream: stream,
            producer: Mutex::new(producer),
            sample_rate,
        })
    }

    /// Push interleaved int16 stereo samples. Excess is dropped when the buffer
    /// is full (the emulator must never block on audio).
    pub fn push_interleaved(&self, samples: &[i16]) {
        let mut producer = self
            .producer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        producer.push_slice(samples);
    }

    /// The rate the device was opened at.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    mut consumer: HeapCons<i16>,
) -> Result<cpal::Stream, AudioError>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    device
        .build_output_stream(
            config,
            move |data: &mut [T], _info: &cpal::OutputCallbackInfo| {
                for frame in data.chunks_exact_mut(2) {
                    let mut pair = [0i16; 2];
                    if consumer.pop_slice(&mut pair) == 2 {
                        frame[0] = T::from_sample(to_f32(pair[0]));
                        frame[1] = T::from_sample(to_f32(pair[1]));
                    } else {
                        // Underrun: emit silence rather than repeat old audio.
                        frame[0] = T::from_sample(0.0f32);
                        frame[1] = T::from_sample(0.0f32);
                    }
                }
            },
            |error| eprintln!("cgb-audio: stream error: {error}"),
            None,
        )
        .map_err(|error| AudioError::Build(error.to_string()))
}

fn to_f32(sample: i16) -> f32 {
    f32::from(sample) / 32768.0
}
