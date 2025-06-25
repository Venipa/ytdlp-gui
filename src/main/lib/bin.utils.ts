import { execSync } from "node:child_process";
import { statSync } from "node:fs";
import platform from "./platform";

const shell = (cmd: string) => execSync(cmd, { encoding: "utf8" });

export function executableIsAvailable(name: string) {
	if (platform.isWindows) return null;
	try {
		const execPath = shell(`which ${name}`);
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
