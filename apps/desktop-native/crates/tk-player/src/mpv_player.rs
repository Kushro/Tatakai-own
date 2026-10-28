//! libmpv-backed player. Structure after limusic-forge `crates/player` (GPL-3.0-or-later, same
//! author; see THIRD_PARTY_NOTICES.md): create the handle, set options through the property API, observe the
//! few properties the UI needs and pump events on a dedicated thread.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use libmpv2::events::{Event, EventContext, PropertyData};
use libmpv2::{Format, Mpv};
use tokio::sync::broadcast;

use crate::headers::{HeaderPlan, mpv_list, mpv_quote};
use crate::preset::{ShaderPreset, glsl_shaders_value, list_separator};
use crate::types::{
    MediaSource, PlayerError, PlayerEvent, Rect, SubtitleSource, TrackInfo, TrackKind,
};

/// Sent when an extension gives no User-Agent (mpv's own "libmpv" is blocked by many CDNs).
const DEFAULT_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

fn mpv_err(e: impl std::fmt::Display) -> PlayerError {
    PlayerError::Mpv(e.to_string())
}

pub struct Player {
    // Field order matters: mpv is dropped (after the event thread is joined in `Drop`) before the
    // surface window is destroyed.
    mpv: Arc<Mpv>,
    #[cfg(windows)]
    surface: crate::surface::VideoSurface,
    tx: broadcast::Sender<PlayerEvent>,
    preset: Mutex<ShaderPreset>,
    shaders_dir: Mutex<Option<PathBuf>>,
    pending_subs: Arc<Mutex<Vec<SubtitleSource>>>,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Player {
    /// Create the player and its child window under `parent_hwnd` (call on the window's own thread).
    pub fn new(parent_hwnd: isize) -> Result<Self, PlayerError> {
        #[cfg(windows)]
        let surface = crate::surface::VideoSurface::create(parent_hwnd)?;
        #[cfg(not(windows))]
        let _ = parent_hwnd;

        // Create first, then set properties: some options return PROPERTY_NOT_FOUND during pre-init.
        let mpv = Mpv::new().map_err(mpv_err)?;
        #[cfg(windows)]
        mpv.set_property("wid", surface.wid()).map_err(mpv_err)?;
        for (key, value) in [
            ("vo", "gpu-next,gpu"),
            ("hwdec", "auto-safe"),
            ("osd-level", "0"),
            ("osd-bar", "no"),
            ("ytdl", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
            ("cache", "yes"),
            ("title", "Tatakai"),
            (
                "stream-lavf-o",
                "reconnect=1,reconnect_streamed=1,reconnect_on_network_error=1,reconnect_delay_max=2,reconnect_max_retries=6",
            ),
        ] {
            // Best effort: an mpv build that refuses one cosmetic option must not kill the player.
            if let Err(e) = mpv.set_property(key, value) {
                tracing::warn!(key, error = %e, "mpv refused an option");
            }
        }
        let _ = mpv.set_property("network-timeout", 20_i64);
        let _ = mpv.set_property("demuxer-max-bytes", 150_i64 * 1024 * 1024);

        let ev = EventContext::new(mpv.ctx);
        ev.disable_deprecated_events().ok();
        ev.observe_property("time-pos", Format::Double, 0).map_err(mpv_err)?;
        ev.observe_property("duration", Format::Double, 1).map_err(mpv_err)?;
        ev.observe_property("pause", Format::Flag, 2).map_err(mpv_err)?;
        ev.observe_property("vo-configured", Format::Flag, 3).map_err(mpv_err)?;
        ev.observe_property("paused-for-cache", Format::Flag, 4).map_err(mpv_err)?;

        let mpv = Arc::new(mpv);
        let (tx, _) = broadcast::channel(256);
        let stop = Arc::new(AtomicBool::new(false));
        let pending_subs = Arc::new(Mutex::new(Vec::new()));
        let handle = {
            let (mpv, tx, stop, subs) = (mpv.clone(), tx.clone(), stop.clone(), pending_subs.clone());
            std::thread::Builder::new()
                .name("mpv-events".into())
                .spawn(move || run_events(mpv, ev, tx, stop, subs))
                .map_err(mpv_err)?
        };
        Ok(Self {
            mpv,
            #[cfg(windows)]
            surface,
            tx,
            preset: Mutex::new(ShaderPreset::Off),
            shaders_dir: Mutex::new(None),
            pending_subs,
            stop,
            thread: Mutex::new(Some(handle)),
        })
    }

    /// Directory holding the pinned Anime4K / FSR shader files (resolved by the app).
    pub fn set_shaders_dir(&self, dir: PathBuf) {
        if let Ok(mut d) = self.shaders_dir.lock() {
            *d = Some(dir);
        }
    }

    /// Load `src` and start playing at `start_secs`.
    pub fn load(&self, src: &MediaSource, start_secs: f64) -> Result<(), PlayerError> {
        let plan = HeaderPlan::from_headers(&src.headers);
        self.mpv.set_property("referrer", plan.referrer.unwrap_or_default().as_str()).map_err(mpv_err)?;
        self.mpv.set_property("user-agent", plan.user_agent.as_deref().unwrap_or(DEFAULT_UA)).map_err(mpv_err)?;
        self.mpv.set_property("http-header-fields", mpv_list(&plan.fields).as_str()).map_err(mpv_err)?;
        let start = if start_secs.is_finite() && start_secs > 0.0 { start_secs } else { 0.0 };
        self.mpv.set_property("start", format!("{start:.3}").as_str()).map_err(mpv_err)?;
        if let Ok(mut s) = self.pending_subs.lock() {
            *s = src.subtitles.clone();
        }
        self.mpv.command("loadfile", &[&mpv_quote(&src.url), "replace"]).map_err(mpv_err)?;
        self.mpv.set_property("pause", false).map_err(mpv_err)
    }

    pub fn play(&self) -> Result<(), PlayerError> {
        self.mpv.set_property("pause", false).map_err(mpv_err)
    }

    pub fn pause(&self) -> Result<(), PlayerError> {
        self.mpv.set_property("pause", true).map_err(mpv_err)
    }

    /// Absolute seek in seconds.
    pub fn seek(&self, secs: f64) -> Result<(), PlayerError> {
        self.mpv.command("seek", &[&format!("{:.3}", secs.max(0.0)), "absolute"]).map_err(mpv_err)
    }

    pub fn seek_relative(&self, delta: f64) -> Result<(), PlayerError> {
        self.mpv.command("seek", &[&format!("{delta:.3}"), "relative"]).map_err(mpv_err)
    }

    /// 0..=100 (values above 100 amplify; clamped to 130 like mpv's default `volume-max`).
    pub fn set_volume(&self, percent: f64) -> Result<(), PlayerError> {
        self.mpv.set_property("volume", percent.clamp(0.0, 130.0)).map_err(mpv_err)
    }

    pub fn set_mute(&self, mute: bool) -> Result<(), PlayerError> {
        self.mpv.set_property("mute", mute).map_err(mpv_err)
    }

    pub fn stop(&self) {
        let _ = self.mpv.command("stop", &[]);
    }

    /// Switch the upscaling preset live. The shader files must exist; on error the current preset stays.
    pub fn set_shader_preset(&self, preset: ShaderPreset) -> Result<(), PlayerError> {
        let value = if preset == ShaderPreset::Off {
            String::new()
        } else {
            let dir = self.shaders_dir.lock().map_err(mpv_err)?.clone().ok_or(PlayerError::NoShadersDir)?;
            let missing = preset.missing_files(&dir);
            if !missing.is_empty() {
                return Err(PlayerError::ShadersMissing(missing.join(", ")));
            }
            glsl_shaders_value(&dir, preset, list_separator())
        };
        self.mpv.set_property("glsl-shaders", value.as_str()).map_err(mpv_err)?;
        if let Ok(mut p) = self.preset.lock() {
            *p = preset;
        }
        Ok(())
    }

    pub fn shader_preset(&self) -> ShaderPreset {
        self.preset.lock().map(|p| *p).unwrap_or_default()
    }

    /// Put the picture at `rect` (CSS px relative to the viewport, `dpr` = devicePixelRatio) or hide it.
    /// Call on the UI thread. Returns whether the child window is shown.
    pub fn set_rect(&self, rect: Option<Rect>, dpr: f64) -> bool {
        #[cfg(windows)]
        {
            self.surface.place(rect, dpr)
        }
        #[cfg(not(windows))]
        {
            let _ = (rect, dpr);
            false
        }
    }

    pub fn events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.tx.subscribe()
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Ok(mut t) = self.thread.lock()
            && let Some(h) = t.take()
        {
            let _ = h.join();
        }
    }
}

fn run_events(
    mpv: Arc<Mpv>,
    mut ev: EventContext,
    tx: broadcast::Sender<PlayerEvent>,
    stop: Arc<AtomicBool>,
    subs: Arc<Mutex<Vec<SubtitleSource>>>,
) {
    // Nobody listening is normal (no subscriber yet); ignore send errors.
    let send = |e: PlayerEvent| {
        let _ = tx.send(e);
    };
    while !stop.load(Ordering::SeqCst) {
        let Some(event) = ev.wait_event(0.25) else { continue };
        match event {
            Ok(Event::PropertyChange { name, change, .. }) => match (name, change) {
                ("time-pos", PropertyData::Double(p)) => send(PlayerEvent::Position(p)),
                ("duration", PropertyData::Double(d)) => send(PlayerEvent::Duration(d)),
                ("pause", PropertyData::Flag(b)) => send(PlayerEvent::Paused(b)),
                ("vo-configured", PropertyData::Flag(b)) => send(PlayerEvent::VideoReady(b)),
                ("paused-for-cache", PropertyData::Flag(b)) => send(PlayerEvent::Buffering(b)),
                _ => {}
            },
            Ok(Event::FileLoaded) => {
                let pending = subs.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
                for s in pending {
                    let title = if s.label.is_empty() { s.lang.clone() } else { s.label.clone() };
                    let _ = mpv.command("sub-add", &[&mpv_quote(&s.url), "auto", &mpv_quote(&title), &mpv_quote(&s.lang)]);
                }
                send(PlayerEvent::Tracks(read_tracks(&mpv)));
            }
            // 0 = MPV_END_FILE_REASON_EOF. Stop/quit/redirect are deliberate; errors arrive as `Err` below.
            Ok(Event::EndFile(reason)) if reason as i32 == 0 => send(PlayerEvent::Eof),
            Ok(Event::Shutdown) => break,
            Ok(_) => {}
            Err(e) => send(PlayerEvent::Error(e.to_string())),
        }
    }
}

fn read_tracks(mpv: &Mpv) -> Vec<TrackInfo> {
    let count = mpv.get_property::<i64>("track-list/count").unwrap_or(0);
    (0..count)
        .filter_map(|i| {
            let kind = match mpv.get_property::<String>(&format!("track-list/{i}/type")).ok()?.as_str() {
                "video" => TrackKind::Video,
                "audio" => TrackKind::Audio,
                "sub" => TrackKind::Sub,
                _ => return None,
            };
            Some(TrackInfo {
                id: mpv.get_property::<i64>(&format!("track-list/{i}/id")).ok()?,
                kind,
                lang: mpv.get_property::<String>(&format!("track-list/{i}/lang")).ok(),
                title: mpv.get_property::<String>(&format!("track-list/{i}/title")).ok(),
                selected: mpv.get_property::<bool>(&format!("track-list/{i}/selected")).unwrap_or(false),
            })
        })
        .collect()
}
