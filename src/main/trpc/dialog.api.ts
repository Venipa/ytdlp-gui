import { BrowserWindow, ipcMain } from "electron";
import { z } from "zod";
import { createChildWindow, loadUrlOfWindow, lockCenterWithParent, waitForWindowClose } from "./dialog.utils";
import { publicProcedure, router } from "./trpc";

let windowHandles: Record<string, BrowserWindow[]> = {};
const addWindowToHandles = (key: string, win: BrowserWindow) => {
	if (!windowHandles[key]) windowHandles[key] = [];
	const instance = windowHandles[key].find((w) => !w.isDestroyed());
	if (instance) return instance;
	windowHandles[key].push(win);
	win.once("closed", () => {
		const idx = windowHandles[key].findIndex((d) => d.id === win.id);
		windowHandles[key].splice(idx, 1);
	});
	return win;
};
const destroyWindow = (win: BrowserWindow) => {
	const entries = Object.values(windowHandles);
	const dataIdx = entries.findIndex((d) => d.find((w) => w === win));
	if (dataIdx === -1) return false;
	const instanceIdx = entries[dataIdx].findIndex((w) => w === win);
	if (instanceIdx === -1) return false;
	const [instance] = entries[dataIdx].splice(instanceIdx, 1);
	instance.destroy();
	return true;
};
export const dialogRouter = router({
	settings: publicProcedure.mutation(async ({ ctx: { window, path } }) => {
		const currentWindow = addWindowToHandles(
			path,
			createChildWindow({
				parent: window,
				height: 600,
				width: 1080,
				resizable: false,
				maximizable: false,
				title: "Settings",
			}),
		);
		await loadUrlOfWindow(currentWindow, "#/settings")
			.then(() => {
				lockCenterWithParent(currentWindow);
				currentWindow.show();
			})
			.finally(() => {
				setTimeout(() => {
					ipcMain.emit("windowState");
				});
			});
		await waitForWindowClose(currentWindow);
		return {
			id: currentWindow.id,
		};
	}),
	destroyWindow: publicProcedure.input(z.number()).mutation(async ({ input: id }) => {
		const win = BrowserWindow.getAllWindows().find((d) => d.id === id);
		if (win) return destroyWindow(win);
		return false;
	}),
});
