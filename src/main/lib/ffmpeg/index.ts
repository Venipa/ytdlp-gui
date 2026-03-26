import { existsSync } from "node:fs";
import path from "node:path";

type BinaryKind = "ffmpeg" | "ffprobe";

const RESOURCE_SEGMENTS = ["out", "main", "resources", "ffmpeg"] as const;
const WINDOWS_EXTENSION = ".exe";

function getBinaryFilename(binaryKind: BinaryKind): string {
	return process.platform === "win32" ? `${binaryKind}${WINDOWS_EXTENSION}` : binaryKind;
}

function toUnpackedPath(candidatePath: string): string {
	if (import.meta.env.DEV) {
		return candidatePath;
	}
	return candidatePath.replace(/([\\/])app\.asar([\\/])/i, "$1app.asar.unpacked$2");
}

function resolveBinaryPath(binaryKind: BinaryKind): string {
	const filename = getBinaryFilename(binaryKind);
	const developmentPaths = [path.resolve(process.cwd(), ...RESOURCE_SEGMENTS, "bin", filename), path.resolve(process.cwd(), ...RESOURCE_SEGMENTS, filename)];
	const packagedPaths = [
		path.join(process.resourcesPath, "app.asar", ...RESOURCE_SEGMENTS, "bin", filename),
		path.join(process.resourcesPath, "app.asar", ...RESOURCE_SEGMENTS, filename),
	];
	const unpackedPaths = packagedPaths.map((candidatePath) => toUnpackedPath(candidatePath));
	const candidates = import.meta.env.PROD ? [...unpackedPaths, ...packagedPaths, ...developmentPaths] : [...developmentPaths, ...unpackedPaths, ...packagedPaths];
	const existingPath = candidates.find((candidatePath) => existsSync(candidatePath));
	return existingPath ?? candidates[0];
}

export const ffmpeg: string = resolveBinaryPath("ffmpeg");
export const ffprobe: string = resolveBinaryPath("ffprobe");
