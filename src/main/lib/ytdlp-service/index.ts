import YtdlpPythonService from "@main/lib/ytdlp-service/python";
import { executableIsAvailable } from "../bin.utils";
const createYtdlpService = () => {
	const pythonPath = executableIsAvailable("python");
	if (!pythonPath) {
		throw new Error("Python not found");
	}
	const service = new YtdlpPythonService({
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
