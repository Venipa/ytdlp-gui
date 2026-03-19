export type YtdlpOptions = {
	format?: string;
	outtmpl?: string;
	postprocessors?: {
		key?: string;
		preferredcodec?: string;
	}[];
	updatetime?: boolean;
} & Record<string, unknown>;
