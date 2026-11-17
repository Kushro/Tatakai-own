//! 0.4.1 "Link stuck after a long pause" (and the end-of-episode / gaps reports): back-pressure with an encoder that
//! does not wait for the sink, stalled runs, runs that end early or at the real end of the source, and the RAM window
//! around a paused playhead. Fake engine over real TLS, like `e2e.rs`.

mod common;

use std::time::Duration;

use common::*;
use serde_json::{Value, json};
use tk_link::SessionTuning;

async fn json(r: reqwest::Response) -> Value {
    r.json().await.unwrap()
}

async fn wait_until<F: std::future::Future<Output = bool>>(what: &str, mut f: impl FnMut() -> F) {
    for _ in 0..400 {
        if f().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn session(h: &Harness, p: &Paired, segments: u32) -> String {
    let body = media_playlist(segments);
    let port = stub(move |_| text(200, &body)).await.0;
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

async fn status(h: &Harness, p: &Paired, sid: &str) -> Value {
    json(p.client.get(h.url(&format!("/v1/sessions/{sid}"))).send().await.unwrap()).await
}

async fn heartbeat(h: &Harness, p: &Paired, sid: &str, secs: f64, paused: bool) {
    let r = p.client.post(h.url(&format!("/v1/sessions/{sid}/position"))).json(&json!({ "position_secs": secs, "paused": paused })).send().await.unwrap();
    assert_eq!(r.status(), 204);
}

async fn get(h: &Harness, p: &Paired, sid: &str, file: &str) -> reqwest::Response {
    p.client.get(h.url(&format!("/v1/sessions/{sid}/{file}"))).send().await.unwrap()
}

fn tuning() -> SessionTuning {
    SessionTuning { segment_wait: Duration::from_secs(5), stall_timeout: Duration::from_millis(600), ..SessionTuning::default() }
}

/// The reported case: the phone pauses for a long time. FFmpeg n9 does not wait for the sink's answer, so holding the
/// answer never stopped it (it encoded the whole episode into held connections). Now the run is parked (killed) once
/// it is far enough ahead, nothing is made during the pause, and production resumes where it stopped when the phone
/// plays again: every segment after the paused position arrives, with its own content.
#[tokio::test]
async fn an_encoder_that_does_not_wait_is_parked_during_a_long_pause_and_resumed_on_play() {
    let engine = FakeEngine::new(300, 1000, Duration::from_millis(5));
    *engine.no_wait.lock().unwrap() = true;
    let mut h = harness(engine.clone(), tuning(), lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 300).await;
    heartbeat(&h, &p, &sid, 0.0, true).await;
    wait_until("the run to be parked", || async { status(&h, &p, &sid).await["parked"] == true }).await;
    let st = status(&h, &p, &sid).await;
    assert_eq!(st["throttled"], true, "{st}");
    // the long pause: heartbeats with paused=true, nothing more is made
    let made = engine.sent.lock().unwrap().len();
    assert!(made <= 2 * 15 + 3, "the run went on during the pause: {made} segments sent");
    for _ in 0..6 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        heartbeat(&h, &p, &sid, 0.0, true).await;
    }
    assert_eq!(engine.sent.lock().unwrap().len(), made, "a parked run makes nothing");
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0]);
    // play: every segment after the paused position, in order, each in time and with its own content
    for k in 0..80u32 {
        heartbeat(&h, &p, &sid, f64::from(k) * 2.0, false).await;
        let started = std::time::Instant::now();
        let r = get(&h, &p, &sid, &format!("seg_{k}.m4s")).await;
        assert_eq!(r.status(), 200, "seg_{k}");
        assert_eq!(r.bytes().await.unwrap()[0], (k % 251) as u8, "seg_{k}");
        assert!(started.elapsed() < Duration::from_secs(3), "seg_{k} took {:?}", started.elapsed());
    }
    let starts = engine.starts.lock().unwrap().clone();
    assert!(starts.len() >= 2 && starts[1] > 0, "production resumed where it stopped: {starts:?}");
}

/// The run stops producing without exiting (what the reported session's pipeline did): before, every request for the
/// next segment waited 15 s and got `503 segment_wait_timeout` forever. The supervisor restarts it where it stopped.
#[tokio::test]
async fn a_run_that_stops_producing_without_exiting_is_restarted_where_it_stopped() {
    let engine = FakeEngine::new(100, 100, Duration::ZERO);
    engine.scripts.lock().unwrap().push_back(RunScript::HangAfter(5));
    let mut h = harness(engine.clone(), tuning(), lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 100).await;
    for k in 0..5 {
        assert_eq!(get(&h, &p, &sid, &format!("seg_{k}.m4s")).await.status(), 200);
    }
    heartbeat(&h, &p, &sid, 8.0, false).await;
    let r = get(&h, &p, &sid, "seg_5.m4s").await;
    assert_eq!(r.status(), 200, "the stalled run was restarted");
    assert_eq!(r.bytes().await.unwrap()[0], 5);
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0, 5]);
}

