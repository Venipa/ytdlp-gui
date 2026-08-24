import { createWriteStream, existsSync } from "node:fs";
import { chmod, mkdir, rename, rm } from "node:fs/promises";
import { dirname, join } from "node:path";
import { Readable } from "node:stream";
import { pipeline } from "node:stream/promises";
import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { createLogger } from "@shared/logger";
import { app } from "electron";
import { dependencyStore } from "@main/trpc/dependencies/store";

const execFileAsync = promisify(execFile);
const log = createLogger("ytdlp-bin");

const GITHUB_LATEST = "https://api.github.com/repos/yt-dlp/yt-dlp/releases/latest";
const USER_AGENT = "ytdlp-gui";

export type GithubReleaseAsset = {
	name: string;
	browser_download_url: string;
};

export type GithubRelease = {
	tag_name: string;
	assets: GithubReleaseAsset[];
};

let cachedRelease: { fetchedAt: number; release: GithubRelease } | null = null;
let cachedReleaseList: { fetchedAt: number; releases: GithubRelease[] } | null = null;
let cachedVersion: string | null = null;
const RELEASE_CACHE_MS = 5 * 60 * 1000;

export function normalizeYtdlpTag(tag: string): string {
	return tag.replace(/^yt-dlp@/, "");
}

export function getYtdlpReleaseAssetCandidates(): string[] {
	const platform = process.platform;
	const arch = process.arch;
	if (platform === "win32") {
		if (arch === "arm64") return ["yt-dlp_arm64.exe"];
		if (arch === "ia32") return ["yt-dlp_x86.exe"];
		return ["yt-dlp.exe"];
	}
	if (platform === "darwin") {
		return ["yt-dlp_macos"];
	}
	if (arch === "arm64") return ["yt-dlp_linux_aarch64", "yt-dlp_musllinux_aarch64"];
	if (arch === "arm") return ["yt-dlp_linux_armv7l"];
	return ["yt-dlp_linux", "yt-dlp_musllinux"];
}

export function getYtdlpReleaseAssetName(): string {
	const [preferred] = getYtdlpReleaseAssetCandidates();
	return preferred ?? "yt-dlp_linux";
}

function getInstallDir(): string {
	return join(app.getPath("userData"), "dependencies", "export", "ytdlp");
}

export function getYtdlpBinaryName(): string {
	return process.platform === "win32" ? "yt-dlp.exe" : "yt-dlp";
}

export function getYtdlpBinaryPath(): string {
	const installed = dependencyStore.get("ytdlp");
	const storedPath = installed?.files?.[0];
	if (storedPath && existsSync(storedPath)) return storedPath;
	const exportPath = join(getInstallDir(), getYtdlpBinaryName());
	if (existsSync(exportPath)) return exportPath;
	return join(app.getPath("userData"), "dependencies", "yt-dlp", getYtdlpBinaryName());
}

export function hasYtdlpBinary(): boolean {
	return existsSync(getYtdlpBinaryPath());
}

function githubHeaders(): Record<string, string> {
	return {
		Accept: "application/vnd.github+json",
		"User-Agent": USER_AGENT,
	};
}

export async function fetchLatestYtdlpRelease(force = false): Promise<GithubRelease> {
	if (!force && cachedRelease && Date.now() - cachedRelease.fetchedAt < RELEASE_CACHE_MS) {
		return cachedRelease.release;
	}
	const response = await fetch(GITHUB_LATEST, { headers: githubHeaders() });
	if (!response.ok) {
		throw new Error(`GitHub release lookup failed (${response.status})`);
	}
	const release = (await response.json()) as GithubRelease;
	cachedRelease = { fetchedAt: Date.now(), release };
	return release;
}

export async function listYtdlpGithubReleases(force = false): Promise<GithubRelease[]> {
	if (!force && cachedReleaseList && Date.now() - cachedReleaseList.fetchedAt < RELEASE_CACHE_MS) {
		return cachedReleaseList.releases;
	}
	const response = await fetch("https://api.github.com/repos/yt-dlp/yt-dlp/releases?per_page=12", { headers: githubHeaders() });
	if (!response.ok) {
		throw new Error(`GitHub releases lookup failed (${response.status})`);
	}
	const releases = (await response.json()) as GithubRelease[];
	cachedReleaseList = { fetchedAt: Date.now(), releases };
	if (releases[0]) {
		cachedRelease = { fetchedAt: Date.now(), release: releases[0] };
	}
	return releases;
}

