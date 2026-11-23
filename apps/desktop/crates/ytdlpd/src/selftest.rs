//! Headless process modes, handled before gpui starts: `--engine-worker` and `--self-test [--site <dir>]`.
use std::{path::PathBuf, process::ExitCode};

/// `--site <dir>` or `--site=<dir>` from `args`.
#[cfg_attr(not(feature = "real-engine"), allow(dead_code))]
fn site_arg(args: &[String]) -> Option<PathBuf> {
    args.iter()
        .enumerate()
        .find_map(|(i, a)| match a.strip_prefix("--site=") {
            Some(v) => Some(PathBuf::from(v)),
            None if a == "--site" => args.get(i + 1).map(PathBuf::from),
            None => None,
        })
}

/// Runs the headless mode the command line asks for. `None` means start the app normally.
pub fn dispatch() -> Option<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let worker = args.first().is_some_and(|a| a == "--engine-worker");
    let selftest = args.iter().any(|a| a == "--self-test");
    if !worker && !selftest {
        return None;
    }
    Some(run(worker, &args))
}

#[cfg(feature = "real-engine")]
fn run(worker: bool, args: &[String]) -> ExitCode {
    if worker {
        engine::worker::run_worker();
    }
    match engine::runtime::self_test(site_arg(args)) {
        Ok(t) => {
            println!("python {}", t.python);
            println!("yt-dlp {} ({})", t.yt_dlp, t.yt_dlp_path);
            println!("yt-dlp-ejs {}", t.yt_dlp_ejs);
            println!("init {} ms", t.init_ms);
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("self-test failed: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(feature = "real-engine"))]
fn run(_worker: bool, _args: &[String]) -> ExitCode {
    eprintln!("ytdlpd was built without the real-engine feature");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_arg_forms() {
        let a = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            site_arg(&a(&["--self-test", "--site", "D:/s"])),
            Some("D:/s".into())
        );
        assert_eq!(site_arg(&a(&["--site=D:/s"])), Some("D:/s".into()));
        assert_eq!(site_arg(&a(&["--self-test"])), None);
    }
}
