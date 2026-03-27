import EventEmitter from "events";
import { execFileSync } from "node:child_process";
import { createWriteStream } from "node:fs";
import { copyFile, mkdir, readdir, rename, rm, stat } from "node:fs/promises";
import { basename, dirname, join, relative } from "node:path";
import { executableIsAvailable } from "@main/lib/bin.utils";
import { appStore } from "@main/stores/app.store";
import { logger } from "@shared/logger";
import { app } from "electron";
import { DependencyKey, dependencyDefinitions } from "./meta";
import { DependencyRelease, DependencySource, DependencyTarget } from "./meta/defineDependency";
import { DependencyInstallState, dependencyStore } from "./store";

type DownloadState = "idle" | "downloading" | "completed" | "error" | "cancelled";
const log = logger.child("dependencies.manager");
const dependencyEvents = new EventEmitter();
const DEPENDENCY_PATH = join(app.getPath("userData"), "dependencies");

export interface DependencyProgressEvent {
	readonly key: DependencyKey;
	readonly version: string;
	readonly state: DownloadState;
	readonly fileName: string | null;
	readonly percent: number;
	readonly downloadedBytes: number;
	readonly totalBytes: number | null;
	readonly message?: string;
}

export interface ActiveDownload {
	readonly key: DependencyKey;
	readonly version: string;
	readonly startedAt: string;
	readonly controller: AbortController;
}

export interface ResolvedRelease {
	readonly release: DependencyRelease;
	readonly target: DependencyTarget;
}

export interface DependencyRemovalResult {
	readonly key: DependencyKey;
	readonly removed: boolean;
	readonly reclaimedBytes: number;
}

function normalizeFileName(url: string, fallback: string): string {
	try {
		const parsedUrl = new URL(url);
		const lastSegment = basename(parsedUrl.pathname);
		return lastSegment || fallback;
	} catch {
		return fallback;
	}
}

function detectArchiveType(fileName: string, source: DependencySource): "zip" | "tar" | null {
	if (source.archiveType) return source.archiveType;
	const normalizedName = fileName.toLowerCase();
	if (normalizedName.endsWith(".zip")) return "zip";
	if (normalizedName.endsWith(".tar") || normalizedName.endsWith(".tar.xz") || normalizedName.endsWith(".txz")) return "tar";
	return null;
}

function normalizePathSeparators(value: string): string {
	return value.replace(/\\/g, "/");
}

function toWildcardRegex(pattern: string): RegExp {
	const escaped = pattern
		.replace(/[.+^${}()|[\]\\]/g, "\\$&")
		.replace(/\*\*/g, "__ALL__")
		.replace(/\*/g, "__ONE__")
		.replace(/__ALL__/g, ".*")
		.replace(/__ONE__/g, "[^/]*");
	return new RegExp(`^${escaped}$`);
}

function matchesExportPattern(filePath: string, exportPattern: string): boolean {
	const normalizedFilePath = normalizePathSeparators(filePath);
	const normalizedPattern = normalizePathSeparators(exportPattern);
	const regex = toWildcardRegex(normalizedPattern);
	return regex.test(normalizedFilePath);
}

function getRuntimePlatform(): DependencyTarget["platform"] {
	const platform = process.platform;
	if (platform === "win32" || platform === "linux" || platform === "darwin") {
		return platform;
	}
	throw new Error(`Unsupported platform: ${platform}`);
}

function getRuntimeArch(): DependencyTarget["arch"] {
	const arch = process.arch;
	if (arch === "x64" || arch === "arm64") {
		return arch;
	}
	throw new Error(`Unsupported architecture: ${arch}`);
}

function resolveLatestRelease(releases: readonly DependencyRelease[]): DependencyRelease {
	const [latestRelease] = releases;
	if (!latestRelease) {
		throw new Error("Dependency has no releases");
	}
	return latestRelease;
}

export class DependenciesManager {
	private static instance: DependenciesManager | null = null;
	private readonly activeDownloads = new Map<DependencyKey, ActiveDownload>();

	public static getInstance(): DependenciesManager {
		if (!DependenciesManager.instance) {
			DependenciesManager.instance = new DependenciesManager();
		}
		return DependenciesManager.instance;
	}

	private constructor() {}

	public getEvents(): EventEmitter {
		return dependencyEvents;
	}

	public getInstallState(key: DependencyKey): DependencyInstallState | null {
		return dependencyStore.get(key, null as DependencyInstallState | null) as DependencyInstallState | null;
	}

