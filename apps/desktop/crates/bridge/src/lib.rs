//! Localhost WebSocket the browser extension talks to. Binds 127.0.0.1 only, checks `Origin` and
//! `Host` before upgrading, pairs a browser with a one-time 8-digit code and then authenticates it
//! with a long random token (only its SHA-256 is stored). The app side is the [`Host`] trait.
//! Runs on its own tokio runtime; no gpui.
mod server;
mod store;
mod wire;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::net::TcpListener;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use futures::future::BoxFuture;
use tokio::sync::{Semaphore, broadcast};

pub use wire::{AddReq, AddState, Added, Checked, Kind, PROTO, PresetInfo, WireError};

/// How long a pairing code lives.
const CODE_TTL: Duration = Duration::from_secs(120);

/// What the app does for the extension.
pub trait Host: Send + Sync + 'static {
    /// Is `url` a supported page (and which presets to offer)?
    fn check(&self, url: String) -> BoxFuture<'static, Result<Checked, WireError>>;
    /// Queue `req.url` or hand it to the intake box.
    fn add(&self, req: AddReq) -> BoxFuture<'static, Result<Added, WireError>>;
    /// A browser just paired (toast + OS notification).
    fn paired(&self, device: String);
}

/// Server settings.
pub struct Config {
    /// Folder holding `extension.json`; its path also names the instance.
    pub data_dir: PathBuf,
    /// Ports to try in order (release 47821..=47830, debug 48821..=48830).
    pub ports: RangeInclusive<u16>,
    /// Chrome extension ids allowed in `Origin`. Any `moz-extension://` origin passes (its id is random per install).
    pub allowed_chrome_ids: &'static [&'static str],
    /// App version shown to the extension.
    pub app_version: String,
    /// `ytdlpd` or `ytdlpd.dev`.
    pub label: String,
}

/// A live pairing code.
#[derive(Clone, Debug)]
pub struct PairCode {
    /// Eight digits.
    pub code: String,
    /// When it stops working.
    pub expires: Instant,
}

/// A paired browser as the Settings page lists it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// Token hash (the id for [`Bridge::remove`]).
    pub hash: String,
    /// Device name.
    pub name: String,
    /// Origin pinned at pairing.
    pub origin: String,
    /// Unix seconds.
    pub created: i64,
    /// Unix seconds.
    pub last_seen: i64,
    /// Has an authenticated socket right now.
    pub connected: bool,
}

pub(crate) struct Code {
    pub(crate) code: String,
    pub(crate) expires: Instant,
    tries: u8,
}

#[derive(Clone)]
pub(crate) enum Ctl {
    Revoke(String),
    Stop,
}

pub(crate) struct Inner {
    pub(crate) cfg: Config,
    pub(crate) host: Arc<dyn Host>,
    pub(crate) port: u16,
    pub(crate) instance: String,
    pub(crate) store: Mutex<store::Store>,
    pub(crate) code: Mutex<Option<Code>>,
    /// Connection id -> token hash once authenticated.
    pub(crate) conns: Mutex<HashMap<u64, Option<String>>>,
    pub(crate) ctl: broadcast::Sender<Ctl>,
    pub(crate) slots: Arc<Semaphore>,
    pub(crate) next_conn: AtomicU64,
}

/// Lock that survives a poisoned mutex (a panicked session must not kill pairing).
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    /// Takes the live code if `given` matches; a wrong guess counts toward the 3-try burn.
    pub(crate) fn redeem(&self, given: &str) -> bool {
        let mut g = lock(&self.code);
        let Some(c) = g.as_mut().filter(|c| Instant::now() < c.expires) else {
            *g = None;
            return false;
        };
        if c.code == given {
            *g = None;
            return true;
        }
        c.tries += 1;
        if c.tries >= 3 {
            *g = None;
        }
        false
    }
}

/// Handle on the running server. Dropping it stops the server.
pub struct Bridge {
    inner: Arc<Inner>,
    rt: Option<tokio::runtime::Runtime>,
}

/// FNV-1a of the data dir, the same stable hash the single-instance socket name uses.
fn instance_id(dir: &std::path::Path) -> String {
    let h = dir
        .to_string_lossy()
        .bytes()
        .fold(0xcbf29ce484222325u64, |h, b| {
            (h ^ u64::from(b)).wrapping_mul(0x100000001b3)
        });
    format!("{h:016x}")
}

