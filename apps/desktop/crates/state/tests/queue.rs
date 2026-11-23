//! Queue behaviour on a manual fake backend and a manual clock (qa.md names).
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use domain::{
    DownloadId, DownloadStatus, EngineError, ErrorCode, Jitter, JobEvent, JobSpec, ManualClock,
    Preset, Settings, XorShift,
};
use gpui::{AppContext as _, Entity, TestAppContext};
use state::fake::{Scene, seed_rows};
use state::{AppSettings, Downloads, FakeBackend, Io, Logs, Store};
use storage::Db;

const T0: i64 = 1_000_000;

struct Rig {
    io: Io,
    db: Db,
    store: Store,
    backend: FakeBackend,
    clock: Arc<ManualClock>,
    downloads: Entity<Downloads>,
}

fn spec(url: &str) -> JobSpec {
    AppSettings::detached(Default::default(), Settings::default()).job_spec(
        url,
        Preset::DEFAULT,
        "bv*+ba/b",
    )
}

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

/// Builds a loaded `Downloads` over an in-memory database; `prepare` may seed rows before loading.
fn rig(
    cx: &mut TestAppContext,
    tweak: impl FnOnce(&mut Settings),
    prepare: impl FnOnce(&Db, &Io),
) -> Rig {
    cx.executor().allow_parking();
    let io = Io::new().unwrap();
    let db = io.handle().block_on(Db::memory()).unwrap();
    prepare(&db, &io);
    let store = Store::with_db(&io, db.clone());
    let backend = FakeBackend::manual();
    let clock = Arc::new(ManualClock::new(T0));
    let mut settings = Settings::default();
    tweak(&mut settings);
    let app_settings = cx.new(|_| AppSettings::detached(Default::default(), settings));
    let logs = cx.new(|_| Logs::default());
    let (b, s, c) = (backend.clone(), store.clone(), clock.clone());
    let downloads = cx.new(|cx| Downloads::new(Arc::new(b), s, c, logs, &app_settings, cx));
    downloads.update(cx, |d, cx| d.load(cx));
    settle(cx, "load", |cx| downloads.read_with(cx, |d, _| d.loaded()));
    Rig {
        io,
        db,
        store,
        backend,
        clock,
        downloads,
    }
}

impl Rig {
    fn flush(&self) {
        self.io.handle().block_on(self.store.flush());
    }
    fn db_rows(&self) -> Vec<domain::DownloadSummary> {
        self.flush();
        self.io.handle().block_on(self.db.list_summaries()).unwrap()
    }
    fn row(&self, cx: &mut TestAppContext, id: DownloadId) -> domain::DownloadSummary {
        self.downloads
            .read_with(cx, |d, _| d.get(id).cloned().expect("row"))
    }
    fn enqueue(&self, cx: &mut TestAppContext, url: &str) -> DownloadId {
        let before = self.downloads.read_with(cx, |d, _| d.rows().len());
        self.downloads
            .update(cx, |d, cx| d.enqueue(spec(url), None, None, cx).detach());
        settle(cx, "enqueue", |cx| {
            self.downloads
                .read_with(cx, |d, _| d.rows().len() == before + 1)
        });
        self.downloads.read_with(cx, |d, _| d.rows()[0].id)
    }
    fn started(&self) -> Vec<DownloadId> {
        self.backend.started().iter().map(|(id, _)| *id).collect()
    }
    fn tick(&self, cx: &mut TestAppContext) -> bool {
        let alive = self.downloads.update(cx, |d, cx| d.on_tick(cx));
        cx.run_until_parked();
        alive
    }
    fn act(
        &self,
        cx: &mut TestAppContext,
        f: impl FnOnce(&mut Downloads, &mut gpui::Context<Downloads>),
    ) {
        self.downloads.update(cx, f);
        cx.run_until_parked();
    }
}

fn err(code: ErrorCode) -> EngineError {
    EngineError::new(code, format!("{code} for the test"))
}

