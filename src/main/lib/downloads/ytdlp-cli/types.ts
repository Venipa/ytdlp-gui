import type { VideoInfo } from "yt-dlp-wrap/types";

export interface YtdlpPostprocessorOption {
	key: string;
	preferredcodec?: string;
	only_multi_video?: boolean;
	when?: string;
}

export interface YtdlpOptions {
	format?: string | null;
	filename?: string;
	windowsfilenames?: boolean;
	forcefilename?: boolean;
	updatetime?: boolean;
	cliargs?: string[];
	info?: VideoInfo;
	postprocessors?: YtdlpPostprocessorOption[];
	postprocessor_args?: Record<string, string[]>;
}

export interface YtdlpOutputEvent {
	type: "status" | "download_status" | "completed";
	raw: string;
	extractor: string;
	videoId: string | null;
	message: string;
	requestId: string | null;
	workerId?: string | null;
	percent?: number;
	speed?: string | null;
	eta?: string | null;
	fragmentIndex?: number | null;
	fragmentCount?: number | null;
}
