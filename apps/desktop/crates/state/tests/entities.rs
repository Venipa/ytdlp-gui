//! Probes, tools and settings entities, plus the pure scheduler.
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domain::{
    Backend, DownloadId, DownloadStatus, DownloadSummary, EngineError, JobEvent, JobHandle,
    JobSpec, Preset, ProbeOpts, ProbeResult, Settings, ToolKind, ToolState,
};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use gpui::{AppContext as _, TestAppContext};
use state::{AppSettings, FakeTools, Limits, ProbeState, Probes, Scheduler, Tools};

fn settle(cx: &mut TestAppContext, what: &str, mut cond: impl FnMut(&mut TestAppContext) -> bool) {
    for _ in 0..400 {
        cx.run_until_parked();
        if cond(cx) {
            return;
        }
        cx.executor().advance_clock(Duration::from_millis(250));
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for: {what}");
}

/// A backend whose probes wait until the test releases them.
#[derive(Default)]
struct Shared {
    waiting: Mutex<Vec<(String, oneshot::Sender<()>)>>,
    in_flight: Mutex<(usize, usize)>, // (now, max)
}

#[derive(Clone, Default)]
struct Gated(Arc<Shared>);

impl Backend for Gated {
    fn probe(
        &self,
        url: String,
        _: ProbeOpts,
    ) -> BoxFuture<'static, Result<ProbeResult, EngineError>> {
        let (tx, rx) = oneshot::channel();
        self.0.waiting.lock().unwrap().push((url.clone(), tx));
        let me = self.0.clone();
        async move {
            {
                let mut f = me.in_flight.lock().unwrap();
                f.0 += 1;
                f.1 = f.1.max(f.0);
            }
            rx.await.ok();
            me.in_flight.lock().unwrap().0 -= 1;
            Ok(ProbeResult {
                url: url.clone(),
                title: Some(url),
                ..Default::default()
            })
        }
        .boxed()
    }
    fn start(
        &self,
        _: DownloadId,
        _: JobSpec,
    ) -> BoxFuture<'static, Result<(JobHandle, BoxStream<'static, JobEvent>), EngineError>> {
        unimplemented!()
    }
    fn kill(&self, _: JobHandle) -> BoxFuture<'static, ()> {
        unimplemented!()
    }
}

#[gpui::test]
fn probes_run_three_at_a_time_keyed_by_url(cx: &mut TestAppContext) {
    let gated = Gated::default();
    let probes = cx.new(|_| Probes::new(Arc::new(gated.clone())));
    let urls: Vec<String> = (0..5).map(|i| format!("https://a.com/{i}")).collect();
    for u in &urls {
        probes.update(cx, |p, cx| p.request(u, ProbeOpts::default(), cx));
    }
    cx.run_until_parked();
    assert_eq!(
        gated.0.waiting.lock().unwrap().len(),
        3,
        "only three probes start"
    );
    // asking again for a loading URL does not start another
    probes.update(cx, |p, cx| p.request(&urls[0], ProbeOpts::default(), cx));
    cx.run_until_parked();
    assert_eq!(gated.0.waiting.lock().unwrap().len(), 3);
    assert_eq!(
        probes.read_with(cx, |p, _| p.get(&urls[4]).cloned()),
        Some(ProbeState::Loading)
    );
    for _ in 0..5 {
        let next = gated.0.waiting.lock().unwrap().pop();
        if let Some((_, tx)) = next {
            tx.send(()).ok();
        }
        cx.run_until_parked();
    }
    for u in &urls {
        let state = probes.read_with(cx, |p, _| p.get(u).cloned());
        assert!(
            matches!(&state, Some(ProbeState::Ready(r)) if &r.url == u),
            "{u}: {state:?}"
        );
    }
    assert_eq!(gated.0.in_flight.lock().unwrap().1, 3);
}

#[gpui::test]
fn probe_timeout_is_abandoned(cx: &mut TestAppContext) {
    let gated = Gated::default();
    let probes =
        cx.new(|_| Probes::new(Arc::new(gated.clone())).with_timeout(Duration::from_secs(1)));
    probes.update(cx, |p, cx| {
        p.request("https://a.com/slow", ProbeOpts::default(), cx)
    });
    cx.run_until_parked();
    cx.executor().advance_clock(Duration::from_secs(2));
    cx.run_until_parked();
    let (state, abandoned) = probes.read_with(cx, |p, _| {
        (p.get("https://a.com/slow").cloned(), p.abandoned())
    });
    assert!(matches!(state, Some(ProbeState::Failed(e)) if e.retryable));
    assert_eq!(abandoned, 1);
}

#[gpui::test]
fn tools_ensure_retries_a_connection_reset(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let api = Arc::new(FakeTools::new(None, true));
    api.fail_first(2);
    let tools = cx.new(|_| Tools::new(api.clone()).with_retry_delay(Duration::from_millis(10)));
    tools.update(cx, |t, cx| t.refresh(cx));
    settle(cx, "detected", |cx| {
        tools.read_with(cx, |t, _| t.detected())
    });
    assert!(!tools.read_with(cx, |t, _| t.all_ready()));
    tools.update(cx, |t, cx| t.ensure_missing(cx));
    settle(cx, "ready after two resets", |cx| {
        tools.read_with(cx, |t, _| t.all_ready())
    });
}