#[gpui::test]
fn global_and_domain_limits(cx: &mut TestAppContext) {
    let r = rig(
        cx,
        |s| (s.concurrent_global, s.concurrent_per_domain) = (3, 2),
        |_, _| {},
    );
    let a: Vec<_> = (1..=3)
        .map(|i| r.enqueue(cx, &format!("https://a.com/{i}")))
        .collect();
    let b: Vec<_> = (1..=2)
        .map(|i| r.enqueue(cx, &format!("https://b.org/{i}")))
        .collect();
    settle(cx, "three started", |_| r.started().len() == 3);
    assert_eq!(r.started(), vec![a[0], a[1], b[0]]); // a3 waits for its domain, b2 for the global slot
    r.backend.finish(a[0]);
    settle(cx, "a3 starts", |_| r.started().len() == 4);
    assert_eq!(r.started()[3], a[2]);
    assert_eq!(r.row(cx, b[1]).status, DownloadStatus::Queued);
}

#[gpui::test]
fn cancel_queued_removes_immediately(cx: &mut TestAppContext) {
    let r = rig(cx, |s| s.concurrent_global = 1, |_, _| {});
    let a = r.enqueue(cx, "https://a.com/1");
    let b = r.enqueue(cx, "https://a.com/2");
    settle(cx, "a started", |_| r.started().len() == 1);
    r.downloads.update(cx, |d, cx| d.cancel(b, cx));
    // no waiting: the status flips in the same call
    assert_eq!(r.row(cx, b).status, DownloadStatus::Cancelled);
    assert_eq!(r.downloads.read_with(cx, |d, _| d.pending_len()), 0);
    assert_eq!(
        r.db_rows().iter().find(|x| x.id == b).unwrap().status,
        DownloadStatus::Cancelled
    );
    r.backend.finish(a);
    settle(cx, "a completed", |cx| {
        r.row(cx, a).status == DownloadStatus::Completed
    });
    assert_eq!(r.started(), vec![a], "b never reached the engine");
}

#[gpui::test]
fn retry_with_backoff_and_penalty_box(cx: &mut TestAppContext) {
    let r = rig(
        cx,
        |s| (s.concurrent_global, s.concurrent_per_domain) = (3, 2),
        |_, _| {},
    );
    let a1 = r.enqueue(cx, "https://a.com/1");
    let a2 = r.enqueue(cx, "https://a.com/2");
    settle(cx, "both started", |_| r.started().len() == 2);

    r.backend.fail(a1, err(ErrorCode::RateLimited));
    settle(cx, "a1 requeued", |cx| {
        r.row(cx, a1).status == DownloadStatus::Queued
    });
    let row = r.row(cx, a1);
    let wait = row.next_attempt_at.unwrap() - T0;
    assert_eq!(row.attempts, 1);
    assert!(
        (12_000..=18_000).contains(&wait),
        "first backoff is 15 s +-20%, got {wait}"
    );
    assert!(r.downloads.read_with(cx, |d, _| d.penalized("a.com")));
    assert!(
        r.downloads.read_with(cx, |d, _| d.ticking()),
        "the tick runs while a retry waits"
    );

    // the gate holds, then opens; the penalty box keeps a.com at one slot while a2 runs
    r.clock.advance_ms(5_000);
    r.tick(cx);
    assert_eq!(r.started().len(), 2);
    r.clock.advance_ms(20_000);
    r.tick(cx);
    assert_eq!(
        r.started().len(),
        2,
        "a.com is penalised to one slot and a2 holds it"
    );
    r.backend.finish(a2);
    settle(cx, "a1 retried", |_| r.started().len() == 3);
    assert_eq!(r.started()[2], a1, "same row, not a new one");
    assert_eq!(r.downloads.read_with(cx, |d, _| d.rows().len()), 2);

    // penalty ends after five minutes
    r.clock.advance_ms(6 * 60_000);
    assert!(!r.downloads.read_with(cx, |d, _| d.penalized("a.com")));
}

