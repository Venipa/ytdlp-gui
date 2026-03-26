import { execFileSync } from "node:child_process";
import { chmod, cp, mkdir, readdir, rm, stat, writeFile } from "node:fs/promises";
import path, { basename, extname, join, resolve } from "node:path";
import type { Plugin } from "vite";
import { Logger } from "./src/shared/logger";

type PlatformTargetKey = "win32-x64" | "linux-x64" | "darwin-arm64";
type SourceType = "archive" | "binary";
type ArchiveType = "zip" | "tar.xz";
type BinaryKind = "ffmpeg" | "ffprobe";
const log = new Logger("ffmpeg-vite-plugin");
interface SourceDescriptor {
	type: SourceType;
	url: string;
	archiveType?: ArchiveType;
}

interface BinarySourceConfig {
	ffmpeg: readonly SourceDescriptor[];
	ffprobe: readonly SourceDescriptor[];
}

const BINARY_EXT = process.platform === "win32" ? ".exe" : "";
const TMP_ROOT = resolve(".tmp", "ffmpeg");
const CACHE_ROOT = join(TMP_ROOT, "downloads");
const EXTRACT_ROOT = join(TMP_ROOT, "extract");
const OUTPUT_ROOT = resolve("out", "main", "resources", "ffmpeg");
const OUTPUT_BIN_ROOT = join(OUTPUT_ROOT, "bin");

const ffmpegSourceUrls: Record<PlatformTargetKey, BinarySourceConfig> = {
	"win32-x64": {
		ffmpeg: [
			{
				type: "archive",
				archiveType: "zip",
				url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-win64-lgpl-shared-7.1.zip",
			},
		],
		ffprobe: [
			{
				type: "archive",
				archiveType: "zip",
				url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-win64-lgpl-shared-7.1.zip",
			},
		],
	},
	"linux-x64": {
		ffmpeg: [
			{
				type: "archive",
				archiveType: "tar.xz",
				url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-lgpl-shared-7.1.tar.xz",
			},
		],
		ffprobe: [
			{
				type: "archive",
				archiveType: "tar.xz",
				url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-lgpl-shared-7.1.tar.xz",
			},
		],
	},
	"darwin-arm64": {
		ffmpeg: [
			{
				type: "binary",
				url: "https://github.com/descriptinc/ffmpeg-ffprobe-static/releases/latest/download/ffmpeg-darwin-arm64",
			},
		],
		ffprobe: [
			{
				type: "binary",
				url: "https://github.com/descriptinc/ffmpeg-ffprobe-static/releases/latest/download/ffprobe-darwin-arm64",
			},
		],
	},
};

function resolveTargetKey(): PlatformTargetKey {
	const key = `${process.platform}-${process.arch}`;
	if (key in ffmpegSourceUrls) {
		return key as PlatformTargetKey;
	}
	throw new Error(`[ffmpeg-vite-plugin] Unsupported build target: ${key}`);
}

function sanitizeFilename(input: string): string {
	return input.replace(/[^\w.-]+/g, "_");
}

function buildBinaryFileName(binaryKind: BinaryKind): string {
	return `${binaryKind}${BINARY_EXT}`;
}

async function ensureDirectory(directoryPath: string): Promise<void> {
	await mkdir(directoryPath, { recursive: true });
}

async function readDirectoryRecursive(directoryPath: string): Promise<string[]> {
	const entries = await readdir(directoryPath, { withFileTypes: true });
	const nested = await Promise.all(
		entries.map(async (entry) => {
			const fullPath = join(directoryPath, entry.name);
			if (entry.isDirectory()) return readDirectoryRecursive(fullPath);
			return [fullPath];
		}),
	);
	return nested.flat();
}

async function pathExists(pathToCheck: string): Promise<boolean> {
	try {
		await stat(pathToCheck);
		return true;
	} catch {
		return false;
	}
}

async function downloadToCache(sourceUrl: string): Promise<string> {
	const urlPath = new URL(sourceUrl).pathname;
	const fileName = sanitizeFilename(basename(urlPath) || "download.bin");
	const destinationPath = join(CACHE_ROOT, fileName);
	try {
		await stat(destinationPath);
		return destinationPath;
	} catch {
		// Cache miss: continue to download.
	}

	const response = await fetch(sourceUrl, {
		headers: {
			"User-Agent": "ytdlp-gui-build",
		},
	});
	if (!response.ok) {
		throw new Error(`[ffmpeg-vite-plugin] Failed to download ${sourceUrl}: ${response.status} ${response.statusText}`);
	}

	const buffer = Buffer.from(await response.arrayBuffer());
	await writeFile(destinationPath, buffer);
	return destinationPath;
}

