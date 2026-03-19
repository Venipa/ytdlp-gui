import { platform as appPlatform } from "@electron-toolkit/utils";
import createYtdlpService from "@main/lib/ytdlp-service";
import YtdlpPythonService, { YtdlpOutputEvent } from "@main/lib/ytdlp-service/python";
import { appStore } from "@main/stores/app.store";
import { Logger } from "@shared/logger";
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
	private _service!: YtdlpPythonService;
	get state() {
		return this._state;
	}
	constructor() {}
	async initialize() {
		log.info("initializing...");
		this._service = createYtdlpService();
		await this._service.waitReady().catch((err) => {
			log.error("failed to wait for ytdlp service to be ready:", err);
			this._state = YTDLP_STATE.ERROR;
		});
		const version = await this._service.getVersion();
		appStore.set("ytdlp", { path: "internal", version, checkForUpdate: false });
		this._state = YTDLP_STATE.READY;
		log.debug("ytdlp python version:", { version });
	}
	async checkUpdates() {
		return {
			updated: false,
			currentVersion: appStore.store.ytdlp.version,
			previousVersion: appStore.store.ytdlp.version ?? "-",
		};
	}
	async extractInfo(url: string, options?: Record<string, unknown>): Promise<VideoInfo> {
		if (!this._service) {
			throw new Error("Ytdlp Python service is not initialized");
		}
		return (await this._service.extractInfo(url, options)) as VideoInfo;
	}
	async download(url: string, options?: Record<string, unknown>): Promise<void> {
		if (!this._service) {
			throw new Error("Ytdlp Python service is not initialized");
		}
		await this._service.download(url, options);
	}
	onOutput(listener: (event: YtdlpOutputEvent) => void): () => void {
		if (!this._service) {
			throw new Error("Ytdlp Python service is not initialized");
		}
		return this._service.onOutput(listener);
	}
	async downloadWithOutput(url: string, options?: Record<string, unknown>, onRequestCreated?: (requestId: string) => void): Promise<void> {
		if (!this._service) {
			throw new Error("Ytdlp Python service is not initialized");
		}
		await this._service.download(url, options, { onRequestCreated });
	}
	get currentDownloadPath() {
		return appStore.store.download?.selected;
	}
}
export function sanitizeId(id: string) {
	return id.replace(/[^a-zA-Z0-9\-]/g, "_");
}
export function sanitizeFilename(filename: string) {
	// Cross-platform filename sanitization:
	// - On Windows: Reserved characters include <>:"/\|?* and ASCII control chars (\x00-\x1F), and some reserved device names are disallowed.
	// - On macOS: Only : (colon) is NOT allowed in filenames, / is path separator.
	// - On Linux: Only / is not allowed (it's the separator), but NULL (\0) is forbidden everywhere.
	//
	// We'll allow more for macOS/Linux, stricter (but still East Asian friendly) for Windows.
	// We'll omit device name checks for brevity and focus on invalid characters.
	//
	// Also, allow common East Asian ranges as before.

	let pattern: RegExp = /[^a-zA-Z0-9]/g;
	if (appPlatform.isWindows) {
		// Windows: Remove <>:"/\\|?*, control chars, NUL (\0-\x1F, \x7F)
		pattern = /[<>:"/\\|?*\x00-\x1F\x7F]|[^\w\u4e00-\u9fff\u3400-\u4dbf\u3040-\u309f\u30a0-\u30ff\u3000-\u303f\uac00-\ud7af\uff00-\uffef]/g;
	} else if (appPlatform.isMacOS) {
		// macOS: Remove colon (:) and NUL (\0); allow everything else except /
		pattern = /[:\/\x00]|[^\w\u4e00-\u9fff\u3400-\u4dbf\u3040-\u309f\u30a0-\u30ff\u3000-\u303f\uac00-\ud7af\uff00-\uffef]/g;
	} else if (appPlatform.isLinux) {
		// Linux and others: Only / and NULL (\0); be permissive with Unicode
		pattern = /[\/\x00]|[^\w\u4e00-\u9fff\u3400-\u4dbf\u3040-\u309f\u30a0-\u30ff\u3000-\u303f\uac00-\ud7af\uff00-\uffef]/g;
	}

	return filename.replace(pattern, "_");
}
