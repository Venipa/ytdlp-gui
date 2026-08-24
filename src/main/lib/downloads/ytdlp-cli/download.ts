import { spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { mkdtemp, writeFile, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createInterface } from "node:readline";
import { EventEmitter } from "node:events";
import { appStore } from "@main/stores/app/app.store";
import { createLogger } from "@shared/logger";
import { PROGRESS_MARKER, buildDownloadArgs, getYtdlpCachePath } from "./argv";
import { getYtdlpBinaryPath, hasYtdlpBinary } from "./binary";
import type { YtdlpOptions, YtdlpOutputEvent } from "./types";

const IN_PROGRESS_PERCENT_MAX = 99.9;
const log = createLogger("ytdlp-download");

const outputEmitter = new EventEmitter();
let activeDownloads = 0;
const downloadWaiters: Array<() => void> = [];

function getMaxConcurrency(): number {
	const raw = appStore.store.features.concurrentDownloads;
	if (!Number.isFinite(raw) || raw < 1) return 1;
	return Math.floor(raw);
}

export function getActiveDownloadCount(): number {
	return activeDownloads;
}

export function onYtdlpDownloadOutput(listener: (event: YtdlpOutputEvent) => void): () => void {
	outputEmitter.on("output", listener);
	return () => {
		outputEmitter.off("output", listener);
	};
}

async function acquireDownloadSlot(): Promise<void> {
	if (activeDownloads < getMaxConcurrency()) {
		activeDownloads += 1;
		return;
	}
	await new Promise<void>((resolve) => {
		downloadWaiters.push(resolve);
	});
	activeDownloads += 1;
}

function releaseDownloadSlot(): void {
	activeDownloads = Math.max(0, activeDownloads - 1);
	const next = downloadWaiters.shift();
	if (next) next();
}

function parseNumber(value: unknown): number | null {
	if (typeof value === "number" && Number.isFinite(value)) return value;
	if (typeof value !== "string") return null;
	const parsed = Number.parseFloat(value.trim());
	return Number.isFinite(parsed) ? parsed : null;
}

function parseInteger(value: unknown): number | null {
	const parsed = parseNumber(value);
	if (parsed === null) return null;
	return Math.trunc(parsed);
}

function computeFragmentOverallPercent(percent: number | null, fragmentIndex: number | null, fragmentCount: number | null): number | null {
	if (percent === null || fragmentIndex === null || fragmentCount === null || fragmentCount <= 0) return null;
	const currentIndex = Math.max(1, Math.min(fragmentIndex, fragmentCount));
	const completedFragments = currentIndex - 1;
	return Math.max(0, Math.min(100, ((completedFragments + percent / 100) / fragmentCount) * 100));
}

function normalizeInProgressPercent(percent: number | null): number | undefined {
	if (percent === null) return undefined;
	const clamped = Math.max(0, Math.min(100, percent));
	return clamped >= 100 ? IN_PROGRESS_PERCENT_MAX : clamped;
}

function emitProgress(requestId: string, payload: Record<string, unknown>): void {
	const status = typeof payload.status === "string" ? payload.status : "";
	const videoId = typeof payload.videoId === "string" ? payload.videoId : null;
	const speed = typeof payload.speed === "string" ? payload.speed : null;
	const eta = typeof payload.eta === "string" ? payload.eta : null;
	const fragmentIndex = parseInteger(payload.fragmentIndex);
	const fragmentCount = parseInteger(payload.fragmentCount);
	const parsedPercent = parseNumber(payload.percent);
	const computedPercent = computeFragmentOverallPercent(parsedPercent, fragmentIndex, fragmentCount) ?? parsedPercent;
	const line =
		typeof payload.line === "string"
			? payload.line
			: `[download] ${videoId ?? "unknown"}: ${status || "progress"}`;
	if (status === "finished") {
		outputEmitter.emit("output", {
			type: "completed",
			raw: line,
			extractor: "download",
			videoId,
			message: line,
			requestId,
			percent: 100,
		} satisfies YtdlpOutputEvent);
		return;
	}
	if (status === "downloading") {
		outputEmitter.emit("output", {
			type: "download_status",
			raw: line,
			extractor: "download",
			videoId,
			message: line,
			requestId,
			percent: normalizeInProgressPercent(computedPercent),
			speed,
			eta,
			fragmentIndex,
			fragmentCount,
		} satisfies YtdlpOutputEvent);
	}
}

function tryParseProgressLine(line: string): Record<string, unknown> | null {
	const trimmed = line.trim();
	const jsonStart = trimmed.indexOf("{");
	if (jsonStart < 0) return null;
	try {
		const parsed = JSON.parse(trimmed.slice(jsonStart)) as Record<string, unknown>;
		if (parsed[PROGRESS_MARKER] === true) return parsed;
		return null;
	} catch {
		return null;
	}
}

function genRequestId(): string {
	return Math.random().toString(36).slice(2, 11);
}

export async function runYtdlpDownload(
	url: string,
	options?: Partial<YtdlpOptions>,
	hooks?: { onRequestCreated?: (requestId: string) => void; signal?: AbortSignal },
): Promise<void> {
	if (!hasYtdlpBinary()) {
		throw new Error("yt-dlp binary is missing");
	}
	if (hooks?.signal?.aborted) {
		throw new Error("cancelled by user");
	}
	const requestId = genRequestId();
	hooks?.onRequestCreated?.(requestId);
	await acquireDownloadSlot();
	const cacheDir = getYtdlpCachePath();
	mkdirSync(cacheDir, { recursive: true });
	let infoDir: string | null = null;
	let infoJsonPath: string | undefined;
	try {
		if (options?.info) {
			infoDir = await mkdtemp(join(tmpdir(), "ytdlp-info-"));
			infoJsonPath = join(infoDir, "info.json");
			await writeFile(infoJsonPath, JSON.stringify(options.info), "utf8");
		}
		const args = buildDownloadArgs(url, options, infoJsonPath);
		await new Promise<void>((resolve, reject) => {
			const child = spawn(getYtdlpBinaryPath(), args, {
				stdio: ["ignore", "pipe", "pipe"],
				windowsHide: true,
				cwd: cacheDir,
			});
			let stderr = "";
			let settled = false;
			const onAbort = () => {
				if (!child.killed) child.kill();
			};
			hooks?.signal?.addEventListener("abort", onAbort, { once: true });
			createInterface({ input: child.stdout, crlfDelay: Infinity }).on("line", (line) => {
				const progress = tryParseProgressLine(line);
				if (progress) emitProgress(requestId, progress);
			});
			child.stderr.setEncoding("utf8");
			child.stderr.on("data", (chunk: string) => {
				stderr += chunk;
				if (stderr.length > 32_000) stderr = stderr.slice(-16_000);
			});
			child.on("error", (error) => {
				if (settled) return;
				settled = true;
				hooks?.signal?.removeEventListener("abort", onAbort);
				reject(error);
			});
			child.on("close", (code) => {
				if (settled) return;
				settled = true;
				hooks?.signal?.removeEventListener("abort", onAbort);
				if (hooks?.signal?.aborted) {
					reject(new Error("cancelled by user"));
					return;
				}
				if (code === 0) {
					outputEmitter.emit("output", {
						type: "completed",
						raw: "Download completed",
						extractor: "download",
						videoId: typeof options?.info?.id === "string" ? options.info.id : null,
						message: "Download completed",
						requestId,
						percent: 100,
					} satisfies YtdlpOutputEvent);
					resolve();
					return;
				}
				reject(new Error(stderr.trim() || `yt-dlp exited with code ${code ?? "null"}`));
			});
		});
	} finally {
		releaseDownloadSlot();
		if (infoDir) {
			await rm(infoDir, { recursive: true, force: true }).catch((error) => {
				log.warn("Failed to remove info json temp dir", { error });
			});
		}
	}
}
