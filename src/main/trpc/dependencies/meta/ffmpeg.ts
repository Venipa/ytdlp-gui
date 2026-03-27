import { defineDependency } from "./defineDependency";

export const ffmpegDependency = defineDependency({
	key: "ffmpeg",
	name: "FFmpeg",
	description: "Multimedia toolkit used by yt-dlp post-processing.",
	releases: [
		{
			version: "7.1",
			targets: [
				{
					platform: "win32",
					arch: "x64",
					sources: [
						{
							id: "ffmpeg-bundle",
							kind: "archive",
							archiveType: "zip",
							exports: ["**/bin/ffprobe.exe", "**/bin/ffmpeg.exe"],
							url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-win64-lgpl-7.1.zip",
						},
					],
				},
				{
					platform: "linux",
					arch: "x64",
					sources: [
						{
							id: "ffmpeg-bundle",
							kind: "archive",
							archiveType: "tar",
							exports: ["**/bin/ffprobe", "**/bin/ffmpeg"],
							url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-lgpl-7.1.tar.xz",
						},
					],
				},
				{
					platform: "darwin",
					arch: "arm64",
					sources: [
						{
							id: "ffmpeg",
							kind: "binary",
							url: "https://www.osxexperts.net/ffmpeg71arm.zip",
						},
						{
							id: "ffprobe",
							kind: "binary",
							url: "https://www.osxexperts.net/ffprobe71arm.zip",
						},
					],
				},
			],
		},
	],
} as const);
