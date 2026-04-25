import { logger } from "@shared/logger";
import YtdlpWorkerManager from "./manager";

const createYtdlpService = () => {
	const service = new YtdlpWorkerManager({});
	let isShuttingDown = false;
	const shutdown = (reason: string) => {
		if (isShuttingDown) return;
		isShuttingDown = true;
		logger.info("Shutting down ytdlp service", { reason });
		service.shutdown();
	};

	const processListeners: Array<{ event: "exit" | NodeJS.Signals; handler: (...args: unknown[]) => void }> = [];
	const registerProcessListener = (event: "exit" | NodeJS.Signals, handler: (...args: unknown[]) => void): void => {
		process.on(event, handler as never);
		processListeners.push({ event, handler });
	};
	const removeProcessListeners = (): void => {
		for (const listener of processListeners) {
			process.off(listener.event, listener.handler as never);
		}
		processListeners.length = 0;
	};

	registerProcessListener("exit", () => shutdown("process:exit"));
	registerProcessListener("SIGINT", () => {
		shutdown("signal:SIGINT");
		process.exit(0);
	});
	registerProcessListener("SIGTERM", () => {
		shutdown("signal:SIGTERM");
		process.exit(0);
	});
	registerProcessListener("SIGABRT", () => {
		shutdown("signal:SIGABRT");
		process.exit(0);
	});

	if (import.meta.env.DEV) {
		logger.info("Vite dev mode, setting up hot reload listeners");
		import.meta.hot?.on("vite:beforeFullReload", () => {
			shutdown("hmr:vite:beforeFullReload");
		});
		import.meta.hot?.on("vite:beforeUpdate", () => {
			shutdown("hmr:vite:beforeUpdate");
		});
		import.meta.hot?.dispose(() => {
			shutdown("hmr:dispose");
			removeProcessListeners();
		});
	}

	return service;
};

export default createYtdlpService;
