# Instrumentation and tests

Debugging a real-time pipeline means seeing frame timing, network conditions, and
codec behaviour while the system runs at 30 frames a second. Two pieces cover
that: a metrics vocabulary in `moq-media` and an overlay in `moq-media-egui` that
draws it.

## Metrics

`moq_media::stats` defines two primitives and groups them into typed structs, so
there are no string keys and no registration.

A `Metric` holds an exponentially smoothed current value and a ring buffer of
history for sparklines. `MetricMeta` carries the label, the unit, the smoothing
factor, and optional `Thresholds` that colour the value green, yellow, or red.
`Thresholds::inverted` flips the comparison for a metric where higher is better,
such as frame rate. A `Label` is a string that changes rarely, such as the
decoder backend that opened.

The groups are `NetStats` (round-trip time, loss, bandwidth in both directions,
path type and address), `EncodeStats` (frame rate, encode time, bitrate, and
labels for codec, encoder, and resolution), `RenderStats` (frame rate, decode
time, and labels for decoder, renderer, and rendition), and `TimingStats` (audio
buffer depth, per-path lag, and the A/V delta). `PublishStats` and
`SubscribeStats` bundle the ones each side needs, and `Timeline` records
per-frame arrival, decode, and render instants for the timeline panel.

## What is filled in today

The publish path records `encode.encoder`, `encode.resolution`, `encode.encode_ms`,
and `encode.bitrate_kbps`. `iroh-live`'s `util::spawn_stats_recorder` fills
`NetStats` from the iroh connection's selected path every 200 ms. The egui overlay
sets `render.rendition` from the track.

The video path records measured FPS and the combined transport-read/decode
duration. Audio playback records the real sink fill level. Video delivery records
its playout hold and the offset from estimated audible audio PTS; positive A/V
offset means video is behind audio. Timing debug logs appear once per second
under `RUST_LOG=moq_media=debug`. These measurements exclude device and compositor
latency and are not a cross-machine capture-to-display measurement.

The per-path lag fields, decoded-frame queue field, and `Timeline` still have no
producer. The timeline's −10s axis is history, not measured delay; an unobserved
audio buffer is labelled unavailable rather than zero.

The Desktop runtime workflow builds the same source and locked dependencies on
Windows 2022, Windows 2025, macOS 14, and macOS 26. Rust is pinned to 1.99.0 and
the macOS deployment target to 14.0. Artifacts include the executable, runner
device inventory, SHA256SUMS, and build-info.json with the commit, compiler,
lockfile hash, runner image, and SDK. Apple silicon artifacts also include a
tarball that preserves executable permissions. This makes inputs traceable and
builds repeatable; runner images and SDKs can still change between runs.

The workflow runs the latency suite, including a three-second viewer pause that
must resume on a recent frame. Tests use generated media and real QUIC; physical
camera, microphone, speaker, and display behaviour still need device checks.

## The debug overlay

`moq_media_egui::overlay::DebugOverlay` draws a translucent bar along the bottom
of a video tile with one clickable section per `StatCategory`: `Net`, `Capture`,
`Render`, and `Time`. Clicking a section opens a detail panel above the bar,
stacking upward, with each metric shown as a value, a unit, a threshold colour,
and a sparkline once it has at least two samples.

`irl publish --preview` enables the `Capture` and `Net` categories; `irl watch`
enables `Net`, `Render`, and `Time`.

The `Time` category also draws a timeline panel over a ten-second window: a
latency graph, one lane of video frame boxes coloured by inter-frame gap with a
white edge on keyframes, an audio lane, an A/V offset lane around a zero line, and
sparklines for audio buffer depth and round-trip time. The mouse wheel scrolls
back in time and switches the indicator from `LIVE` to `PAUSED`; a double click
returns to live. It reads `Timeline`, so it stays empty until something records
into it.

## Tests

`iroh-live/tests/e2e.rs` runs three tests over a real QUIC connection between two
iroh endpoints. Every source is generated, so no camera, microphone, or speaker is
needed, but the codecs are real: openh264 and Opus encode and decode, and the
bytes cross an actual transport. `publish_subscribe_video` asserts five frames
with non-zero size and non-decreasing timestamps. `publish_subscribe_audio`
decodes through `moq_audio::decode::Consumer` rather than the playback engine, so
it proves the transport and the codec without needing an output device.
`adaptive_rendition_switching` drives the adaptation loop with made-up
`NetworkSignals` and asserts the downgrade lands.

`iroh-rooms/tests/room.rs` covers discovery, subscription, chat, and peer
departure. Nothing there touches media: the broadcasts carry a plain data track
with hand-written frames, since `iroh-rooms` has no media dependency.

`iroh-live-relay/tests/relay_bridge.rs` covers bridging between the WebTransport
and iroh sides of the relay. `tests/e2e-browser/` is a Playwright suite that
builds the relay and the CLI, serves the embedded web client, and watches a
stream in Chromium.

`iroh-live/tests/patchbay.rs` is the only place anything impairs a link. It puts
the publisher and the subscriber in separate network namespaces with a router
between them and applies netem latency, jitter and loss, so the impairment
reaches QUIC rather than being described to the pipeline after the fact. Two
tests hold the delivery cadence to account across a latency ramp and a loss
spike; `adaptation_follows_a_real_link` runs the whole adaptive chain, from
dropped packets through QUIC's loss detection and the path stats the signal
producer samples to a rendition downgrade, and back up once the loss clears;
`a_switch_does_not_blank_the_picture` holds the decode supervisor to its overlap,
that a replacement decoder takes over on its own first frame rather than after
the incumbent is gone. It is Linux-only and needs unprivileged user namespaces,
set up from an ELF initialiser before the harness has a second thread. nextest
gives the binary a single-threaded group of its own, because the timing
assertions do not survive sharing a machine with the rest of the suite.

```sh
cargo make test           # cargo nextest run --locked --workspace
cargo make test-patchbay  # the network simulation suite, including ignored tests
cargo make test-e2e       # builds the relay and CLI, then runs Playwright
cargo make test-full      # check-all, then both of the above
```

## What is gone

The `frame_dump` example, which saved frames as PNGs and checked them against an
SMPTE pattern by PSNR, was removed with the in-house decoder it drove. The
`pi-zero-demo codec-test` subcommand went with the V4L2 M2M codec it tested.

The patchbay suite went the same way when the pipeline it drove was replaced, but
it is back, rewritten against the new one; the A/V sync measurements it also
carried are not, because the timestamping audio backend they sampled has no
counterpart yet.
