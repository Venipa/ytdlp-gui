//! View-local sample data so the screens render before `state` is wired in. The screens read
//! plain data; wiring replaces this module with the state entities.

use domain::{
    DownloadId, DownloadStatus, DownloadSummary, ErrorCode, FormatInfo, MediaKind, PlaylistEntry,
    ProbeResult, ToolKind, ToolSource, ToolState, ToolStatus,
};

/// One list row: the summary plus the live numbers the summary does not carry.
#[derive(Clone, Debug)]
pub struct Row {
    pub summary: DownloadSummary,
    /// Bytes per second while downloading.
    pub speed: Option<f64>,
    /// Seconds left while downloading.
    pub eta: Option<f64>,
}

/// One line of the log view.
#[derive(Clone, Debug)]
pub struct LogLine {
    pub time: &'static str,
    pub level: &'static str,
    pub job: Option<i64>,
    pub text: &'static str,
}

fn summary(id: i64, title: &str, domain: &str, status: DownloadStatus) -> DownloadSummary {
    let mut s = DownloadSummary {
        id: DownloadId(id),
        url: format!("https://{domain}/watch?v=sample{id}"),
        domain: domain.into(),
        extractor: None,
        video_id: Some(format!("sample{id}")),
        title: Some(title.into()),
        uploader: Some("Sample channel".into()),
        title_lc: String::new(),
        uploader_lc: String::new(),
        duration_s: Some(213.),
        thumbnail_url: None,
        thumbnail_path: None,
        kind: MediaKind::Video,
        preset: "video:1080".into(),
        format_label: Some("1080p".into()),
        status,
        status_reason: None,
        phase: None,
        progress: 0.,
        bytes_done: None,
        bytes_total: None,
        file_path: None,
        file_size: None,
        file_missing: false,
        error_code: None,
        error_message: None,
        attempts: 1,
        next_attempt_at: None,
        playlist_id: None,
        playlist_index: None,
        position: id as f64,
        created_at: 0,
        started_at: None,
        finished_at: None,
        updated_at: 0,
    };
    s.refresh_lc();
    s
}

/// Everything the screens show.
pub struct Data {
    pub rows: Vec<Row>,
    pub tools: Vec<ToolStatus>,
    pub logs: Vec<LogLine>,
    pub probe: ProbeResult,
    pub playlist: ProbeResult,
}