	get store() {
		return dependencyStore;
	}

	public getActiveDownloads(): readonly ActiveDownload[] {
		return Array.from(this.activeDownloads.values());
	}

	public getDefinitions() {
		const platform = getRuntimePlatform();
		const arch = getRuntimeArch();
		return Object.entries(dependencyDefinitions).map(([key, definition]) => {
			const installed = this.getInstallState(key as DependencyKey);
			const compatibleVersions = definition.releases
				.filter((release) => release.targets.some((target) => target.platform === platform || (target.platform === "all" && target.arch === arch) || target.arch === "all"))
				.map((release) => release.version);
			return {
				key,
				name: definition.name,
				description: definition.description,
				latestVersion: definition.releases[0]?.version ?? null,
				versions: definition.releases.map((release) => release.version),
				compatibleVersions,
				installed,
				isDownloading: this.activeDownloads.has(key as DependencyKey),
			};
		});
	}

	public resolveRelease(key: DependencyKey, version?: string): ResolvedRelease {
		const definition = dependencyDefinitions[key];
		const platform = getRuntimePlatform();
		const arch = getRuntimeArch();
		const release = version ? definition.releases.find((item) => item.version === version) : resolveLatestRelease(definition.releases);

		if (!release) {
			throw new Error(`Version ${version ?? "<latest>"} was not found for ${key}`);
		}

		const target = release.targets.find((item) => item.platform === platform && item.arch === arch);
		if (!target) {
			throw new Error(`Version ${release.version} is not available for ${platform}/${arch}`);
		}
		return { release, target };
	}

	public cancelDownload(key: DependencyKey): boolean {
		const active = this.activeDownloads.get(key);
		if (!active) return false;
		active.controller.abort();
		return true;
	}

	public async downloadDependency(key: DependencyKey, version?: string): Promise<{ key: DependencyKey; version: string; startedAt: string }> {
		if (this.activeDownloads.has(key)) {
			throw new Error(`Dependency ${key} is already downloading`);
		}

		const resolved = this.resolveRelease(key, version);
		const controller = new AbortController();
		const startedAt = new Date().toISOString();
		const job: ActiveDownload = {
			key,
			version: resolved.release.version,
			startedAt,
			controller,
		};
		this.activeDownloads.set(key, job);
		await this.runDownload(job, resolved.target.sources).catch((error: unknown) => {
			const message = error instanceof Error ? error.message : String(error);
			log.error("downloadDependency failed", { key, version: resolved.release.version, error: message });
		});
		return { key, version: resolved.release.version, startedAt };
	}
	public async downloadDependencyPromise(key: DependencyKey, version?: string): Promise<{ key: DependencyKey; version: string; startedAt: string }> {
		if (this.activeDownloads.has(key)) {
			throw new Error(`Dependency ${key} is already downloading`);
		}
		const resolved = this.resolveRelease(key, version);
		const startedAt = new Date().toISOString();
		const job = {
			key,
			version: resolved.release.version,
		};
		return new Promise((resolve, reject) => {
			this.createProgressEvent({
				...job,
				state: "downloading",
				fileName: null,
				percent: 0,
				downloadedBytes: 0,
				totalBytes: null,
			});
			this.downloadDependency(job.key, job.version).then(resolve).catch(reject);
		});
	}

	public async getFfmpegPath(): Promise<string | null> {
		return appStore.get("ytdlp.ffmpegPath", null as string | null);
	}

	public async removeDependency(key: DependencyKey): Promise<DependencyRemovalResult> {
		const active = this.activeDownloads.get(key);
		if (active) {
			active.controller.abort();
			this.activeDownloads.delete(key);
		}

		const installed = this.getInstallState(key);
		if (!installed) {
			return {
				key,
				removed: false,
				reclaimedBytes: 0,
			};
		}

		await rm(installed.path, { recursive: true, force: true });
		dependencyStore.delete(key);

		this.createProgressEvent({
			key,
			version: installed.version,
			state: "idle",
			fileName: null,
			percent: 0,
			downloadedBytes: 0,
			totalBytes: null,
			message: "Dependency removed",
		});

		return {
			key,
			removed: true,
			reclaimedBytes: installed.usedSpaceBytes,
		};
	}

	private createProgressEvent(payload: DependencyProgressEvent): DependencyProgressEvent {
		dependencyEvents.emit("progress", payload);
		return payload;
	}

