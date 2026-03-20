import YtdlpWorkerManager from "@main/lib/ytdlp-service/manager";
import { executableIsAvailable } from "../bin.utils";
const createYtdlpService = () => {
	const pythonPath = executableIsAvailable("python") || executableIsAvailable("python3");
	if (!pythonPath) {
		throw new Error("Python not found");
	}
	const service = new YtdlpWorkerManager({
		pythonPath: pythonPath,
	});
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
