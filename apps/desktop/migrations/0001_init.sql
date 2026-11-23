CREATE TABLE playlists (
  id INTEGER PRIMARY KEY,
  url TEXT NOT NULL,
  extractor TEXT,
  remote_id TEXT,                      -- extractor's playlist id
  title TEXT, uploader TEXT,
  entry_count INTEGER,
  created_at INTEGER NOT NULL
);

CREATE TABLE downloads (
  id INTEGER PRIMARY KEY,              -- also the call number shown in the UI
  url TEXT NOT NULL,                   -- as entered
  webpage_url TEXT,
  extractor TEXT,                      -- yt-dlp extractor_key
  video_id TEXT,
  domain TEXT NOT NULL,                -- normalised host, per-domain concurrency
  title TEXT, uploader TEXT, duration_s REAL,
  thumbnail_url TEXT, thumbnail_path TEXT,
  kind TEXT NOT NULL CHECK (kind IN ('video','audio')),
  preset TEXT NOT NULL,                -- 'video:1080' | 'audio:mp3' | 'custom'
  format_spec TEXT NOT NULL,
  format_label TEXT,                   -- what the row shows, e.g. '1080p mp4'
  options_json TEXT NOT NULL,          -- full JobSpec snapshot; retry reproduces it
  status TEXT NOT NULL CHECK (status IN ('queued','probing','downloading','processing','paused','completed','failed','cancelled')),
  status_reason TEXT,                  -- e.g. 'interrupted'
  phase TEXT,                          -- 'merging', 'postprocess:ExtractAudio', ...
  progress REAL NOT NULL DEFAULT 0,
  bytes_done INTEGER, bytes_total INTEGER,
  output_dir TEXT,                     -- final directory at download time
  file_path TEXT, file_size INTEGER,
  file_missing INTEGER NOT NULL DEFAULT 0,
  error_code TEXT, error_message TEXT,
  attempts INTEGER NOT NULL DEFAULT 0,
  next_attempt_at INTEGER,             -- backoff gate, survives restart
  playlist_id INTEGER REFERENCES playlists(id) ON DELETE SET NULL,
  playlist_index INTEGER,
  position REAL NOT NULL,              -- queue order, reorderable
  created_at INTEGER NOT NULL, started_at INTEGER, finished_at INTEGER, updated_at INTEGER NOT NULL
);
CREATE INDEX downloads_created ON downloads (created_at DESC);
CREATE INDEX downloads_queue   ON downloads (status, position);
CREATE INDEX downloads_media   ON downloads (extractor, video_id);  -- "already downloaded" hint

CREATE TABLE download_meta (
  download_id INTEGER PRIMARY KEY REFERENCES downloads(id) ON DELETE CASCADE,
  info_json TEXT NOT NULL              -- trimmed info dict: no heatmap or thumbnails, WITH formats (re-opens Choose formats)
);

CREATE TABLE download_logs (
  id INTEGER PRIMARY KEY,
  download_id INTEGER NOT NULL REFERENCES downloads(id) ON DELETE CASCADE,
  at INTEGER NOT NULL, level TEXT NOT NULL, message TEXT NOT NULL
);
CREATE INDEX download_logs_job ON download_logs (download_id, id);   -- trimmed to the last 500 per job when the job finishes

CREATE TABLE tools (
  name TEXT PRIMARY KEY,
  source TEXT NOT NULL CHECK (source IN ('managed','path','custom')),
  path TEXT NOT NULL, version TEXT, sha256 TEXT, installed_at INTEGER, checked_at INTEGER
);

CREATE TABLE app_state (key TEXT PRIMARY KEY, value TEXT NOT NULL);   -- first_run_done, ytdlp_active_site, ...
