import { platform as appPlatform } from "@electron-toolkit/utils";
import { executableIsAvailable } from "@main/lib/bin.utils";
import YTDLWrapper from "@main/lib/ytdlp-wrapper";
import { appStore } from "@main/stores/app.store";
import { Endpoints } from "@octokit/types";
import { Logger } from "@shared/logger";
import { resulter } from "@shared/promises/helper";
import { nt as calvNewerThan } from "calver";
import { app } from "electron";
import { platform } from "os";
import path from "path";
import { VideoInfo } from "yt-dlp-wrap/types";
import { pushLogToClient } from "./events.ee";
const YTDLP_PLATFORM = platform();
const ytdlpPath = app.getPath("userData");
const log = new Logger("YTDLP");
type GithubRelease = Endpoints["GET /repos/{owner}/{repo}/releases"]["response"]["data"][0];
enum YTDLP_STATE {
	NONE,
	READY,
	CONVERTING,
	DOWNLOADING,
	UPDATE_CHECKING,
	UPDATE_ERROR,
	ERROR,
}
export class YTDLP {
	private _state: YTDLP_STATE = YTDLP_STATE.NONE;
	private _ytd: YTDLWrapper = new YTDLWrapper();
	get state() {
		return this._state;
	}
	constructor() {}
	async initialize() {
		log.info("initializing...");
		const ytdVersion = await this._ytd.getVersion();
		log.debug({ ytdVersion });
		if (appPlatform.isWindows) appStore.store.ytdlp.useGlobal = false;
		const ytdlpPath = appStore.store.ytdlp.useGlobal && executableIsAvailable("yt-dlp");
		if (!ytdlpPath) await this.checkUpdates();
		else {
			this._ytd.setBinaryPath(ytdlpPath);
		}
	}
	async checkUpdates(forceLatestUpdate?: boolean) {
		this._state = YTDLP_STATE.UPDATE_CHECKING;
		const currentYtdlp = appStore.get("ytdlp");
		log.info("checking for ytdlp updates...");
		const latestRelease =
			((forceLatestUpdate || currentYtdlp.checkForUpdate) &&
				(await YTDLWrapper.getGithubReleases(1, 1).then(([grelease]: [GithubRelease]) => {
					return {
						...grelease,
						version: grelease.tag_name.replace(/^yt-dlp /, ""),
					};
				}))) ||
			null;
		log.debug("ytdlp version compare...", {
			latest: latestRelease?.version,
			current: currentYtdlp.version,
			config: currentYtdlp,
			platform: YTDLP_PLATFORM,
		});
		let updated = false;
		let previousVersion = currentYtdlp?.version ?? "-";
		if (
			latestRelease &&
			(forceLatestUpdate ||
				!currentYtdlp?.version ||
				(currentYtdlp.version !== latestRelease.version && !calvNewerThan(currentYtdlp.version.replace(/\./g, "-"), latestRelease.version.replace(/\./g, "-"))))
		) {
			log.debug("found new version of ytdlp, trying to download...", {
				tag_name: latestRelease.tag_name,
				version: latestRelease.version,
			});
			await this.downloadUpdate(latestRelease)
				.then(({ path, version }) => {
					appStore.set("ytdlp", { path, version, checkForUpdate: true });
					log.debug("downloaded new ytdlp executable:", path, version);
					updated = true;
				})
				.catch((err) => {
					log.error("failed to download update...\n", err);
				});
		}
		if (appStore.store.ytdlp?.path) this.ytdlp.setBinaryPath(appStore.store.ytdlp.path);
		this._state = YTDLP_STATE.READY;
		return {
			updated,
			currentVersion: appStore.store.ytdlp.version,
			previousVersion,
		};
	}
	private async downloadUpdate(release: GithubRelease & { version: string }): Promise<{ version: string; path: string }> {
		const newYtdlPath = path.join(ytdlpPath, appPlatform.isWindows ? "ytdlp.exe" : "ytdlp");
		await YTDLWrapper.downloadFromGithub(newYtdlPath, release.version, YTDLP_PLATFORM);
		return { version: release.version, path: newYtdlPath };
	}
	async getVideoInfo(...args: string[]) {
		const ignoreGenericUrls = ["--ies", "default,-generic"];
		const platformFilenames = [appPlatform.isWindows ? "--windows-filenames" : "--restrict-filenames"];
		const videoArgs = [...args]
			.concat((args.includes("--ies") && ignoreGenericUrls) || "")
			.concat((args.includes(platformFilenames[0]) && platformFilenames) || "")
			.filter(Boolean);
		pushLogToClient(`yt-dlp ${videoArgs.join(" ")}`);
		return await resulter<VideoInfo>(this._ytd.getVideoInfo(videoArgs));
	}
	readonly exec: typeof this._ytd.exec = this._ytd.exec.bind(this._ytd);
	get ytdlp() {
		return this._ytd;
	}
	get currentDownloadPath() {
		return appStore.store.download?.selected;
	}
}
