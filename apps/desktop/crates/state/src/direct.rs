//! Adding a download without the Downloads screen: the browser extension and the screen's
//! Download button share one path, so a site rule or a duplicate means the same in both.
use std::fmt;

use domain::{DownloadId, DownloadStatus, Preset, VideoQuality};
use gpui::{App, Task};

use crate::AppState;

/// Why [`AppState::add_direct`] refused.
#[derive(Debug)]
pub enum AddDirectError {
    /// The download list is still loading; try again in a moment.
    Busy,
    /// The row could not be stored.
    Failed(String),
}

impl fmt::Display for AddDirectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Busy => f.write_str("the download list is still loading"),
            Self::Failed(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for AddDirectError {}

/// The short label a row shows for a preset (`best`, `1080p`, `mp3`).
fn preset_label(preset: Preset) -> String {
    match preset {
        Preset::Video(VideoQuality::Best) => "best".into(),
        Preset::Video(q) => format!("{}p", q.as_str()),
        Preset::Audio(a) => a.as_str().into(),
        Preset::Custom => "custom".into(),
    }
}

/// Links `add_direct` is storing right now: the row only shows up in the list after the insert,
/// so a second call for the same link in that window would queue it twice.
#[derive(Default)]
struct Inflight(std::collections::HashSet<String>);

impl gpui::Global for Inflight {}

impl AppState {
    /// The preset a link is added with when nobody chose one: the first matching site rule's
    /// preset, else the remembered `last_preset`.
    pub fn default_preset(&self, url: &str, cx: &App) -> Preset {
        let s = self.settings.read(cx).get();
        domain::rule_for(&s.site_rules, url)
            .and_then(|(_, rule)| rule.preset)
            .or_else(|| s.last_preset.parse().ok())
            .unwrap_or(Preset::DEFAULT)
    }

    /// Stores and queues one download (`probe` fills in what a probe already read). Resolves with
    /// the new id once the row is stored.
    #[allow(clippy::too_many_arguments)] // one call site per kind of download, each passing all of it
    pub fn enqueue_job(
        &self,
        url: &str,
        preset: Preset,
        format_spec: &str,
        playlist_index: Option<u32>,
        probe: Option<&domain::ProbeResult>,
        label: Option<String>,
        cx: &mut App,
    ) -> Task<anyhow::Result<DownloadId>> {
        let spec = self.job_spec(url, preset, format_spec, playlist_index, cx);
        self.downloads
            .update(cx, |d, cx| d.enqueue(spec, probe, label, cx))
    }

    /// Queues `url` now, with `preset` or [`Self::default_preset`]. A link that already has a row
    /// that is not failed or cancelled adds nothing and resolves with that row and `true`.
    /// Refuses with `Busy` until the list has loaded.
    pub fn add_direct(
        &self,
        url: String,
        preset: Option<Preset>,
        cx: &mut App,
    ) -> Task<Result<(Option<DownloadId>, bool), AddDirectError>> {
        let downloads = self.downloads.read(cx);
        if !downloads.loaded() {
            return Task::ready(Err(AddDirectError::Busy));
        }
        let same = downloads.rows().iter().find(|r| {
            r.url == url && !matches!(r.status, DownloadStatus::Failed | DownloadStatus::Cancelled)
        });
        if let Some(row) = same {
            return Task::ready(Ok((Some(row.id), true)));
        }
        if !cx.default_global::<Inflight>().0.insert(url.clone()) {
            return Task::ready(Ok((None, true)));
        }
        let preset = preset.unwrap_or_else(|| self.default_preset(&url, cx));
        let queued = self.enqueue_job(&url, preset, "", None, None, Some(preset_label(preset)), cx);
        cx.spawn(async move |cx| {
            let stored = queued.await;
            // the row is in the list by now, so the list takes over the duplicate check
            cx.update(|cx| cx.global_mut::<Inflight>().0.remove(&url));
            match stored {
                Ok(id) => Ok((Some(id), false)),
                Err(e) => Err(AddDirectError::Failed(format!("{e:#}"))),
            }
        })
    }
}
