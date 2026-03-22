import { VideoInfo } from "yt-dlp-wrap/types";

export interface YtdlpPyOptions {
	/** Username for authentication purposes. */
	username?: string;
	/** Password for authentication purposes. */
	password?: string;
	/** Password for accessing a video. */
	videopassword?: string;
	/** Adobe Pass multiple-system operator identifier. */
	ap_mso?: string;
	/** Adobe Pass account username. */
	ap_username?: string;
	/** Adobe Pass account password. */
	ap_password?: string;
	/** Use netrc for authentication. */
	usenetrc?: boolean;
	/** Location of the netrc file. */
	netrc_location?: string;
	/** Use a shell command to get credentials. */
	netrc_cmd?: string;
	/** Print additional info to stdout. */
	verbose?: boolean;
	/** Do not print normal messages to stdout. */
	quiet?: boolean;
	/** Do not print warnings. */
	no_warnings?: boolean;
	/** Templates to force-print by lifecycle stage. */
	forceprint?: Record<string, string[]> | string[];
	/** Templates written to files by lifecycle stage. */
	print_to_file?: Record<string, Array<[template: string, filename: string]>>;
	/** Force printing info_dict as JSON. */
	forcejson?: boolean;
	/** Print the whole playlist/video info as one JSON line. */
	dump_single_json?: boolean;
	/** Always write download archive even in simulate/skip modes. */
	force_write_download_archive?: boolean;
	/** Do not download media when true/null policy resolves to simulate. */
	simulate?: boolean | null;
	/** Format selector expression or callback. */
	format?: string | ((ctx: unknown) => unknown);
	/** Allow unplayable formats to be extracted/downloaded. */
	allow_unplayable_formats?: boolean;
	/** Ignore "No video formats" extraction errors. */
	ignore_no_formats_error?: boolean;
	/** Sorting fields used to rank formats. */
	format_sort?: string[];
	/** Force the provided format sort behavior. */
	format_sort_force?: boolean;
	/** Prefer free container formats when quality is equal. */
	prefer_free_formats?: boolean;
	/** Allow merging multiple video streams. */
	allow_multiple_video_streams?: boolean;
	/** Allow merging multiple audio streams. */
	allow_multiple_audio_streams?: boolean;
	/** Whether to verify selected/all/no formats are downloadable. */
	check_formats?: boolean | "selected" | null;
	/** Output path map (home/temp/per-type keys). */
	paths?: Record<string, string>;
	/**
	 * Output filename template(s).
	 * Common placeholders for path/filename building include:
	 * `%(title)s`, `%(id)s`, `%(ext)s`, `%(uploader)s`, `%(uploader_id)s`,
	 * `%(channel)s`, `%(channel_id)s`, `%(playlist)s`, `%(playlist_index)s`,
	 * `%(upload_date>%Y-%m-%d)s`, `%(duration_string)s`, `%(resolution)s`, `%(format_id)s`.
	 * Example: `%(uploader)s/%(playlist)s/%(playlist_index)s - %(title)s.%(ext)s`
	 */
	outtmpl?: Record<string, string> | string;
	/** Placeholder for unavailable metadata fields in templates. */
	outtmpl_na_placeholder?: string;
	/** Restrict filenames (no spaces and '&'). */
	restrictfilenames?: boolean;
	/** Maximum filename length (extension excluded). */
	trim_file_name?: number;
	/** Force Windows-compatible filenames on non-Windows. */
	windowsfilenames?: boolean;
	/** Continue on errors; "only_download" ignores only download errors. */
	ignoreerrors?: boolean | "only_download";
	/** Max allowed failures before skipping playlist remainder. */
	skip_playlist_after_errors?: number;
	/** Regexes of allowed extractor names. */
	allowed_extractors?: string[];
	/** Overwrite policy for media/metadata files. */
	overwrites?: boolean | null;
	/** Playlist index selection expression. */
	playlist_items?: string;
	/** Download playlist entries in random order. */
	playlistrandom?: boolean;
	/** Process playlist entries lazily as received. */
	lazy_playlist?: boolean;
	/** Download only titles matching this expression. */
	matchtitle?: string;
	/** Reject downloads for titles matching this expression. */
	rejecttitle?: string;
	/** Optional logger with debug/warning/error methods. */
	logger?: {
		debug: (message: string) => void;
		warning: (message: string) => void;
		error: (message: string) => void;
	};
	/** Print all logs to stderr instead of stdout. */
	logtostderr?: boolean;
	/** Show progress in the terminal title. */
	consoletitle?: boolean;
	/** Write description to `.description` file. */
	writedescription?: boolean;
	/** Write metadata to `.info.json` file. */
	writeinfojson?: boolean;
	/** Remove internal metadata fields from written infojson. */
	clean_infojson?: boolean;
	/** Extract video comments into metadata. */
	getcomments?: boolean;
	/** Write thumbnail to disk. */
	writethumbnail?: boolean;
	/** Allow write* side files for playlist entries too. */
	allow_playlist_files?: boolean;
	/** Write all thumbnail variants. */
	write_all_thumbnails?: boolean;
	/** Write platform shortcut file (.url/.webloc/.desktop). */
	writelink?: boolean;
	/** Write Windows `.url` shortcut. */
	writeurllink?: boolean;
	/** Write macOS `.webloc` shortcut. */
	writewebloclink?: boolean;
	/** Write Linux `.desktop` shortcut. */
	writedesktoplink?: boolean;
	/** Write subtitle files. */
	writesubtitles?: boolean;
	/** Write auto-generated subtitle files. */
	writeautomaticsub?: boolean;
	/** List available subtitles and exit. */
	listsubtitles?: boolean;
	/** Subtitle format code. */
	subtitlesformat?: string;
	/** Subtitle languages list (supports include/exclude forms). */
	subtitleslangs?: string[];
	/** Keep original media after post-processing. */
	keepvideo?: boolean;
	/** Date range constraint object. */
	daterange?: unknown;
	/** Skip actual media download step. */
	skip_download?: boolean;
	/** Cache directory path, or false to disable filesystem cache. */
	cachedir?: string | false;
	/** Prefer single video over playlist when uncertain. */
	noplaylist?: boolean;
	/** User age limit used for filtering. */
	age_limit?: number;
	/** Minimum view count filter. */
	min_views?: number | null;
	/** Maximum view count filter. */
	max_views?: number | null;
	/** Download archive set or archive filepath. */
	download_archive?: Set<string> | string;
	/** Stop process after hitting an already archived item. */
	break_on_existing?: boolean;
	/** Apply break_on_* behavior per input URL. */
	break_per_url?: boolean;
	/** Cookie file path (read/write). */
	cookiefile?: string;
	/** Browser cookie loading tuple. */
	cookiesfrombrowser?: [browser: string, profile?: string, keyring?: string | null, container?: string];
	/** Allow HTTPS connections without RFC5746 secure renegotiation. */
	legacyserverconnect?: boolean;
	/** Disable SSL certificate verification. */
	nocheckcertificate?: boolean;
	/** Client certificate PEM path. */
	client_certificate?: string;
	/** Private key path for client certificate. */
	client_certificate_key?: string;
	/** Password for encrypted client certificate key. */
	client_certificate_password?: string;
	/** Prefer HTTP over HTTPS when extractor supports it. */
	prefer_insecure?: boolean;
	/** Enable `file://` URLs. */
	enable_file_urls?: boolean;
	/** Custom HTTP headers used for all requests. */
	http_headers?: Record<string, string>;
	/** Proxy server URL. */
	proxy?: string;
	/** Proxy URL used for geo verification requests. */
	geo_verification_proxy?: string;
	/** Socket timeout in seconds. */
	socket_timeout?: number;
	/** Workaround for terminals with bidi text issues. */
	bidi_workaround?: boolean;
	/** Print sent/received HTTP traffic. */
	debug_printtraffic?: boolean;
	/** Prefix used for non-URL input search. */
	default_search?: string;
	/** Override system text encoding. */
	encoding?: string;
	/** Flat extraction mode. */
	extract_flat?: boolean | "in_playlist" | "discard" | "discard_in_playlist";
	/** Wait range for scheduled streams. */
	wait_for_video?: [minSeconds: number, maxSeconds: number];
	/** Postprocessor definitions. */
	postprocessors?: Array<Record<string, unknown> & { key: string; when?: string }>;
	/** Download progress hook callbacks. */
	progress_hooks?: Array<
		(progress: {
			status?: "downloading" | "error" | "finished" | string;
			info_dict?: Record<string, unknown>;
			filename?: string;
			tmpfilename?: string;
			downloaded_bytes?: number;
			total_bytes?: number | null;
			total_bytes_estimate?: number | null;
			elapsed?: number;
			eta?: number | null;
			speed?: number | null;
			fragment_index?: number;
			fragment_count?: number;
			[key: string]: unknown;
		}) => void
	>;
	/** Postprocessor progress hook callbacks. */
	postprocessor_hooks?: Array<
		(progress: {
			status?: "started" | "processing" | "finished" | string;
			postprocessor?: string;
			info_dict?: Record<string, unknown>;
			[key: string]: unknown;
		}) => void
	>;
	/** Allowed output extensions when merging streams. */
	merge_output_format?: string;
	/** Expected final extension after processing. */
	final_ext?: string;
	/** Auto-fix mode for known container/stream issues. */
	fixup?: "never" | "warn" | "detect_or_warn";
	/** Local source IP to bind outgoing requests. */
	source_address?: string;
	/** Client impersonation target object. */
	impersonate?: unknown;
	/** Sleep between extraction requests (seconds). */
	sleep_interval_requests?: number;
	/** Sleep before each download (or lower bound). */
	sleep_interval?: number;
	/** Upper bound for randomized pre-download sleep. */
	max_sleep_interval?: number;
	/** Sleep before each subtitle download (seconds). */
	sleep_interval_subtitles?: number;
	/** List available media formats and exit. */
	listformats?: boolean;
	/** List available thumbnails and exit. */
	list_thumbnails?: boolean;
	/** Match filter callback for per-item accept/reject logic. */
	match_filter?: ((infoDict: unknown) => string | null | undefined) | ((infoDict: unknown, args: { incomplete: boolean }) => string | null | undefined);
	/** Color policy map or global value. */
	color?: string | Record<string, string>;
	/** Enable geo bypass via forged headers. */
	geo_bypass?: boolean;
	/** Geo bypass country code. */
	geo_bypass_country?: string;
	/** Geo bypass IP block in CIDR format. */
	geo_bypass_ip_block?: string;
	/** External downloader executable by protocol. */
	external_downloader?: Record<string, string>;
	/** Compatibility options list. */
	compat_opts?: string[];
	/** Templates for download/postprocess progress lines. */
	progress_template?: Record<string, string>;
	/** Retry sleep callbacks by retry category. */
	retry_sleep_functions?: Record<string, (attempts: number) => number>;
	/** Callback returning section ranges to download. */
	download_ranges?: (infoDict: unknown, ydl: unknown) => Iterable<{ start_time: number; end_time: number; title?: string; index?: number }>;
	/** Re-encode for precise cuts when using ranges. */
	force_keyframes_at_cuts?: boolean;
	/** Do not print progress bar. */
	noprogress?: boolean;
	/** Download livestreams from start when possible. */
	live_from_start?: boolean;
	/** Warn when yt-dlp is older than threshold. */
	warn_when_outdated?: boolean;
	/** Downloader option: avoid `.part` files. */
	nopart?: boolean;
	/** Downloader option: update file modified time. */
	updatetime?: boolean;
	/** Downloader option: I/O buffer size. */
	buffersize?: number;
	/** Downloader option: rate limit (bytes/sec). */
	ratelimit?: number;
	/** Downloader option: throttled rate threshold. */
	throttledratelimit?: number;
	/** Downloader option: minimum file size. */
	min_filesize?: number;
	/** Downloader option: maximum file size. */
	max_filesize?: number;
	/** Downloader option: test mode. */
	test?: boolean;
	/** Downloader option: disable dynamic buffer resize. */
	noresizebuffer?: boolean;
	/** Downloader option: retry count. */
	retries?: number;
	/** Downloader option: file access retry count. */
	file_access_retries?: number;
	/** Downloader option: fragment retry count. */
	fragment_retries?: number;
	/** Downloader option: continue partial downloads. */
	continuedl?: boolean;
	/** Downloader option: use MPEG-TS for HLS. */
	hls_use_mpegts?: boolean;
	/** Downloader option: HTTP chunk size. */
	http_chunk_size?: number;
	/** Downloader option: external downloader args. */
	external_downloader_args?: Record<string, string[]> | string[];
	/** Downloader option: concurrent fragment downloads. */
	concurrent_fragment_downloads?: number;
	/** Downloader option: progress update interval. */
	progress_delta?: number;
	/** FFmpeg binary path or directory. */
	ffmpeg_location?: string;
	/** Extra args for postprocessors/executables. */
	postprocessor_args?: Record<string, string[]> | string[];
	/** Extractor retry count for known extractor errors. */
	extractor_retries?: number;
	/** Whether to process dynamic DASH manifests. */
	dynamic_mpd?: boolean;
	/** Split HLS formats at discontinuities. */
	hls_split_discontinuity?: boolean;
	/** Extractor-specific arguments map. */
	extractor_args?: Record<string, Record<string, string[]>>;
	/** JS runtime enable/config map; null disables default runtime. */
	js_runtimes?: Record<string, { path?: string }> | null;
	/** Allowed remote components list. */
	remote_components?: string[];
	/** Mark videos watched (extractor-dependent). */
	mark_watched?: boolean;
	/** Deprecated: stop when match_filter rejects an item. */
	break_on_reject?: boolean;
	/** Deprecated: force generic extractor usage. */
	force_generic_extractor?: boolean;
	/** Deprecated: playlist start index. */
	playliststart?: number;
	/** Deprecated: playlist end index. */
	playlistend?: number;
	/** Deprecated: reverse playlist order. */
	playlistreverse?: boolean;
	/** Deprecated: force print final URL. */
	forceurl?: boolean;
	/** Deprecated: force print title. */
	forcetitle?: boolean;
	/** Deprecated: force print ID. */
	forceid?: boolean;
	/** Deprecated: force print thumbnail URL. */
	forcethumbnail?: boolean;
	/** Deprecated: force print description. */
	forcedescription?: boolean;
	/** Deprecated: force print final filename. */
	forcefilename?: boolean;
	/** Deprecated: force print duration. */
	forceduration?: boolean;
	/** Deprecated: same as subtitleslangs=['all'] with write subtitles flags. */
	allsubtitles?: boolean;
	/** Deprecated: final per-file post hook callbacks. */
	post_hooks?: Array<(filename: string) => void>;
	/** Deprecated HLS preference option. */
	hls_prefer_native?: boolean | null;
	/** Deprecated alias for no color output. */
	no_color?: boolean;
	/** Deprecated alias for overwrites=false. */
	no_overwrites?: boolean;
	/** Local project option used before Python parse merge. */
	cliargs?: string[];
	/** Optional filename metadata field. */
	filename?: string;
	/** Optional info dictionary. Used for downloading with existing info.
	 * Improves latency by skipping metadata webpage extraction.
	 */
	info?: VideoInfo;
	/** Allow additional unknown yt-dlp options. */
	[key: string]: unknown;
}

export type YtdlpOptions = YtdlpPyOptions;
