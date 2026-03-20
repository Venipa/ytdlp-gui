import YtdlpWorkerManager from "@main/lib/ytdlp-service/manager";
const createYtdlpService = () => {
	const service = new YtdlpWorkerManager({});
	process.on("exit", () => {
		service.shutdown();
	});
	process.on("SIGINT", () => {
		service.shutdown();
	});
	process.on("SIGTERM", () => {
		service.shutdown();
	});
	return service;
};

export default createYtdlpService;
