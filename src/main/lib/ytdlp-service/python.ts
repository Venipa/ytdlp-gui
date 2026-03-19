import { EventEmitter } from "node:events";
import { existsSync, mkdirSync } from "node:fs";
import path, { dirname, join, resolve } from "node:path";
import { parseJson, stringifyJson } from "@shared/json";
import { createLogger } from "@shared/logger";
import { app } from "electron";
import { PythonShell } from "python-shell";
import ytdlPyWorkerPath from "./worker.py?asset&asarUnpack";
const log = createLogger("ytdlp-py-service");
let initialized = false;
// Helper: Generate a unique ID for RPC calls
function genId(): string {
	return Math.random().toString(36).substr(2, 9);
}

function buildPythonPath(workerScriptPath: string): string | undefined {
	const candidateDirs: string[] = [
		resolve(dirname(workerScriptPath), "..", "resources", "python-deps"),
		join(process.resourcesPath, "app.asar.unpacked", "resources", "python-deps"),
	];
	const existingDirs = candidateDirs.filter((directoryPath) => existsSync(directoryPath));
	if (existingDirs.length === 0) {
		return process.env.PYTHONPATH;
	}

	const delimiter = process.platform === "win32" ? ";" : ":";
	const existingPythonPath = process.env.PYTHONPATH ? [process.env.PYTHONPATH] : [];
	return [...existingDirs, ...existingPythonPath].join(delimiter);
}

interface RpcRequest {
	id: string;
	method: string;
	params?: string;
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

interface YtdlpServiceOptions {
	pythonPath?: string;
	pythonOptions?: string[];
	cwd?: string;
}
class YtdlpPythonService {
	private pyshell: PythonShell;
	private pending: Record<string, PendingCall> = {};
	private readonly outputEmitter = new EventEmitter();
	private readonly readyPromise: Promise<void>;
	private resolveReadyPromise: (() => void) | null = null;
	private rejectReadyPromise: ((reason: unknown) => void) | null = null;

