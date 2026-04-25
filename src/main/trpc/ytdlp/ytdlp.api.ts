import { existsSync, rmSync, statSync } from "fs";
import { YtdlpOptions } from "@main/lib/downloads/ytdlp-service/ytdlp-options";
import platform from "@main/lib/system/platform";
import { appStore } from "@main/stores/app/app.store";
import { db } from "@main/stores/database/app-database";
import { SelectDownload, queries } from "@main/stores/database/app-database.helpers";
import { downloads } from "@main/stores/database/app-database.schema";
import { getYtdlpPostprocessors } from "@main/trpc/ytdlp/ytdlp.ppa";
import { getDownloadPathWithFilename, getOuttmpl, sanitizeFilename, sanitizeId } from "@main/trpc/ytdlp/ytdlp.utils";
import { logger } from "@shared/logger";
import queuePromise from "@shared/promises/helper";
import { TRPCError } from "@trpc/server";
import { observable } from "@trpc/server/observable";
import { asc, desc, sql } from "drizzle-orm";
import { cloneDeep, omit } from "lodash-es";
import { VideoInfo } from "yt-dlp-wrap/types";
import { YTDLDownloadStatus, YTDLItem, YTDLMediaType, YTDLStatus } from "ytdlp-gui/types";
import { z } from "zod";
import { publicProcedure, router } from "../core/trpc";
import { pushLogToClient } from "../events/events.ee";
import { ytdl } from "./ytdlp.core";
import { pushToastToClient, ytdlpEvents } from "./ytdlp.ee";

const AUDIO_MEDIA_TYPES = ["audio-bestaudio", "audio-mp3", "audio-m4a", "audio-opus"] as const;
const LIST_SORT_KEYS = ["created", "title", "source", "state", "filesize", "type", "finishedAt"] as const;
const LIST_SORT_DIRECTIONS = ["asc", "desc"] as const;

const MEDIA_TYPE_TO_FORMAT: Record<YTDLMediaType, string | null> = {
	auto: null,
	"video-best": "bestvideo*+bestaudio/best",
	"video-4k": "bestvideo*[height<=2160]+bestaudio/best",
	"video-2k": "bestvideo*[height<=2048]+bestaudio/best",
	"video-1440p": "bestvideo*[height<=1440]+bestaudio/best",
	"video-1080p": "bestvideo*[height<=1080]+bestaudio/best",
	"video-720p": "bestvideo*[height<=720]+bestaudio/best",
	"video-480p": "bestvideo*[height<=480]+bestaudio/best",
	"audio-bestaudio": "bestaudio/best",
	"audio-mp3": "bestaudio/best",
	"audio-m4a": "bestaudio[ext=m4a]/bestaudio/best",
	"audio-opus": "bestaudio[acodec*=opus]/bestaudio/best",
};

const parseSpeedToBytesPerSecond = (speed: string): number => {
	// Matches patterns like "1234KiB/s", "12.3MiB/s", "567B/s", etc.
	const match = speed.match(/^(\d+(?:\.\d+)?)([KMGT]?i?B)\/s$/i);
	if (!match) return 0;
	const value = parseFloat(match[1]);
	const unit = match[2].toUpperCase();

	const unitMultipliers: Record<string, number> = {
		B: 1,
		KB: 1000,
		KIB: 1024,
		MB: 1000 * 1000,
		MIB: 1024 * 1024,
		GB: 1000 * 1000 * 1000,
		GIB: 1024 * 1024 * 1024,
		TB: 1000 * 1000 * 1000 * 1000,
		TIB: 1024 * 1024 * 1024 * 1024,
	};

	const multiplier = unitMultipliers[unit] ?? 1;
	return Math.round(value * multiplier);
};

function isAudioMediaType(type: YTDLMediaType): boolean {
	return AUDIO_MEDIA_TYPES.includes(type as (typeof AUDIO_MEDIA_TYPES)[number]);
}

class DownloadQueueManager {
	private activeDownloads = new Map<number, AbortController>();
	private finishedDownloads = new Map<string, "success" | "error" | "cancelled">();
	private processingUrls = new Set<string>();
	private metaCache = new Map<string, VideoInfo>();
	private progressState = new Map<number, { percent: number }>();

