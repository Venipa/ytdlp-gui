import { type ChildProcess, spawn } from "node:child_process";
import { mkdirSync } from "node:fs";
import { createLogger } from "@shared/logger";
import { buildExtractArgs, getYtdlpCachePath } from "./argv";
import { getYtdlpBinaryPath, hasYtdlpBinary } from "./binary";
import { getConcurrencyDomain, getConcurrencyPolicy } from "./concurrency";
import type { YtdlpOptions } from "./types";

const EXTRACT_TIMEOUT_MS = 5 * 60 * 1000;
const log = createLogger("ytdlp-extract-pool");

interface ExtractTask {
	url: string;
	options?: Partial<YtdlpOptions>;
	resolve: (value: unknown) => void;
	reject: (reason: unknown) => void;
}

interface ActiveExtract {
	domain: string;
	process: ChildProcess;
	reject: (reason: unknown) => void;
}

function parseYtdlpJson(stdout: string): unknown {
	const trimmed = stdout.trim();
	const start = trimmed.indexOf("{");
	const end = trimmed.lastIndexOf("}");
	if (start < 0 || end <= start) {
		throw new Error("yt-dlp extract produced no JSON");
	}
	return JSON.parse(trimmed.slice(start, end + 1));
}

export class YtdlpExtractPool {
	private readonly queue: ExtractTask[] = [];
	private readonly active = new Set<ActiveExtract>();
	private shuttingDown = false;

	constructor() {
		this.registerProcessShutdown();
	}

	get liveCount(): number {
		return this.active.size;
	}

	get busyCount(): number {
		return this.active.size;
	}

	async extractInfo(url: string, options?: Partial<YtdlpOptions>): Promise<unknown> {
		if (this.shuttingDown) {
			throw new Error("yt-dlp extract pool is shutting down");
		}
		if (!hasYtdlpBinary()) {
			throw new Error("yt-dlp binary is missing");
		}
		return await new Promise<unknown>((resolve, reject) => {
			this.queue.push({ url, options, resolve, reject });
			this.schedule();
		});
	}

	async killAll(reason: string): Promise<void> {
		log.info("Killing extract workers", { reason, count: this.active.size });
		for (const job of [...this.active]) {
			job.reject(new Error(`Extract worker stopped (${reason})`));
			if (!job.process.killed) job.process.kill();
		}
		this.active.clear();
	}

	shutdown(): void {
		this.shuttingDown = true;
		for (const task of this.queue.splice(0)) {
			task.reject(new Error("yt-dlp extract pool shutdown"));
		}
		void this.killAll("shutdown");
	}

	private schedule(): void {
		if (this.shuttingDown) return;
		while (this.queue.length > 0) {
			const taskIndex = this.findRunnableTaskIndex();
			if (taskIndex === -1) return;
			const [task] = this.queue.splice(taskIndex, 1);
			this.run(task);
		}
	}

	private findRunnableTaskIndex(): number {
		const policy = getConcurrencyPolicy();
		if (policy.mode === "global") return this.active.size < policy.limit ? 0 : -1;
		return this.queue.findIndex((task) => {
			const domain = getConcurrencyDomain(task.url);
			let activeForDomain = 0;
			for (const job of this.active) {
				if (job.domain === domain) activeForDomain += 1;
			}
			return activeForDomain < policy.limit;
		});
	}

	private run(task: ExtractTask): void {
		const cacheDir = getYtdlpCachePath();
		mkdirSync(cacheDir, { recursive: true });
		const child = spawn(getYtdlpBinaryPath(), buildExtractArgs(task.url, task.options), {
			stdio: ["ignore", "pipe", "pipe"],
			windowsHide: true,
			cwd: cacheDir,
		});
		const job: ActiveExtract = {
			domain: getConcurrencyDomain(task.url),
			process: child,
			reject: task.reject,
		};
		this.active.add(job);
		let stdout = "";
		let stderr = "";
		const timeout = setTimeout(() => {
			this.finish(job, () => task.reject(new Error("yt-dlp extract timed out")));
			if (!child.killed) child.kill();
		}, EXTRACT_TIMEOUT_MS);
		child.stdout.setEncoding("utf8");
		child.stderr.setEncoding("utf8");
		child.stdout.on("data", (chunk: string) => {
			stdout += chunk;
		});
		child.stderr.on("data", (chunk: string) => {
			stderr += chunk;
			if (stderr.length > 32_000) stderr = stderr.slice(-16_000);
		});
		child.on("error", (error) => {
			clearTimeout(timeout);
			this.finish(job, () => task.reject(error));
		});
		child.on("close", (code) => {
			clearTimeout(timeout);
			this.finish(job, () => {
				try {
					task.resolve(parseYtdlpJson(stdout));
				} catch (error) {
					const detail = stderr.trim() || (error instanceof Error ? error.message : String(error));
					task.reject(new Error(code ? `yt-dlp extract exited (${code}): ${detail}` : detail));
				}
			});
		});
		log.info("Spawned extract", { url: task.url, live: this.active.size });
	}

	private finish(job: ActiveExtract, settle: () => void): void {
		if (!this.active.delete(job)) return;
		try {
			settle();
		} finally {
			this.schedule();
		}
	}

	private registerProcessShutdown(): void {
		const shutdown = () => this.shutdown();
		process.once("exit", shutdown);
		process.once("SIGINT", shutdown);
		process.once("SIGTERM", shutdown);
	}
}

let extractPool: YtdlpExtractPool | null = null;

export function getYtdlpExtractPool(): YtdlpExtractPool {
	if (!extractPool) {
		extractPool = new YtdlpExtractPool();
	}
	return extractPool;
}
