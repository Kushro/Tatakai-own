//! Linux embedded-video probe: a bare GTK window laid out like the app's (a `gtk::Box` holding a WebKitGTK web
//! view) that plays a file through the same GL surface as the app, with a page that leaves a hole and moves a
//! card over it. Shows, without the app's UI and sources, whether mpv's picture reaches the screen under the web
//! view and whether the page's transparent pixels repaint cleanly.
//!
//! `cargo run -p tk-player --example linux_embed_probe -- <file-or-url> [enhanced-config-dir]`
//! (`TK_MPV_LOG=<file>` adds mpv's own log; `RUST_LOG=tk_player=trace` the surface's.)

#[cfg(target_os = "linux")]
fn main() {
    use std::path::PathBuf;
    use std::sync::Arc;

    use gtk::glib;
    use gtk::prelude::*;
    use tk_player::{EmbedTarget, EngineConfig, EngineMode, MediaSource, Player, PlayerEvent, Rect};
    use webkit2gtk::WebViewExt;

    const PAGE: &str = r#"<!doctype html><html><head><style>
      html, body { margin: 0; height: 100%; background: transparent; overflow: hidden; font: 14px sans-serif; }
      #bar { position: absolute; inset: 0 0 auto 0; height: 36px; background: #303030; color: #fff; padding: 8px; box-sizing: border-box; }
      #hole { position: absolute; inset: 36px 0 0 0; outline: 2px dashed #f0f; outline-offset: -2px; }
      #card { position: absolute; bottom: 60px; width: 160px; height: 90px; background: #2a6; border-radius: 8px;
              animation: slide 4s linear infinite alternate; }
      @keyframes slide { from { left: 20px; } to { left: calc(100% - 180px); } }
    </style></head><body><div id="bar">probe: mpv should show under the dashed hole</div>
    <div id="hole"></div><div id="card"></div></body></html>"#;

    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::from_default_env()).init();
    let mut args = std::env::args().skip(1);
    let url = args.next().expect("usage: linux_embed_probe <file-or-url> [enhanced-config-dir]");
    let mode = match args.next() {
        Some(dir) => EngineMode::Enhanced { config_dir: PathBuf::from(dir), uosc: false },
        None => EngineMode::Embedded,
    };

    gtk::init().expect("gtk init");
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("tk-player probe");
    window.set_default_size(960, 600);
    if std::env::var_os("PROBE_NO_DECOR").is_some() {
        window.set_decorated(false);
    }
    let vbox = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let webview = webkit2gtk::WebView::new();
    webview.load_html(PAGE, None);
    vbox.pack_start(&webview, true, true, 0);
    window.add(&vbox);
    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();
    // PROBE_DELAY_MS=<n>: let the page load and draw first, like the app (the player starts long after).
    if let Some(ms) = std::env::var("PROBE_DELAY_MS").ok().and_then(|s| s.parse::<u64>().ok()) {
        let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < end {
            gtk::main_iteration_do(false);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    let cfg = EngineConfig { mode, shaders_dir: PathBuf::from("/nonexistent") };
    let player = Arc::new(Player::new(&EmbedTarget::from_gtk_box(&vbox), &cfg).expect("player"));
    let mut rx = player.events();
    std::thread::spawn(move || {
        while let Ok(ev) = rx.blocking_recv() {
            if !matches!(ev, PlayerEvent::Position(_)) {
                eprintln!("probe event: {ev:?}");
            }
        }
    });
    player.load(&MediaSource { url, headers: Default::default(), subtitles: Vec::new() }, 0.0).expect("load");
    {
        let (player, webview) = (player.clone(), webview.clone());
        glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
            let (w, h) = (webview.allocated_width() as f64, webview.allocated_height() as f64);
            player.set_rect(Some(Rect { x: 0.0, y: 36.0, w, h: h - 36.0 }), f64::from(webview.scale_factor()));
            glib::ControlFlow::Continue
        });
    }
    // PROBE_STOP_SECS=<n>: after n seconds do what the player screen does on Back (hide, stop), timing it.
    if let Some(secs) = std::env::var("PROBE_STOP_SECS").ok().and_then(|s| s.parse::<u64>().ok()) {
        let player = player.clone();
        glib::timeout_add_local_once(std::time::Duration::from_secs(secs), move || {
            let t = std::time::Instant::now();
            player.set_rect(None, 1.0);
            player.stop();
            eprintln!("probe: hide + stop took {:?}", t.elapsed());
        });
    }
    gtk::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {}
