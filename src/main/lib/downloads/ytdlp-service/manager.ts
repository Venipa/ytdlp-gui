import { EventEmitter } from "node:events";
import { appStore } from "@main/stores/app/app.store";
import { createLogger } from "@shared/logger";
import YtdlpPythonWorkerService, { YtdlpOutputEvent } from "./python";

const log = createLogger("ytdlp-worker-manager");

interface YtdlpWorkerManagerOptions {
	pythonPath?: string;
	pythonOptions?: string[];
	cwd?: string;
}

interface ManagedWorker {
	id: string;
	service: YtdlpPythonWorkerService;
	busy: boolean;
	retireWhenIdle: boolean;
	disposeOutput: () => void;
	disposeClose: () => void;
}

type TaskRunner<T> = (worker: ManagedWorker) => Promise<T>;

interface TaskItem {
	run: TaskRunner<unknown>;
	resolve: (value: unknown) => void;
	reject: (reason: unknown) => void;
}

function normalizeConcurrency(value: number): number {
	if (!Number.isFinite(value) || value < 1) {
		return 1;
	}
	return Math.floor(value);
}

class YtdlpWorkerManager {
	private readonly outputEmitter = new EventEmitter();
	private readonly workers = new Map<string, ManagedWorker>();
	private readonly taskQueue: TaskItem[] = [];
	private nextWorkerId = 1;
	private targetConcurrency = normalizeConcurrency(appStore.store.features.concurrentDownloads);
	private readonly options: YtdlpWorkerManagerOptions;
	private isShuttingDown = false;
	private restartTimer: NodeJS.Timeout | null = null;

	private static readonly WORKER_RECOVERY_DELAY_MS = 1500;

	constructor(options: YtdlpWorkerManagerOptions = {}) {
		this.options = options;
		this.reconcileWorkers();
		appStore.onDidChange("features.concurrentDownloads", (concurrentDownloads: any) => {
			if (this.isShuttingDown) return;
			this.targetConcurrency = normalizeConcurrency(concurrentDownloads);
			log.info("Updated ytdlp worker concurrency", {
				raw: concurrentDownloads,
				normalized: this.targetConcurrency,
			});
			this.reconcileWorkers();
			this.schedule();
		});
	}

	private createWorker(): ManagedWorker {
		const workerId = `worker-${this.nextWorkerId++}`;
		const service = new YtdlpPythonWorkerService({
			...this.options,
			workerId,
		});
		const managedWorker: ManagedWorker = {
			id: workerId,
			service,
			busy: false,
			retireWhenIdle: false,
			disposeOutput: service.onOutput((event: YtdlpOutputEvent) => {
				this.outputEmitter.emit("output", event);
			}),
			disposeClose: () => {},
		};
		managedWorker.disposeClose = service.onClose((error) => {
			this.handleWorkerCrash(managedWorker, error);
		});
		this.workers.set(workerId, managedWorker);
		log.info("Created ytdlp worker", { workerId, totalWorkers: this.workers.size });
		return managedWorker;
	}

	private handleWorkerCrash(worker: ManagedWorker, error: Error): void {
		if (this.isShuttingDown) return;
		if (!this.workers.has(worker.id)) return;
		this.workers.delete(worker.id);
		worker.disposeOutput();
		worker.disposeClose();
		worker.busy = false;
		log.warn("ytdlp worker crashed, scheduling revive", {
			workerId: worker.id,
			error: error.message,
		});
		this.scheduleWorkerRecovery();
	}

	private scheduleWorkerRecovery(): void {
		if (this.isShuttingDown) return;
		if (this.restartTimer) return;
		this.restartTimer = setTimeout(() => {
			this.restartTimer = null;
			if (this.isShuttingDown) return;
			this.reconcileWorkers();
			this.schedule();
		}, YtdlpWorkerManager.WORKER_RECOVERY_DELAY_MS);
	}

	private shutdownWorker(worker: ManagedWorker): void {
		if (!this.workers.has(worker.id)) return;
		this.workers.delete(worker.id);
		worker.disposeOutput();
		worker.disposeClose();
		worker.service.shutdown();
		log.info("Stopped ytdlp worker", { workerId: worker.id, totalWorkers: this.workers.size });
	}

	private getIdleWorker(): ManagedWorker | null {
		for (const worker of this.workers.values()) {
			if (!worker.busy && !worker.retireWhenIdle) {
				return worker;
			}
		}
		return null;
	}

