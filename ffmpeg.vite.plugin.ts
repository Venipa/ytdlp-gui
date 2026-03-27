import { execFileSync } from "node:child_process";
import { chmod, cp, mkdir, readdir, rm, stat, writeFile } from "node:fs/promises";
import path, { basename, extname, join, resolve } from "node:path";
import type { Plugin } from "vite";
import { Logger } from "./src/shared/logger";

type PlatformTargetKey = "win32-x64" | "linux-x64" | "darwin-arm64";
type SourceType = "archive" | "binary";
type ArchiveType = "zip" | "tar";
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
				archiveType: "tar",
				url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-lgpl-shared-7.1.tar.xz",
			},
		],
		ffprobe: [
			{
				type: "archive",
				archiveType: "tar",
				url: "https://github.com/BtbN/FFmpeg-Builds/releases/download/latest/ffmpeg-n7.1-latest-linux64-lgpl-shared-7.1.tar.xz",
			},
		],
	},
	"darwin-arm64": {
		ffmpeg: [
			{
				type: "binary",
				url: "https://www.osxexperts.net/ffmpeg71arm.zip",
			},
		],
		ffprobe: [
			{
				type: "binary",
				url: "https://www.osxexperts.net/ffprobe71arm.zip",
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

function detectArchiveType(filePath: string): ArchiveType | null {
	const normalizedPath = filePath.toLowerCase();
	if (normalizedPath.endsWith(".zip")) return "zip";
	if (normalizedPath.endsWith(".tar.xz") || normalizedPath.endsWith(".txz")) return "tar";
	if (normalizedPath.endsWith(".tar")) return "tar";
	return null;
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

async function downloadToCache(sourceUrl: string, prefix?: string): Promise<string> {
	const urlPath = new URL(sourceUrl).pathname;
	const fileName = sanitizeFilename(prefix ? `${prefix}-${basename(urlPath)}` : basename(urlPath) || "download.bin");
	const destinationPath = join(CACHE_ROOT, fileName);
	try {
		log.info("Checking cache", { destinationPath });
		await stat(destinationPath);
		return destinationPath;
	} catch {
		// Cache miss: continue to download.
	}

	const response = await fetch(sourceUrl, {
		headers: {
			"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
      "Referer": new URL(sourceUrl).origin,
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
    const isTarXz = archivePath.endsWith(".xz");
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

	const tarArgs = isTarXz ? ["-xJf", archivePath, "-C", destinationPath] : ["-xf", archivePath, "-C", destinationPath];
	execFileSync("tar", tarArgs, { stdio: "inherit" });
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

async function resolveBinaryFromSources(targetKey: PlatformTargetKey, binaryKind: BinaryKind, sources: readonly SourceDescriptor[]): Promise<string> {
	const errors: string[] = [];
	for (const source of sources) {
		try {
			const cachedPath = await downloadToCache(source.url, `${targetKey}-${source.type}`);
			if (source.type === "binary") {
				const archiveType = detectArchiveType(cachedPath);
				if (archiveType) {
					const extractedDir = await extractArchive(cachedPath, archiveType);
					const binaryPath = await findBinaryFilePath(extractedDir, binaryKind);
					if (!binaryPath) {
						throw new Error(`Could not find ${buildBinaryFileName(binaryKind)} in extracted binary archive`);
					}
					return binaryPath;
				}
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

function isLikelyBinaryArtifact(filePath: string, mode: number): boolean {
	const fileName = path.basename(filePath).toLowerCase();
	const isNamedFfTool = fileName.startsWith("ffmpeg") || fileName.startsWith("ffprobe") || fileName.startsWith("ffplay");
	const hasKnownExtension =
		fileName.endsWith(".exe") ||
		fileName.endsWith(".dll") ||
		fileName.endsWith(".dylib") ||
		fileName.endsWith(".so") ||
		fileName.includes(".so.") ||
		fileName.endsWith(".bin");
	const isExecutable = (mode & 0o111) !== 0;
	return isNamedFfTool || hasKnownExtension || isExecutable;
}

async function listBinaryArtifacts(directoryPath: string): Promise<string[]> {
	const files = await readDirectoryRecursive(directoryPath);
	const candidateFiles: string[] = [];
	for (const filePath of files) {
		const fileStats = await stat(filePath);
		if (!fileStats.isFile()) continue;
		if (!isLikelyBinaryArtifact(filePath, fileStats.mode)) continue;
		candidateFiles.push(filePath);
	}
	return candidateFiles;
}

type CopyBinArtifactsOptions = {
	flattenArchives?: boolean;
};

async function copyBinArtifacts(ffmpegBinaryPath: string, ffprobeBinaryPath: string, options: CopyBinArtifactsOptions = {}): Promise<string[]> {
	const ffmpegBinDir = resolveSourceBinDirectory(ffmpegBinaryPath);
	const ffprobeBinDir = resolveSourceBinDirectory(ffprobeBinaryPath);

	await rm(OUTPUT_BIN_ROOT, { recursive: true, force: true });
	await ensureDirectory(OUTPUT_BIN_ROOT);

	if (!options.flattenArchives && ffmpegBinDir && ffprobeBinDir && ffmpegBinDir === ffprobeBinDir) {
		await cp(ffmpegBinDir, OUTPUT_BIN_ROOT, { recursive: true, force: true });
	} else {
		const sourceRoots = Array.from(new Set([resolvePackageRootFromBinary(ffmpegBinaryPath), resolvePackageRootFromBinary(ffprobeBinaryPath)]));
		const copiedNames = new Set<string>();
		for (const sourceRoot of sourceRoots) {
			const binaryArtifacts = await listBinaryArtifacts(sourceRoot);
			for (const binaryArtifact of binaryArtifacts) {
				const destinationPath = join(OUTPUT_BIN_ROOT, path.basename(binaryArtifact));
				await cp(binaryArtifact, destinationPath, { force: true });
				copiedNames.add(path.basename(destinationPath).toLowerCase());
			}
		}
		const expectedBinaries = [buildBinaryFileName("ffmpeg"), buildBinaryFileName("ffprobe")].map((name) => name.toLowerCase());
		for (const expectedBinary of expectedBinaries) {
			if (!copiedNames.has(expectedBinary)) {
				throw new Error(`[ffmpeg-vite-plugin] Missing required binary after flattening archive: ${expectedBinary}`);
			}
		}
	}

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

async function copySharedFolders(ffmpegBinaryPath: string): Promise<string[]> {
	const packageRoot = resolvePackageRootFromBinary(ffmpegBinaryPath);
	const sharedFolderNames = ["lib", "include"] as const;
	const copiedFolders: string[] = [];
	for (const folderName of sharedFolderNames) {
		const sourceFolderPath = join(packageRoot, folderName);
		if (!(await pathExists(sourceFolderPath))) continue;
		const destinationFolderPath = join(OUTPUT_ROOT, folderName);
		await cp(sourceFolderPath, destinationFolderPath, { recursive: true, force: true });
		copiedFolders.push(destinationFolderPath);
	}
	return copiedFolders;
}
type ResolveBinariesFromSharedArchiveOptions = {
	prefix?: string;
};
async function resolveBinariesFromSharedArchive(source: SourceDescriptor, options: ResolveBinariesFromSharedArchiveOptions = {}): Promise<{ ffmpeg: string; ffprobe: string }> {
	if (source.type !== "archive" || !source.archiveType) {
		throw new Error("[ffmpeg-vite-plugin] Shared archive source requires archive type.");
	}
	const cachedPath = await downloadToCache(source.url, options.prefix ? `${options.prefix}-${source.type}-${source.archiveType}` : undefined);
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
				const resolvedPaths = await resolveBinariesFromSharedArchive(sharedArchiveSource, { prefix: `${targetKey}-${sharedArchiveSource.type || "binary"}` });
				resolvedFfmpegPath = resolvedPaths.ffmpeg;
				resolvedFfprobePath = resolvedPaths.ffprobe;
			} else {
				resolvedFfmpegPath = await resolveBinaryFromSources(targetKey, "ffmpeg", sources.ffmpeg);
				resolvedFfprobePath = await resolveBinaryFromSources(targetKey, "ffprobe", sources.ffprobe);
			}
			const usesBinaryTypeSource = [...sources.ffmpeg, ...sources.ffprobe].some((source) => source.type === "binary");
			const copiedBinArtifacts = await copyBinArtifacts(resolvedFfmpegPath, resolvedFfprobePath, { flattenArchives: usesBinaryTypeSource });
			const copiedSharedFolders = await copySharedFolders(resolvedFfmpegPath);

			log.info("Prepared binaries", {
				targetKey,
				ffmpeg: join(OUTPUT_BIN_ROOT, buildBinaryFileName("ffmpeg")),
				ffprobe: join(OUTPUT_BIN_ROOT, buildBinaryFileName("ffprobe")),
				binArtifacts: copiedBinArtifacts,
				sharedFolders: copiedSharedFolders,
				cacheRoot: CACHE_ROOT,
			});
		},
	};
}