/// A run that keeps stalling: after the automatic restarts the session fails (`engine_stalled`) and its media answer
/// a final `410 segment_unavailable` (the phone renews the session) instead of `503` forever.
#[tokio::test]
async fn a_session_whose_runs_keep_stalling_fails_with_segment_unavailable() {
    let engine = FakeEngine::new(100, 100, Duration::ZERO);
    {
        let mut s = engine.scripts.lock().unwrap();
        s.push_back(RunScript::HangAfter(2));
        for _ in 0..6 {
            s.push_back(RunScript::HangAfter(0));
        }
    }
    let tuning = SessionTuning { stall_timeout: Duration::from_millis(300), ..tuning() };
    let mut h = harness(engine.clone(), tuning, lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 100).await;
    assert_eq!(get(&h, &p, &sid, "seg_1.m4s").await.status(), 200);
    wait_until("the session to fail", || async { status(&h, &p, &sid).await["state"] == "failed" }).await;
    let st = status(&h, &p, &sid).await;
    assert_eq!(st["error"], "engine_stalled", "{st}");
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0, 2, 2, 2], "three restarts where it stopped, then it gave up");
    for file in ["seg_2.m4s", "init.mp4"] {
        let r = get(&h, &p, &sid, file).await;
        assert_eq!(r.status(), 410, "{file}");
        assert_eq!(json(r).await["error"]["code"], "segment_unavailable", "{file}");
    }
}

/// End of the episode: the pipeline makes fewer segments than the container duration announces. That is the end of
/// the source (`ended`, `end_secs`), the missing tail answers 404 like past the playlist, and nothing restarts there
/// (before: a restart per request that made nothing, "preparing" and finally "your PC could not go on").
#[tokio::test]
async fn the_real_end_of_the_source_is_reported_and_never_restarted() {
    let engine = FakeEngine::new(10, 100, Duration::ZERO);
    let mut h = harness(engine.clone(), tuning(), lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 12).await;
    wait_until("the end of the source", || async { status(&h, &p, &sid).await["ended"] == true }).await;
    let st = status(&h, &p, &sid).await;
    assert_eq!(st["end_secs"], 20.0, "{st}");
    assert_eq!(st["state"], "ready", "{st}");
    assert_eq!(get(&h, &p, &sid, "seg_9.m4s").await.status(), 200);
    for k in [10, 11] {
        assert_eq!(get(&h, &p, &sid, &format!("seg_{k}.m4s")).await.status(), 404, "seg_{k}");
    }
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0], "nothing restarts at the end");
}

/// Gaps: a run whose input broke ends with a cut-short last segment (the playlist says 2 s). It is dropped and the next
/// run makes it again, so the video does not jump; `produced` tells the phone what the PC holds.
#[tokio::test]
async fn an_input_that_breaks_early_drops_the_cut_segment_and_goes_on_from_there() {
    let engine = FakeEngine::new(100, 100, Duration::ZERO);
    engine.scripts.lock().unwrap().push_back(RunScript::StopAfter(6));
    let mut h = harness(engine.clone(), tuning(), lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 100).await;
    // segments 0..=5 made, 5 dropped when the run ends early: the PC holds 0 s .. 10 s
    wait_until("the early end", || async { status(&h, &p, &sid).await["produced"] == json!({ "from_secs": 0.0, "to_secs": 10.0 }) }).await;
    assert!(status(&h, &p, &sid).await.get("ended").is_none());
    let r = get(&h, &p, &sid, "seg_5.m4s").await;
    assert_eq!(r.status(), 200);
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0, 5], "segment 5 was made again by a new run");
}

/// A paused playhead with a small RAM budget: the segments right after it are what the phone plays next and are never
/// evicted (before, the budget dropped the oldest, i.e. those, and resuming needed a restart).
#[tokio::test]
async fn segments_after_a_paused_playhead_are_never_evicted_over_budget() {
    let engine = FakeEngine::new(200, 1000, Duration::ZERO);
    let tuning = SessionTuning { max_ram_bytes: 5_000, ..tuning() };
    let mut h = harness(engine.clone(), tuning, lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 200).await;
    heartbeat(&h, &p, &sid, 0.0, true).await;
    wait_until("back-pressure", || async { status(&h, &p, &sid).await["throttled"] == true }).await;
    for k in [0, 1, 10, 15] {
        let r = get(&h, &p, &sid, &format!("seg_{k}.m4s")).await;
        assert_eq!(r.status(), 200);
    }
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0], "served from RAM, no restart");
    let st = status(&h, &p, &sid).await;
    assert_eq!(st["produced"]["from_secs"], 0.0, "{st}");
}

/// Short seeks inside what the PC holds (60 s behind, what it made ahead) never restart the engine.
#[tokio::test]
async fn short_seeks_inside_the_window_do_not_restart() {
    let engine = FakeEngine::new(200, 100, Duration::ZERO);
    let mut h = harness(engine.clone(), tuning(), lenient_default()).await;
    let p = h.pair("Tab").await;
    let sid = session(&h, &p, 200).await;
    for k in 0..30u32 {
        heartbeat(&h, &p, &sid, f64::from(k) * 2.0, false).await;
        assert_eq!(get(&h, &p, &sid, &format!("seg_{k}.m4s")).await.status(), 200);
    }
    // -10 s, -50 s and +10 s from 58 s
    for k in [24u32, 4, 34] {
        heartbeat(&h, &p, &sid, f64::from(k) * 2.0, false).await;
        let started = std::time::Instant::now();
        let r = get(&h, &p, &sid, &format!("seg_{k}.m4s")).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.bytes().await.unwrap()[0], (k % 251) as u8);
        assert!(started.elapsed() < Duration::from_secs(1), "seg_{k}");
    }
    assert_eq!(engine.starts.lock().unwrap().as_slice(), [0]);
}