	private normalizeProgressValue(value: unknown): number | null {
		if (typeof value === "number" && Number.isFinite(value)) return value;
		if (typeof value === "string") {
			const parsed = Number.parseFloat(value);
			return Number.isFinite(parsed) ? parsed : null;
		}
		return null;
	}

	private getFragmentFloorPercent(fragmentIndex: unknown, fragmentCount: unknown): number {
		const index = this.normalizeProgressValue(fragmentIndex);
		const count = this.normalizeProgressValue(fragmentCount);
		if (!index || !count || count <= 0) return 0;
		const completedFragments = Math.max(0, index - 1);
		return Math.min(100, (completedFragments / count) * 100);
	}

	private getStableDownloadPercent(
		downloadId: number,
		incomingPercent: unknown,
		fragmentIndex?: unknown,
		fragmentCount?: unknown,
	): number {
		const previous = this.progressState.get(downloadId)?.percent ?? 0;
		const rawPercent = this.normalizeProgressValue(incomingPercent) ?? previous;
		const fragmentFloor = this.getFragmentFloorPercent(fragmentIndex, fragmentCount);
		const monotonicPercent = Math.max(previous, rawPercent, fragmentFloor);
		const clampedPercent = Math.min(100, Math.max(0, monotonicPercent));
		this.progressState.set(downloadId, { percent: clampedPercent });
		return clampedPercent;
	}

	private clearDownloadProgress(downloadId: number): void {
		this.progressState.delete(downloadId);
	}

	async addToQueue(urls: string[], type: YTDLMediaType = "auto") {
		const uniqueUrls = urls.filter((url) => !this.processingUrls.has(url));
		if (uniqueUrls.length === 0) return [];

		uniqueUrls.forEach((url) => this.processingUrls.add(url));
		const dbEntries = await this.addToDatabase(uniqueUrls, type);

		const files = await queuePromise(dbEntries.map((u) => () => this.checkMetadata(u).catch(() => null))).then(
			(files) => files.filter((s) => !!s),
		);

		this.startDownloads(files);
		return files;
	}

	private startDownloads(items: Array<{ dbFile: SelectDownload; videoInfo: VideoInfo }>): void {
		if (!items.length) return;
		for (const item of items) {
			void this.processDownload(item.dbFile, item.videoInfo).catch((err) => {
				log.error("failed to download media", err);
			});
		}
	}

	private async addToDatabase(urls: string[], type: YTDLMediaType = "auto"): Promise<SelectDownload[]> {
		const validUrls = urls.filter((url) => typeof url === "string" && /^https/gi.test(url));
		if (validUrls.length === 0) return [];

		const batchItems = validUrls.map((url) =>
			queries.downloads.createDownload({
				filepath: "",
				filesize: 0,
				meta: {} as any,
				metaId: "",
				source: new URL(url).hostname,
				title: url,
				url,
				state: "queued",
				type: type ?? null,
				error: null,
				retryCount: 0,
			}),
		);

		const items = await db.batch(batchItems as any).then((s) => s.map(([item]) => item));
		ytdlpEvents.emit("list", items);
		log.debug("addToDatabase", { items });
		return items as SelectDownload[];
	}

	private async checkMetadata(dbFile: SelectDownload): Promise<{ dbFile: SelectDownload; videoInfo: VideoInfo }> {
		const { url } = dbFile;
		log.debug("meta", `checking metadata for url`, url, dbFile);

		if (!url) throw new Error("Invalid url format");

		ytdlpEvents.emit("status", { action: "getVideoInfo", state: "progressing" });

		let dbFileRecord = await queries.downloads.findDownloadById(dbFile.id);
		if (!dbFileRecord) throw new Error("Entry has been not found or has been removed");

		// Check for existing metadata
		const existingDbFile = await queries.downloads.findDownloadByExactUrl(url, dbFile.id);
		if (existingDbFile?.metaId) {
			return this.handleExistingMetadata(dbFileRecord, existingDbFile);
		}

		return this.fetchNewMetadata(dbFileRecord);
	}

