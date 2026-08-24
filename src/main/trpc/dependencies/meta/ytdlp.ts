import { defineDependency } from "./defineDependency";

export const ytdlpDependency = defineDependency({
	key: "ytdlp",
	name: "yt-dlp",
	description: "CLI used to extract metadata and download media from yt-dlp/yt-dlp GitHub releases.",
	releases: [
		{
			version: "latest",
			targets: [
				{
					platform: "win32",
					arch: "x64",
					sources: [
						{
							id: "ytdlp",
							kind: "binary",
							url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe",
						},
					],
				},
				{
					platform: "win32",
					arch: "arm64",
					sources: [
						{
							id: "ytdlp",
							kind: "binary",
							url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_arm64.exe",
						},
					],
				},
				{
					platform: "linux",
					arch: "x64",
					sources: [
						{
							id: "ytdlp",
							kind: "binary",
							url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux",
						},
					],
				},
				{
					platform: "linux",
					arch: "arm64",
					sources: [
						{
							id: "ytdlp",
							kind: "binary",
							url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux_aarch64",
						},
					],
				},
				{
					platform: "darwin",
					arch: "arm64",
					sources: [
						{
							id: "ytdlp",
							kind: "binary",
							url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos",
						},
					],
				},
				{
					platform: "darwin",
					arch: "x64",
					sources: [
						{
							id: "ytdlp",
							kind: "binary",
							url: "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos",
						},
					],
				},
			],
		},
	],
} as const);
