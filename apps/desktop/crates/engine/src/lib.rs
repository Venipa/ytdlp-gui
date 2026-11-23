//! yt-dlp engine: embedded CPython (PyO3), worker process, JSON-lines IPC, presets, error classification. No gpui.
//!
//! Host process: [`runtime::init`] + [`probe::probe`] (in-process), [`host::spawn_worker`] for downloads.
//! Worker process (`<exe> --engine-worker`): [`worker::run_worker`].
pub mod backend;
pub mod classify;
pub mod host;
pub mod ipc;
pub mod partials;
pub mod probe;
pub mod runtime;
pub mod spec;
pub mod worker;

pub use backend::{EngineBackend, EngineConfig};
pub use classify::classify;
pub use host::{WorkerConfig, WorkerHandle, spawn_worker};
pub use ipc::{Command, Event};
pub use probe::probe;