#[gpui::test]
fn tools_ensure_gives_up_after_three_tries(cx: &mut TestAppContext) {
    let api = Arc::new(FakeTools::new(None, true));
    api.fail_first(10);
    let tools = cx.new(|_| Tools::new(api).with_retry_delay(Duration::from_millis(10)));
    tools.update(cx, |t, cx| t.refresh(cx));
    settle(cx, "detected", |cx| {
        tools.read_with(cx, |t, _| t.detected())
    });
    tools.update(cx, |t, cx| t.ensure(ToolKind::Ffmpeg, cx));
    settle(cx, "failed", |cx| {
        tools.read_with(cx, |t, _| {
            matches!(
                t.get(ToolKind::Ffmpeg).map(|s| &s.state),
                Some(ToolState::Failed(_))
            )
        })
    });
    // a custom path is validated
    tools.update(cx, |t, cx| {
        t.use_custom(ToolKind::Ffmpeg, "C:/bad/thing.exe".into(), cx)
    });
    settle(cx, "rejected", |cx| {
        tools.read_with(cx, |t, _| matches!(t.get(ToolKind::Ffmpeg).map(|s| &s.state), Some(ToolState::Failed(e)) if e.message.contains("is not ffmpeg")))
    });
}

#[gpui::test]
fn settings_round_trip_invalid_json_and_reload(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config").join("settings.json");
    let settings = cx.new(|_| AppSettings::detached(path.clone(), Settings::default()));
    assert_eq!(
        settings.read_with(cx, |s, _| s.get().clone()),
        Settings::default()
    );

    settings.update(cx, |s, cx| {
        s.update(|v| v.theme = "light".into(), cx);
        s.save_now();
    });
    assert_eq!(
        Settings::load(&path, &Settings::default()).settings.theme,
        "light"
    );

    // an external edit is applied by reload (the file watcher itself is tested in src/settings.rs)
    let edited = Settings {
        concurrent_global: 7,
        ..Default::default()
    };
    edited.save(&path).unwrap();
    settings.update(cx, |s, cx| s.reload(cx));
    assert_eq!(settings.read_with(cx, |s, _| s.get().concurrent_global), 7);

    // invalid JSON keeps the last good value and reports the error
    std::fs::write(&path, "{ not json").unwrap();
    settings.update(cx, |s, cx| s.reload(cx));
    let (value, error) = settings.read_with(cx, |s, _| {
        (s.get().concurrent_global, s.error().map(str::to_owned))
    });
    assert_eq!(value, 7);
    assert!(error.unwrap().contains("invalid settings.json"));
    std::fs::write(&path, "{}").unwrap();
    settings.update(cx, |s, cx| s.reload(cx));
    assert!(settings.read_with(cx, |s, _| s.error().is_none()));

    // debounced save writes after a quiet period
    settings.update(cx, |s, cx| s.update(|v| v.font_size = 15.0, cx));
    cx.executor().advance_clock(Duration::from_secs(1));
    cx.run_until_parked();
    assert_eq!(
        Settings::load(&path, &Settings::default())
            .settings
            .font_size,
        15.0
    );
}

fn row(id: i64, url: &str, status: DownloadStatus) -> DownloadSummary {
    let mut r = DownloadSummary::new(DownloadId(id), url, Preset::DEFAULT, 0);
    r.status = status;
    r
}

#[test]
fn scheduler_limits_and_penalty_are_pure() {
    use DownloadStatus::*;
    let limits = Limits::default(); // 3 global, 2 per domain, 1 slot for 5 min when penalised
    let mut s = Scheduler::default();
    let rows = vec![
        row(1, "https://a.com/1", Downloading),
        row(2, "https://a.com/2", Queued),
        row(3, "https://a.com/3", Queued),
        row(4, "https://b.org/1", Queued),
    ];
    assert_eq!(
        s.pick(&rows, 0, &limits),
        vec![DownloadId(2), DownloadId(4)]
    );
    s.penalize("a.com", 0, None, &limits);
    assert_eq!(
        s.pick(&rows, 1000, &limits),
        vec![DownloadId(4)],
        "a.com is down to one slot and has one running"
    );
    assert_eq!(
        s.pick(&rows, 5 * 60_000 + 1, &limits),
        vec![DownloadId(2), DownloadId(4)],
        "penalty expired"
    );
    // a backoff gate holds a row back
    let mut gated = rows.clone();
    gated[1].next_attempt_at = Some(10_000);
    assert_eq!(
        s.pick(&gated, 9_999, &limits),
        vec![DownloadId(3), DownloadId(4)]
    );
    assert_eq!(s.pick(&Vec::new(), 0, &limits), vec![]);
}
