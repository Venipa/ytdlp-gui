import { parseJson, stringifyJson } from "@shared/json";
import { createLogger } from "@shared/logger";
import { PythonShell } from "python-shell";
import ytdlPyWorker from "./worker.py?asset&asarUnpack";
const log = createLogger("ytdlp-py-service");
let initialized = false;
// Helper: Generate a unique ID for RPC calls
function genId(): string {
	return Math.random().toString(36).substr(2, 9);
}

interface RpcRequest {
	id: string;
	method: string;
	params?: Record<string, unknown>;
}

interface RpcResponse {
	id: string;
	result?: unknown;
	error?: string | { message: string; code?: number; data?: unknown };
}

interface PendingCall {
	resolve: (value: unknown) => void;
	reject: (reason: unknown) => void;
}

interface YtdlpServiceOptions {
	pythonPath?: string;
	pythonOptions?: string[];
	scriptPath?: string;
}

class YtdlpPythonService {
	private pyshell: PythonShell;
	private pending: Record<string, PendingCall> = {};

	constructor(options: YtdlpServiceOptions = {}) {
		if (initialized) throw new Error("YtdlpPythonService already initialized");
		this.pyshell = new PythonShell(ytdlPyWorker, {
			pythonPath: options.pythonPath,
			pythonOptions: options.pythonOptions || ["-u"],
			scriptPath: options.scriptPath,
			parser(param) {
				log.debug("PythonShell parser", { param });
				return parseJson(param);
			},
			formatter(param) {
				log.debug("PythonShell formatter", { param });
				return stringifyJson(param);
			},
			windowsHide: true,
			mode: "json",
		});
		this.pyshell.on("message", this.onPythonMessage.bind(this));
		this.pyshell.on("error", (err: Error) => {
			// Reject all pending on process error
			Object.values(this.pending).forEach((p) => p.reject(err));
			this.pending = {};
		});
		this.pyshell.on("close", () => {
			// Reject all pending on process exit
			Object.values(this.pending).forEach((p) => p.reject(new Error("Python process closed")));
			this.pending = {};
			log.warn("YtdlpPythonService closed");
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
	call(method: string, params?: Record<string, unknown>): Promise<unknown> {
		const id = genId();
		const req: RpcRequest = { id, method, params };
		return new Promise((resolve, reject) => {
			this.pending[id] = { resolve, reject };
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
	// Internal: Handle typed JSON messages from Python
	private onPythonMessage(msg: unknown): void {
		// Validate and parse JSON RPC response
		if (!msg || typeof msg !== "object") {
			log.error("Invalid message format:", msg);
			return;
		}
		log.debug("Python message:", msg);

		const response = msg as RpcResponse;
		const { id, result, error } = response;

		if (!id || typeof id !== "string") {
			log.error("Invalid response ID:", response);
			return;
		}

		if (id === "ready") {
			this._ready = true;
			this._binVersion = (result as { version: string }).version;
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

	// Convenience wrapper for yt-dlp "extract_info"
	extractInfo(url: string, options?: Record<string, unknown>): Promise<unknown> {
		return this.call("extract_info", { url, options });
	}

	// Convenience wrapper for yt-dlp "get_version"
	getVersion(): Promise<string> {
		return this.call("get_version") as Promise<string>;
	}

	// Convenience wrapper for yt-dlp "download"
	download(url: string, options?: Record<string, unknown>): Promise<unknown> {
		return this.call("download", { url, options });
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
