//! A permanent capture failure must be visible even while its supervisor stays
//! alive waiting for replacement. This never opens a microphone: the deliberately
//! malformed device id is rejected during format discovery.

#![cfg(feature = "capture")]

use std::{
    io::{self, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use moq_media::{audio::capture, publish::LocalBroadcast};

struct LogWriter(Arc<Mutex<Vec<u8>>>);

impl Write for LogWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .expect("log buffer lock")
            .extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn a_failed_microphone_is_logged_and_can_shut_down() {
    let logs = Arc::new(Mutex::new(Vec::new()));
    let output = logs.clone();
    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .with_writer(move || LogWriter(output.clone()))
        .init();

    let local = LocalBroadcast::new(moq_net::broadcast::Producer::new(Default::default()))
        .expect("create broadcast");
    let mut config = capture::Config::default();
    config.source = capture::Source::Microphone(Some("irl-ci-invalid-device-id".into()));
    local.audio().set(config);

    let reported = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let text = String::from_utf8_lossy(&logs.lock().expect("log buffer lock")).into_owned();
            if text.contains("audio capture failed") {
                break text;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;

    // Shutdown is checked even if the warning is missing: a failed, parked
    // publication must release its thread when the call ends.
    tokio::time::timeout(Duration::from_secs(5), local.finish())
        .await
        .expect("a failed capture task shuts down promptly");
    let text = reported.unwrap_or_else(|_| {
        panic!(
            "microphone failure was never reported; logs:\n{}",
            String::from_utf8_lossy(&logs.lock().expect("log buffer lock"))
        )
    });
    println!("{text}");
    assert!(
        text.contains("irl-ci-invalid-device-id"),
        "log the selected input"
    );
    assert!(
        text.contains("not an input device id"),
        "log the underlying failure"
    );
}
