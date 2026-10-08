//! The audio decode task: one track into the shared playback engine.
//!
//! `moq_audio::playback::Engine` owns the output device and mixes up to 64
//! sinks into it, so a process watching several broadcasts opens one engine and
//! one sink per broadcast. The engine is created lazily and shared, because
//! opening a second output device would fight the first for the speaker.

use n0_error::{Result, e};
use n0_future::task::{AbortOnDropHandle, spawn};
use std::time::{Duration, Instant};
use tracing::{Instrument, debug, error_span, info, warn};

use super::{AudioTrack, RemoteBroadcast, SubscribeError, audio_decode_config};

/// Opens `rendition` and starts playing it.
pub(super) async fn open(
    broadcast: &RemoteBroadcast,
    rendition: &str,
) -> Result<AudioTrack, SubscribeError> {
    let catalog = broadcast.catalog();
    let config = catalog.audio().get(rendition).cloned().ok_or_else(|| {
        e!(SubscribeError::NoRendition {
            name: rendition.to_string(),
        })
    })?;

    let context = broadcast.decode_context();
    let decode = audio_decode_config(&context.policy);
    let mut consumer =
        moq_audio::decode::Consumer::new(broadcast.consumer(), &config, rendition, decode).await?;

    let mut input = moq_audio::playback::Input::default();
    input.format = moq_audio::Format::F32;
    input.sample_rate = consumer.sample_rate();
    input.layout = consumer.layout();
    let mut sink = crate::playback::engine().await?.sink(input)?;
    let control = sink.control();
    info!(
        rendition,
        sample_rate = consumer.sample_rate(),
        channels = consumer.layout().channels(),
        "audio playing",
    );

    let task = spawn(
        async move {
            let mut reported = Instant::now();
            // `consumer.read()` sits in a `select!`, which the video side goes
            // out of its way to avoid. It is safe here because the audio
            // consumer reads through a poll function whose state lives in
            // `&mut self`, with no `Sink` to poison, and because the only
            // competing arm is terminal: a cancelled read is never re-polled.
            // A third arm would break both halves of that argument.
            loop {
                tokio::select! {
                    _ = context.shutdown.cancelled() => {
                        debug!("audio playback cancelled");
                        return;
                    }
                    frame = consumer.read() => match frame {
                        Ok(Some(frame)) => {
                            // The video clock steers off how much audio is
                            // still buffered ahead of the speaker, which is the
                            // only latency either side can actually measure.
                            context.sync.set_audio_buffered(Some(sink.buffered()));
                            let written = match sink.write(&frame.data) {
                                Ok(written) => written,
                                Err(err) => {
                                    warn!(error = %err, "audio sink write failed");
                                    return;
                                }
                            };
                            let buffered = sink.buffered();
                            context.stats.timing.audio_buf_ms.record_ms(buffered);
                            if written.accepted_sample_frames > 0 {
                                let start = Duration::from_micros(frame.timestamp.as_micros() as u64);
                                let accepted = Duration::from_secs_f64(
                                    written.accepted_sample_frames as f64 / f64::from(sink.input().sample_rate),
                                );
                                context.sync.record_audio_position(start + accepted, buffered);
                            }
                            if reported.elapsed() >= Duration::from_secs(1) {
                                debug!(
                                    pts_ms = (frame.timestamp.as_micros() / 1000) as u64,
                                    buffered_ms = buffered.as_secs_f64() * 1000.0,
                                    dropped_samples = written.dropped_sample_frames,
                                    "audio playout timing",
                                );
                                reported = Instant::now();
                            }
                        }
                        Ok(None) => {
                            debug!("audio track ended");
                            return;
                        }
                        Err(err) => {
                            warn!(error = %err, "audio decode failed");
                            return;
                        }
                    },
                }
            }
        }
        .instrument(error_span!("audio", broadcast = %broadcast.name())),
    );

    Ok(AudioTrack {
        _broadcast: broadcast.clone(),
        rendition: rendition.to_string(),
        control,
        _task: AbortOnDropHandle::new(task),
    })
}
