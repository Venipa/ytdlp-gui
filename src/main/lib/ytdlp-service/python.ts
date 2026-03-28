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
import { dependenciesManager } from "@main/trpc/dependencies/handler";
import { parseJson, stringifyJson } from "@shared/json";
import { createLogger } from "@shared/logger";
import { app } from "electron";
import { PythonShell } from "python-shell";
import ytdlPyWorkerPath from "./worker.py?asset&asarUnpack";
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

interface YtdlpTemplateProgressPayload {
	status?: string;
	videoId?: string;
	percent?: string;
	speed?: string;
	eta?: string;
	fragmentIndex?: string;
	fragmentCount?: string;
	total?: string;
	totalEstimate?: string;
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
	private static readonly IN_PROGRESS_PERCENT_MAX = 99.9;
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
				return param;
			},
			...(import.meta.env.DEV ? {} : { windowsHide: true }),
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
	async call(method: string, params?: { options?: YtdlpPyOptions; [key: string]: unknown }, callOptions?: RpcCallOptions): Promise<unknown> {
		const id = genId();
		const ffmpegDep = dependenciesManager.getInstallState("ffmpeg")?.files?.[0];
		if (!ffmpegDep) {
			throw new Error("FFmpeg path not found");
		}
		const reqParams = Object.assign({ options: {} }, params ?? {});
		Object.assign(reqParams.options, {
			quiet: true,
			no_warnings: true,
			compat_opts: ["abort-on-error"],
			ffmpeg_location: ffmpegDep,
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
	private parseIntegerValue(value: unknown): number | null {
		if (typeof value !== "string" && typeof value !== "number") {
			return null;
		}
		const parsed = Number.parseInt(String(value).trim(), 10);
		return Number.isFinite(parsed) ? parsed : null;
	}
	private computeFragmentOverallPercent(percent: number | null | undefined, fragmentIndex: number | null | undefined, fragmentCount: number | null | undefined): number | null {
		if (
			typeof percent !== "number" ||
			!Number.isFinite(percent) ||
			typeof fragmentIndex !== "number" ||
			!Number.isFinite(fragmentIndex) ||
			typeof fragmentCount !== "number" ||
			!Number.isFinite(fragmentCount) ||
			fragmentCount <= 0
		) {
			return null;
		}
		// yt-dlp fragment index is generally 1-based in logs.
		const currentIndex = Math.max(1, Math.min(Math.trunc(fragmentIndex), Math.trunc(fragmentCount)));
		const completedFragments = currentIndex - 1;
		const overall = ((completedFragments + percent / 100) / fragmentCount) * 100;
		return Math.max(0, Math.min(100, overall));
	}
	private normalizeInProgressPercent(percent: number | null | undefined): number | undefined {
		if (typeof percent !== "number" || !Number.isFinite(percent)) {
			return undefined;
		}
		const clamped = Math.max(0, Math.min(100, percent));
		return clamped >= 100 ? YtdlpPythonWorkerService.IN_PROGRESS_PERCENT_MAX : clamped;
	}
	private createTaggedProgressOutputEvent(parsed: {
		id?: string;
		status?: string;
		videoId?: string;
		line?: string;
		percent?: number | null;
		speed?: string | null;
		eta?: string | null;
		fragmentIndex?: number | null;
		fragmentCount?: number | null;
	}): YtdlpOutputEvent {
		const requestId = typeof parsed.id === "string" ? parsed.id : null;
		const status = typeof parsed.status === "string" ? parsed.status : null;
		const normalizedVideoId = typeof parsed.videoId === "string" ? parsed.videoId : null;
		const line = typeof parsed.line === "string" ? parsed.line : `[download] ${normalizedVideoId ?? "unknown"}: ${status ?? "progress"}`;
		if (status === "completed") {
			return {
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
		if (status === "downloading" || status === "finished") {
			const parsedPercent = typeof parsed.percent === "number" ? parsed.percent : null;
			const fragmentIndex = typeof parsed.fragmentIndex === "number" ? parsed.fragmentIndex : null;
			const fragmentCount = typeof parsed.fragmentCount === "number" ? parsed.fragmentCount : null;
			const computedPercent = this.computeFragmentOverallPercent(parsedPercent, fragmentIndex, fragmentCount) ?? parsedPercent;
			return {
				type: "download_status",
				raw: line,
				extractor: "download",
				videoId: normalizedVideoId,
				message: line,
				requestId,
				workerId: this.workerId,
				percent: this.normalizeInProgressPercent(computedPercent),
				speed: parsed.speed ?? null,
				eta: parsed.eta ?? null,
				fragmentIndex,
				fragmentCount,
			};
		}
		return {
			type: "status",
			raw: line,
			extractor: "download",
			videoId: normalizedVideoId,
			message: line,
			requestId,
			workerId: this.workerId,
		};
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
			log.error("YtdlpPythonService error", { error });
			const errorMessage = typeof error === "string" ? error : error.message || "Unknown error";
			pending.reject(new Error(errorMessage));
		} else {
			pending.resolve(result);
		}
	}
	private splitLeadingJsonObject(value: string): { json: string; rest: string } | null {
		if (!value.startsWith("{")) {
			return null;
		}
		let depth = 0;
		let inString = false;
		let escaped = false;
		for (let index = 0; index < value.length; index += 1) {
			const char = value[index];
			if (inString) {
				if (escaped) {
					escaped = false;
				} else if (char === "\\") {
					escaped = true;
				} else if (char === '"') {
					inString = false;
				}
				continue;
			}
			if (char === '"') {
				inString = true;
				continue;
			}
			if (char === "{") {
				depth += 1;
				continue;
			}
			if (char === "}") {
				depth -= 1;
				if (depth === 0) {
					return {
						json: value.slice(0, index + 1),
						rest: value.slice(index + 1).trim(),
					};
				}
			}
		}
		return null;
	}
	private parseYtdlpPayload(line: string) {
		if (!line.startsWith("{")) return null;
		const splitPayload = this.splitLeadingJsonObject(line);
		if (!splitPayload) return null;
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
		}>(splitPayload.json);
		if (!parsed || typeof parsed !== "object") return null;
		return parsed;
	}
	private splitMixedOutputLine(line: string): string[] {
		const trimmed = line.trim();
		if (!trimmed) return [];
		const parts: string[] = [];
		let remaining = trimmed;

		while (remaining.length > 0) {
			if (remaining.startsWith("{")) {
				const splitPayload = this.splitLeadingJsonObject(remaining);
				if (!splitPayload) {
					parts.push(remaining);
					break;
				}
				parts.push(splitPayload.json);
				remaining = splitPayload.rest;
				continue;
			}

			const nextJsonIndex = remaining.indexOf("{");
			if (nextJsonIndex < 0) {
				parts.push(remaining);
				break;
			}
			const head = remaining.slice(0, nextJsonIndex).trim();
			if (head) {
				parts.push(head);
			}
			remaining = remaining.slice(nextJsonIndex).trimStart();
		}

		return parts.filter((part) => part.length > 0);
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
		const outputEvent = this.createTaggedProgressOutputEvent(parsed);
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
