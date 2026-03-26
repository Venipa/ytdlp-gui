/*
 * SPDX-License-Identifier: LicenseRef-Proprietary
 * Copyright (c) 2026 Venipa. All rights reserved.
 *
 * This file is proprietary. It may not be copied, modified, distributed, or used
 * except by the copyright holder (Venipa) without explicit written permission.
 * Repository licensing: see LICENSING.md at the project root.
 */

import { EventEmitter } from "node:events";
import { existsSync, mkdirSync } from "node:fs";
import path, { dirname, join, resolve } from "node:path";
import { platform } from "@electron-toolkit/utils";
import { checkDebugFlag } from "@main/lib/debug";
import { YtdlpPyOptions } from "@main/lib/ytdlp-service/ytdlp-options";
import { parseJson, stringifyJson } from "@shared/json";
import { createLogger } from "@shared/logger";
import { app } from "electron";
import _ffmpegStaticPath from "ffmpeg-static";
import _ffprobeStaticPath from "ffprobe-static";
import { PythonShell } from "python-shell";
import ytdlPyWorkerPath from "./worker.py?asset&asarUnpack";
const ffmpegStaticPath = import.meta.env.PROD ? (_ffmpegStaticPath?.replace("app.asar", "app.asar.unpacked") ?? null) : _ffmpegStaticPath;
const ffprobeStaticPath = import.meta.env.PROD ? (_ffprobeStaticPath?.replace("app.asar", "app.asar.unpacked") ?? null) : _ffprobeStaticPath;
const log = createLogger("ytdlp-py-service");
// Helper: Generate a unique ID for RPC calls
function genId(): string {
	return Math.random().toString(36).substr(2, 9);
}
function buildPythonPath(workerScriptPath: string) {
	const candidateDirs = [
		import.meta.env.DEV && resolve(process.cwd(), ".venv"), // dev mode uses the project root .venv
		resolve(dirname(workerScriptPath), "..", "venv"),
		join(process.resourcesPath, "app.asar.unpacked", "out/main/resources/venv"),
	].filter(Boolean) as string[];
	const existingDirs = candidateDirs.filter((directoryPath) => existsSync(directoryPath));
	log.info("buildPythonPath", { existingDirs, candidateDirs, resourcesPath: process.resourcesPath });
	const delimiter = process.platform === "win32" ? ";" : ":";
	return {
		path: existingDirs.join(delimiter),
		paths: existingDirs,
	};
}

interface RpcRequest {
	id: string;
	method: string;
	params?: YtdlpPyOptions;
}

interface RpcResponse {
	id: string;
	result?: unknown;
	error?: string | { message: string; code?: number; data?: unknown };
}

interface YtdlpProgressLine {
	extractor: string;
	id: string | null;
	message: string;
}

export interface YtdlpOutputEvent {
	type: "status" | "download_status" | "completed";
	raw: string;
	extractor: string;
	videoId: string | null;
	message: string;
	requestId: string | null;
	workerId?: string | null;
	percent?: number;
	speed?: string | null;
	eta?: string | null;
	fragmentIndex?: number | null;
	fragmentCount?: number | null;
}

interface PendingCall {
	method: string;
	resolve: (value: unknown) => void;
	reject: (reason: unknown) => void;
}

interface RpcCallOptions {
	onRequestCreated?: (id: string) => void;
}

interface YtdlpWorkerServiceOptions {
	pythonPath?: string;
	pythonOptions?: string[];
	cwd?: string;
	workerId?: string;
}
class YtdlpPythonWorkerService {
	private pyshell: PythonShell;
	private pending: Record<string, PendingCall> = {};
	private readonly outputEmitter = new EventEmitter();
	private readonly lifecycleEmitter = new EventEmitter();
	private readonly workerId: string | null;
	private readonly readyPromise: Promise<void>;
	private resolveReadyPromise: (() => void) | null = null;
	private rejectReadyPromise: ((reason: unknown) => void) | null = null;

