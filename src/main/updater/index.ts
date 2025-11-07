import { appStore } from "@main/stores/app.store";
import { isProduction } from "@shared/config";
import { TRPCError } from "@trpc/server";
import { BrowserWindow, app, dialog } from "electron";
import { UpdateInfo, autoUpdater } from "electron-updater";
import semver from "semver";
const [GITHUB_AUTHOR, GITHUB_REPOSITORY] = import.meta.env.VITE_GITHUB_REPOSITORY?.split("/", 2) ?? [null, null];
let updateQueuedInFrontend = false;
export const setUpdateHandledByFrontend = (value: boolean) => (updateQueuedInFrontend = value);
export function isUpdateInRange(ver: string) {
	if (!isProduction) return true;
	return semver.gtr(ver, app.getVersion(), {
		includePrerelease: appStore.store.beta,
	});
}
export function checkForUpdates() {
	return autoUpdater.checkForUpdates().then((info) => (info && isUpdateInRange(info.updateInfo.version) && info) || null);
}
export async function checkForUpdatesAndNotify() {
	try {
		const info = await checkForUpdates().catch(() => null);
		if (!info) return;
		return info && (await proceedUpdateDialog(info.updateInfo));
	} catch (ex: any) {
		throw new TRPCError({ message: ex.message, code: "INTERNAL_SERVER_ERROR" });
	}
}
export async function proceedUpdateDialog(info: UpdateInfo) {
	try {
		const releaseNotes = (typeof info.releaseNotes === "string" ? info.releaseNotes : info.releaseNotes?.map((x) => x.note).join("\n"))?.replace(/<[^>]+>/g, "").trimStart();
		return await dialog
			.showMessageBox({
				title: `Update available (${info.version})`,
				message: `Hey there is a new version which you can update to.\n\n${process.platform === "win32" ? releaseNotes : info.releaseName}`,
				type: "question",
				buttons: ["Update now", "Update on quit", "Cancel"],
				cancelId: -1,
			})
			.then(({ response }) => {
				if (response === 0) autoUpdater.quitAndInstall();
				else if (response === 1) autoUpdater.autoInstallOnAppQuit = true;
			});
	} catch (ex: any) {
		throw new TRPCError({ message: ex.message, code: "INTERNAL_SERVER_ERROR" });
	}
}
export function attachAutoUpdaterIPC(win: BrowserWindow) {
	function resetUI() {
		win.webContents.send("update-available", false);
		win.webContents.send("update-checking", false);
	}
	autoUpdater.on("update-available", (info) => info && isUpdateInRange(info.version) && win.webContents.send("update-available", info));
	autoUpdater.on("update-not-available", (info) => {
		resetUI();
	});
	autoUpdater.on("checking-for-update", () => win.webContents.send("update-checking", new Date().toISOString()));
	autoUpdater.signals.progress((info) => {
		win.webContents.send("update-download-progress", info);
	});
	autoUpdater.signals.updateDownloaded(async (x) => {
		win.webContents.send("update-download-done", x);
		if (updateQueuedInFrontend) return;
		return await proceedUpdateDialog(x).catch((err) => {
			resetUI();
		});
	});
	win.webContents.on("did-finish-load", () => {
		checkForUpdatesAndNotify();
	});
	if (!import.meta.env.VITE_GITHUB_REPOSITORY)
		autoUpdater.setFeedURL({
			provider: "github",
			owner: GITHUB_AUTHOR,
			repo: GITHUB_REPOSITORY,
		});
	autoUpdater.autoDownload = false;
}
