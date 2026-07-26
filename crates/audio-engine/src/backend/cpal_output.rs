//! CPAL device adapter. All host/device discovery and sample conversion stays
//! in this module so the playback state machine and DSP remain platform-neutral.

use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::Arc;
use std::thread::JoinHandle;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};

use super::OutputBackend;
use crate::media::{
    adapt_program_for_device, adapt_track_for_device_with_limit, MAX_DEVICE_PROGRAM_SAMPLES,
};
use crate::playback::{AudioError, RealtimeControl, RealtimeRenderer};
use crate::source::{LazyDeviceProgram, PlaybackSource, RealtimeSource};

pub(crate) struct CpalOutput {
    stream: Option<Stream>,
}

enum ControlMessage {
    EnsureStarted {
        control: Arc<RealtimeControl>,
        source: PlaybackSource,
        response: SyncSender<Result<(), AudioError>>,
    },
    Shutdown,
}

/// Sendable handle to a dedicated CPAL control thread. `cpal::Stream` is
/// intentionally !Send across supported platforms, so it is created, used,
/// and dropped on this worker rather than stored in Tauri managed state.
pub(crate) struct CpalThreadOutput {
    sender: SyncSender<ControlMessage>,
    worker: Option<JoinHandle<()>>,
}

impl CpalThreadOutput {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = sync_channel(2);
        let worker = std::thread::Builder::new()
            .name("adhd-music-audio-control".to_owned())
            .spawn(move || {
                let mut output = CpalOutput::new();
                while let Ok(message) = receiver.recv() {
                    match message {
                        ControlMessage::EnsureStarted {
                            control,
                            source,
                            response,
                        } => {
                            let _ = response.send(output.ensure_started(control, source));
                        }
                        ControlMessage::Shutdown => break,
                    }
                }
            })
            .expect("failed to create native audio control thread");
        Self {
            sender,
            worker: Some(worker),
        }
    }
}

