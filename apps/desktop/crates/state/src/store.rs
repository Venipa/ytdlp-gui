//! One writer task owns the database: every persistence call is queued in order, so an update never
//! overtakes the insert it depends on and the gpui thread never waits on sqlite.
use std::future::Future;

use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use storage::Db;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::Io;

type Job = Box<dyn FnOnce(Db) -> BoxFuture<'static, ()> + Send>;

/// Handle to the writer task; cheap to clone.
#[derive(Clone)]
pub struct Store {
    tx: UnboundedSender<Job>,
}

impl Store {
    /// Starts the writer; jobs queue until `open` resolves. When it fails every job is dropped
    /// (reads then return an error).
    pub fn spawn(io: &Io, open: impl Future<Output = Result<Db>> + Send + 'static) -> Store {
        let (tx, mut rx) = unbounded_channel::<Job>();
        io.spawn(async move {
            let db = match open.await {
                Ok(db) => db,
                Err(error) => {
                    log::error!("store: cannot open the database: {error:#}");
                    return;
                }
            };
            while let Some(job) = rx.recv().await {
                job(db.clone()).await;
            }
        });
        Store { tx }
    }

    /// A store over an already open database.
    pub fn with_db(io: &Io, db: Db) -> Store {
        Self::spawn(io, async move { Ok(db) })
    }

    /// Queues a write; failures are logged.
    pub fn write<Fut>(&self, work: impl FnOnce(Db) -> Fut + Send + 'static)
    where
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        let job: Job = Box::new(move |db| {
            Box::pin(async move {
                if let Err(error) = work(db).await {
                    log::error!("store: write failed: {error:#}");
                }
            })
        });
        if self.tx.send(job).is_err() {
            log::warn!("store: write dropped, the database is closed");
        }
    }

    /// Queues a read (ordered after every earlier write) and returns its result.
    pub fn read<T, Fut>(
        &self,
        work: impl FnOnce(Db) -> Fut + Send + 'static,
    ) -> impl Future<Output = Result<T>> + 'static
    where
        T: Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        let (reply, answer) = tokio::sync::oneshot::channel();
        let job: Job = Box::new(move |db| {
            Box::pin(async move {
                reply.send(work(db).await).ok();
            })
        });
        let sent = self
            .tx
            .send(job)
            .map_err(|_| anyhow!("the database is closed"));
        async move {
            sent?;
            answer
                .await
                .map_err(|_| anyhow!("the database is not available"))?
        }
    }

    /// Resolves once every earlier write has run.
    pub fn flush(&self) -> impl Future<Output = ()> + 'static {
        let read = self.read(|_| async { Ok(()) });
        async move {
            read.await.ok();
        }
    }

    /// Checkpoints and closes the database after earlier writes (shutdown).
    pub fn close(&self) -> impl Future<Output = ()> + 'static {
        let read = self.read(|db| async move {
            db.close().await;
            Ok(())
        });
        async move {
            read.await.ok();
        }
    }
}