export function pickYtdlpAsset(release: GithubRelease): GithubReleaseAsset | null {
	const assetsByName = new Map(release.assets.map((asset) => [asset.name, asset]));
	for (const name of getYtdlpReleaseAssetCandidates()) {
		const match = assetsByName.get(name);
		if (match) return match;
	}
	return null;
}

export function getYtdlpFallbackDownloadUrl(): string {
	return `https://github.com/yt-dlp/yt-dlp/releases/latest/download/${getYtdlpReleaseAssetName()}`;
}

function pickAsset(release: GithubRelease): GithubReleaseAsset {
	const match = pickYtdlpAsset(release);
	if (match) return match;
	throw new Error(`No yt-dlp asset among ${getYtdlpReleaseAssetCandidates().join(", ")} in ${release.tag_name}`);
}

async function downloadAsset(url: string, destination: string): Promise<void> {
	await mkdir(dirname(destination), { recursive: true });
	const response = await fetch(url, { headers: { "User-Agent": USER_AGENT }, redirect: "follow" });
	if (!response.ok || !response.body) {
		throw new Error(`yt-dlp download failed (${response.status})`);
	}
	const tmpPath = `${destination}.new`;
		await pipeline(Readable.fromWeb(response.body as never), createWriteStream(tmpPath));
	if (process.platform !== "win32") {
		await chmod(tmpPath, 0o755);
	}
	if (existsSync(destination)) {
		const oldPath = `${destination}.old`;
		try {
			await rm(oldPath, { force: true });
		} catch {
			// ignore
		}
		await rename(destination, oldPath);
		await rename(tmpPath, destination);
		try {
			await rm(oldPath, { force: true });
		} catch {
			// Windows may keep .old locked until reboot
		}
	} else {
		await rename(tmpPath, destination);
	}
	if (process.platform !== "win32") {
		await chmod(destination, 0o755);
	}
}

export async function readYtdlpVersion(): Promise<string> {
	if (cachedVersion) return cachedVersion;
	const binaryPath = getYtdlpBinaryPath();
	if (!existsSync(binaryPath)) {
		throw new Error("yt-dlp binary is missing");
	}
	const { stdout } = await execFileAsync(binaryPath, ["--ignore-config", "--no-update", "--version"], {
		windowsHide: true,
		timeout: 15_000,
	});
	cachedVersion = stdout.trim().split(/\r?\n/, 1)[0] ?? "";
	return cachedVersion;
}

export function clearYtdlpVersionCache(): void {
	cachedVersion = null;
}

export async function installYtdlpIfMissing(): Promise<{ version: string; updated: boolean }> {
	if (hasYtdlpBinary()) {
		return { version: await readYtdlpVersion(), updated: false };
	}
	const result = await downloadLatestYtdlp();
	return { version: result.version, updated: true };
}

export async function downloadLatestYtdlp(onBeforeReplace?: () => void | Promise<void>): Promise<{ version: string; latestTag: string }> {
	const release = await fetchLatestYtdlpRelease(true);
	const latestTag = release.tag_name.replace(/^yt-dlp@/, "");
	const binaryPath = getYtdlpBinaryPath();
	if (existsSync(binaryPath)) {
		await onBeforeReplace?.();
	}
	const asset = pickAsset(release);
	log.info("Downloading yt-dlp", { tag: release.tag_name, asset: asset.name, path: binaryPath });
	await downloadAsset(asset.browser_download_url, binaryPath);
	clearYtdlpVersionCache();
	const version = await readYtdlpVersion();
	return { version, latestTag };
}

export async function checkAndUpdateYtdlp(options?: {
	force?: boolean;
	onBeforeReplace?: () => void | Promise<void>;
}): Promise<{ version: string; updated: boolean; latestTag: string }> {
	if (!hasYtdlpBinary()) {
		const installed = await downloadLatestYtdlp(options?.onBeforeReplace);
		return { version: installed.version, updated: true, latestTag: installed.latestTag };
	}
	const currentVersion = await readYtdlpVersion();
	const release = await fetchLatestYtdlpRelease(options?.force ?? false);
	const latestTag = release.tag_name.replace(/^yt-dlp@/, "");
	const isCurrent = currentVersion === latestTag || currentVersion.includes(latestTag);
	if (isCurrent && !options?.force) {
		return { version: currentVersion, updated: false, latestTag };
	}
	const downloaded = await downloadLatestYtdlp(options?.onBeforeReplace);
	return { version: downloaded.version, updated: true, latestTag: downloaded.latestTag };
}