#[gpui::test]
fn retry_same_row(cx: &mut TestAppContext) {
    let r = rig(cx, |_| {}, |_, _| {});
    let id = r.enqueue(cx, "https://a.com/1");
    settle(cx, "started", |_| r.started().len() == 1);
    let before =
        r.io.handle()
            .block_on(r.db.get_detail(id))
            .unwrap()
            .unwrap()
            .options_json;
    r.backend.fail(id, err(ErrorCode::Login));
    settle(cx, "failed", |cx| {
        r.row(cx, id).status == DownloadStatus::Failed
    });
    assert_eq!(
        r.row(cx, id).attempts,
        0,
        "a final failure does not count as a retry"
    );

    r.act(cx, |d, cx| d.retry(id, cx));
    let row = r.row(cx, id);
    assert_eq!((row.attempts, row.error_code), (1, None));
    settle(cx, "restarted", |_| r.started().len() == 2);
    assert_eq!(r.downloads.read_with(cx, |d, _| d.rows().len()), 1);
    let detail =
        r.io.handle()
            .block_on(r.db.get_detail(id))
            .unwrap()
            .unwrap();
    assert_eq!(detail.options_json, before);
    assert_eq!(r.db_rows()[0].attempts, 1);
}

#[gpui::test]
fn pause_resume_cancel_kill_workers(cx: &mut TestAppContext) {
    let r = rig(cx, |_| {}, |_, _| {});
    let a = r.enqueue(cx, "https://a.com/1");
    let b = r.enqueue(cx, "https://b.org/1");
    settle(cx, "started", |_| r.started().len() == 2);
    r.backend.send(a, JobEvent::Phase("downloading".into()));
    r.backend
        .send(a, JobEvent::Files(vec!["F:/nonexistent/a.part".into()]));
    let (ha, hb) = (
        r.backend.handle_of(a).unwrap(),
        r.backend.handle_of(b).unwrap(),
    );
    r.act(cx, |d, cx| d.pause(a, cx));
    assert_eq!(r.row(cx, a).status, DownloadStatus::Paused);
    assert_eq!(r.backend.killed(), vec![ha]);
    r.act(cx, |d, cx| d.resume(a, cx));
    settle(cx, "a restarted", |_| r.started().len() == 3);
    r.act(cx, |d, cx| d.cancel(b, cx));
    assert_eq!(r.backend.killed(), vec![ha, hb]);
    assert_eq!(r.row(cx, b).status, DownloadStatus::Cancelled);
    // an active row cannot be deleted, a cancelled one can
    assert!(!r.downloads.update(cx, |d, cx| d.delete(a, cx)));
    assert!(r.downloads.update(cx, |d, cx| d.delete(b, cx)));
    assert_eq!(r.db_rows().len(), 1);
}

