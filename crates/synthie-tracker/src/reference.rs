//! Reference playback of a loaded MOD with its original samples, on its own
//! output stream, to compare against the tracker's synth rendition.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use xmrs::prelude::Module;
use xmrsplayer::prelude::XmrsPlayer;

/// The original module, playing or silent on a second cpal stream.
pub struct Reference {
    /// Stored with `Release` after `restart`, so the audio thread, loading
    /// it with `Acquire`, never starts playing before it sees the rewind.
    playing: Arc<AtomicBool>,
    /// Set to have the audio thread rewind to the start position.
    restart: Arc<AtomicBool>,
    _stream: cpal::Stream,
}

impl Reference {
    /// Load `bytes` as a MOD and open a silent output stream for it; play
    /// starts at order position `start`, where the grid's import began.
    ///
    /// # Errors
    ///
    /// Fails if the module does not load or no output stream opens.
    pub fn new(bytes: &[u8], start: usize) -> Result<Self> {
        let module = Module::load_mod(bytes).map_err(|e| anyhow::anyhow!("{e:?}"))?;
        // shortcut: leaks the one module per run because the player borrows it;
        // own it alongside the player if modules ever get swapped at runtime.
        let module: &'static Module = Box::leak(Box::new(module));

        let device = cpal::default_host()
            .default_output_device()
            .context("no default audio output device")?;
        let config = device
            .default_output_config()
            .context("failed to query default output config")?;
        let channels = usize::from(config.channels());

        let mut player = XmrsPlayer::new(module, config.sample_rate(), 0);
        player.set_max_loop_count(0); // loop until stopped

        let playing = Arc::new(AtomicBool::new(false));
        let restart = Arc::new(AtomicBool::new(false));
        let (playing_cb, restart_cb) = (Arc::clone(&playing), Arc::clone(&restart));

        let stream = device
            .build_output_stream(
                config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    let playing = playing_cb.load(Ordering::Acquire);
                    if restart_cb.swap(false, Ordering::Acquire) {
                        player.goto(start, 0, 0);
                    }
                    if !playing {
                        data.fill(0.0);
                        return;
                    }
                    for frame in data.chunks_mut(channels) {
                        let (l, r) = player.sample(true).unwrap_or((0, 0));
                        let (l, r) = (f32::from(l) / 32_768.0, f32::from(r) / 32_768.0);
                        match frame {
                            [mono] => *mono = f32::midpoint(l, r),
                            [fl, fr, rest @ ..] => {
                                (*fl, *fr) = (l, r);
                                rest.fill(0.0);
                            }
                            [] => {}
                        }
                    }
                },
                |_| {}, // the synth stream already reports device loss
                None,
            )
            .context("failed to build reference output stream")?;
        stream.play().context("failed to start reference stream")?;

        Ok(Self {
            playing,
            restart,
            _stream: stream,
        })
    }

    /// Whether the original is sounding.
    #[must_use]
    pub fn is_playing(&self) -> bool {
        self.playing.load(Ordering::Acquire)
    }

    /// Play from the start position.
    pub fn play(&self) {
        self.restart.store(true, Ordering::Relaxed);
        self.playing.store(true, Ordering::Release);
    }

    /// Fall silent.
    pub fn stop(&self) {
        self.playing.store(false, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-pattern M.K. module whose only note plays a looped square wave.
    #[test]
    fn player_renders_an_imported_module() {
        let mut b = vec![0; 1084 + 1024];
        b[20 + 22..20 + 30].copy_from_slice(&[0, 16, 0, 64, 0, 0, 0, 16]); // 32 bytes, vol 64, looped
        b[950] = 1;
        b[1080..1084].copy_from_slice(b"M.K.");
        b[1084..1088].copy_from_slice(&[0x01, 0xAC, 0x10, 0x00]); // instrument 1, period 428
        b.extend((0..32).map(|i| if i < 16 { 0x60_u8 } else { 0xA0 }));

        let module = Module::load_mod(&b).expect("xmrs loads the module");
        let mut player = XmrsPlayer::new(&module, 48_000, 0);
        assert!(
            player.by_ref().take(9_600).any(|s| s != 0),
            "the note sounds"
        );
        assert!(player.goto(0, 0, 0));
        assert!(crate::tracker::modfile::import(&b, 0).is_ok());
    }
}
