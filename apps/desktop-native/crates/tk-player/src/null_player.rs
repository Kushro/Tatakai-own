//! Player used when the `mpv` feature is off: same API, no playback (tests, CI without libmpv).

use std::path::PathBuf;
use std::sync::Mutex;

use tokio::sync::broadcast;

use crate::preset::{Anime4kQuality, ShaderPreset};
use crate::embed::EmbedTarget;
use crate::engine::EngineConfig;
use crate::types::{KeyAction, MediaSource, PlayerError, PlayerEvent, Rect};

pub struct Player {
    tx: broadcast::Sender<PlayerEvent>,
    preset: Mutex<ShaderPreset>,
    shaders_dir: Mutex<Option<PathBuf>>,
}

impl Player {
    pub fn new(_target: &EmbedTarget, _cfg: &EngineConfig) -> Result<Self, PlayerError> {
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

    pub fn shader_quality(&self) -> Anime4kQuality {
        Anime4kQuality::Fast
    }

    pub fn set_shader_quality(&self, _quality: Anime4kQuality) -> Result<(), PlayerError> {
        Ok(())
    }

    pub fn set_deband(&self, _on: bool) -> Result<(), PlayerError> {
        Ok(())
    }

    pub fn set_image_adjust(&self, _brightness: i8, _contrast: i8, _saturation: i8, _gamma: i8) -> Result<(), PlayerError> {
        Ok(())
    }

    pub fn is_embedded(&self) -> bool {
        false
    }

    pub fn set_rect(&self, _rect: Option<Rect>, _dpr: f64) -> bool {
        false
    }

    pub fn forward_mouse(&self, _x: f64, _y: f64, _dpr: f64) -> Result<(), PlayerError> {
        Ok(())
    }

    pub fn flash_uosc_controls(&self) -> Result<(), PlayerError> {
        Ok(())
    }

    pub fn forward_key(&self, _name: &str, _action: KeyAction) -> Result<(), PlayerError> {
        Ok(())
    }

    pub fn events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.tx.subscribe()
    }

    // --- wave A wrappers (same shape as `mpv_player::Player`) ---

    pub fn set_sub_track(&self, _id: Option<i64>) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_secondary_sub_track(&self, _id: Option<i64>) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_audio_track(&self, _id: Option<i64>) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_speed(&self, _speed: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_interpolation(&self, _on: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_loop_file(&self, _on: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_sub_delay(&self, _secs: f64) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_sub_style(&self, _style: &crate::subs::SubStyle) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn add_subtitle(&self, _sub: &crate::types::SubtitleSource, _select: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn screenshot_to_file(&self, _path: &std::path::Path, _with_subs: bool) -> Result<(), PlayerError> {
        Err(PlayerError::Unavailable)
    }
    pub fn set_stable_volume(&self, _on: bool) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn set_option_str(&self, _name: &str, _value: &str) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn command_raw(&self, _name: &str, _args: &[&str]) -> Result<(), PlayerError> {
        Ok(())
    }
    pub fn property_f64(&self, _name: &str) -> Option<f64> {
        None
    }
    pub fn property_str(&self, _name: &str) -> Option<String> {
        None
    }
}