	private async handleExistingMetadata(
		dbFile: SelectDownload,
		existingDbFile: SelectDownload,
	): Promise<{ dbFile: SelectDownload; videoInfo: VideoInfo }> {
		if (!existingDbFile.meta) {
			throw new Error("Existing file metadata is missing");
		}

		Object.assign(dbFile, {
			metaId: existingDbFile.metaId,
			meta: existingDbFile.meta,
			filesize: dbFile.filesize || existingDbFile.filesize,
			source: new URL(dbFile.url).hostname,
			state: "fetching_meta",
			title: existingDbFile.title,
			type: dbFile.type ?? "auto",
			error: null,
			retryCount: 0,
		});
		await this.updateDownloadEntry(dbFile);
		return await this.fetchNewMetadata(dbFile);
		// return { dbFile, videoInfo: existingDbFile.meta as VideoInfo };
	}

	private async fetchNewMetadata(dbFile: SelectDownload): Promise<{ dbFile: SelectDownload; videoInfo: VideoInfo }> {
		const { url } = dbFile;
		let videoInfo: VideoInfo | null = null;
		const hostname = new URL(url).hostname.replace(/^www\./, "").toLowerCase();
		const outtmpl = getOuttmpl().replace("%(source)s", sanitizeFilename(hostname));
		try {
			videoInfo = await ytdl.extractInfo(url, {
				...(isAudioMediaType((dbFile.type ?? "auto") as YTDLMediaType) && { format: "bestaudio/best" }),
				windowsfilenames: platform.isWindows,
				forcefilename: true,
				filename: outtmpl,
			});
			// fallback to default filename if not set
			if (!videoInfo.filename) {
				videoInfo.filename = `[${dbFile.source}_${sanitizeId(videoInfo.id)}] ${sanitizeFilename(videoInfo.title)}.${videoInfo.ext}`;
			}
			log.debug("fetchNewMetadata", { videoInfoId: videoInfo.id });
		} catch (videoInfoError) {
			log.error("extractInfo", videoInfoError);
		}

		if (!videoInfo) {
			await this.deleteDownloadItem(dbFile);
			const videoNotFoundMessage = "URL not supported, video not found or authentication required";
			pushToastToClient(`${dbFile.title}`, "error", videoNotFoundMessage);
			throw new TRPCError({
				code: "NOT_FOUND",
				message: videoNotFoundMessage,
			});
		}
		// if (!videoInfo.filename) {
		// 	videoInfo.filename = `[${dbFile.source}_${sanitizeId(videoInfo.id)}] ${sanitizeFilename(videoInfo.title)}.${videoInfo.ext}`;
		// }
		const trimmedVideoInfo = this.trimVideoInfo(videoInfo);
		dbFile.meta = trimmedVideoInfo;
		dbFile.metaId = videoInfo.id;
		dbFile.state = "fetching_meta";
		dbFile.source = new URL(url).hostname;
		dbFile.filepath = getDownloadPathWithFilename(videoInfo.filename);

		pushLogToClient(`[${dbFile.id}=${dbFile.metaId}] added new download: ${dbFile.title}`, "info");

		const updatedDbFile = await this.updateDownloadEntry(dbFile);
		return { dbFile: updatedDbFile, videoInfo: trimmedVideoInfo };
	}

	private trimVideoInfo(videoInfo: VideoInfo): VideoInfo {
		return cloneDeep(omit(videoInfo, "formats", "thumbnails", "automatic_captions", "heatmap")) as VideoInfo;
	}

	private async deleteDownloadItem(dbFile: SelectDownload) {
		const result = await queries.downloads.deleteDownload(dbFile.id);
		if (result.length) {
			dbFile.state = "deleted";
			ytdlpEvents.emit("list", [dbFile]);
			ytdlpEvents.emit("status", {
				id: dbFile.id,
				action: "download",
				data: null,
				state: "deleted",
			});
		}
	}

	async processDownload(dbFile: SelectDownload, videoInfo: VideoInfo) {
		if (!ytdl.currentDownloadPath) {
			throw new TRPCError({
				code: "NOT_FOUND",
				message: "YTDLP has not been found, make sure the app is running with sufficient permission.",
			});
		}

		const controller = new AbortController();
		this.activeDownloads.set(dbFile.id, controller);

		try {
			await this.prepareDownload(dbFile, videoInfo);
			await this.executeDownload(dbFile, videoInfo, controller);
			await this.finalizeDownload(dbFile);
		} catch (error) {
			await this.handleDownloadError(dbFile, error);
		} finally {
			this.cleanupDownload(dbFile);
		}
	}

