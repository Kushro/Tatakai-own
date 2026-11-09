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

use crate::embed::EmbedTarget;
use crate::engine::EngineConfig;
use crate::headers::{HeaderPlan, mpv_list, mpv_quote};
use crate::preset::{ShaderPreset, glsl_shaders_value, list_separator};
use crate::types::{
    KeyAction, MediaSource, PlayerError, PlayerEvent, Rect, SubtitleSource, TrackInfo, TrackKind,
};

/// Sent when an extension gives no User-Agent (mpv's own "libmpv" is blocked by many CDNs).
const DEFAULT_UA: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36";

fn mpv_err(e: impl std::fmt::Display) -> PlayerError {
    PlayerError::Mpv(e.to_string())
}

/// libmpv refuses to start (`mpv_create` returns NULL) unless LC_NUMERIC is "C". GTK, which WebKitGTK pulls in
/// outside Windows, sets the user's locale at start-up (e.g. es_AR, decimal comma), so reset just that category.
#[cfg(unix)]
fn force_c_numeric_locale() {
    unsafe extern "C" {
        fn setlocale(category: std::ffi::c_int, locale: *const std::ffi::c_char) -> *mut std::ffi::c_char;
    }
    // glibc and musl: LC_NUMERIC = 1.
    const LC_NUMERIC: std::ffi::c_int = 1;
    // SAFETY: called on the UI thread before mpv starts its threads; the argument is a static C string.
    unsafe {
        setlocale(LC_NUMERIC, c"C".as_ptr());
    }
}

pub struct Player {
    // Field order matters: on Linux the GL surface (mpv render context) goes first, then mpv is dropped (after
    // the event thread is joined in `Drop`), then the Windows surface window is destroyed.
    #[cfg(target_os = "linux")]
    gl: crate::linux::GlSurface,
    mpv: Arc<Mpv>,
    #[cfg(windows)]
    surface: crate::surface::VideoSurface,
    tx: broadcast::Sender<PlayerEvent>,
    preset: Mutex<ShaderPreset>,
    shaders_dir: Mutex<Option<PathBuf>>,
    pending_subs: Arc<Mutex<Vec<SubtitleSource>>>,
    /// False in the mpv-anime-build engine: its own shader profiles own `glsl-shaders` (enhanced_conf.rs).
    presets_apply: bool,
    stop: Arc<AtomicBool>,
    thread: Mutex<Option<JoinHandle<()>>>,
}

impl Player {
    /// Create the player and its video surface in `target` with the engine `cfg` (call on the window's own
    /// thread). Windows: child window under the HWND; Linux: GL widget under the web view (`linux/`).
    pub fn new(target: &EmbedTarget, cfg: &EngineConfig) -> Result<Self, PlayerError> {
        #[cfg(windows)]
        let surface = crate::surface::VideoSurface::create(target.hwnd())?;
        #[cfg(unix)]
        force_c_numeric_locale();

        // Create (with the engine's pre-init options), then set properties: some options return
        // PROPERTY_NOT_FOUND during pre-init.
        let mpv = Mpv::with_initializer(|init| {
            // Diagnostics: TK_MPV_LOG=<file> makes mpv write its own verbose log there.
            if let Some(path) = std::env::var_os("TK_MPV_LOG").filter(|p| !p.is_empty()) {
                init.set_option("log-file", path.to_string_lossy().as_ref())?;
            }
            crate::enhanced::pre_init(&init, &cfg.mode)
        })
        .map_err(mpv_err)?;
        #[cfg(windows)]
        mpv.set_property("wid", surface.wid()).map_err(mpv_err)?;
        for (key, value) in crate::enhanced::base_options(&cfg.mode) {
            // Best effort: an mpv build that refuses one cosmetic option must not kill the player.
            if let Err(e) = mpv.set_property(key, value) {
                tracing::warn!(key, error = %e, "mpv refused an option");
            }
        }
        let _ = mpv.set_property("network-timeout", crate::types::NETWORK_TIMEOUT_SECS);
        let _ = mpv.set_property("demuxer-max-bytes", 150_i64 * 1024 * 1024);

        let ev = EventContext::new(mpv.ctx);
        ev.disable_deprecated_events().ok();
        ev.observe_property("time-pos", Format::Double, 0).map_err(mpv_err)?;
        ev.observe_property("duration", Format::Double, 1).map_err(mpv_err)?;
        ev.observe_property("pause", Format::Flag, 2).map_err(mpv_err)?;
        ev.observe_property("vo-configured", Format::Flag, 3).map_err(mpv_err)?;
        ev.observe_property("paused-for-cache", Format::Flag, 4).map_err(mpv_err)?;
        crate::enhanced::post_init(&mpv, &cfg.mode)?;
        #[cfg(target_os = "linux")]
        let gl = crate::linux::GlSurface::attach(&mpv, target)?;
        #[cfg(not(any(windows, target_os = "linux")))]
        let _ = target;

        let mpv = Arc::new(mpv);
        // The render context must be freed before the core: the surface keeps a reference until it is gone.
        #[cfg(target_os = "linux")]
        gl.keep_core(mpv.clone());
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
            #[cfg(target_os = "linux")]
            gl,
            mpv,
            #[cfg(windows)]
            surface,
            tx,
            preset: Mutex::new(ShaderPreset::Off),
            shaders_dir: Mutex::new(None),
            pending_subs,
            presets_apply: crate::enhanced_conf::shader_presets_apply(&cfg.mode),
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
        // Torrent streams (loopback HTTP) may wait for pieces longer than a remote server would answer.
        if let Err(e) = self.mpv.set_property("network-timeout", src.network_timeout_secs()) {
            tracing::warn!(error = %e, "mpv refused network-timeout");
        }
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
        if !self.presets_apply {
            // Enhanced engine: the build switches shaders itself; ours would stack on top of its profiles.
            return Ok(());
        }
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
        #[cfg(target_os = "linux")]
        {
            self.gl.place(rect, dpr)
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            let _ = (rect, dpr);
            false
        }
    }

