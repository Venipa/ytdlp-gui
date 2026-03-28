import YtdlpWorkerManager from "@main/lib/ytdlp-service/manager";
import { logger } from "@shared/logger";
const createYtdlpService = () => {
	const service = new YtdlpWorkerManager({});
	const shutdown = () => {
		service.shutdown();
	};
	process.on("exit", shutdown);
	process.on("SIGINT", shutdown);
	process.on("SIGTERM", shutdown);
	process.on("SIGABRT", shutdown);
	process.on("SIGKILL", (error) => {
		logger.error("SIGKILL received", { error });
		shutdown();
	});
	return service;
};

export default createYtdlpService;
