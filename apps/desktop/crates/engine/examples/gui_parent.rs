//! G1 check: a GUI-subsystem exe (no console) that is both the host and, re-executed with `--engine-worker`, the worker.
//! `gui_parent <url> <events-file> <out-dir>`: writes every event line to <events-file>; exit 0 when a `result` arrived.
#![cfg_attr(windows, windows_subsystem = "windows")]
use std::{io::Write, time::Duration};

use domain::Preset;
use engine::Event;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.first().map(String::as_str) == Some("--engine-worker") {
        engine::worker::run_worker();
    }
    let (url, events, out) = (&a[0], &a[1], &a[2]);
    let mut f = std::fs::File::create(events).unwrap();
    let job = engine::spec::simple_job(url.clone(), Preset::DEFAULT, out);
    let mut w = engine::spawn_worker(&std::env::current_exe().unwrap(), &job).expect("spawn");
    let mut ok = false;
    while let Ok(ev) = w.events.recv_timeout(Duration::from_secs(60)) {
        writeln!(f, "{}", serde_json::to_string(&ev).unwrap()).unwrap();
        ok |= matches!(ev, Event::Result { .. });
        if ev.is_terminal() {
            break;
        }
    }
    let _ = w.wait();
    std::process::exit(if ok { 0 } else { 1 });
}
