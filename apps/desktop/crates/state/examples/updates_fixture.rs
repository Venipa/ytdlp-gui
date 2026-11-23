//! Serves a fake newer release for the updater on 127.0.0.1 (no GitHub):
//!   cargo run -p state --example updates_fixture -- <port> <version> <package file> [--prerelease]
//! then start the app with `YTDLPD_UPDATES_API=http://127.0.0.1:<port>`.
use state::updates::{asset_name, testkit::Fixture};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let (Some(port), Some(version), Some(file)) = (args.get(1), args.get(2), args.get(3)) else {
        eprintln!("usage: updates_fixture <port> <version> <package file> [--prerelease]");
        std::process::exit(2);
    };
    let fixture = Fixture::start_on(port.parse().expect("a port")).expect("cannot listen");
    let bytes = std::fs::read(file).expect("cannot read the package");
    let name = asset_name(version, std::env::consts::OS, std::env::consts::ARCH)
        .expect("no package for this platform");
    let prerelease = args.iter().any(|a| a == "--prerelease");
    fixture.release(&format!("v{version}"), prerelease, &[(&name, &bytes)], None);
    println!("serving {name} ({} bytes) at {}", bytes.len(), fixture.base);
    loop {
        std::thread::park();
    }
}