    /// Whether the picture is drawn inside the app window. False when mpv had to open its own window (Linux
    /// without a usable GL surface, e.g. WebKitGTK's DMA-BUF renderer disabled).
    pub fn is_embedded(&self) -> bool {
        #[cfg(windows)]
        {
            true
        }
        #[cfg(target_os = "linux")]
        {
            self.gl.is_active()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            false
        }
    }

    /// Relay a pointer position to mpv (Enhanced + uosc: the page captures the pointer over the hole).
    /// `x` / `y` are CSS px relative to the hole's top-left corner, `dpr` = devicePixelRatio.
    pub fn forward_mouse(&self, x: f64, y: f64, dpr: f64) -> Result<(), PlayerError> {
        let s = if dpr.is_finite() && dpr > 0.0 { dpr } else { 1.0 };
        let (px, py) = ((x * s).round().max(0.0) as i64, (y * s).round().max(0.0) as i64);
        self.mpv.command("mouse", &[&px.to_string(), &py.to_string()]).map_err(mpv_err)
    }

    /// Enhanced + uosc: show uosc's timeline and controls for a moment. uosc reveals them only when the pointer
    /// nears the bottom edge; this makes any pointer movement over the video bring them up, like our own bar.
    pub fn flash_uosc_controls(&self) -> Result<(), PlayerError> {
        self.mpv.command("script-message-to", &["uosc", "flash-elements", "timeline,controls"]).map_err(mpv_err)
    }

    /// Relay a key or mouse button by its mpv name (`SPACE`, `LEFT`, `f`, `MBTN_LEFT`, `WHEEL_UP` ...).
    pub fn forward_key(&self, name: &str, action: KeyAction) -> Result<(), PlayerError> {
        let cmd = match action {
            KeyAction::Down => "keydown",
            KeyAction::Up => "keyup",
            KeyAction::Press => "keypress",
        };
        self.mpv.command(cmd, &[&mpv_quote(name)]).map_err(mpv_err)
    }

    pub fn events(&self) -> broadcast::Receiver<PlayerEvent> {
        self.tx.subscribe()
    }

    // --- wave A (docs/redesign/15-parity-wave-a-contract.md section 3): thin mpv property / command wrappers so
    // workers d (subtitles) and e (flow) never edit this file. Each one maps 1:1 to an mpv option. ---

    /// Select a subtitle track by mpv id (`sid`); `None` turns subtitles off.
    pub fn set_sub_track(&self, id: Option<i64>) -> Result<(), PlayerError> {
        match id {
            Some(id) => self.mpv.set_property("sid", id),
            None => self.mpv.set_property("sid", "no"),
        }
        .map_err(mpv_err)
    }

    /// Secondary subtitle (`secondary-sid`, drawn at the top); `None` turns it off.
    pub fn set_secondary_sub_track(&self, id: Option<i64>) -> Result<(), PlayerError> {
        match id {
            Some(id) => self.mpv.set_property("secondary-sid", id),
            None => self.mpv.set_property("secondary-sid", "no"),
        }
        .map_err(mpv_err)
    }

