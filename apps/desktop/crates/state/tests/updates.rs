//! The updater state machine against the local fixture server (no GitHub).
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use domain::{ManualClock, Settings, UpdateChannel, UpdateMode};
use gpui::{AppContext as _, Entity, TestAppContext};
use state::updates::testkit::{Fixture, sha256_hex};
use state::updates::{asset_name, current_version};
use state::{
    AppSettings, Downloads, FakeBackend, Io, Logs, Store, UpdateError, UpdateState, Updates,
};
use storage::Db;

fn settle(cx: &mut TestAppContext, what: &str, mut cond: impl FnMut(&mut TestAppContext) -> bool) {
    for _ in 0..800 {
        cx.run_until_parked();
        if cond(cx) {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    panic!("timed out waiting for: {what}");
}

struct Rig {
    updates: Entity<Updates>,
    settings: Entity<AppSettings>,
    launched: Arc<Mutex<Vec<(PathBuf, bool)>>>,
    folder: tempfile::TempDir,
}

fn rig(cx: &mut TestAppContext, api: &str, mode: UpdateMode, channel: UpdateChannel) -> Rig {
    rig_after(cx, api, mode, channel, None)
}

/// A rig whose database was last used by version `last`.
fn rig_after(
    cx: &mut TestAppContext,
    api: &str,
    mode: UpdateMode,
    channel: UpdateChannel,
    last: Option<&str>,
) -> Rig {
    cx.executor().allow_parking();
    let io = Io::new().unwrap();
    let db = io.handle().block_on(Db::memory()).unwrap();
    if let Some(last) = last {
        io.handle()
            .block_on(db.state_set("updates_last_version", last))
            .unwrap();
        io.handle()
            .block_on(db.state_set("updates_last_checked", "5"))
            .unwrap();
    }
    let store = Store::with_db(&io, db);
    let settings = Settings {
        update_mode: mode,
        update_channel: channel,
        ..Settings::default()
    };
    let settings = cx.new(|_| AppSettings::detached(Default::default(), settings));
    let logs = cx.new(|_| Logs::default());
    let (s, st) = (settings.clone(), store.clone());
    let downloads = cx.new(|cx| {
        Downloads::new(
            Arc::new(FakeBackend::manual()),
            st,
            Arc::new(ManualClock::new(0)),
            logs,
            &s,
            cx,
        )
    });
    let folder = tempfile::tempdir().unwrap();
    let launched: Arc<Mutex<Vec<(PathBuf, bool)>>> = Arc::default();
    let (f, api, l) = (
        folder.path().to_path_buf(),
        api.to_string(),
        launched.clone(),
    );
    let s = settings.clone();
    let updates = cx.new(|cx| {
        let mut u = Updates::new(s, downloads, store, io, f, api, false, cx);
        u.set_launcher(Arc::new(move |path, relaunch| {
            l.lock().unwrap().push((path.to_path_buf(), relaunch));
            Ok(())
        }));
        u
    });
    Rig {
        updates,
        settings,
        launched,
        folder,
    }
}

fn state(cx: &mut TestAppContext, r: &Rig) -> UpdateState {
    r.updates.read_with(cx, |u, _| u.state().clone())
}

/// A version newer than the running one.
fn newer() -> String {
    let v: Vec<u64> = current_version()
        .split(['.', '-'])
        .take(3)
        .map(|p| p.parse().unwrap())
        .collect();
    format!("{}.{}.{}", v[0], v[1] + 1, 0)
}

fn package(version: &str) -> String {
    asset_name(version, std::env::consts::OS, std::env::consts::ARCH).unwrap()
}

#[gpui::test]
fn manual_check_download_ready_relaunch(cx: &mut TestAppContext) {
    let fixture = Fixture::start().unwrap();
    let version = newer();
    let name = package(&version);
    let bytes = vec![7u8; 300_000];
    fixture.release(&format!("v{version}"), false, &[(&name, &bytes)], None);
    let r = rig(cx, &fixture.base, UpdateMode::Manual, UpdateChannel::Stable);

    r.updates.update(cx, |u, cx| u.check(cx));
    assert_eq!(state(cx, &r), UpdateState::Checking);
    settle(cx, "available", |cx| {
        matches!(state(cx, &r), UpdateState::Available(_))
    });
    let UpdateState::Available(release) = state(cx, &r) else {
        unreachable!()
    };
    assert_eq!(release.version, version);
    assert_eq!(release.size(), Some(300_000));
    assert!(r.updates.read_with(cx, |u, _| u.last_checked().is_some()));

    r.updates.update(cx, |u, cx| u.download(cx));
    assert!(matches!(state(cx, &r), UpdateState::Downloading { .. }));
    settle(cx, "ready", |cx| {
        matches!(state(cx, &r), UpdateState::Ready(_))
    });
    let staged = r.folder.path().join(&name);
    assert_eq!(
        sha256_hex(&std::fs::read(&staged).unwrap()),
        sha256_hex(&bytes)
    );

    r.updates.update(cx, |u, cx| u.relaunch(cx));
    assert!(matches!(state(cx, &r), UpdateState::Installing(_)));
    assert_eq!(*r.launched.lock().unwrap(), vec![(staged, true)]);
}

#[gpui::test]
fn automatic_mode_downloads_by_itself_and_install_on_quit_is_a_toggle(cx: &mut TestAppContext) {
    let fixture = Fixture::start().unwrap();
    let version = newer();
    let name = package(&version);
    fixture.release(&format!("v{version}"), false, &[(&name, b"pkg")], None);
    let r = rig(
        cx,
        &fixture.base,
        UpdateMode::Automatic,
        UpdateChannel::Stable,
    );
    r.updates.update(cx, |u, cx| u.check(cx));
    settle(cx, "ready", |cx| {
        matches!(state(cx, &r), UpdateState::Ready(_))
    });
    r.updates.update(cx, |u, cx| u.toggle_install_on_quit(cx));
    assert!(r.updates.read_with(cx, |u, _| u.install_on_quit()));
}

#[gpui::test]
fn checksum_mismatch_fails_closed_and_try_again_downloads_again(cx: &mut TestAppContext) {
    let fixture = Fixture::start().unwrap();
    let version = newer();
    let name = package(&version);
    let tag = format!("v{version}");
    let wrong = format!("{}  {name}\n", "0".repeat(64));
    fixture.release(&tag, false, &[(&name, b"pkg")], Some(&wrong));
    let r = rig(
        cx,
        &fixture.base,
        UpdateMode::Automatic,
        UpdateChannel::Stable,
    );
    r.updates.update(cx, |u, cx| u.check(cx));
    settle(cx, "failed", |cx| {
        matches!(state(cx, &r), UpdateState::Failed { .. })
    });
    assert!(matches!(
        state(cx, &r),
        UpdateState::Failed {
            error: UpdateError::Checksum,
            release: Some(_)
        }
    ));
    assert!(!r.folder.path().join(&name).exists(), "nothing is kept");
    assert!(r.launched.lock().unwrap().is_empty());

    // a release without a line for the package fails closed too
    fixture.release(&tag, false, &[(&name, b"pkg")], Some(""));
    r.updates.update(cx, |u, cx| u.retry(cx));
    settle(cx, "failed again", |cx| {
        matches!(
            state(cx, &r),
            UpdateState::Failed {
                error: UpdateError::Checksum,
                ..
            }
        )
    });

    // fixed checksums: Try again gets it
    fixture.release(&tag, false, &[(&name, b"pkg")], None);
    r.updates.update(cx, |u, cx| u.retry(cx));
    settle(cx, "ready", |cx| {
        matches!(state(cx, &r), UpdateState::Ready(_))
    });
}

#[gpui::test]
fn broken_download_and_offline(cx: &mut TestAppContext) {
    let fixture = Fixture::start().unwrap();
    let version = newer();
    let name = package(&version);
    let tag = format!("v{version}");
    fixture.release(&tag, false, &[(&name, b"pkg")], None);
    // the connection breaks off after 1000 of 5000 bytes
    fixture.put_truncated(&format!("/download/{tag}/{name}"), vec![1u8; 1000], 5000);
    let r = rig(cx, &fixture.base, UpdateMode::Manual, UpdateChannel::Stable);
    r.updates.update(cx, |u, cx| u.check(cx));
    settle(cx, "available", |cx| {
        matches!(state(cx, &r), UpdateState::Available(_))
    });
    r.updates.update(cx, |u, cx| u.download(cx));
    settle(cx, "download failed", |cx| {
        matches!(
            state(cx, &r),
            UpdateState::Failed {
                error: UpdateError::Download { .. },
                release: Some(_)
            }
        )
    });
    assert!(!r.folder.path().join(&name).exists());

    // nobody listens on this port
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    let r = rig(cx, &base, UpdateMode::Manual, UpdateChannel::Stable);
    r.updates.update(cx, |u, cx| u.check(cx));
    settle(cx, "offline", |cx| {
        state(cx, &r)
            == UpdateState::Failed {
                error: UpdateError::Offline,
                release: None,
            }
    });
    // Try again checks again
    r.updates.update(cx, |u, cx| u.retry(cx));
    assert_eq!(state(cx, &r), UpdateState::Checking);
}

#[gpui::test]
fn prereleases_only_on_beta_and_a_channel_switch_checks_again(cx: &mut TestAppContext) {
    let fixture = Fixture::start().unwrap();
    let version = format!("{}-beta.1", newer());
    let name = package(&version);
    fixture.release(&format!("v{version}"), true, &[(&name, b"pkg")], None);
    let r = rig(cx, &fixture.base, UpdateMode::Manual, UpdateChannel::Stable);
    r.updates.update(cx, |u, cx| u.check(cx));
    settle(cx, "idle after check", |cx| {
        state(cx, &r) == UpdateState::Idle
            && r.updates.read_with(cx, |u, _| u.last_checked().is_some())
    });
    r.settings.update(cx, |s, cx| {
        s.update(|s| s.update_channel = UpdateChannel::Beta, cx)
    });
    settle(
        cx,
        "beta offers it",
        |cx| matches!(state(cx, &r), UpdateState::Available(ref rel) if rel.version == version),
    );
}

#[gpui::test]
fn the_first_run_of_a_newer_version_says_so_until_dismissed(cx: &mut TestAppContext) {
    let (mode, channel) = (UpdateMode::Manual, UpdateChannel::Stable);
    for (last, says) in [
        ("0.0.1", true),
        (current_version(), false),
        ("999.0.0", false),
    ] {
        let r = rig_after(cx, "", mode, channel, Some(last));
        settle(cx, "read", |cx| {
            r.updates.read_with(cx, |u, _| u.last_checked().is_some())
        });
        assert_eq!(r.updates.read_with(cx, |u, _| u.updated()), says, "{last}");
        r.updates.update(cx, |u, cx| u.dismiss_updated(cx));
        assert!(!r.updates.read_with(cx, |u, _| u.updated()));
    }
}