	private countEnabledWorkers(): number {
		return Array.from(this.workers.values()).filter((worker) => !worker.retireWhenIdle).length;
	}

	private activateWorkers(count: number): number {
		if (count <= 0) return 0;
		let activated = 0;
		for (const worker of this.workers.values()) {
			if (activated >= count) break;
			if (worker.retireWhenIdle) {
				worker.retireWhenIdle = false;
				activated++;
				log.info("Reused ytdlp worker", { workerId: worker.id });
			}
		}
		return activated;
	}

	private retireWorkers(count: number): void {
		if (count <= 0) return;
		const workers = Array.from(this.workers.values()).reverse();
		let remaining = count;
		for (const worker of workers) {
			if (remaining <= 0) break;
			if (worker.retireWhenIdle) continue;
			if (!worker.busy) {
				this.shutdownWorker(worker);
				remaining--;
				continue;
			}
			worker.retireWhenIdle = true;
			remaining--;
			log.info("Marked ytdlp worker for retirement", { workerId: worker.id });
		}
	}

	private reconcileWorkers(): void {
		const enabledCount = this.countEnabledWorkers();
		if (enabledCount < this.targetConcurrency) {
			const missing = this.targetConcurrency - enabledCount;
			const activated = this.activateWorkers(missing);
			const toCreate = missing - activated;
			for (let i = 0; i < toCreate; i++) {
				this.createWorker();
			}
			return;
		}
		if (enabledCount > this.targetConcurrency) {
			this.retireWorkers(enabledCount - this.targetConcurrency);
		}
	}

	private schedule(): void {
		while (!this.isShuttingDown && this.taskQueue.length > 0) {
			const worker = this.getIdleWorker();
			if (!worker) return;

			const task = this.taskQueue.shift();
			if (!task) return;

			worker.busy = true;
			void task
				.run(worker)
				.then((result) => task.resolve(result))
				.catch((error) => task.reject(error))
				.finally(() => {
					worker.busy = false;
					if (worker.retireWhenIdle) {
						this.shutdownWorker(worker);
					}
					this.reconcileWorkers();
					this.schedule();
				});
		}
	}

	private enqueueTask<T>(run: TaskRunner<T>): Promise<T> {
		if (this.isShuttingDown) {
			return Promise.reject(new Error("YtdlpWorkerManager is shutting down"));
		}
		return new Promise<T>((resolve, reject) => {
			const taskRun: TaskRunner<unknown> = async (worker) => await run(worker);
			this.taskQueue.push({
				run: taskRun,
				resolve: (value) => resolve(value as T),
				reject,
			});
			this.reconcileWorkers();
			this.schedule();
		});
	}

	private async runOnWorker<T>(worker: ManagedWorker, fn: (service: YtdlpPythonWorkerService) => Promise<T>): Promise<T> {
		await worker.service.waitReady();
		return await fn(worker.service);
	}

	waitReady(): Promise<void> {
		this.reconcileWorkers();
		return Promise.all(Array.from(this.workers.values()).map((worker) => worker.service.waitReady())).then(() => undefined);
	}

	getVersion(): Promise<string> {
		return this.enqueueTask((worker) => this.runOnWorker(worker, async (service) => await service.getVersion()));
	}

	extractInfo(url: string, options?: Record<string, unknown>): Promise<unknown> {
		return this.enqueueTask((worker) => this.runOnWorker(worker, async (service) => await service.extractInfo(url, options)));
	}

	download(url: string, options?: Record<string, unknown>, callOptions?: { onRequestCreated?: (id: string) => void }): Promise<unknown> {
		return this.enqueueTask((worker) => this.runOnWorker(worker, async (service) => await service.download(url, options, callOptions)));
	}

	onOutput(listener: (event: YtdlpOutputEvent) => void): () => void {
		this.outputEmitter.on("output", listener);
		return () => {
			this.outputEmitter.off("output", listener);
		};
	}

	shutdown(): void {
		if (this.isShuttingDown) return;
		this.isShuttingDown = true;
		if (this.restartTimer) {
			clearTimeout(this.restartTimer);
			this.restartTimer = null;
		}
		const error = new Error("YtdlpWorkerManager shutdown");
		while (this.taskQueue.length > 0) {
			const task = this.taskQueue.shift();
			task?.reject(error);
		}
		for (const worker of Array.from(this.workers.values())) {
			this.shutdownWorker(worker);
		}
	}
}

export default YtdlpWorkerManager;