    /// Select an audio track by mpv id (`aid`); `None` mutes the audio track selection.
    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), PlayerError> {
        match id {
            Some(id) => self.mpv.set_property("aid", id),
            None => self.mpv.set_property("aid", "no"),
        }
        .map_err(mpv_err)
    }

    /// Playback speed (`speed`), clamped to 0.25..=4.0.
    pub fn set_speed(&self, speed: f64) -> Result<(), PlayerError> {
        let v = if speed.is_finite() { speed.clamp(0.25, 4.0) } else { 1.0 };
        self.mpv.set_property("speed", v).map_err(mpv_err)
    }

    /// Motion interpolation ("Smoothing"): `interpolation` + `video-sync=display-resample` (what it needs to work).
    pub fn set_interpolation(&self, on: bool) -> Result<(), PlayerError> {
        self.mpv.set_property("video-sync", if on { "display-resample" } else { "audio" }).map_err(mpv_err)?;
        self.mpv.set_property("interpolation", on).map_err(mpv_err)
    }

    /// Repeat the current file (`loop-file`).
    pub fn set_loop_file(&self, on: bool) -> Result<(), PlayerError> {
        self.mpv.set_property("loop-file", if on { "inf" } else { "no" }).map_err(mpv_err)
    }

    /// Subtitle delay in seconds (`sub-delay`; positive = subtitles later).
    pub fn set_sub_delay(&self, secs: f64) -> Result<(), PlayerError> {
        let v = if secs.is_finite() { secs.clamp(-60.0, 60.0) } else { 0.0 };
        self.mpv.set_property("sub-delay", v).map_err(mpv_err)
    }

    /// Apply a subtitle style (`sub-*` options from `crate::subs::sub_options`). Best effort per option: an mpv
    /// build that refuses one option logs it and keeps the rest.
    pub fn set_sub_style(&self, style: &crate::subs::SubStyle) -> Result<(), PlayerError> {
        for (key, value) in crate::subs::sub_options(style) {
            if let Err(e) = self.mpv.set_property(key, value.as_str()) {
                tracing::warn!(key, error = %e, "mpv refused a subtitle option");
            }
        }
        Ok(())
    }

    /// Load an external subtitle file or URL (`sub-add`); `select` makes it the active track at once.
    pub fn add_subtitle(&self, sub: &SubtitleSource, select: bool) -> Result<(), PlayerError> {
        let title = if sub.label.is_empty() { sub.lang.clone() } else { sub.label.clone() };
        let flag = if select { "select" } else { "auto" };
        self.mpv.command("sub-add", &[&mpv_quote(&sub.url), flag, &mpv_quote(&title), &mpv_quote(&sub.lang)]).map_err(mpv_err)
    }

    /// Save the current video frame (without the UI, with subtitles when `with_subs`) to `path` (PNG by extension).
    pub fn screenshot_to_file(&self, path: &std::path::Path, with_subs: bool) -> Result<(), PlayerError> {
        let p = path.to_string_lossy();
        self.mpv.command("screenshot-to-file", &[&mpv_quote(&p), if with_subs { "subtitles" } else { "video" }]).map_err(mpv_err)
    }

    /// Loudness normalisation ("stable volume"): `af=lavfi=[dynaudnorm]` on, empty filter chain off.
    pub fn set_stable_volume(&self, on: bool) -> Result<(), PlayerError> {
        self.mpv.set_property("af", if on { "lavfi=[dynaudnorm=f=150:g=15]" } else { "" }).map_err(mpv_err)
    }

    /// Escape hatch for an option this API does not wrap yet (string form, like `mpv --name=value`).
    pub fn set_option_str(&self, name: &str, value: &str) -> Result<(), PlayerError> {
        self.mpv.set_property(name, value).map_err(mpv_err)
    }

    /// Escape hatch for a raw mpv command. Arguments are passed as given (quote with `crate::headers::mpv_quote`
    /// when they may contain spaces).
    pub fn command_raw(&self, name: &str, args: &[&str]) -> Result<(), PlayerError> {
        self.mpv.command(name, args).map_err(mpv_err)
    }

    /// Read a numeric property (`sub-delay`, `speed`, `volume` ...); `None` when mpv has no value.
    pub fn property_f64(&self, name: &str) -> Option<f64> {
        self.mpv.get_property::<f64>(name).ok()
    }

    /// Read a string property (`sid`, `aid`, `media-title` ...); `None` when mpv has no value.
    pub fn property_str(&self, name: &str) -> Option<String> {
        self.mpv.get_property::<String>(name).ok()
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