async function extractArchive(archivePath: string, archiveType: ArchiveType): Promise<string> {
	const archiveStem = sanitizeFilename(basename(archivePath, extname(archivePath)));
	const destinationPath = join(EXTRACT_ROOT, archiveStem);
	const extractionCacheKeyPath = join(TMP_ROOT, `${basename(archivePath)}.ckey`);
	if ((await pathExists(extractionCacheKeyPath)) && (await pathExists(destinationPath))) {
		return destinationPath;
	}

	await rm(destinationPath, { recursive: true, force: true });
	await ensureDirectory(destinationPath);

	if (archiveType === "zip") {
		if (process.platform === "win32") {
			execFileSync("powershell", ["-NoProfile", "-Command", `Expand-Archive -Path "${archivePath}" -DestinationPath "${destinationPath}" -Force`], {
				stdio: "inherit",
			});
		} else {
			execFileSync("unzip", ["-o", archivePath, "-d", destinationPath], { stdio: "inherit" });
		}
		await writeFile(extractionCacheKeyPath, destinationPath);
		return destinationPath;
	}

	execFileSync("tar", ["-xJf", archivePath, "-C", destinationPath], { stdio: "inherit" });
	await writeFile(extractionCacheKeyPath, destinationPath);
	return destinationPath;
}

async function findBinaryFilePath(directoryPath: string, binaryKind: BinaryKind): Promise<string | null> {
	const expectedFileName = buildBinaryFileName(binaryKind).toLowerCase();
	const files = await readDirectoryRecursive(directoryPath);
	const exactMatch = files.find((filePath) => path.basename(filePath).toLowerCase() === expectedFileName);
	if (exactMatch) return exactMatch;

	const relaxedMatch = files.find((filePath) => path.basename(filePath).toLowerCase().startsWith(binaryKind));
	return relaxedMatch ?? null;
}

async function resolveBinaryFromSources(binaryKind: BinaryKind, sources: readonly SourceDescriptor[]): Promise<string> {
	const errors: string[] = [];
	for (const source of sources) {
		try {
			const cachedPath = await downloadToCache(source.url);
			if (source.type === "binary") {
				return cachedPath;
			}
			if (!source.archiveType) {
				throw new Error(`Archive source is missing archiveType for ${source.url}`);
			}
			const extractedDir = await extractArchive(cachedPath, source.archiveType);
			const binaryPath = await findBinaryFilePath(extractedDir, binaryKind);
			if (!binaryPath) {
				throw new Error(`Could not find ${buildBinaryFileName(binaryKind)} in extracted archive`);
			}
			return binaryPath;
		} catch (error) {
			const message = error instanceof Error ? error.message : String(error);
			errors.push(`${source.url} -> ${message}`);
		}
	}
	throw new Error(`[ffmpeg-vite-plugin] Failed to resolve ${binaryKind} binary.\n${errors.join("\n")}`);
}

function resolveSourceBinDirectory(binaryPath: string): string | null {
	const parentDir = path.dirname(binaryPath);
	return path.basename(parentDir).toLowerCase() === "bin" ? parentDir : null;
}

async function copyBinArtifacts(ffmpegBinaryPath: string, ffprobeBinaryPath: string): Promise<string[]> {
	const ffmpegBinDir = resolveSourceBinDirectory(ffmpegBinaryPath);
	const ffprobeBinDir = resolveSourceBinDirectory(ffprobeBinaryPath);

	// await rm(OUTPUT_BIN_ROOT, { recursive: true, force: true });

	// Shared packages expose a `bin` directory that already contains ffmpeg, ffprobe and related runtime binaries.
	if (!ffmpegBinDir || !ffprobeBinDir || ffmpegBinDir !== ffprobeBinDir) {
		throw new Error("[ffmpeg-vite-plugin] Expected shared archive binaries to be located in the same `bin` directory.");
	}

	await cp(ffmpegBinDir, OUTPUT_BIN_ROOT, { recursive: true, force: true });

	if (process.platform !== "win32") {
		const copiedPaths = await readDirectoryRecursive(OUTPUT_BIN_ROOT);
		for (const copiedPath of copiedPaths) {
			try {
				await chmod(copiedPath, 0o755);
			} catch {
				// Best effort permission update for non-windows platforms.
			}
		}
	}
	return [OUTPUT_BIN_ROOT];
}

