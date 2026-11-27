//! `AppState::add_direct` and `default_preset`: busy until loaded, rule preset, duplicates, retry of failed rows.
use std::sync::Arc;
use std::time::Duration;

use domain::{AudioFormat, DownloadStatus, MatchKind, Preset, SiteRule, VideoQuality};
use gpui::TestAppContext;
use state::{AddDirectError, AppState, FakeTools, InitArgs};

fn settle(cx: &mut TestAppContext, what: &str, mut cond: impl FnMut(&mut TestAppContext) -> bool) {
    for _ in 0..600 {
        cx.run_until_parked();
        if cond(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for: {what}");
}

fn app(cx: &mut TestAppContext, dir: &std::path::Path) -> AppState {
    cx.executor().allow_parking();
    let args = InitArgs {
        data_dir: Some(dir.to_path_buf()),
        tools: Some(Arc::new(FakeTools::new(None, false))),
        ..Default::default()
    };
    cx.update(|cx| state::init(args, cx).unwrap())
}

#[gpui::test]
fn add_direct_rules(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let app = app(cx, dir.path());
    let url = "https://a.example/watch?v=1".to_string();

    // the list has not loaded yet
    let early = cx.update(|cx| app.add_direct(url.clone(), None, cx));
    if !cx.update(|cx| app.downloads.read(cx).loaded()) {
        cx.run_until_parked();
        assert!(matches!(
            cx.foreground_executor().block_test(early),
            Err(AddDirectError::Busy)
        ));
    }
    settle(cx, "load", |cx| {
        cx.update(|cx| app.downloads.read(cx).loaded())
    });

    // default: a site rule's preset beats last_preset
    app.settings.update(cx, |s, cx| {
        s.update(
            |s| {
                s.last_preset = "audio:mp3".into();
                s.site_rules = vec![SiteRule {
                    id: "r".into(),
                    on: true,
                    kind: MatchKind::Domain,
                    pattern: "a.example".into(),
                    clipboard: None,
                    preset: Some(Preset::Video(VideoQuality::P720)),
                    fix: None,
                    output_dir: None,
                    outtmpl: None,
                }]
            },
            cx,
        )
    });
    cx.update(|cx| {
        assert_eq!(
            app.default_preset(&url, cx),
            Preset::Video(VideoQuality::P720)
        );
        assert_eq!(
            app.default_preset("https://b.example/x", cx),
            Preset::Audio(AudioFormat::Mp3)
        );
    });

    // queues once, then reports the duplicate with the same id
    let first = cx.update(|cx| app.add_direct(url.clone(), None, cx));
    let (id, dup) = cx.foreground_executor().block_test(first).unwrap();
    assert!(id.is_some() && !dup);
    let second = cx.update(|cx| app.add_direct(url.clone(), None, cx));
    assert_eq!(
        cx.foreground_executor().block_test(second).unwrap(),
        (id, true)
    );
    let row_preset = cx.update(|cx| {
        app.downloads
            .read(cx)
            .get(id.unwrap())
            .unwrap()
            .preset
            .clone()
    });
    assert_eq!(row_preset, "video:720");

    // two quick calls for a new link: one queues, the other is a duplicate
    let other = "https://a.example/watch?v=2".to_string();
    let (x, y) = cx.update(|cx| {
        (
            app.add_direct(other.clone(), None, cx),
            app.add_direct(other.clone(), None, cx),
        )
    });
    let (rx, ry) = (
        cx.foreground_executor().block_test(x).unwrap(),
        cx.foreground_executor().block_test(y).unwrap(),
    );
    assert_eq!((rx.1, ry.1), (false, true));
    let again = cx.update(|cx| app.add_direct(other, None, cx));
    assert!(cx.foreground_executor().block_test(again).unwrap().1);

    // a cancelled row does not count: the click retries as a new row
    app.downloads.update(cx, |d, cx| d.cancel(id.unwrap(), cx));
    settle(cx, "cancelled", |cx| {
        cx.update(|cx| app.downloads.read(cx).get(id.unwrap()).unwrap().status)
            == DownloadStatus::Cancelled
    });
    let third = cx.update(|cx| app.add_direct(url, None, cx));
    let (id3, dup3) = cx.foreground_executor().block_test(third).unwrap();
    assert!(!dup3 && id3 != id);
}