impl Data {
    /// A ledger with every row state of the design.
    pub fn sample() -> Self {
        let mut rows = Vec::new();
        let mut add = |mut s: DownloadSummary, speed, eta| {
            s.refresh_lc();
            rows.push(Row {
                summary: s,
                speed,
                eta,
            })
        };
        let mut active = summary(
            131,
            "Song title one",
            "youtube.com",
            DownloadStatus::Downloading,
        );
        active.kind = MediaKind::Audio;
        active.preset = "audio:mp3".into();
        active.format_label = Some("mp3".into());
        active.progress = 0.62;
        active.bytes_done = Some(2_600_000);
        active.bytes_total = Some(4_300_000);
        active.phase = Some("downloading".into());
        add(active, Some(2_411_724.), Some(12.));
        let mut merging = summary(
            132,
            "Lecture 3, graph theory",
            "youtube.com",
            DownloadStatus::Processing,
        );
        merging.progress = 1.;
        merging.phase = Some("merging".into());
        add(merging, None, None);
        let mut paused = summary(
            130,
            "A long lecture recording",
            "vimeo.com",
            DownloadStatus::Paused,
        );
        paused.bytes_total = Some(1_288_490_189);
        paused.progress = 0.31;
        add(paused, None, None);
        let mut queued = summary(133, "Queued clip", "soundcloud.com", DownloadStatus::Queued);
        queued.format_label = None;
        add(queued, None, None);
        let mut done = summary(
            129,
            "Archived clip",
            "youtube.com",
            DownloadStatus::Completed,
        );
        done.format_label = Some("720p".into());
        done.file_size = Some(92_274_688);
        done.finished_at = Some(1);
        add(done, None, None);
        let mut gone = summary(
            128,
            "Never Gonna Give You Up",
            "youtube.com",
            DownloadStatus::Completed,
        );
        gone.file_missing = true;
        gone.file_size = Some(40_000_000);
        add(gone, None, None);
        let mut failed = summary(127, "Broken link", "youtube.com", DownloadStatus::Failed);
        failed.format_label = None;
        failed.error_code = Some(ErrorCode::Login.to_string());
        failed.error_message = Some("Sign in to confirm you are not a bot".into());
        add(failed, None, None);
        let cancelled = summary(
            126,
            "Cancelled one",
            "example.com",
            DownloadStatus::Cancelled,
        );
        add(cancelled, None, None);

        let tool = |kind, version: &str, state, source| ToolStatus {
            kind,
            state,
            source: Some(source),
            path: None,
            version: Some(version.into()),
        };
        let tools = vec![
            tool(
                ToolKind::YtDlp,
                "2026.10.26",
                ToolState::Ready,
                ToolSource::Managed,
            ),
            tool(
                ToolKind::Ffmpeg,
                "9.1",
                ToolState::Downloading {
                    done: 41 << 20,
                    total: Some(88 << 20),
                },
                ToolSource::Managed,
            ),
            tool(
                ToolKind::Ffprobe,
                "9.1",
                ToolState::Checking,
                ToolSource::Managed,
            ),
            tool(ToolKind::Deno, "2.9.8", ToolState::Ready, ToolSource::Path),
            tool(
                ToolKind::Python,
                "3.14.10",
                ToolState::Ready,
                ToolSource::Managed,
            ),
        ];

        let line = |time, level, job, text| LogLine {
            time,
            level,
            job,
            text,
        };
        let logs = vec![
            line(
                "12:01:03",
                "info",
                Some(131),
                "[youtube] Extracting URL: https://youtube.com/watch?v=sample131",
            ),
            line(
                "12:01:04",
                "info",
                Some(131),
                "[download] Destination: Song title one [sample131].mp3",
            ),
            line(
                "12:01:05",
                "warn",
                Some(131),
                "No supported JavaScript runtime could be found.",
            ),
            line(
                "12:01:09",
                "error",
                Some(127),
                "ERROR: Sign in to confirm you are not a bot",
            ),
            line("12:02:00", "info", None, "tools: ffmpeg 9.1 downloading"),
        ];

        let format = |id: &str, ext: &str, size: u64, note: &str| FormatInfo {
            id: id.into(),
            ext: Some(ext.into()),
            filesize: Some(size),
            note: Some(note.into()),
            ..Default::default()
        };
        let video = |id: &str, ext: &str, codec: &str, size, note| FormatInfo {
            width: Some(1920),
            height: Some(1080),
            vcodec: Some(codec.into()),
            ..format(id, ext, size, note)
        };
        let audio = |id: &str, ext: &str, codec: &str, size, note| FormatInfo {
            acodec: Some(codec.into()),
            ..format(id, ext, size, note)
        };
        let probe = ProbeResult {
            url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            title: Some("Never Gonna Give You Up".into()),
            uploader: Some("Rick Astley".into()),
            duration_s: Some(213.),
            formats: vec![
                video("137", "mp4", "avc1", 117_440_512, "1080p"),
                video("248", "webm", "vp9", 77_594_624, "1080p"),
                audio("140", "m4a", "mp4a 128k", 3_565_158, "medium"),
                audio("251", "webm", "opus 160k", 3_879_731, "medium"),
            ],
            ..Default::default()
        };
        let entry = |i: u32, title: &str, secs: f64| PlaylistEntry {
            index: i,
            id: None,
            url: format!("https://youtu.be/lecture{i}"),
            title: Some(title.into()),
            duration_s: Some(secs),
            thumbnail_url: None,
        };
        let playlist = ProbeResult {
            title: Some("Lecture series 2024".into()),
            entries: Some(vec![
                entry(1, "Introduction", 724.),
                entry(2, "Sets and maps", 2890.),
                entry(3, "Already downloaded", 1882.),
            ]),
            ..probe.clone()
        };
        Self {
            rows,
            tools,
            logs,
            probe,
            playlist,
        }
    }
}