	private getDestinationPath(key: DependencyKey, version: string, fileName: string): string {
		return join(DEPENDENCY_PATH, key, version, fileName);
	}

	private getExportRootPath(key: DependencyKey): string {
		return join(DEPENDENCY_PATH, "export", key);
	}

	private async moveFile(sourcePath: string, destinationPath: string): Promise<void> {
		await mkdir(dirname(destinationPath), { recursive: true });
		try {
			await rename(sourcePath, destinationPath);
		} catch {
			await copyFile(sourcePath, destinationPath);
			await rm(sourcePath, { force: true });
		}
	}

	private async collectFilesRecursively(directoryPath: string): Promise<string[]> {
		const entries = await readdir(directoryPath, { withFileTypes: true });
		const files = await Promise.all(
			entries.map(async (entry) => {
				const fullPath = join(directoryPath, entry.name);
				if (entry.isDirectory()) return this.collectFilesRecursively(fullPath);
				return [fullPath];
			}),
		);
		return files.flat();
	}

	private extractArchive(archivePath: string, destinationPath: string, archiveType: "zip" | "tar"): void {
		if (archiveType === "zip") {
			if (process.platform === "win32") {
				execFileSync("powershell", ["-NoProfile", "-Command", `Expand-Archive -Path "${archivePath}" -DestinationPath "${destinationPath}" -Force`], { stdio: "ignore" });
				return;
			}
			execFileSync("unzip", ["-o", archivePath, "-d", destinationPath], { stdio: "ignore" });
			return;
		}

		const isTarXz = archivePath.toLowerCase().endsWith(".xz") || archivePath.toLowerCase().endsWith(".txz");
		const tarArgs = isTarXz ? ["-xJf", archivePath, "-C", destinationPath] : ["-xf", archivePath, "-C", destinationPath];
		execFileSync("tar", tarArgs, { stdio: "ignore" });
	}

	private async resolveExtractedFiles(extractRootPath: string, exports: readonly string[] | undefined): Promise<string[]> {
		const extractedFiles = await this.collectFilesRecursively(extractRootPath);
		if (!exports?.length) return extractedFiles;

		const matchedFiles = extractedFiles.filter((filePath) => {
			const relativePath = normalizePathSeparators(relative(extractRootPath, filePath));
			return exports.some((pattern) => matchesExportPattern(relativePath, pattern));
		});
		return matchedFiles;
	}

	private async resolveInstalledFilesFromSource({
		key,
		version,
		source,
		fileName,
		downloadedPath,
		exportRootPath,
	}: {
		readonly key: DependencyKey;
		readonly version: string;
		readonly source: DependencySource;
		readonly fileName: string;
		readonly downloadedPath: string;
		readonly exportRootPath: string;
	}): Promise<string[]> {
		const archiveType = detectArchiveType(fileName, source);
		if (!archiveType) {
			const destinationPath = join(exportRootPath, fileName);
			await this.moveFile(downloadedPath, destinationPath);
			return [destinationPath];
		}

		const extractRootPath = join(DEPENDENCY_PATH, key, version, "extracted", source.id);
		await rm(extractRootPath, { recursive: true, force: true });
		await mkdir(extractRootPath, { recursive: true });
		this.extractArchive(downloadedPath, extractRootPath, archiveType);
		const matchedFiles = await this.resolveExtractedFiles(extractRootPath, source.exports);
		await rm(downloadedPath, { force: true }).catch(() => undefined);
		if (!matchedFiles.length) {
			throw new Error(`No files matched exports for ${key}:${source.id}`);
		}
		const exportedFiles = await Promise.all(
			matchedFiles.map(async (matchedFilePath) => {
				const relativeMatchedPath = relative(extractRootPath, matchedFilePath);
				const destinationPath = join(exportRootPath, relativeMatchedPath);
				await this.moveFile(matchedFilePath, destinationPath);
				return destinationPath;
			}),
		);
		await rm(extractRootPath, { recursive: true, force: true }).catch(() => undefined);
		return exportedFiles;
	}

	private async getUsedSpaceBytes(files: readonly string[]): Promise<number> {
		let bytes = 0;
		for (const filePath of files) {
			try {
				const stats = await stat(filePath);
				if (stats.isFile()) {
					bytes += stats.size;
				}
			} catch {
				continue;
			}
		}
		return bytes;
	}

