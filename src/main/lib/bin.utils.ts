import { execSync } from "node:child_process";
import { statSync } from "node:fs";
import { createLogger } from "@shared/logger";
import platform from "./platform";

const shell = (cmd: string) => execSync(cmd, { encoding: "utf8" });
const log = createLogger("bin.utils");
export function executableIsAvailable(name: string) {
	try {
		const whichCmd = platform.isWindows ? "where" : "which";
		const execPath = shell(`${whichCmd} ${name}`)?.trim();
		log.debug("executableIsAvailable", { name, execPath });
		return (statSync(execPath) && execPath) || null;
	} catch (error) {
		return null;
	}
}
export function fileExists(path: string) {
	try {
		return (statSync(path) && path) || null;
	} catch (error) {
		return null;
	}
}