	constructor(options: YtdlpWorkerServiceOptions = {}) {
		this.workerId = options.workerId ?? null;
		this.readyPromise = new Promise<void>((resolve, reject) => {
			this.resolveReadyPromise = resolve;
			this.rejectReadyPromise = reject;
		});
		const workerScriptPath = ytdlPyWorkerPath;
		const cwd = options.cwd ?? path.join(app.getPath("userData"), "ytdlp_cache");
		if (!existsSync(cwd)) mkdirSync(cwd, { recursive: true });
		const { path: pythonPathEnv, paths: pythonPaths } = buildPythonPath(workerScriptPath);
		if (!pythonPathEnv) {
			throw new Error("Python path not found");
		}
		const firstPyPath = pythonPaths[0];

		const pythonPath = join(firstPyPath, platform.isWindows ? "Scripts" : "bin", platform.isWindows ? "python.exe" : "python");
		// TODO: Uncomment this when we have a way to chmod +x the Python executable
		// if (!platform.isWindows && executableIsAvailable("chmod")) {
		// 	try {
		// 		execSync(`chmod +x "${pythonPath}"`, { stdio: "inherit" });
		// 	} catch (error) {
		// 		log.error("Failed to chmod +x Python executable", { error });
		// 	}
		// }
		log.info("Python path", { pythonPath });
		this.pyshell = new PythonShell(workerScriptPath, {
			pythonPath: options.pythonPath ?? pythonPath,
			cwd,
			encoding: "utf-8",
			pythonOptions: options.pythonOptions || ["-u"],
			env: {
				...process.env,
				...(pythonPathEnv ? { PYTHONPATH: pythonPathEnv } : {}),
				...(this.workerId ? { YTDLP_WORKER_ID: this.workerId } : {}),
			},
			parser(param) {
				return param;
			},
			formatter(param) {
				return stringifyJson(param);
			},
			stderrParser(param) {
				log.error("PythonShell stderrParser", { param });
				return param;
			},
			...(import.meta.env.DEV ? { detached: true, windowsHide: false } : { windowsHide: true }),
			mode: "text",
		});
		if (import.meta.env.DEV) {
			// Attach a handler that pipes any output from the spawned Python process to the parent process's stdout/stderr for debugging/dev shells.
			this.pyshell.childProcess?.stdout?.addListener("data", (data) => {
				log.debug("PythonShell stdout", { data });
			});
			this.pyshell.childProcess?.stderr?.addListener("data", (data) => {
				log.debug("PythonShell stderr", { data });
			});
		}
		this.pyshell.on("message", this.onPythonMessage.bind(this));
		this.pyshell.on("error", (err: Error) => {
			this.rejectAllPending(err);
			this.rejectReady(err);
			this.lifecycleEmitter.emit("close", err);
		});
		this.pyshell.on("close", (err: Error | null) => {
			const closeError = err ?? new Error("Python process closed");
			this.rejectAllPending(closeError);
			this.rejectReady(closeError);
			this.lifecycleEmitter.emit("close", closeError);
			log.warn("YtdlpPythonService closed", { error: err });
		});
		log.info("YtdlpPythonService initialized", { workerId: this.workerId });
	}
	// Sends a typed JSON RPC call to Python
	call(method: string, params?: { options?: YtdlpPyOptions; [key: string]: unknown }, callOptions?: RpcCallOptions): Promise<unknown> {
		const id = genId();

		const reqParams = Object.assign({ options: {} }, params ?? {});
		Object.assign(reqParams.options, {
			quiet: true,
			no_warnings: true,
			ffmpeg_location: ffmpegStaticPath,
			ffprobe_location: ffprobeStaticPath,
		} as YtdlpPyOptions);
		const req: RpcRequest = { id, method, params: reqParams };
		log.debug("Sending RPC request", { id, method, params: reqParams });
		return new Promise((resolve, reject) => {
			this.pending[id] = { method, resolve, reject };
			callOptions?.onRequestCreated?.(id);
			try {
				// PythonShell in json mode automatically serializes to JSON
				this.pyshell.send(req);
			} catch (e) {
				delete this.pending[id];
				reject(e);
			}
		});
	}
	private _ready = false;
	private _binVersion: string | null = null;
	get binVersion(): string | null {
		return this._binVersion;
	}
	get ready(): boolean {
		return this._ready;
	}
	waitReady(): Promise<void> {
		if (this._ready) return Promise.resolve();
		return this.readyPromise;
	}
	private resolveReady(): void {
		if (this._ready) {
			return;
		}
		this._ready = true;
		if (this.resolveReadyPromise) {
			this.resolveReadyPromise();
			this.resolveReadyPromise = null;
			this.rejectReadyPromise = null;
		}
	}
	private rejectReady(reason: unknown): void {
		this._ready = false;
		if (this.rejectReadyPromise) {
			this.rejectReadyPromise(reason);
			this.resolveReadyPromise = null;
			this.rejectReadyPromise = null;
		}
	}
	private rejectAllPending(reason: unknown): void {
		Object.values(this.pending).forEach((pendingCall) => pendingCall.reject(reason));
		this.pending = {};
	}
	onOutput(listener: (event: YtdlpOutputEvent) => void): () => void {
		this.outputEmitter.on("output", listener);
		return () => {
			this.outputEmitter.off("output", listener);
		};
	}
	onClose(listener: (error: Error) => void): () => void {
		this.lifecycleEmitter.on("close", listener);
		return () => {
			this.lifecycleEmitter.off("close", listener);
		};
	}
	private emitOutput(event: YtdlpOutputEvent): void {
		this.outputEmitter.emit("output", event);
	}
	private getActiveRequestIdByMethod(method: string): string | null {
		const activeEntry = Object.entries(this.pending).find(([, pendingCall]) => pendingCall.method === method);
		return activeEntry ? activeEntry[0] : null;
	}
	private parseDownloadStatus(progressLine: YtdlpProgressLine, rawLine: string, requestIdOverride: string | null): YtdlpOutputEvent {
		const requestId = requestIdOverride ?? this.getActiveRequestIdByMethod("download");
		const progressMatch = /^(\d+(?:\.\d+)?)%\s+of\s+.+?(?:\s+at\s+(.+?))?(?:\s+ETA\s+(.+?))?(?:\s+\(frag\s+(\d+)\/(\d+)\))?$/i.exec(progressLine.message);
		if (!progressMatch) {
			return {
				type: "status",
				raw: rawLine,
				extractor: progressLine.extractor,
				videoId: progressLine.id,
				message: progressLine.message,
				requestId,
				workerId: this.workerId,
			};
		}

		return {
			type: "download_status",
			raw: rawLine,
			extractor: progressLine.extractor,
			videoId: progressLine.id,
			message: progressLine.message,
			requestId,
			workerId: this.workerId,
			percent: Number.parseFloat(progressMatch[1]),
			speed: progressMatch[2] ?? null,
			eta: progressMatch[3] ?? null,
			fragmentIndex: progressMatch[4] ? Number.parseInt(progressMatch[4], 10) : null,
			fragmentCount: progressMatch[5] ? Number.parseInt(progressMatch[5], 10) : null,
		};
	}
	private parseStatusOutputEvent(rawLine: string, progressLine: YtdlpProgressLine, requestIdOverride: string | null): YtdlpOutputEvent {
		if (progressLine.extractor.toLowerCase() === "download") {
			return this.parseDownloadStatus(progressLine, rawLine, requestIdOverride);
		}
		return {
			type: "status",
			raw: rawLine,
			extractor: progressLine.extractor,
			videoId: progressLine.id,
			message: progressLine.message,
			requestId: requestIdOverride ?? this.getActiveRequestIdByMethod("download"),
			workerId: this.workerId,
		};
	}
	private parseYtdlpProgressLine(line: string): YtdlpProgressLine | null {
		const withIdMatch = /^\[([^\]]+)\]\s+([^:]+):\s*(.+)$/.exec(line);
		if (withIdMatch) {
			return {
				extractor: withIdMatch[1],
				id: withIdMatch[2],
				message: withIdMatch[3],
			};
		}