	private async prepareDownload(dbFile: SelectDownload, videoInfo: VideoInfo) {
		dbFile.meta = this.trimVideoInfo(videoInfo);
		dbFile.metaId = videoInfo.id;
		dbFile.title = videoInfo.title;
		dbFile.state = "downloading";
		dbFile.filesize = videoInfo.filesize_approx || videoInfo.filesize || 0;
		if (!dbFile.type || dbFile.type === "auto") {
			dbFile.type = videoInfo._type?.toLowerCase() ?? "auto";
		}
		await this.updateDownloadEntry(dbFile);
	}

	private async executeDownload(dbFile: SelectDownload, videoInfo: VideoInfo, controller: AbortController) {
		const settings = appStore.store.ytdlp;
		const downloadPath = dbFile.filepath ?? getDownloadPathWithFilename(videoInfo.filename);
		const selectedMediaType = (dbFile.type ?? "auto") as YTDLMediaType;
		const selectedFormat = MEDIA_TYPE_TO_FORMAT[selectedMediaType] ?? MEDIA_TYPE_TO_FORMAT["video-best"]!;
		const ytdlpOptions: YtdlpOptions = {
			format: selectedFormat,
			filename: downloadPath,
			windowsfilenames: platform.isWindows,
		};
		const cliargs = settings?.cliargs ?? [];

		if (selectedMediaType === "audio-mp3") {
			ytdlpOptions.postprocessors = [
				{
					key: "FFmpegExtractAudio",
					preferredcodec: "mp3",
				},
			];
		}
		const postprocessors = getYtdlpPostprocessors(dbFile.url);
		if (postprocessors) {
			ytdlpOptions.postprocessors = [postprocessors.key];
			ytdlpOptions.postprocessor_args = postprocessors.value;
		}
		if (settings.flags?.nomtime) {
			ytdlpOptions.updatetime = false;
		}

		if (controller.signal.aborted) {
			throw new Error("cancelled by user");
		}

		let activeRequestId: string | null = null;
		const stopOutputListener = ytdl.onOutput((event) => {
			if (activeRequestId && event.requestId && event.requestId !== activeRequestId) {
				return;
			}
			if (!activeRequestId && event.requestId) {
				activeRequestId = event.requestId;
			}
			if (event.videoId && dbFile.metaId && event.videoId !== dbFile.metaId) {
				return;
			}

			if (event.type === "download_status") {
				const stablePercent = this.getStableDownloadPercent(
					dbFile.id,
					event.percent,
					event.fragmentIndex,
					event.fragmentCount,
				);
				ytdlpEvents.emit("status", {
					id: dbFile.id,
					action: "download",
					data: event,
					state: "progressing",
				});
				ytdlpEvents.emit("download", {
					id: dbFile.id,
					percent: stablePercent,
					speed: parseSpeedToBytesPerSecond(event.speed ?? "") ?? 0,
					eta: event.eta ?? "",
					fragmentIndex: event.fragmentIndex,
					fragmentCount: event.fragmentCount,
					message: event.message,
				});
				return;
			}

			if (event.type === "completed") {
				this.clearDownloadProgress(dbFile.id);
				ytdlpEvents.emit("status", {
					id: dbFile.id,
					action: "download",
					data: event,
					state: "done",
				});
				ytdlpEvents.emit("download", { id: dbFile.id, percent: 100, message: event.message });
				return;
			}

			ytdlpEvents.emit("status", {
				id: dbFile.id,
				action: "download",
				data: event,
				state: "progressing",
			});
		});
		ytdlpOptions.filename = dbFile.filepath;
		ytdlpOptions.windowsfilenames = platform.isWindows;
		ytdlpOptions.info = videoInfo;
		ytdlpOptions.cliargs = cliargs;
		ytdlpEvents.emit("status", {
			id: dbFile.id,
			action: "download",
			data: videoInfo,
			state: "progressing",
		});
		log.info("executeDownload", { filename: videoInfo.filename, downloadPath, selectedMediaType, selectedFormat });
		this.updateDownloadEntry(dbFile);
		try {
			await ytdl.downloadWithOutput(dbFile.url, ytdlpOptions, (requestId) => {
				activeRequestId = requestId;
			});
		} finally {
			stopOutputListener();
		}
	}

