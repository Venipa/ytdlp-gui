//! `engine::suitable` against the real bundled yt-dlp.
use engine::{UrlKind, runtime, suitable};

fn init() {
    runtime::init(runtime::InitOptions {
        home: None,
        site_updates: None,
        sink: runtime::log_sink,
    })
    .expect("init");
}

#[test]
fn matches_without_extracting() {
    init();
    let key = |u: &str| suitable(u).unwrap().map(|s| s.key);
    assert_eq!(
        key("https://www.youtube.com/watch?v=jNQXAC9IVRw").as_deref(),
        Some("Youtube")
    );
    assert_eq!(
        key("https://www.youtube.com/embed/jNQXAC9IVRw").as_deref(),
        Some("Youtube")
    );
    assert_eq!(key("https://vimeo.com/76979871").as_deref(), Some("Vimeo"));
    // Generic would match these; Known* would match the DRM site
    assert_eq!(key("https://example.com/"), None);
    assert_eq!(key("https://example.com/video.mp4"), None);
    assert_eq!(key("https://www.disneyplus.com/video/abc"), None);
    let p = suitable("https://www.youtube.com/playlist?list=PLabc")
        .unwrap()
        .unwrap();
    assert_eq!(p.kind, UrlKind::Playlist);
    let t = std::time::Instant::now();
    suitable("https://vimeo.com/1").unwrap();
    eprintln!("steady suitable: {:?}", t.elapsed());
}