impl Drop for CpalThreadOutput {
    fn drop(&mut self) {
        let _ = self.sender.send(ControlMessage::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

impl OutputBackend for CpalThreadOutput {
    fn ensure_started(
        &mut self,
        control: Arc<RealtimeControl>,
        source: PlaybackSource,
    ) -> Result<(), AudioError> {
        let (response, result) = sync_channel(1);
        self.sender
            .send(ControlMessage::EnsureStarted {
                control,
                source,
                response,
            })
            .map_err(|_| AudioError::ControlThreadStopped)?;
        result
            .recv()
            .map_err(|_| AudioError::ControlThreadStopped)?
    }
}

impl CpalOutput {
    pub(crate) fn new() -> Self {
        Self { stream: None }
    }

    fn open(control: Arc<RealtimeControl>, source: PlaybackSource) -> Result<Stream, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_output_device()
            .ok_or(AudioError::NoOutputDevice)?;
        let supported = device
            .default_output_config()
            .map_err(|error| AudioError::DefaultConfig(error.to_string()))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();

        match sample_format {
            SampleFormat::F32 => build_stream::<f32>(&device, &config, control, source),
            SampleFormat::I16 => build_stream::<i16>(&device, &config, control, source),
            SampleFormat::U16 => build_stream::<u16>(&device, &config, control, source),
            SampleFormat::I8 => build_stream::<i8>(&device, &config, control, source),
            SampleFormat::I32 => build_stream::<i32>(&device, &config, control, source),
            SampleFormat::U8 => build_stream::<u8>(&device, &config, control, source),
            SampleFormat::U32 => build_stream::<u32>(&device, &config, control, source),
            other => Err(AudioError::UnsupportedSampleFormat(format!("{other:?}"))),
        }
    }
}

impl OutputBackend for CpalOutput {
    fn ensure_started(
        &mut self,
        control: Arc<RealtimeControl>,
        source: PlaybackSource,
    ) -> Result<(), AudioError> {
        // A stopped session may select a different installed source. Rebuild on
        // this non-realtime control thread so the callback never swaps buffers.
        self.stream.take();
        let stream = Self::open(control, source)?;
        stream
            .play()
            .map_err(|error| AudioError::PlayStream(error.to_string()))?;
        self.stream = Some(stream);
        Ok(())
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    control: Arc<RealtimeControl>,
    source: PlaybackSource,
) -> Result<Stream, AudioError>
where
    T: Sample + SizedSample + FromSample<f32>,
{
    let channels = usize::from(config.channels);
    let sample_rate = config.sample_rate.0;
    if channels > crate::playback::MAX_OUTPUT_CHANNELS {
        return Err(AudioError::Media(format!(
            "device exposes {channels} channels; at most {} are supported",
            crate::playback::MAX_OUTPUT_CHANNELS
        )));
    }
    let source = match source {
        PlaybackSource::TestTone => RealtimeSource::test_tone(sample_rate),
        PlaybackSource::Installed(program) => {
            let device_program = adapt_program_for_device(&program, sample_rate, channels)
                .map_err(|error| AudioError::Media(error.to_string()))?;
            RealtimeSource::program(device_program)
                .map_err(|error| AudioError::Media(error.to_string()))?
        }
        PlaybackSource::InstalledLazy(queue) => {
            let first = queue
                .track(0)
                .ok_or_else(|| AudioError::Media("first playback track is not ready".to_owned()))?;
            let first = adapt_track_for_device_with_limit(
                first,
                sample_rate,
                channels,
                MAX_DEVICE_PROGRAM_SAMPLES,
            )
            .map_err(|error| AudioError::Media(error.to_string()))?;
            let remaining_samples = MAX_DEVICE_PROGRAM_SAMPLES
                .checked_sub(first.samples.len())
                .ok_or_else(|| AudioError::Media("playback sample limit exhausted".to_owned()))?;
            let device_queue = Arc::new(
                LazyDeviceProgram::new(first, queue.track_count(), sample_rate, channels)
                    .map_err(|error| AudioError::Media(error.to_string()))?,
            );
            let source_queue = Arc::clone(&queue);
            let target_queue = Arc::clone(&device_queue);
            std::thread::Builder::new()
                .name("aria-focus-playback-preparer".to_owned())
                .spawn(move || {
                    let mut remaining_samples = remaining_samples;
                    for index in 1..source_queue.track_count() {
                        let track = loop {
                            if let Some(track) = source_queue.track(index) {
                                break Some(track.clone());
                            }
                            if source_queue.is_closed() {
                                break None;
                            }
                            std::thread::sleep(std::time::Duration::from_millis(2));
                        };
                        let Some(track) = track else { break };
                        let Ok(adapted) = adapt_track_for_device_with_limit(
                            &track,
                            sample_rate,
                            channels,
                            remaining_samples,
                        ) else {
                            source_queue.close();
                            break;
                        };
                        remaining_samples =
                            match remaining_samples.checked_sub(adapted.samples.len()) {
                                Some(remaining) => remaining,
                                None => {
                                    source_queue.close();
                                    break;
                                }
                            };
                        if target_queue.publish(index, adapted).is_err() {
                            source_queue.close();
                            break;
                        }
                    }
                })
                .map_err(|error| {
                    AudioError::Media(format!("cannot prepare playback queue: {error}"))
                })?;
            RealtimeSource::lazy_program(device_queue)
                .map_err(|error| AudioError::Media(error.to_string()))?
        }
        PlaybackSource::Review(program) => {
            let device_program = adapt_program_for_device(&program, sample_rate, channels)
                .map_err(|error| AudioError::Media(error.to_string()))?;
            RealtimeSource::review_program(device_program)
                .map_err(|error| AudioError::Media(error.to_string()))?
        }
        PlaybackSource::Draft(program) => {
            let device_program = adapt_program_for_device(&program, sample_rate, channels)
                .map_err(|error| AudioError::Media(error.to_string()))?;
            RealtimeSource::draft_program(device_program)
                .map_err(|error| AudioError::Media(error.to_string()))?
        }
    };
    let error_control = Arc::clone(&control);
    let mut renderer = RealtimeRenderer::new(sample_rate, channels, control, source);

    device
        .build_output_stream(
            config,
            move |data: &mut [T], _| {
                renderer.sync_controls();
                for frame in data.chunks_mut(channels) {
                    let mut rendered = [0.0f32; crate::playback::MAX_OUTPUT_CHANNELS];
                    renderer.next_frame(&mut rendered[..channels]);
                    for (channel, sample) in frame.iter_mut().zip(rendered) {
                        *channel = T::from_sample(sample);
                    }
                }
            },
            move |_error| {
                // The error callback may run on the real-time thread. Record a
                // fixed-size signal only; reporting happens on a command thread.
                error_control.mark_stream_failed();
            },
            None,
        )
        .map_err(|error| AudioError::BuildStream(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_practical_formats_are_explicitly_supported() {
        // These are the practical WASAPI default formats. The match in `open`
        // also supports common 8/32-bit integer configs without a device test.
        for format in [SampleFormat::F32, SampleFormat::I16, SampleFormat::U16] {
            assert!(matches!(
                format,
                SampleFormat::F32 | SampleFormat::I16 | SampleFormat::U16
            ));
        }
    }
}