#[gpui::test]
fn scheduler_no_leaks(cx: &mut TestAppContext) {
    let r = rig(
        cx,
        |s| (s.concurrent_global, s.concurrent_per_domain) = (3, 2),
        |_, _| {},
    );
    let mut rng = XorShift(7);
    let urls = [
        "https://a.com/1",
        "https://a.com/2",
        "https://b.org/1",
        "https://c.net/1",
        "https://c.net/2",
    ];
    for i in 0..200 {
        let rows = r.downloads.read_with(cx, |d, _| d.rows().to_vec());
        let pick = |rng: &mut XorShift, f: &dyn Fn(&domain::DownloadSummary) -> bool| {
            let c: Vec<_> = rows.iter().filter(|r| f(r)).collect();
            (!c.is_empty()).then(|| c[(rng.unit() * c.len() as f64) as usize].id)
        };
        match (rng.unit() * 6.0) as u32 {
            0 | 1 => {
                r.enqueue(cx, urls[(rng.unit() * urls.len() as f64) as usize]); // same URL again is allowed
            }
            2 => {
                if let Some(id) = pick(&mut rng, &|r| !r.status.is_terminal()) {
                    r.act(cx, |d, cx| d.cancel(id, cx));
                }
            }
            3 => {
                if let Some(id) = pick(&mut rng, &|r| r.status.is_active() && r.attempts < 9) {
                    r.backend.finish(id);
                }
            }
            4 => {
                if let Some(id) = pick(&mut rng, &|r| r.status.is_active()) {
                    r.backend.fail(
                        id,
                        err(if i % 2 == 0 {
                            ErrorCode::Network
                        } else {
                            ErrorCode::Private
                        }),
                    );
                }
            }
            _ => {
                if let Some(id) = pick(&mut rng, &|r| {
                    r.status.is_active() || r.status == DownloadStatus::Paused
                }) {
                    let paused = r.row(cx, id).status == DownloadStatus::Paused;
                    r.act(cx, |d, cx| {
                        if paused {
                            d.resume(id, cx)
                        } else {
                            d.pause(id, cx)
                        }
                    });
                }
            }
        }
        cx.run_until_parked();
        r.clock.advance_ms(30_000);
        r.tick(cx);
        let (active, jobs) = r
            .downloads
            .read_with(cx, |d, _| (d.count(|s| s.is_active()), d.running_jobs()));
        assert!(active <= 3, "global limit held at op {i}");
        assert_eq!(
            jobs, active,
            "every active row has exactly one worker at op {i}"
        );
    }
    // drain: cancel everything that is left
    let ids: Vec<_> = r.downloads.read_with(cx, |d, _| {
        d.rows()
            .iter()
            .filter(|x| !x.status.is_terminal())
            .map(|x| x.id)
            .collect()
    });
    for id in ids {
        r.act(cx, |d, cx| d.cancel(id, cx));
    }
    let (running, pending, jobs, ticking) = r.downloads.read_with(cx, |d, _| {
        (
            d.count(|s| s.is_active()),
            d.pending_len(),
            d.running_jobs(),
            d.ticking(),
        )
    });
    assert_eq!((running, pending, jobs), (0, 0, 0));
    r.tick(cx);
    assert!(
        !r.downloads.read_with(cx, |d, _| d.ticking()),
        "the tick stops when nothing runs (was {ticking})"
    );
    // the same URL can be added again after fail or cancel
    r.enqueue(cx, urls[0]);
}

#[gpui::test]
fn restart_recovery(cx: &mut TestAppContext) {
    use DownloadStatus::*;
    let wanted = [
        Completed,
        Completed,
        Probing,
        Downloading,
        Processing,
        Paused,
        Failed,
        Cancelled,
    ];
    let r = rig(
        cx,
        |_| {},
        |db, io| {
            let mut rows = seed_rows(Scene::Downloading, T0);
            rows.truncate(wanted.len());
            for (row, st) in rows.iter_mut().zip(wanted) {
                row.status = st;
            }
            io.handle().block_on(db.seed_downloads(&rows)).unwrap();
        },
    );
    let want: Vec<_> = wanted
        .iter()
        .map(|s| if s.is_active() { Paused } else { *s })
        .collect();
    for rows in [
        r.downloads.read_with(cx, |d, _| d.rows().to_vec()),
        r.db_rows(),
    ] {
        let mut by_pos = rows;
        by_pos.sort_by(|a, b| a.position.total_cmp(&b.position));
        assert_eq!(by_pos.iter().map(|x| x.status).collect::<Vec<_>>(), want);
        for (row, was) in by_pos.iter().zip(wanted) {
            let interrupted = row.status_reason.as_deref() == Some("interrupted");
            assert_eq!(interrupted, was.is_active(), "{was}");
        }
    }
    assert!(
        r.started().is_empty(),
        "interrupted rows wait for the user (resume_on_start is off)"
    );
}

