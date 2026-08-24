import path from "path";
import { platform as appPlatform } from "@electron-toolkit/utils";
import { fetchLatestYtdlpRelease, hasYtdlpBinary, normalizeYtdlpTag, readYtdlpVersion } from "@main/lib/downloads/ytdlp-cli/binary";
import { onYtdlpDownloadOutput, runYtdlpDownload } from "@main/lib/downloads/ytdlp-cli/download";
import { getYtdlpExtractPool } from "@main/lib/downloads/ytdlp-cli/extract-pool";
import type { YtdlpOptions, YtdlpOutputEvent } from "@main/lib/downloads/ytdlp-cli/types";
import { DEFAULT_OUTTMPL } from "@main/stores/app/AppStore";
import { appStore } from "@main/stores/app/app.store";
import { dependenciesManager } from "@main/trpc/dependencies/handler";
import { Logger } from "@shared/logger";
import { app } from "electron";
import { VideoInfo } from "yt-dlp-wrap/types";

const log = new Logger("YTDLP");

export enum YTDLP_STATE {
	NONE,
	READY,
	CONVERTING,
	DOWNLOADING,
	UPDATE_CHECKING,
	UPDATE_ERROR,
	ERROR,
	MISSING_BINARY,
}

export class YTDLP {
	private _state: YTDLP_STATE = YTDLP_STATE.NONE;
	private initializePromise: Promise<void> | null = null;
	private outputUnsubscribe: (() => void) | null = null;
	private readonly outputListeners = new Set<(event: YtdlpOutputEvent) => void>();

	get state() {
		return this._state;
	}

	async initialize() {
		if (this._state === YTDLP_STATE.READY && hasYtdlpBinary()) {
			return;
		}
		if (this.initializePromise) {
			return await this.initializePromise;
		}
		this.initializePromise = this.initializeInternal();
		try {
			await this.initializePromise;
		} finally {
			this.initializePromise = null;
		}
	}

	private async initializeInternal() {
		log.info("initializing...");
		if (!hasYtdlpBinary()) {
			this._state = YTDLP_STATE.MISSING_BINARY;
			await dependenciesManager.downloadDependencyPromise("ytdlp");
		}
		if (!hasYtdlpBinary()) {
			this._state = YTDLP_STATE.ERROR;
			throw new Error("yt-dlp binary is missing");
		}
		if (!this.outputUnsubscribe) {
			this.outputUnsubscribe = onYtdlpDownloadOutput((event) => {
				for (const listener of this.outputListeners) listener(event);
			});
		}
		const version = await readYtdlpVersion();
		appStore.set("ytdlp.version", version);
		this._state = YTDLP_STATE.READY;
		log.debug("ytdlp version:", { version });
		void this.checkForYtdlpUpdate().catch((error) => {
			log.warn("background yt-dlp update check failed", { error });
		});
	}

	async checkForYtdlpUpdate(force = false): Promise<{ version: string; updated: boolean; latestTag: string }> {
		this._state = YTDLP_STATE.UPDATE_CHECKING;
		try {
			if (hasYtdlpBinary() && !force) {
				const currentVersion = await readYtdlpVersion();
				const release = await fetchLatestYtdlpRelease();
				const latestTag = normalizeYtdlpTag(release.tag_name);
				if (currentVersion === latestTag || currentVersion.includes(latestTag)) {
					this._state = YTDLP_STATE.READY;
					return { version: currentVersion, updated: false, latestTag };
				}
			}
			const result = await dependenciesManager.downloadDependencyPromise("ytdlp");
			appStore.set("ytdlp.version", result.version);
			this._state = YTDLP_STATE.READY;
			return { version: result.version, updated: true, latestTag: result.version };
		} catch (error) {
			this._state = hasYtdlpBinary() ? YTDLP_STATE.READY : YTDLP_STATE.UPDATE_ERROR;
			throw error;
		}
	}

	async extractInfo(url: string, options?: Partial<YtdlpOptions>): Promise<VideoInfo> {
		if (this._state !== YTDLP_STATE.READY && !hasYtdlpBinary()) {
			throw new Error("yt-dlp is not initialized");
		}
		return (await getYtdlpExtractPool().extractInfo(url, options)) as VideoInfo;
	}

	async download(url: string, options?: Partial<YtdlpOptions>, signal?: AbortSignal): Promise<void> {
		if (this._state !== YTDLP_STATE.READY && !hasYtdlpBinary()) {
			throw new Error("yt-dlp is not initialized");
		}
		await runYtdlpDownload(url, options, { signal });
	}

	onOutput(listener: (event: YtdlpOutputEvent) => void): () => void {
		this.outputListeners.add(listener);
		return () => {
			this.outputListeners.delete(listener);
		};
	}

	async downloadWithOutput(
		url: string,
		options?: Partial<YtdlpOptions>,
		onRequestCreated?: (requestId: string) => void,
		signal?: AbortSignal,
	): Promise<void> {
		if (this._state !== YTDLP_STATE.READY && !hasYtdlpBinary()) {
			throw new Error("yt-dlp is not initialized");
		}
		await runYtdlpDownload(url, options, { onRequestCreated, signal });
	}

	get currentDownloadPath() {
		return appStore.store.download?.selected;
	}
}

export function sanitizeId(id: string) {
	return id.replace(/[^a-zA-Z0-9\-]/g, "_");
}

export function sanitizeFilename(filename: string) {
	let pattern: RegExp = /[^a-zA-Z0-9]/g;
	if (appPlatform.isWindows) {
		pattern =
			/[<>:"/\\|?*\x00-\x1F\x7F]|[^\w\u4e00-\u9fff\u3400-\u4dbf\u3040-\u309f\u30a0-\u30ff\u3000-\u303f\uac00-\ud7af\uff00-\uffef\u1100-\u11ff\u3130-\u318f\ua960-\ua97f\ud7b0-\ud7ff]/g;
	} else if (appPlatform.isMacOS) {
		pattern = /[:\/\x00]|[^\w\u4e00-\u9fff\u3400-\u4dbf\u3040-\u309f\u30a0-\u30ff\u3000-\u303f\uac00-\ud7af\uff00-\uffef\u1100-\u11ff\u3130-\u318f\ua960-\ua97f\ud7b0-\ud7ff]/g;
	} else if (appPlatform.isLinux) {
		pattern = /[\/\x00]|[^\w\u4e00-\u9fff\u3400-\u4dbf\u3040-\u309f\u30a0-\u30ff\u3000-\u303f\uac00-\ud7af\uff00-\uffef\u1100-\u11ff\u3130-\u318f\ua960-\ua97f\ud7b0-\ud7ff]/g;
	}

	return filename.replace(pattern, "_");
}

export { DEFAULT_OUTTMPL };

export function getOuttmpl() {
	const outtmpl = appStore.get("ytdlp.outtmpl", null) ?? DEFAULT_OUTTMPL;
	return outtmpl;
}
export function getDownloadPath() {
	const downloadPath = appStore.get("download.selected", app.getPath("downloads"));
	return downloadPath;
}

export function getDownloadPathWithFilename(filename: string) {
	const downloadPath = getDownloadPath();
	return path.join(downloadPath, filename);
}

export type { YtdlpOptions, YtdlpOutputEvent };
