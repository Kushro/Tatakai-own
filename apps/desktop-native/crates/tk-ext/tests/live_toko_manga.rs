//! Live diagnostic (network, a local toko.kai): how long `manga/chapters` takes for several titles asked at once,
//! the way "New chapters from your manga" asks them (BUG-121: each call has a 25 s budget from submission, and
//! the extension runs them one after the other on its single JS thread). Ignored by default; run with
//! `TOKO_KAI=/path/to/toko.kai cargo test -p tk-ext --test live_toko_manga -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use serde_json::json;
use tk_ext::ExtensionHost;

#[tokio::test(flavor = "multi_thread")]
#[ignore = "network + a local toko.kai"]
async fn followed_titles_chapter_lists_in_the_native_runtime() {
    let kai = std::env::var("TOKO_KAI").expect("TOKO_KAI");
    let dir = tempfile::tempdir().unwrap();
    let host = ExtensionHost::open(dir.path().to_path_buf()).unwrap();
    let ext = host.install_kai(std::path::Path::new(&kai)).unwrap();
    let titles = [
        (105778, "Chainsaw Man"),
        (30013, "One Piece"),
        (101517, "Jujutsu Kaisen"),
        (87216, "Martial God Asura"),
        (30002, "Berserk"),
        (86635, "Kimetsu no Yaiba"),
        (100994, "Kaguya-sama wa Kokurasetai"),
        (30642, "Vagabond"),
    ];
    let t0 = Instant::now();
    let calls = titles.iter().map(|(al, title)| {
        let (host, id) = (&host, ext.id.clone());
        async move {
            let params = json!({ "anilistId": al, "title": title, "titles": [title] });
            let r = tokio::time::timeout(Duration::from_secs(25), host.call(&id, "manga/chapters", params)).await;
            let ms = t0.elapsed().as_millis();
            match r {
                Ok(Ok(v)) => {
                    let n: usize = tk_core::manga::parse_ext_chapters(&id, &v).iter().map(|l| l.chapters.len()).sum();
                    println!("{title:>28}: answered at {ms:>6} ms ({n} chapters)");
                }
                Ok(Err(e)) => println!("{title:>28}: error at {ms:>6} ms: {e}"),
                Err(_) => println!("{title:>28}: gave up at {ms:>6} ms"),
            }
        }
    });
    futures_util::future::join_all(calls).await;
    println!("all eight settled at {} ms", t0.elapsed().as_millis());
    // The next call after the abandoned ones: before BUG-121 it waited for every queued call to run anyway.
    let t1 = Instant::now();
    let r = host.call(&ext.id, "manga/chapters", json!({ "anilistId": 105778, "title": "Chainsaw Man", "titles": ["Chainsaw Man"] })).await;
    println!("next call: {} ms ({})", t1.elapsed().as_millis(), if r.is_ok() { "ok" } else { "error" });
}
