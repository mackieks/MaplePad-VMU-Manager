use rodio::{Decoder, OutputStreamBuilder, Sink};
use std::{io::Cursor, sync::mpsc, time::Duration};

const TRACKS: [&[u8]; 2] = [
    include_bytes!("../assets/0.ogg"),
    include_bytes!("../assets/1.ogg"),
];

pub struct Music {
    pub enabled: bool,
    pub error: Option<String>,
    commands: mpsc::Sender<bool>,
    errors: mpsc::Receiver<String>,
}
impl Music {
    pub fn new(enabled: bool) -> Self {
        let (commands, receiver) = mpsc::channel();
        let (errors, messages) = mpsc::channel();
        std::thread::spawn(move || {
            let mut playing = enabled;
            // Own the output stream on this worker for its entire lifetime.
            // Dropping the app's sender stops playback and releases the device.
            while !playing {
                match receiver.recv() {
                    Ok(on) => playing = on,
                    Err(_) => return,
                }
            }
            let mut stream = match OutputStreamBuilder::open_default_stream() {
                Ok(stream) => stream,
                Err(e) => {
                    let _ = errors.send(format!("Music unavailable: {e}"));
                    return;
                }
            };
            stream.log_on_drop(false);
            let sink = Sink::connect_new(stream.mixer());
            sink.set_volume(0.35);
            let mut next = 0;
            loop {
                // Queue a successor before the current track ends. Recreate
                // decoders from embedded Vorbis, without caching minutes of PCM.
                while sink.len() < 2 {
                    match Decoder::try_from(Cursor::new(TRACKS[next])) {
                        Ok(source) => sink.append(source),
                        Err(e) => {
                            let _ = errors.send(format!("Cannot decode music: {e}"));
                            return;
                        }
                    }
                    next = (next + 1) % TRACKS.len();
                }
                match receiver.recv_timeout(Duration::from_millis(200)) {
                    Ok(true) => sink.play(),
                    Ok(false) => sink.pause(),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
        Self {
            enabled,
            error: None,
            commands,
            errors: messages,
        }
    }
    pub fn poll(&mut self) {
        while let Ok(error) = self.errors.try_recv() {
            self.error = Some(error);
        }
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if self.commands.send(enabled).is_err() && enabled {
            *self = Self::new(true);
        }
        if let Some(path) = settings_path() {
            let result = std::fs::create_dir_all(path.parent().unwrap())
                .and_then(|_| std::fs::write(path, if enabled { "on" } else { "off" }));
            if let Err(e) = result {
                self.error = Some(format!("Cannot save music preference: {e}"));
            }
        }
    }
}
fn settings_path() -> Option<std::path::PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(|p| std::path::PathBuf::from(p).join("MaplePad VMU Manager/music.txt"))
}
pub fn load_enabled() -> bool {
    !settings_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| s.trim() == "off")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;
    #[test]
    fn embedded_tracks_decode_without_an_audio_device() {
        for (bytes, expected_seconds) in TRACKS.into_iter().zip([133.026667, 194.173333]) {
            let decoder = Decoder::try_from(Cursor::new(bytes)).unwrap();
            assert_eq!(decoder.sample_rate(), 44100);
            assert_eq!(decoder.channels(), 2);
            let samples_per_second =
                f64::from(decoder.sample_rate()) * f64::from(decoder.channels());
            // Ogg need not report duration up front. Decode through EOF to check
            // the actual length, finite samples, and non-silent output instead.
            let mut count = 0_u64;
            let mut peak = 0.0_f32;
            for sample in decoder {
                assert!(sample.is_finite());
                peak = peak.max(sample.abs());
                count += 1;
            }
            let seconds = count as f64 / samples_per_second;
            assert!(
                (seconds - expected_seconds).abs() < 0.05,
                "decoded {seconds} seconds"
            );
            assert!(peak > 0.01);
        }
    }
}
