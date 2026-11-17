//! 0.4.0: the source's subtitles over Link (`GET /v1/sessions/{id}` `subtitles` and
//! `GET /v1/sessions/{id}/subtitles/{n}.ass|.vtt`), end to end over mTLS with the fake engine, plus one run of the
//! real FFmpeg / ffprobe when they are installed.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use tk_link::SessionTuning;
use tk_link::media::engine::Engine;

/// A ToonsHub-like release: two ASS tracks (English default, Latin American Spanish), forced SRT signs, a PGS track.
const TRACKS: &str = r#"{"streams":[
    {"codec_name":"ass","disposition":{"default":1,"forced":0},"tags":{"language":"eng","title":"English"}},
    {"codec_name":"ass","disposition":{"default":0,"forced":0},"tags":{"language":"spa","title":"Spanish(Latin America)"}},
    {"codec_name":"subrip","disposition":{"default":0,"forced":1},"tags":{"language":"eng","title":"Signs"}},
    {"codec_name":"hdmv_pgs_subtitle","tags":{"language":"por"}}
]}"#;

async fn json(r: reqwest::Response) -> Value {
    r.json().await.unwrap()
}

async fn wait_until<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    for _ in 0..200 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn upstream() -> u16 {
    let body = media_playlist(3);
    stub(move |_| text(200, &body)).await.0
}

/// A new session of a new source.
async fn session(h: &Harness, p: &Paired) -> String {
    session_of(h, p, upstream().await).await
}