		const withoutIdMatch = /^\[([^\]]+)\]\s+(.+)$/.exec(line);
		if (withoutIdMatch) {
			return {
				extractor: withoutIdMatch[1],
				id: null,
				message: withoutIdMatch[2],
			};
		}
		return null;
	}
	private onPythonRpcResponse(response: RpcResponse): void {
		const { id, result, error } = response;

		if (!id || typeof id !== "string") {
			log.error("Invalid response ID:", response);
			return;
		}

		if (id === "ready") {
			if (error !== undefined && error !== null) {
				const errorMessage = typeof error === "string" ? error : error.message || "Unknown ready error";
				this.rejectReady(new Error(errorMessage));
				log.error("YtdlpPythonService failed to initialize", { error: errorMessage });
				return;
			}
			const readyResult = (result as { version?: string | null } | undefined) ?? {};
			this._binVersion = readyResult.version ?? null;
			this.resolveReady();
			log.info("YtdlpPythonService ready", { version: this._binVersion });
			return;
		}

		const pending = this.pending[id];
		if (!pending) {
			log.warn("Received response for unknown ID:", id);
			return;
		}

		delete this.pending[id];

		if (error !== undefined && error !== null) {
			const errorMessage = typeof error === "string" ? error : error.message || "Unknown error";
			pending.reject(new Error(errorMessage));
		} else {
			pending.resolve(result);
		}
	}
	private parseYtdlpPayload(line: string) {
		if (!line.startsWith("{")) return null;
		const parsed = parseJson<{
			result?: unknown;
			id?: string;
			error?: string;
			__ytdlp_progress__?: boolean;
			line?: string;
			workerId?: string;
			status?: string;
			videoId?: string;
			percent?: number | null;
			speed?: string | null;
			eta?: string | null;
			fragmentIndex?: number | null;
			fragmentCount?: number | null;
		}>(line);
		if (!parsed || typeof parsed !== "object") return null;
		return parsed;
	}
	private splitMixedOutputLine(line: string): string[] {
		const trimmed = line.trim();
		if (!trimmed) return [];
		if (trimmed.startsWith("{")) return [trimmed];

		for (let index = trimmed.indexOf("{"); index !== -1; index = trimmed.indexOf("{", index + 1)) {
			const left = trimmed.slice(0, index).trim();
			const right = trimmed.slice(index).trim();
			if (!left || !right) continue;
			if (!this.parseYtdlpPayload(right)) continue;
			return [left, right];
		}
		return [trimmed];
	}
	private handleTaggedProgressPayload(parsed: {
		__ytdlp_progress__?: boolean;
		id?: string;
		line?: string;
		workerId?: string;
		status?: string;
		videoId?: string;
		percent?: number | null;
		speed?: string | null;
		eta?: string | null;
		fragmentIndex?: number | null;
		fragmentCount?: number | null;
	}): boolean {
		if (!parsed.__ytdlp_progress__) return false;
		const requestId = typeof parsed.id === "string" ? parsed.id : null;
		const status = typeof parsed.status === "string" ? parsed.status : null;
		const normalizedVideoId = typeof parsed.videoId === "string" ? parsed.videoId : null;
		const line = typeof parsed.line === "string" ? parsed.line : `[download] ${normalizedVideoId ?? "unknown"}: ${status ?? "progress"}`;

		let outputEvent: YtdlpOutputEvent | null = null;
		if (status === "downloading") {
			outputEvent = {
				type: "download_status",
				raw: line,
				extractor: "download",
				videoId: normalizedVideoId,
				message: line,
				requestId,
				workerId: this.workerId,
				percent: typeof parsed.percent === "number" ? parsed.percent : undefined,
				speed: parsed.speed ?? null,
				eta: parsed.eta ?? null,
				fragmentIndex: typeof parsed.fragmentIndex === "number" ? parsed.fragmentIndex : null,
				fragmentCount: typeof parsed.fragmentCount === "number" ? parsed.fragmentCount : null,
			};
		} else if (status === "finished") {
			outputEvent = {
				type: "download_status",
				raw: line,
				extractor: "download",
				videoId: normalizedVideoId,
				message: line,
				requestId,
				workerId: this.workerId,
				percent: 100,
			};
		} else if (status === "completed") {
			outputEvent = {
				type: "completed",
				raw: line,
				extractor: "download",
				videoId: normalizedVideoId,
				message: line,
				requestId,
				workerId: this.workerId,
				percent: 100,
			};
		}

		if (!outputEvent) {
			if (typeof parsed.line !== "string") {
				log.warn("Tagged progress missing line", { id: parsed.id, workerId: parsed.workerId ?? this.workerId });
				return true;
			}
			const progressLine = this.parseYtdlpProgressLine(parsed.line);
			if (!progressLine) {
				log.debug("Tagged progress line did not parse", { line: parsed.line, workerId: parsed.workerId ?? this.workerId });
				return true;
			}
			outputEvent = this.parseStatusOutputEvent(parsed.line, progressLine, requestId);
		}

		this.emitOutput(outputEvent);
		if (checkDebugFlag("ytdlp")) log.debug("yt-dlp output", outputEvent);
		return true;
	}
	// Internal: Handle mixed Python output from stdout
	private onPythonMessage(msg: unknown): void {
		if (msg && typeof msg === "object") {
			this.onPythonRpcResponse(msg as RpcResponse);
			return;
		}
		if (typeof msg !== "string") {
			log.debug("Ignoring unsupported Python output", { output: msg });
			return;
		}

		const lines = msg
			.split(/\r?\n/g)
			.map((line) => line.trim())
			.filter(Boolean);
		try {
			for (const line of lines) {
				const lineParts = this.splitMixedOutputLine(line);
				for (const part of lineParts) {
					const parsedJson = this.parseYtdlpPayload(part);
					if (parsedJson) {
						if (this.handleTaggedProgressPayload(parsedJson)) {
							continue;
						}
						const resultObject = parsedJson.result && typeof parsedJson.result === "object" ? (parsedJson.result as { id?: string }) : null;
						log.debug("yt-dlp JSON RPC response", { videoId: resultObject?.id ?? "unknown" });
						this.onPythonRpcResponse(parsedJson as RpcResponse);
						continue;
					}

					const progressLine = this.parseYtdlpProgressLine(part);
					if (progressLine) {
						const outputEvent = this.parseStatusOutputEvent(part, progressLine, null);
						this.emitOutput(outputEvent);
						if (checkDebugFlag("ytdlp")) log.debug("yt-dlp output", outputEvent);
						continue;
					}

					if (part.startsWith("Deleting original file ")) {
						const outputEvent: YtdlpOutputEvent = {
							type: "status",
							raw: part,
							extractor: "postprocess",
							videoId: null,
							message: part,
							requestId: this.getActiveRequestIdByMethod("download"),
							workerId: this.workerId,
						};
						this.emitOutput(outputEvent);
						if (checkDebugFlag("ytdlp")) log.debug("yt-dlp output", outputEvent);
						continue;
					}

					log.debug("Python non-JSON output", { line: part });
				}
			}
		} catch (error) {
			log.error("Error parsing Python output", { error, output: msg });
		}
	}

	// Convenience wrapper for yt-dlp "extract_info"
	extractInfo(url: string, options?: YtdlpPyOptions): Promise<unknown> {
		if (options?.filename) {
			options.outtmpl = options.filename;
			delete options.filename;
		}
		return this.call("extract_info", { url, options });
	}

	// Convenience wrapper for yt-dlp "get_version"
	getVersion(): Promise<string> {
		return this.call("get_version") as Promise<string>;
	}

	// Convenience wrapper for yt-dlp "download"
	download(url: string, options?: YtdlpPyOptions, callOptions?: RpcCallOptions): Promise<unknown> {
		if (options?.filename) {
			options.outtmpl = options.filename;
			delete options.filename;
		}
		return this.call("download", { url, options }, callOptions);
	}

	shutdown(): void {
		this.pyshell.end((err: Error | null) => {
			if (err) {
				log.error("Error shutting down:", err);
			}
		});
	}
}

export default YtdlpPythonWorkerService;