	constructor(options: YtdlpServiceOptions = {}) {
		if (initialized) throw new Error("YtdlpPythonService already initialized");
		this.readyPromise = new Promise<void>((resolve, reject) => {
			this.resolveReadyPromise = resolve;
			this.rejectReadyPromise = reject;
		});
		const workerScriptPath = ytdlPyWorkerPath;
		let cwd = options.cwd ?? path.join(app.getPath("userData"), "ytdlp_cache");
		try {
			if (!existsSync(cwd)) mkdirSync(cwd, { recursive: true });
		} finally {
			cwd = undefined as any;
		}
		const pythonPathEnv = buildPythonPath(workerScriptPath);
		this.pyshell = new PythonShell(workerScriptPath, {
			pythonPath: options.pythonPath,
			cwd,
			pythonOptions: options.pythonOptions || ["-u"],
			env: {
				...process.env,
				...(pythonPathEnv ? { PYTHONPATH: pythonPathEnv } : {}),
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
			windowsHide: true,
			mode: "text",
		});
		this.pyshell.on("message", this.onPythonMessage.bind(this));
		this.pyshell.on("error", (err: Error) => {
			this.rejectAllPending(err);
			this.rejectReady(err);
		});
		this.pyshell.on("close", (err: Error | null) => {
			const closeError = err ?? new Error("Python process closed");
			this.rejectAllPending(closeError);
			this.rejectReady(closeError);
			log.warn("YtdlpPythonService closed", { error: err });
		});
		process.on("SIGINT", () => {
			log.info("SIGINT received");
			this.shutdown();
		});
		process.on("SIGTERM", () => {
			log.info("SIGTERM received");
			this.shutdown();
		});
		log.info("YtdlpPythonService initialized");
		initialized = true;
	}
	// Sends a typed JSON RPC call to Python
	call(method: string, params?: Record<string, unknown>, callOptions?: RpcCallOptions): Promise<unknown> {
		const id = genId();
		const req: RpcRequest = { id, method, params: stringifyJson(params ?? {}) };
		log.debug("Sending RPC request", { id, method, params });
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
	private emitOutput(event: YtdlpOutputEvent): void {
		this.outputEmitter.emit("output", event);
	}
	private getActiveRequestIdByMethod(method: string): string | null {
		const activeEntry = Object.entries(this.pending).find(([, pendingCall]) => pendingCall.method === method);
		return activeEntry ? activeEntry[0] : null;
	}
	private parseDownloadStatus(progressLine: YtdlpProgressLine, rawLine: string): YtdlpOutputEvent {
		const requestId = this.getActiveRequestIdByMethod("download");
		const completedMatch = /^100(?:\.0+)?%\s+of\s+.+\sin\s+.+$/i.exec(progressLine.message);
		if (completedMatch) {
			return {
				type: "completed",
				raw: rawLine,
				extractor: progressLine.extractor,
				videoId: progressLine.id,
				message: progressLine.message,
				requestId,
				percent: 100,
			};
		}

		const progressMatch = /^(\d+(?:\.\d+)?)%\s+of\s+.+?(?:\s+at\s+(.+?))?(?:\s+ETA\s+(.+?))?(?:\s+\(frag\s+(\d+)\/(\d+)\))?$/i.exec(progressLine.message);
		if (!progressMatch) {
			return {
				type: "status",
				raw: rawLine,
				extractor: progressLine.extractor,
				videoId: progressLine.id,
				message: progressLine.message,
				requestId,
			};
		}

		return {
			type: "download_status",
			raw: rawLine,
			extractor: progressLine.extractor,
			videoId: progressLine.id,
			message: progressLine.message,
			requestId,
			percent: Number.parseFloat(progressMatch[1]),
			speed: progressMatch[2] ?? null,
			eta: progressMatch[3] ?? null,
			fragmentIndex: progressMatch[4] ? Number.parseInt(progressMatch[4], 10) : null,
			fragmentCount: progressMatch[5] ? Number.parseInt(progressMatch[5], 10) : null,
		};
	}
	private parseStatusOutputEvent(rawLine: string, progressLine: YtdlpProgressLine): YtdlpOutputEvent {
		if (progressLine.extractor.toLowerCase() === "download") {
			return this.parseDownloadStatus(progressLine, rawLine);
		}
		return {
			type: "status",
			raw: rawLine,
			extractor: progressLine.extractor,
			videoId: progressLine.id,
			message: progressLine.message,
			requestId: this.getActiveRequestIdByMethod("download"),
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
		const parsed = parseJson<{ result?: unknown; id?: string; error?: string }>(line);
		if (!parsed || typeof parsed !== "object") return null;
		return parsed;
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

		for (const line of lines) {
			const progressLine = this.parseYtdlpProgressLine(line);
			if (progressLine) {
				const outputEvent = this.parseStatusOutputEvent(line, progressLine);
				this.emitOutput(outputEvent);
				log.debug("yt-dlp output", outputEvent);
				continue;
			}

			const parsedJson = this.parseYtdlpPayload(line);
			if (parsedJson) {
				const resultObject = parsedJson.result && typeof parsedJson.result === "object" ? (parsedJson.result as { id?: string }) : null;
				log.debug("yt-dlp JSON RPC response", { videoId: resultObject?.id ?? "unknown" });
				this.onPythonRpcResponse(parsedJson as RpcResponse);
				continue;
			}

			if (line.startsWith("Deleting original file ")) {
				const outputEvent: YtdlpOutputEvent = {
					type: "status",
					raw: line,
					extractor: "postprocess",
					videoId: null,
					message: line,
					requestId: this.getActiveRequestIdByMethod("download"),
				};
				this.emitOutput(outputEvent);
				log.debug("yt-dlp output", outputEvent);
				continue;
			}

			log.debug("Python non-JSON output", { line });
		}
	}

	// Convenience wrapper for yt-dlp "extract_info"
	extractInfo(url: string, options?: Record<string, unknown>): Promise<unknown> {
		return this.call("extract_info", { url, options });
	}

	// Convenience wrapper for yt-dlp "get_version"
	getVersion(): Promise<string> {
		return this.call("get_version") as Promise<string>;
	}

	// Convenience wrapper for yt-dlp "download"
	download(url: string, options?: Record<string, unknown>, callOptions?: RpcCallOptions): Promise<unknown> {
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

export default YtdlpPythonService;
