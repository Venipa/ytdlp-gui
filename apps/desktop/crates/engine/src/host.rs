//! Host side: spawn `<exe> --engine-worker`, stream typed events, kill the whole process tree.
use std::{
    io::{BufRead, BufReader, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command as Proc, ExitStatus, Stdio},
    sync::mpsc::{Receiver, Sender, channel},
    time::Instant,
};

use domain::JobSpec;

use crate::{
    ipc::{Command, Event},
    spec::{Env, to_opts},
};

#[cfg(windows)]
mod tree {
    use std::os::windows::{io::AsRawHandle, process::CommandExt};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE},
        System::{
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject,
            },
            Threading::CREATE_NO_WINDOW,
        },
    };

    /// A Job Object with KILL_ON_JOB_CLOSE: closing the handle (also when the host dies) kills every process in it.
    /// One job per worker (nested jobs are fine on Windows 8+) so a single tree can be killed on its own.
    pub struct Tree(HANDLE);
    // SAFETY: a job handle is just a kernel object reference, usable from any thread.
    unsafe impl Send for Tree {}

    pub fn prepare(c: &mut std::process::Command) {
        c.creation_flags(CREATE_NO_WINDOW);
    }

    impl Tree {
        /// shortcut: assignment happens after spawn, so a grandchild forked in that microsecond window would escape; workers only fork after Python is up, upgrade to CREATE_SUSPENDED if that ever matters.
        pub fn adopt(child: &std::process::Child) -> std::io::Result<Self> {
            // SAFETY: plain Win32 calls with valid, locally owned arguments.
            unsafe {
                let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if job.is_null() {
                    return Err(std::io::Error::last_os_error());
                }
                let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let ok = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    (&raw const info).cast(),
                    size_of_val(&info) as u32,
                ) != 0
                    && AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) != 0;
                if !ok {
                    let e = std::io::Error::last_os_error();
                    CloseHandle(job);
                    return Err(e);
                }
                Ok(Tree(job))
            }
        }
        pub fn kill(&self) {
            // SAFETY: valid job handle owned by self.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            // SAFETY: closing our own handle; KILL_ON_JOB_CLOSE reaps anything still running.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[cfg(unix)]
mod tree {
    use std::os::unix::process::CommandExt;

    /// The child leads its own process group; `killpg` reaches the whole tree.
    pub struct Tree(i32);

    pub fn prepare(c: &mut std::process::Command) {
        c.process_group(0);
        #[cfg(target_os = "linux")]
        // SAFETY: prctl is async-signal-safe; asks the kernel to SIGKILL the worker when the host thread dies.
        unsafe {
            c.pre_exec(|| {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                Ok(())
            });
        }
        // macOS: no PDEATHSIG; the worker exits on stdin EOF (see worker.rs).
    }

    impl Tree {
        pub fn adopt(child: &std::process::Child) -> std::io::Result<Self> {
            Ok(Tree(child.id() as i32))
        }
        pub fn kill(&self) {
            // SAFETY: killpg on the group we created.
            unsafe { libc::killpg(self.0, libc::SIGKILL) };
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            self.kill();
        }
    }
}

/// A child process plus its kill-tree. Dropping it kills the tree.
pub struct TreeChild {
    /// The direct child.
    pub child: Child,
    tree: tree::Tree,
}

impl TreeChild {
    /// Spawn `cmd` as the root of a killable tree.
    pub fn spawn(cmd: &mut Proc) -> std::io::Result<Self> {
        tree::prepare(cmd);
        let mut child = cmd.spawn()?;
        match tree::Tree::adopt(&child) {
            Ok(tree) => Ok(Self { child, tree }),
            Err(e) => {
                let _ = child.kill();
                Err(e)
            }
        }
    }
    /// Kill the child and everything it started, then reap the child.
    pub fn kill(&mut self) -> ExitStatus {
        self.tree.kill();
        self.child.wait().expect("wait after kill")
    }
}

/// Where the worker finds its runtime; everything optional (defaults: exe-relative runtime, no overlay).
#[derive(Debug, Clone, Default)]
pub struct WorkerConfig {
    pub python_home: Option<PathBuf>,
    pub site_updates: Option<PathBuf>,
    /// Extra env for the child (proxy etc.).
    pub env: Vec<(String, String)>,
}

/// A running download worker.
pub struct WorkerHandle {
    proc: TreeChild,
    stdin: Option<ChildStdin>, // kept open: EOF tells the worker its parent is gone
    /// Typed events; the channel closes when the worker's pipes close.
    pub events: Receiver<Event>,
    /// When `spawn_worker` was called (for time-to-ready).
    pub spawned_at: Instant,
}

fn pump(r: impl std::io::Read + Send + 'static, tx: Sender<Event>, stderr: bool) {
    std::thread::spawn(move || {
        for line in BufReader::new(r).lines().map_while(Result::ok) {
            let ev = match (stderr, serde_json::from_str::<Event>(&line)) {
                (false, Ok(ev)) => ev,
                (stderr, _) => Event::Log {
                    level: if stderr { "stderr" } else { "stdout" }.into(),
                    msg: line,
                },
            };
            if tx.send(ev).is_err() {
                break;
            }
        }
    });
}

/// Spawn `exe --engine-worker` for `job` with default config.
pub fn spawn_worker(exe: &std::path::Path, job: &JobSpec) -> std::io::Result<WorkerHandle> {
    spawn_worker_with(exe, job, &Env::default(), &WorkerConfig::default())
}

/// Spawn `exe --engine-worker` for `job`.
pub fn spawn_worker_with(
    exe: &std::path::Path,
    job: &JobSpec,
    env: &Env,
    cfg: &WorkerConfig,
) -> std::io::Result<WorkerHandle> {
    let mut c = Proc::new(exe);
    c.arg("--engine-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(h) = &cfg.python_home {
        c.env("YTDLPD_PYTHON_HOME", h);
    }
    if let Some(s) = &cfg.site_updates {
        c.env("YTDLPD_SITE_UPDATES", s);
    }
    c.envs(cfg.env.iter().map(|(k, v)| (k, v)));
    // partials, cookie copies and aria2c's files go to a folder this user can write, never the
    // install folder the app was started in
    let temp = crate::spec::temp_dir(&job.output_dir, env.temp_dir.as_deref());
    if let Some(t) = &temp {
        c.env("TEMP", t)
            .env("TMP", t)
            .env("TMPDIR", t)
            .current_dir(t);
    }
    let env = &Env {
        temp_dir: temp,
        ..env.clone()
    };
    let spawned_at = Instant::now();
    let mut proc = TreeChild::spawn(&mut c)?;
    let mut stdin = proc.child.stdin.take().expect("piped stdin");
    let line = serde_json::to_string(&Command::Download {
        url: job.url.clone(),
        spec: to_opts(job, env),
    })
    .expect("command serialises");
    writeln!(stdin, "{line}")?;
    stdin.flush()?;
    let (tx, events) = channel();
    pump(
        proc.child.stdout.take().expect("piped stdout"),
        tx.clone(),
        false,
    );
    pump(proc.child.stderr.take().expect("piped stderr"), tx, true);
    Ok(WorkerHandle {
        proc,
        stdin: Some(stdin),
        events,
        spawned_at,
    })
}

impl WorkerHandle {
    /// Move the event receiver out (the handle keeps control: pid, kill, wait). Leaves a closed channel behind.
    pub fn take_events(&mut self) -> Receiver<Event> {
        std::mem::replace(&mut self.events, channel().1)
    }
    /// OS process id of the worker.
    pub fn pid(&self) -> u32 {
        self.proc.child.id()
    }
    /// Ask the worker to exit (graceful; follow with [`kill`](Self::kill) after about 1 s).
    pub fn request_exit(&mut self) {
        if let Some(s) = self.stdin.as_mut() {
            let _ = writeln!(s, r#"{{"cmd":"exit"}}"#).and_then(|()| s.flush());
        }
    }
    /// Kill the worker and all its children (ffmpeg, deno), reap it.
    pub fn kill(&mut self) -> ExitStatus {
        self.stdin = None;
        self.proc.kill()
    }
    /// Wait for the worker to exit on its own.
    pub fn wait(&mut self) -> std::io::Result<ExitStatus> {
        self.stdin = None;
        self.proc.child.wait()
    }
}
