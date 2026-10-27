//! Player used when the `mpv` feature is off: same API, no playback (tests, CI without libmpv).

use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::broadcast;

use crate::preset::ShaderPreset;
use crate::types::{MediaSource, PlayerError, PlayerEvent, Rect};

pub struct Player {
    tx: broadcast::Sender<PlayerEvent>,
    preset: Mutex<ShaderPreset>,
    shaders_dir: Mutex<Option<PathBuf>>,
}

impl Player {
    pub fn new(_parent_hwnd: isize) -> Result<Self, PlayerError> {
        let (tx, _) = broadcast::channel(16);
        Ok(Self { tx, preset: Mutex::new(ShaderPreset::Off), shaders_dir: Mutex::new(None) })
    }

    pub fn set_shaders_dir(&self, dir: PathBuf) {
        if let Ok(mut d) = self.shaders_dir.lock() {
            *d = Some(dir);
        }
    }

    pub fn load(&self, _src: &MediaSource, _start_secs: f64) -> Result<(), PlayerError> {
        let _ = self.tx.send(PlayerEvent::Error("player unavailable in this build".into()));
        Err(PlayerError::Unavailable)
    }

    pub fn play(&self) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn pause(&self) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn seek(&self, _secs: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn seek_relative(&self, _delta: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_volume(&self, _percent: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_mute(&self, _mute: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn stop(&self) {}

    pub fn set_shader_preset(&self, preset: ShaderPreset) -> Result<(), PlayerError> {
        if let Ok(mut p) = self.preset.lock() {
            *p = preset;
        }
        Ok(())
    }

    pub fn shader_preset(&self) -> ShaderPreset {
        self.preset.lock().map(|p| *p).unwrap_or_default()
    }

    pub fn set_rect(&self, _rect: Option<Rect>, _dpr: f64) -> bool {
        false
    }

    pub fn events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.tx.subscribe()
    }
}