	private async finalizeDownload(dbFile: SelectDownload) {
		if (!dbFile.error) {
			if (dbFile.type === "audio") dbFile.filepath = dbFile.filepath + ".mp3";
			try {
				const fileStats = statSync(dbFile.filepath);
				dbFile.filesize = fileStats.size;
				dbFile.state = "completed";
				this.finishedDownloads.set(dbFile.url, "success");
				pushLogToClient(`[${dbFile.id}=${dbFile.metaId}] finished download: ${dbFile.title}`, "success");
			} catch (error) {
				log.error("failed to get file stats", { error, filepath: dbFile.filepath, meta: dbFile.meta });
				this.finishedDownloads.set(dbFile.url, "error");
				dbFile.state = "error";
				dbFile.error = error;
				pushLogToClient(`[${dbFile.id}=${dbFile.metaId}] failed to download: ${dbFile.title}`, "error");
				ytdlpEvents.emit("list", [dbFile]);

				throw error;
			}
		}
		ytdlpEvents.emit("list", [dbFile]);

		await this.updateDownloadEntry(dbFile);
	}

	private async handleDownloadError(dbFile: SelectDownload, error: any) {
		const errorMessage =
			error instanceof Error ? error.message : typeof error === "string" ? error : "Unknown error";
		const normalizedError = {
			name: error instanceof Error ? error.name : "DownloadError",
			message: errorMessage,
			stack: error instanceof Error ? error.stack : undefined,
		};
		dbFile.state = "error";
		dbFile.error = normalizedError;
		this.finishedDownloads.set(dbFile.url, "error");
		ytdlpEvents.emit("status", {
			id: dbFile.id,
			action: "download",
			error: normalizedError,
			state: "done",
		});
		ytdlpEvents.emit("download", {
			id: dbFile.id,
			percent: 100,
			speed: "",
			eta: "",
			message: `failed: ${errorMessage}`,
		});
		this.clearDownloadProgress(dbFile.id);
		pushToastToClient(`${dbFile.title}`, "error", `Failed to download: ${errorMessage}`);
		log.error("handleDownloadError", { error });
		await this.updateDownloadEntry(dbFile);
	}

	private async updateDownloadEntry(dbFile: SelectDownload) {
		const updated = await queries.downloads.updateDownload(dbFile.id, dbFile);
		ytdlpEvents.emit("list", [updated]);
		return updated;
	}
	private cleanupDownload(dbFile: SelectDownload) {
		this.clearDownloadProgress(dbFile.id);
		this.activeDownloads.delete(dbFile.id);
		this.processingUrls.delete(dbFile.url);
		if (!this.activeDownloads.size && this.finishedDownloads.size) {
			pushToastToClient(
				`Downloads completed`,
				"success",
				`${Array.from(this.finishedDownloads.values()).filter((d) => d === "success").length} downloads completed`,
			);
			this.finishedDownloads.clear();
		}
	}

	async cancelDownload(id: number) {
		const controller = this.activeDownloads.get(id);
		if (controller) {
			const dbFile = await queries.downloads.findDownloadById(id);
			if (dbFile) this.finishedDownloads.set(dbFile.url, "cancelled");

			controller.abort("cancelled by user");
			this.activeDownloads.delete(id);
			this.clearDownloadProgress(id);
		}
	}
}

const downloadQueueManager = new DownloadQueueManager();

