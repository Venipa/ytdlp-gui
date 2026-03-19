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
	return service;
};

export default createYtdlpService;