#[gpui::test]
fn restart_recovery_resume_on_start(cx: &mut TestAppContext) {
    use DownloadStatus::*;
    let r = rig(
        cx,
        |s| {
            s.resume_on_start = true;
            s.concurrent_global = 2;
        },
        |db, io| {
            let mut rows = seed_rows(Scene::Downloading, T0);
            rows.truncate(4);
            for (row, st) in rows
                .iter_mut()
                .zip([Downloading, Probing, Processing, Paused])
            {
                row.status = st;
            }
            io.handle().block_on(db.seed_downloads(&rows)).unwrap();
        },
    );
    settle(cx, "two resumed rows start", |_| r.started().len() == 2);
    let rows = r.downloads.read_with(cx, |d, _| d.rows().to_vec());
    assert_eq!(
        rows.iter().filter(|x| x.status == Paused).count(),
        1,
        "the user-paused row stays paused"
    );
    assert_eq!(rows.iter().filter(|x| x.status.is_active()).count(), 2);
}

#[gpui::test]
fn notify_rate(cx: &mut TestAppContext) {
    let r = rig(cx, |_| {}, |_, _| {});
    let ids: Vec<_> = (1..=3)
        .map(|i| r.enqueue(cx, &format!("https://s{i}.com/x")))
        .collect();
    settle(cx, "started", |_| r.started().len() == 3);
    for id in &ids {
        r.backend.send(*id, JobEvent::Phase("downloading".into()));
    }
    cx.run_until_parked();
    let count = Rc::new(Cell::new(0u32));
    let c = count.clone();
    cx.update(|app| {
        app.observe(&r.downloads, move |_, _| c.set(c.get() + 1))
            .detach()
    });
    // 10 000 progress calls over 2 s (8 ticks of 250 ms)
    let mut sent = 0u64;
    for _ in 0..8 {
        for _ in 0..1250 {
            let id = ids[(sent % 3) as usize];
            sent += 1;
            r.backend.send(
                id,
                JobEvent::Progress {
                    done: sent,
                    total: Some(1_000_000),
                    speed: None,
                    eta: None,
                    frag: None,
                },
            );
        }
        cx.run_until_parked();
        r.clock.advance_ms(250);
        r.tick(cx);
    }
    assert_eq!(sent, 10_000);
    assert!(count.get() <= 10, "notifies in 2 s: {}", count.get());
    // storage is not touched per progress call: nothing was written within 5 s
    assert!(r.db_rows().iter().all(|x| x.bytes_done.is_none()));
    r.clock.advance_ms(5_000);
    r.backend.send(
        ids[0],
        JobEvent::Progress {
            done: 42,
            total: Some(100),
            speed: None,
            eta: None,
            frag: None,
        },
    );
    cx.run_until_parked();
    assert_eq!(
        r.db_rows()
            .iter()
            .find(|x| x.id == ids[0])
            .unwrap()
            .bytes_done,
        Some(42)
    );
}

#[gpui::test]
fn finished_records_path_and_logs(cx: &mut TestAppContext) {
    let r = rig(cx, |_| {}, |_, _| {});
    let id = r.enqueue(cx, "https://a.com/1");
    settle(cx, "started", |_| r.started().len() == 1);
    let file = std::env::temp_dir().join("ytdlpd-state-test.bin");
    std::fs::write(&file, b"12345").unwrap();
    r.backend.send(
        id,
        JobEvent::Log {
            level: "info".into(),
            message: "hello".into(),
        },
    );
    r.backend
        .send(id, JobEvent::Phase("postprocess:Merger".into()));
    r.backend
        .send(id, JobEvent::Path(file.to_string_lossy().into_owned()));
    r.backend.finish(id);
    settle(cx, "completed", |cx| {
        r.row(cx, id).status == DownloadStatus::Completed
    });
    let row = r.row(cx, id);
    assert_eq!(
        (row.file_size, row.progress, row.phase),
        (Some(5), 1.0, None)
    );
    let db = r.db_rows().remove(0);
    assert_eq!(db.file_path.as_deref(), file.to_str());
    assert_eq!(r.io.handle().block_on(r.db.list_logs(id)).unwrap().len(), 1);
    std::fs::remove_file(file).ok();
    assert_eq!(r.downloads.read_with(cx, |d, _| d.running_jobs()), 0);
}
