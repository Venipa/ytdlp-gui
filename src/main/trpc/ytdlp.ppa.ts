const YtdlpPostprocessorArgs = {
	"coub.com": ["Merger+ffmpeg_i1:-stream_loop -1", "Merger+ffmpeg_o1:-shortest -shortest_buf_duration 0"],
} as Record<string, string[]>;

export function getYtdlpPostprocessors(url: string) {
	const hostname = new URL(url).hostname;
	if (!hostname || !YtdlpPostprocessorArgs[hostname as keyof typeof YtdlpPostprocessorArgs]?.length) return null;
	return YtdlpPostprocessorArgs[hostname as keyof typeof YtdlpPostprocessorArgs] as string[];
}
