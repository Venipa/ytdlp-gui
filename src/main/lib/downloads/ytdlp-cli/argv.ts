import { join } from "node:path";
import { appStore } from "@main/stores/app/app.store";
import { getYtdlpCookiesFilePath } from "@main/stores/cookies/cookies.store";
import { dependencyStore } from "@main/trpc/dependencies/store";
import { app } from "electron";
import type { YtdlpOptions, YtdlpPostprocessorOption } from "./types";

export function getYtdlpCachePath(): string {
	return join(app.getPath("userData"), "ytdlp_cache");
}

export const PROGRESS_MARKER = "__ytdlp_progress__";

const PROGRESS_TEMPLATE = `download:{"${PROGRESS_MARKER}":true,"status":"%(progress.status)s","videoId":"%(info.id)s","percent":"%(progress._percent)s","speed":"%(progress.speed|0)s","eta":"%(progress._eta_str)s","fragmentIndex":"%(progress.fragment_index)s","fragmentCount":"%(progress.fragment_count)s"}`;

export function getFfmpegLocation(): string | null {
	const installed = dependencyStore.get("ffmpeg")?.files?.[0];
	if (installed) return installed;
	return appStore.get("ytdlp.ffmpegPath", null as string | null) ?? null;
}

export function getStoreCliArgs(): string[] {
	return appStore.store.ytdlp.cliargs ?? [];
}

export function buildBaseArgs(): string[] {
	const args = ["--ignore-config", "--no-update", "--no-warnings", "--cache-dir", getYtdlpCachePath()];
	const ffmpeg = getFfmpegLocation();
	if (ffmpeg) args.push("--ffmpeg-location", ffmpeg);
	const cookiesFile = getYtdlpCookiesFilePath();
	if (cookiesFile) args.push("--cookies", cookiesFile);
	return args;
}

export function buildExtractArgs(url: string, options?: Partial<YtdlpOptions>): string[] {
	const args = [...buildBaseArgs(), "--dump-single-json"];
	if (options?.windowsfilenames) args.push("--windows-filenames");
	if (options?.format) args.push("-f", options.format);
	if (options?.filename) args.push("-o", options.filename);
	args.push(...(options?.cliargs ?? getStoreCliArgs()), "--", url);
	return args;
}

function appendPostprocessors(args: string[], postprocessors: YtdlpPostprocessorOption[] | undefined): void {
	if (!postprocessors) return;
	for (const postprocessor of postprocessors) {
		if (postprocessor.key === "FFmpegExtractAudio" && postprocessor.preferredcodec) {
			args.push("-x", "--audio-format", postprocessor.preferredcodec);
			continue;
		}
		const whenSuffix = postprocessor.when ? `:when=${postprocessor.when}` : "";
		args.push("--use-postprocessor", `${postprocessor.key}${whenSuffix}`);
	}
}

function appendPostprocessorArgs(args: string[], postprocessorArgs: Record<string, string[]> | undefined): void {
	if (!postprocessorArgs) return;
	for (const [name, values] of Object.entries(postprocessorArgs)) {
		if (!values.length) continue;
		args.push("--postprocessor-args", `${name}:${values.join(" ")}`);
	}
}

export function buildDownloadArgs(url: string, options?: Partial<YtdlpOptions>, infoJsonPath?: string): string[] {
	const args = [...buildBaseArgs(), "--newline", "--progress", "--progress-template", PROGRESS_TEMPLATE];
	if (options?.windowsfilenames) args.push("--windows-filenames");
	if (options?.format) args.push("-f", options.format);
	if (options?.filename) args.push("-o", options.filename);
	if (options?.updatetime === false) args.push("--no-mtime");
	appendPostprocessors(args, options?.postprocessors);
	appendPostprocessorArgs(args, options?.postprocessor_args);
	args.push(...(options?.cliargs ?? getStoreCliArgs()));
	if (infoJsonPath) {
		args.push("--load-info-json", infoJsonPath);
	} else {
		args.push("--", url);
	}
	return args;
}