	private async runDownload(job: ActiveDownload, sources: readonly DependencySource[]): Promise<void> {
		const files: string[] = [];
		try {
			const exportRootPath = this.getExportRootPath(job.key);
			await rm(exportRootPath, { recursive: true, force: true });
			await mkdir(exportRootPath, { recursive: true });

			for (const [index, source] of sources.entries()) {
				const fallbackName = `${source.id}-${index + 1}.bin`;
				const fileName = normalizeFileName(source.url, fallbackName);
				const destination = this.getDestinationPath(job.key, job.version, fileName);
				await mkdir(join(DEPENDENCY_PATH, job.key, job.version), { recursive: true });
				await this.downloadSource({
					key: job.key,
					version: job.version,
					source,
					fileName,
					destinationPath: destination,
					controller: job.controller,
				});
				this.createProgressEvent({
					key: job.key,
					version: job.version,
					state: "downloading",
					fileName,
					percent: 100,
					downloadedBytes: 0,
					totalBytes: null,
					message: `Extracting ${fileName}`,
				});
				const installedFiles = await this.resolveInstalledFilesFromSource({
					key: job.key,
					version: job.version,
					source,
					fileName,
					downloadedPath: destination,
					exportRootPath,
				});
				files.push(...installedFiles);
			}
			const usedSpaceBytes = await this.getUsedSpaceBytes(files);

			const installedState: DependencyInstallState = {
				path: exportRootPath,
				version: job.version,
				files,
				usedSpaceBytes,
				updatedAt: new Date().toISOString(),
			};
			dependencyStore.set(job.key, installedState);

			this.createProgressEvent({
				key: job.key,
				version: job.version,
				state: "completed",
				fileName: null,
				percent: 100,
				downloadedBytes: 0,
				totalBytes: null,
				message: "Download completed",
			});
		} catch (error: unknown) {
			const wasCancelled = job.controller.signal.aborted;
			const message = error instanceof Error ? error.message : String(error);
			this.createProgressEvent({
				key: job.key,
				version: job.version,
				state: wasCancelled ? "cancelled" : "error",
				fileName: null,
				percent: 0,
				downloadedBytes: 0,
				totalBytes: null,
				message,
			});
		} finally {
			this.activeDownloads.delete(job.key);
		}
	}

	private async downloadSource({
		key,
		version,
		source,
		fileName,
		destinationPath,
		controller,
	}: {
		readonly key: DependencyKey;
		readonly version: string;
		readonly source: DependencySource;
		readonly fileName: string;
		readonly destinationPath: string;
		readonly controller: AbortController;
	}): Promise<void> {
		this.createProgressEvent({
			key,
			version,
			state: "downloading",
			fileName,
			percent: 0,
			downloadedBytes: 0,
			totalBytes: null,
			message: `Downloading ${fileName}`,
		});

		const response = await fetch(source.url, { signal: controller.signal });
		if (!response.ok || !response.body) {
			throw new Error(`Failed to download ${source.url}: ${response.status} ${response.statusText}`);
		}

		const totalBytesHeader = response.headers.get("content-length");
		const totalBytes = totalBytesHeader ? Number.parseInt(totalBytesHeader, 10) : null;
		const stream = createWriteStream(destinationPath);
		const reader = response.body.getReader();

		let downloadedBytes = 0;
		try {
			while (true) {
				const chunk = await reader.read();
				if (chunk.done) break;
				const buffer = Buffer.from(chunk.value);
				stream.write(buffer);
				downloadedBytes += buffer.byteLength;
				const percent = totalBytes ? Math.min(100, Math.round((downloadedBytes / totalBytes) * 100)) : 0;
				this.createProgressEvent({
					key,
					version,
					state: "downloading",
					fileName,
					percent,
					downloadedBytes,
					totalBytes,
				});
			}
		} catch (error) {
			stream.destroy();
			await rm(destinationPath, { force: true }).catch(() => undefined);
			throw error;
		}

		await new Promise<void>((resolve, reject) => {
			stream.end((error) => {
				if (error) reject(error);
				else resolve();
			});
		});
	}
	public executableIsAvailable(key: DependencyKey, setAsEnvironment: boolean = false): boolean {
		const path = executableIsAvailable(key);
		if (!path) return false;
		if (setAsEnvironment) {
			dependencyStore.set(key, {
				path,
				version: "environment",
				files: [path],
				usedSpaceBytes: 0,
				updatedAt: new Date().toISOString(),
			} as DependencyInstallState);
		}
		return true;
	}
}

export const dependenciesManager = DependenciesManager.getInstance();