/// Binds the first free port of `cfg.ports` on 127.0.0.1 and starts serving.
pub fn start(cfg: Config, host: Arc<dyn Host>) -> Result<Bridge> {
    let (listener, port) = cfg
        .ports
        .clone()
        .find_map(|p| TcpListener::bind(("127.0.0.1", p)).ok().map(|l| (l, p)))
        .context("cannot bind a port for the browser extension")?;
    listener
        .set_nonblocking(true)
        .context("cannot make the listener non-blocking")?;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("bridge")
        .enable_all()
        .build()
        .context("cannot start the bridge runtime")?;
    let (ctl, _) = broadcast::channel(16);
    let inner = Arc::new(Inner {
        instance: instance_id(&cfg.data_dir),
        store: Mutex::new(store::Store::load(&cfg.data_dir)),
        cfg,
        host,
        port,
        code: Mutex::new(None),
        conns: Mutex::new(HashMap::new()),
        ctl,
        slots: Arc::new(Semaphore::new(server::MAX_CONNECTIONS)),
        next_conn: AtomicU64::new(0),
    });
    let listener = {
        let _g = rt.enter();
        tokio::net::TcpListener::from_std(listener).context("cannot listen")?
    };
    rt.spawn(server::accept_loop(inner.clone(), listener));
    log::info!("bridge: listening on 127.0.0.1:{port}");
    Ok(Bridge {
        inner,
        rt: Some(rt),
    })
}

impl Bridge {
    /// The port in use.
    pub fn port(&self) -> u16 {
        self.inner.port
    }

    /// Makes a new one-time code (replacing any live one, which also resets its try counter).
    pub fn new_code(&self) -> PairCode {
        // shortcut: modulo bias of 1e8 over 2^64 is below 1e-11, irrelevant for a 120 s code
        let mut b = [0u8; 8];
        getrandom::fill(&mut b).expect("the OS random source works");
        let code = format!("{:08}", u64::from_le_bytes(b) % 100_000_000);
        let expires = Instant::now() + CODE_TTL;
        *lock(&self.inner.code) = Some(Code {
            code: code.clone(),
            expires,
            tries: 0,
        });
        PairCode { code, expires }
    }

    /// A code is open: made, not yet used, burned or expired.
    pub fn code_live(&self) -> bool {
        lock(&self.inner.code)
            .as_ref()
            .is_some_and(|c| Instant::now() < c.expires)
    }

    /// Drops the live code (the pairing panel closed).
    pub fn cancel_code(&self) {
        *lock(&self.inner.code) = None;
    }

    /// Paired browsers.
    pub fn devices(&self) -> Vec<Device> {
        let conns = lock(&self.inner.conns);
        lock(&self.inner.store)
            .devices
            .iter()
            .map(|d| Device {
                connected: conns.values().any(|h| h.as_deref() == Some(&d.hash)),
                hash: d.hash.clone(),
                name: d.name.clone(),
                origin: d.origin.clone(),
                created: d.created,
                last_seen: d.last_seen,
            })
            .collect()
    }

    /// Unpairs the device with this token hash and closes its sockets with 4403.
    pub fn remove(&self, token_hash: &str) {
        {
            let mut s = lock(&self.inner.store);
            s.devices.retain(|d| d.hash != token_hash);
            if let Err(e) = s.save() {
                log::error!("bridge: {e:#}");
            }
        }
        self.inner.ctl.send(Ctl::Revoke(token_hash.into())).ok();
    }

    /// Closes every socket (1001) and stops listening. Blocks up to half a second so the close
    /// frames flush; call it from a plain thread, not from async code.
    pub fn stop(mut self) {
        self.inner.ctl.send(Ctl::Stop).ok();
        // sessions send their close frame and leave `conns`; dropping the runtime earlier would cut them off
        let until = Instant::now() + Duration::from_millis(500);
        while !lock(&self.inner.conns).is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Some(rt) = self.rt.take() {
            rt.shutdown_timeout(Duration::from_millis(100));
        }
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        self.inner.ctl.send(Ctl::Stop).ok();
        if let Some(rt) = self.rt.take() {
            rt.shutdown_background();
        }
    }
}