async fn session_of(h: &Harness, p: &Paired, port: u16) -> String {
    let r = p
        .client
        .post(h.url("/v1/sessions"))
        .json(&json!({ "source": { "kind": "url", "url": format!("http://127.0.0.1:{port}/list.m3u8"), "container": "hls" } }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    json(r).await["session_id"].as_str().unwrap().to_owned()
}

async fn renew(h: &Harness, p: &Paired, sid: &str) -> String {
    let r = p
        .client
        .post(h.url("/v1/sessions"))
        .json(&json!({ "source": { "kind": "session", "session_id": sid }, "start_secs": 2.0 }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 201);
    json(r).await["session_id"].as_str().unwrap().to_owned()
}

async fn status(h: &Harness, p: &Paired, sid: &str) -> Value {
    json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await
}

async fn file(h: &Harness, p: &Paired, sid: &str, name: &str) -> reqwest::Response {
    p.client.get(h.url(&format!("/v1/sessions/{sid}/subtitles/{name}"))).send().await.unwrap()
}

async fn listed(h: &Harness, p: &Paired, sid: &str) -> Value {
    wait_until("the subtitle listing", || async { status(h, p, sid).await.get("subtitles").is_some() }).await;
    status(h, p, sid).await
}

fn engine_with_tracks() -> std::sync::Arc<FakeEngine> {
    let engine = FakeEngine::new(3, 100, Duration::from_millis(1));
    *engine.subtitle_probe.lock().unwrap() = Some(TRACKS.to_owned());
    engine
}

#[tokio::test]
async fn a_session_lists_the_source_subtitles_and_serves_each_text_track_from_one_pass() {
    let engine = engine_with_tracks();
    let mut h = harness(engine.clone(), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let other = h.pair("Tablet").await;
    let sid = session(&h, &p).await;

    let st = listed(&h, &p, &sid).await;
    assert_eq!(st["subtitles_state"], "idle", "listed; nothing read until the phone asks");
    assert_eq!(
        st["subtitles"],
        json!([
            { "index": 0, "language": "eng", "title": "English", "codec": "ass", "default": true, "forced": false, "text": true, "formats": ["ass", "vtt"] },
            { "index": 1, "language": "spa", "title": "Spanish(Latin America)", "codec": "ass", "default": false, "forced": false, "text": true, "formats": ["ass", "vtt"] },
            { "index": 2, "language": "eng", "title": "Signs", "codec": "subrip", "default": false, "forced": true, "text": true, "formats": ["vtt"] },
            { "index": 3, "language": "por", "codec": "hdmv_pgs_subtitle", "default": false, "forced": false, "text": false, "formats": [] },
        ])
    );
    assert!(engine.extractions.lock().unwrap().is_empty());

    let r = file(&h, &p, &sid, "1.ass").await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "text/x-ssa; charset=utf-8");
    assert_eq!(r.headers()["cache-control"], "no-store");
    assert!(r.text().await.unwrap().contains("track 1"));
    let r = file(&h, &p, &sid, "2.vtt").await;
    assert_eq!(r.headers()["content-type"], "text/vtt; charset=utf-8");
    assert!(r.text().await.unwrap().starts_with("WEBVTT"));
    assert_eq!(file(&h, &p, &sid, "0.vtt").await.status(), 200);
    assert_eq!(status(&h, &p, &sid).await["subtitles_state"], "ready");
    // everything came from a single pass over the session's loopback input
    let passes = engine.extractions.lock().unwrap().clone();
    assert_eq!(passes.len(), 1);
    assert!(passes[0].starts_with("http://127.0.0.1:"), "{}", passes[0]);
    // not served: SRT as .ass, the image track, a missing track, other names
    for name in ["2.ass", "3.vtt", "9.vtt", "1.srt", "x.vtt", "1.vtt.gz"] {
        assert_eq!(file(&h, &p, &sid, name).await.status(), 404, "{name}");
    }
    // private to the device, gone with the session
    assert_eq!(file(&h, &other, &sid, "1.ass").await.status(), 404);
    assert_eq!(p.client.delete(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap().status(), 204);
    assert_eq!(file(&h, &p, &sid, "1.ass").await.status(), 410);
}

#[tokio::test]
async fn a_long_extraction_answers_retry_after_until_the_files_are_ready() {
    let engine = engine_with_tracks();
    *engine.extract_delay.lock().unwrap() = Duration::from_millis(1500);
    let tuning = SessionTuning { subtitle_wait: Duration::from_millis(300), ..quick_tuning() };
    let mut h = harness(engine.clone(), tuning, lenient_default()).await;
    let p = h.pair("Pixel").await;
    let sid = session(&h, &p).await;
    listed(&h, &p, &sid).await;

    let r = file(&h, &p, &sid, "0.ass").await;
    assert_eq!(r.status(), 503);
    assert_eq!(r.headers()["retry-after"], "1");
    assert_eq!(json(r).await["error"]["code"], "segment_wait_timeout");
    assert_eq!(status(&h, &p, &sid).await["subtitles_state"], "extracting");
    // asking again while it runs does not start a second pass
    assert_eq!(file(&h, &p, &sid, "1.vtt").await.status(), 503);
    wait_until("the extraction", || async { file(&h, &p, &sid, "0.ass").await.status() == 200 }).await;
    assert_eq!(engine.extractions.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn a_profile_change_keeps_the_extracted_subtitles_and_restarts_an_unfinished_pass() {
    let engine = engine_with_tracks();
    let tuning = SessionTuning { subtitle_wait: Duration::from_millis(500), ..quick_tuning() };
    let mut h = harness(engine.clone(), tuning, lenient_default()).await;
    let p = h.pair("Pixel").await;
    let port = upstream().await;
    let sid = session_of(&h, &p, port).await;
    listed(&h, &p, &sid).await;
    assert_eq!(file(&h, &p, &sid, "1.ass").await.status(), 200);

    // the renewed session (another profile, same source) lists and serves them at once, without reading the source again
    let renewed = renew(&h, &p, &sid).await;
    let st = status(&h, &p, &renewed).await;
    assert_eq!((st["subtitles"].as_array().map(Vec::len), st["subtitles_state"].as_str()), (Some(4), Some("ready")));
    assert!(file(&h, &p, &renewed, "1.ass").await.text().await.unwrap().contains("track 1"));
    assert_eq!(engine.extractions.lock().unwrap().len(), 1);

    // so does a session of the same source by the source itself (what a phone's profile change sends), even after the
    // previous one closed
    assert_eq!(p.client.delete(h.url(&format!("/v1/sessions/{renewed}"))).send().await.unwrap().status(), 204);
    let same = session_of(&h, &p, port).await;
    assert_eq!(status(&h, &p, &same).await["subtitles_state"], "ready");
    assert_eq!(file(&h, &p, &same, "0.vtt").await.status(), 200);
    assert_eq!(engine.extractions.lock().unwrap().len(), 1);

    // a pass the replaced session was running stops with it (its input is gone) and the new session starts it again
    *engine.extract_delay.lock().unwrap() = Duration::from_secs(30);
    let fresh = session(&h, &p).await;
    listed(&h, &p, &fresh).await;
    assert_eq!(file(&h, &p, &fresh, "0.vtt").await.status(), 503);
    *engine.extract_delay.lock().unwrap() = Duration::ZERO;
    let next = renew(&h, &p, &fresh).await;
    assert_eq!(status(&h, &p, &next).await["subtitles_state"], "idle");
    assert_eq!(file(&h, &p, &next, "0.vtt").await.status(), 200);
    let passes = engine.extractions.lock().unwrap().clone();
    assert_eq!(passes.len(), 3);
    assert_ne!(passes[1], passes[2], "the restarted pass reads the new session's input");
}

#[tokio::test]
async fn without_a_listing_there_are_no_subtitles_and_a_failed_pass_is_unavailable() {
    // an engine that cannot list (no ffprobe): no `subtitles` in the status, nothing served
    let engine = FakeEngine::new(3, 100, Duration::from_millis(1));
    let mut h = harness(engine.clone(), quick_tuning(), lenient_default()).await;
    let p = h.pair("Pixel").await;
    let sid = session(&h, &p).await;
    wait_until("the failed listing", || async { status(&h, &p, &sid).await["subtitles_state"] == "unavailable" }).await;
    assert!(status(&h, &p, &sid).await.get("subtitles").is_none());
    assert_eq!(file(&h, &p, &sid, "0.vtt").await.status(), 404);

    *engine.subtitle_probe.lock().unwrap() = Some(TRACKS.to_owned());
    *engine.extract_fails.lock().unwrap() = true;
    let sid = session(&h, &p).await;
    listed(&h, &p, &sid).await;
    let r = file(&h, &p, &sid, "0.ass").await;
    assert_eq!(r.status(), 503);
    assert!(r.headers().get("retry-after").is_none(), "final: the phone stops asking");
    assert_eq!(json(r).await["error"]["code"], "source_unavailable");
    assert_eq!(status(&h, &p, &sid).await["subtitles_state"], "failed");
}

/// The real binaries: an MKV with an ASS track (styled, starting at 1.5 s) and an SRT one is listed and extracted in one
/// pass through a loopback HTTP input, with the source's own timestamps. Skipped where FFmpeg is not installed.
#[tokio::test]
async fn real_ffmpeg_lists_and_extracts_ass_and_webvtt_with_the_source_timeline() {
    let ffmpeg = std::path::PathBuf::from(if cfg!(windows) { "C:/ffmpeg/bin/ffmpeg.exe" } else { "/usr/bin/ffmpeg" });
    if !ffmpeg.is_file() || !ffmpeg.with_file_name(if cfg!(windows) { "ffprobe.exe" } else { "ffprobe" }).is_file() {
        eprintln!("ffmpeg / ffprobe not installed: skipped");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let ass = dir.path().join("in.ass");
    std::fs::write(
        &ass,
        "[Script Info]\nScriptType: v4.00+\nPlayResX: 640\nPlayResY: 360\n\n[V4+ Styles]\nFormat: Name, Fontname, Fontsize, PrimaryColour, SecondaryColour, OutlineColour, BackColour, Bold, Italic, Underline, StrikeOut, ScaleX, ScaleY, Spacing, Angle, BorderStyle, Outline, Shadow, Alignment, MarginL, MarginR, MarginV, Encoding\nStyle: Sign,Arial,40,&H0000FFFF,&H000000FF,&H00000000,&H00000000,0,0,0,0,100,100,0,0,1,2,0,8,10,10,10,1\n\n[Events]\nFormat: Layer, Start, End, Style, Name, MarginL, MarginR, MarginV, Effect, Text\nDialogue: 0,0:00:01.50,0:00:03.00,Sign,,0,0,0,,{\\pos(320,40)}Hola PC\n",
    )
    .unwrap();
    let srt = dir.path().join("in.srt");
    std::fs::write(&srt, "1\n00:00:02,000 --> 00:00:03,500\nSigns line\n").unwrap();
    let mkv = dir.path().join("ep.mkv");
    let made = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi", "-i", "color=c=black:s=64x64:r=24:d=4"])
        .arg("-i")
        .arg(&ass)
        .arg("-i")
        .arg(&srt)
        .args(["-map", "0:v", "-map", "1:s", "-map", "2:s", "-c:v", "ffv1", "-c:s:0", "copy", "-c:s:1", "srt"])
        .args([
            "-metadata:s:s:0",
            "language=spa",
            "-metadata:s:s:0",
            "title=Español (Latinoamérica)",
            "-disposition:s:0",
            "default",
        ])
        .args(["-metadata:s:s:1", "language=eng", "-metadata:s:s:1", "title=Signs"])
        .arg(&mkv)
        .status()
        .unwrap();
    assert!(made.success(), "could not build the test MKV");
    let bytes = std::fs::read(&mkv).unwrap();
    let (port, _) = stub(move |_| (200, vec![("content-type".into(), "video/x-matroska".into())], bytes.clone())).await;
    let input = format!("http://127.0.0.1:{port}/ep.mkv");

    let engine = tk_link::media::engine::FfmpegEngine::new(ffmpeg).unwrap();
    let tracks = engine.subtitle_tracks(&input).await.expect("listing");
    assert_eq!(tracks.len(), 2);
    assert_eq!(
        (tracks[0].language.as_deref(), tracks[0].title.as_deref(), tracks[0].default),
        (Some("spa"), Some("Español (Latinoamérica)"), true)
    );
    assert_eq!((tracks[0].codec.as_str(), tracks[0].formats.clone()), ("ass", vec!["ass", "vtt"]));
    assert_eq!((tracks[1].codec.as_str(), tracks[1].formats.clone()), ("subrip", vec!["vtt"]));

    let out = dir.path().join("out");
    std::fs::create_dir_all(&out).unwrap();
    let outputs = tk_link::subtitles::plan_outputs(&tracks, &out);
    engine.extract_subtitles(&input, &outputs).await.expect("extraction");
    let ass_out = std::fs::read_to_string(out.join("0.ass")).unwrap();
    assert!(ass_out.contains("Style: Sign,Arial,40"), "styles kept: {ass_out}");
    assert!(ass_out.contains("0:00:01.50,0:00:03.00,Sign") && ass_out.contains("\\pos(320,40)"), "{ass_out}");
    let vtt = std::fs::read_to_string(out.join("0.vtt")).unwrap();
    assert!(vtt.starts_with("WEBVTT") && vtt.contains("00:01.500 --> 00:03.000") && vtt.contains("Hola PC"), "{vtt}");
    let signs = std::fs::read_to_string(out.join("1.vtt")).unwrap();
    assert!(signs.contains("00:02.000 --> 00:03.500") && signs.contains("Signs line"), "{signs}");
}
