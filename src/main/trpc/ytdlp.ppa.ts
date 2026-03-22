const YtdlpPostprocessors = [
	"FFmpegExtractAudio",
	"FFmpegVideoConvertor",
	"FFmpegVideoRemuxer",
	"FFmpegEmbedSubtitle",
	"FFmpegMetadata",
	"FFmpegMerger",
	"FFmpegFixup",
	"FFmpegFixupStretched",
	"FFmpegFixupM4a",
	"FFmpegFixupM3u8",
	"FFmpegFixupTimestamp",
	"FFmpegCopyStream",
	"FFmpegFixupDuration",
	"FFmpegFixupDuplicateMoov",
	"FFmpegSubtitlesConvertor",
	"FFmpegThumbnailsConvertor",
	"FFmpegConcat",
] as const;
type YtdlpPostprocessorTuple = { key: { key: (typeof YtdlpPostprocessors)[number]; only_multi_video?: boolean; when?: string }; args: { [key: string]: string[] } };
/**
 * Merger+ffmpeg_i1 / o1 are not needed in python worker, only in cli.
 */
const YtdlpPostprocessorArgs = {
	"coub.com": {
		key: { key: "FFmpegConcat", only_multi_video: true, when: "playlist" },
		args: {
			"merger+ffmpeg_i1": ["-stream_loop", "-1"],
			"merger+ffmpeg_o1": ["-shortest", "-shortest_buf_duration", "0"],
		},
	},
} as Record<string, YtdlpPostprocessorTuple>;

export function getYtdlpPostprocessors(url: string) {
	const hostname = new URL(url).hostname;
	if (!hostname || !YtdlpPostprocessorArgs[hostname as keyof typeof YtdlpPostprocessorArgs]?.key) return null;
	return {
		key: YtdlpPostprocessorArgs[hostname as keyof typeof YtdlpPostprocessorArgs].key,
		value: YtdlpPostprocessorArgs[hostname as keyof typeof YtdlpPostprocessorArgs].args,
	};
}