const log = logger.child("ytdlp.api");
export const ytdlpRouter = router({
	state: publicProcedure.query(() => ytdl.state.toString()),
	downloadMedia: publicProcedure
		.input(
			z.object({
				url: z.string().url().array(),
				type: z
					.enum([
						"auto",
						"video-best",
						"video-4k",
						"video-2k",
						"video-1440p",
						"video-1080p",
						"video-720p",
						"video-480p",
						"audio-bestaudio",
						"audio-mp3",
						"audio-m4a",
						"audio-opus",
					])
					.default("auto"),
			}),
		)
		.mutation(async ({ input: { url: urls, type } }) => {
			return await downloadQueueManager.addToQueue(urls, type);
		}),
	cancel: publicProcedure
		.input(z.union([z.string(), z.number()]).transform((v) => Number(v)))
		.mutation(async ({ input: id }) => {
			await downloadQueueManager.cancelDownload(id);
			ytdlpEvents.emit("cancel", id);
		}),
	status: publicProcedure.subscription(() => {
		return observable<YTDLStatus>((emit) => {
			function onStatusChange(data: any) {
				emit.next(data as YTDLStatus);
			}

			ytdlpEvents.on("status", onStatusChange);

			return () => {
				ytdlpEvents.off("status", onStatusChange);
			};
		});
	}),
	retry: publicProcedure.input(z.number()).mutation(async ({ input: id }) => {
		const dbFile = await db.query.downloads.findFirst({
			where(fields, { eq }) {
				return eq(fields.id, id);
			},
		});
		if (!dbFile) throw new TRPCError({ code: "NOT_FOUND", message: "id not found in database" });
		ytdlpEvents.emit("add", dbFile.url, dbFile.type);
	}),
	delete: publicProcedure
		.input(
			z
				.union([
					z.number(),
					z.object({
						id: z.number(),
						deleteFile: z.boolean().default(false),
					}),
				])
				.transform((input) => (typeof input === "number" ? { id: input, deleteFile: false } : input)),
		)
		.mutation(async ({ input: { id, deleteFile } }) => {
			const dbFile = await queries.downloads.findDownloadById(id);
			if (!dbFile) throw new TRPCError({ code: "NOT_FOUND", message: "id not found in database" });
			if (deleteFile && dbFile.filepath && existsSync(dbFile.filepath)) {
				try {
					rmSync(dbFile.filepath, { force: true });
				} catch (error) {
					log.error("failed to delete file from disk", { id, filepath: dbFile.filepath, error });
					throw new TRPCError({ code: "INTERNAL_SERVER_ERROR", message: "failed to delete file from disk" });
				}
			}
			const result = await queries.downloads.deleteDownload(id);
			if (!result.length) throw new TRPCError({ code: "NOT_FOUND", message: "id not found in database" });
			ytdlpEvents.emit("list", [
				{
					id: id,
					state: "deleted",
				},
			]);
		}),
	onDownload: publicProcedure.subscription(() => {
		return observable<YTDLDownloadStatus>((emit) => {
			function onStatusChange(data: any) {
				emit.next(data);
			}

			ytdlpEvents.on("download", onStatusChange);

			return () => {
				ytdlpEvents.off("download", onStatusChange);
			};
		});
	}),
	onIdDownload: publicProcedure.input(z.number()).subscription(({ input: id }) => {
		return observable<YTDLDownloadStatus>((emit) => {
			function onStatusChange(data: { id: number }) {
				if (data.id === id) emit.next(data as any);
			}

			ytdlpEvents.on("download", onStatusChange);

			return () => {
				ytdlpEvents.off("download", onStatusChange);
			};
		});
	}),
	onAutoAdd: publicProcedure.subscription(() => {
		return observable<string>((emit) => {
			async function onAutoAddHandle(url: string) {
				if (
					!url ||
					(await queries.downloads.findDownloadByUrl(url, ["completed", "fetching_meta", "downloading"]))
						.length
				) {
					ytdlpEvents.emit("toast", { message: "File already downloaded.", type: "error" });
					return;
				}
				ytdlpEvents.emit("autoAddCapture", url);
				let videoInfo: VideoInfo | null = null;
				try {
					videoInfo = await ytdl.extractInfo(url);
				} catch {
					videoInfo = null;
				}
				if (!videoInfo?.url) return;
				emit.next(url);
			}

			ytdlpEvents.on("autoAdd", onAutoAddHandle);

			return () => {
				ytdlpEvents.off("autoAdd", onAutoAddHandle);
			};
		});
	}),
	onAutoAddCapture: publicProcedure.subscription(() => {
		return observable<{ url: string }>((emit) => {
			async function onAutoAddCaptureHandle(url: string) {
				emit.next({ url: new URL(url).toString() });
			}

			ytdlpEvents.on("autoAddCapture", onAutoAddCaptureHandle);

			return () => {
				ytdlpEvents.off("autoAddCapture", onAutoAddCaptureHandle);
			};
		});
	}),
	onToast: publicProcedure.subscription(() => {
		return observable<string[]>((emit) => {
			async function onToastRelay(data: string | { message: string; type?: string; description?: string }) {
				emit.next(
					typeof data === "string" ? [data] : ([data.message, data.description, data.type] as string[]),
				);
			}

			ytdlpEvents.on("toast", onToastRelay);

			return () => {
				ytdlpEvents.off("toast", onToastRelay);
			};
		});
	}),
	stats: publicProcedure.query(async () => {
		const items = await db.select().from(downloads).all();
		return items.reduce(
			(acc, r) => {
				if (r.state === "completed") acc.overallCount++;
				let type = r.type?.toLowerCase()?.split("-")[0];
				if (type === "auto") type = "video";
				if (type && r.state === "completed") acc.count[type]++;
				if (r.state) acc.state[r.state.toLowerCase()]++;
				if (r.filesize) {
					acc.overallUsage += r.filesize;
					if (type) {
						acc.size[type] += r.filesize;
					}
					if (r.state === "completed") acc.completedUsage += r.filesize;
				}
				return acc;
			},
			{
				overallUsage: 0,
				completedUsage: 0,
				overallCount: 0,
				state: { completed: 0, downloading: 0, error: 0, cancelled: 0 },
				count: { video: 0, audio: 0, other: 0 },
				size: { video: 0, audio: 0, other: 0 },
			},
		);
	}),
	list: publicProcedure
		.input(
			z
				.object({
					sortBy: z.enum(LIST_SORT_KEYS).default("created"),
					sortDir: z.enum(LIST_SORT_DIRECTIONS).default("desc"),
				})
				.default({
					sortBy: "created",
					sortDir: "desc",
				}),
		)
		.query(async ({ input }) => {
			const { sortBy, sortDir } = input;
			const activeRank = sql<number>`CASE WHEN ${downloads.state} IN ('downloading', 'fetching_meta', 'queued', 'converting') THEN 1 ELSE 0 END`;
			const finishedRank = sql<number>`CASE WHEN ${downloads.state} = 'completed' AND ${downloads.created} IS NOT NULL THEN 1 ELSE 0 END`;
			const columnMap = {
				created: downloads.created,
				title: downloads.title,
				source: downloads.source,
				state: downloads.state,
				filesize: downloads.filesize,
				type: downloads.type,
				finishedAt: downloads.created,
			} as const;
			const sortColumn = columnMap[sortBy];
			const sortExpr = sortDir === "asc" ? asc(sortColumn) : desc(sortColumn);
			if (sortBy === "finishedAt") {
				return await db
					.select()
					.from(downloads)
					.orderBy(desc(activeRank), desc(finishedRank), sortExpr, desc(downloads.created))
					.all();
			}
			return await db.select().from(downloads).orderBy(desc(activeRank), sortExpr, desc(downloads.created)).all();
		}),
	listSync: publicProcedure.subscription(() => {
		return observable<YTDLItem[]>((emit) => {
			function onStatusChange(data: any) {
				emit.next(data);
			}

			ytdlpEvents.on("list", onStatusChange);

			// Initial state
			db.select()
				.from(downloads)
				.orderBy(desc(downloads.created))
				.all()
				.then((items) => {
					emit.next(items as YTDLItem[]);
				})
				.catch((error) => {
					log.error("Failed to fetch initial download list", error);
				});

			return () => {
				ytdlpEvents.off("list", onStatusChange);
			};
		});
	}),
} as const);

async function handleYtAddEvent(url: string, type: YTDLMediaType = "auto") {
	await downloadQueueManager.addToQueue([url], type);
}

ytdlpEvents.on("add", handleYtAddEvent);
process.on("exit", () => {
	ytdlpEvents.off("add", handleYtAddEvent);
});