function resolvePackageRootFromBinary(binaryPath: string): string {
	const parentDir = path.dirname(binaryPath);
	return path.basename(parentDir).toLowerCase() === "bin" ? path.dirname(parentDir) : parentDir;
}

async function copySharedRuntimeArtifacts(ffmpegBinaryPath: string): Promise<string[]> {
	const copiedPaths: string[] = [];
	const packageRoot = resolvePackageRootFromBinary(ffmpegBinaryPath);
	const sharedFolders = ["lib", "include"] as const;
	for (const folderName of sharedFolders) {
		const folderPath = join(packageRoot, folderName);
		if (!(await pathExists(folderPath))) continue;
		const destinationPath = join(OUTPUT_ROOT, folderName);
		await cp(folderPath, destinationPath, { recursive: true, force: true });
		copiedPaths.push(destinationPath);
	}

	return copiedPaths;
}

async function resolveBinariesFromSharedArchive(source: SourceDescriptor): Promise<{ ffmpeg: string; ffprobe: string }> {
	if (source.type !== "archive" || !source.archiveType) {
		throw new Error("[ffmpeg-vite-plugin] Shared archive source requires archive type.");
	}
	const cachedPath = await downloadToCache(source.url);
	const extractedDir = await extractArchive(cachedPath, source.archiveType);
	const resolvedFfmpegPath = await findBinaryFilePath(extractedDir, "ffmpeg");
	const resolvedFfprobePath = await findBinaryFilePath(extractedDir, "ffprobe");
	if (!resolvedFfmpegPath || !resolvedFfprobePath) {
		throw new Error(`[ffmpeg-vite-plugin] Shared archive is missing ffmpeg or ffprobe binaries: ${source.url}`);
	}
	return {
		ffmpeg: resolvedFfmpegPath,
		ffprobe: resolvedFfprobePath,
	};
}

export default function ffmpegVitePlugin(): Plugin {
	return {
		name: "ffmpeg-vite-plugin",
		apply: "build",
    cacheKey: "ffmpeg-vite-build-cache",
		async closeBundle(error?: Error) {
			if (error) {
				log.error("Error during build:", error);
				return;
			}
			await ensureDirectory(CACHE_ROOT);
			await ensureDirectory(EXTRACT_ROOT);
			await ensureDirectory(OUTPUT_ROOT);
			const targetKey = resolveTargetKey();
			const sources = ffmpegSourceUrls[targetKey];
			let resolvedFfmpegPath: string;
			let resolvedFfprobePath: string;
			const sharedArchiveSource = sources.ffmpeg.find(
				(source) =>
					source.type === "archive" &&
					sources.ffprobe.some((probeSource) => probeSource.type === "archive" && probeSource.url === source.url && probeSource.archiveType === source.archiveType),
			);
			if (sharedArchiveSource?.type === "archive") {
				const resolvedPaths = await resolveBinariesFromSharedArchive(sharedArchiveSource);
				resolvedFfmpegPath = resolvedPaths.ffmpeg;
				resolvedFfprobePath = resolvedPaths.ffprobe;
			} else {
				resolvedFfmpegPath = await resolveBinaryFromSources("ffmpeg", sources.ffmpeg);
				resolvedFfprobePath = await resolveBinaryFromSources("ffprobe", sources.ffprobe);
			}
			const copiedBinArtifacts = await copyBinArtifacts(resolvedFfmpegPath, resolvedFfprobePath);
			const copiedSharedArtifacts = await copySharedRuntimeArtifacts(resolvedFfmpegPath);

			log.info("Prepared binaries", {
				targetKey,
				ffmpeg: join(OUTPUT_BIN_ROOT, buildBinaryFileName("ffmpeg")),
				ffprobe: join(OUTPUT_BIN_ROOT, buildBinaryFileName("ffprobe")),
				binArtifacts: copiedBinArtifacts,
				cacheRoot: CACHE_ROOT,
				sharedArtifacts: copiedSharedArtifacts,
			});
		},
	};
}
