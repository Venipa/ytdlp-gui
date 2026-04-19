import { electronApp, optimizer, platform } from "@electron-toolkit/utils";
import { isDevelopmentOrDebug, isProduction } from "@shared/config";
import { Logger } from "@shared/logger";
import "@shared/primitivies";
import { execSync } from "child_process";
import { existsSync } from "fs";
import { join } from "path";
import { BrowserWindow, Menu, MenuItem, Tray, app, shell } from "electron";
// @ts-ignore
import { autoUpdater } from "electron-updater";
// @ts-ignore
import iconWin from "~/build/icon.ico?asset";
// @ts-ignore
import icon from "~/build/icon_24x24.png?asset";
// @ts-ignore
import builderConfig from "../../electron-builder.yml";
import { executableIsAvailable } from "./lib/system/bin.utils";
import { ClipboardMonitor } from "./lib/window/clipboardMonitor";
import contextMenu from "./lib/window/contextMenu";
import { wrapWindowHandler } from "./lib/window/windowUtils";
import { appStore } from "./stores/app/app.store";
import { runMigrate } from "./stores/database/app-database";
import { trpcIpcHandler } from "./trpc";
import { loadUrlOfWindow } from "./trpc/dialog/dialog.utils";
import { pushLogToClient } from "./trpc/events/events.ee";
import { pushWindowState } from "./trpc/window/window.api";
import { checkBrokenLinks } from "./trpc/ytdlp/ytdlp.core";
import { ytdlpEvents } from "./trpc/ytdlp/ytdlp.ee";
import { attachAutoUpdaterIPC } from "./updater";
const log = new Logger("App");
const trayIcon = platform.isWindows ? iconWin : icon;

if (import.meta.env.DEV) {
	const pythonPath = executableIsAvailable("python")
		? executableIsAvailable("python3.13")
		: executableIsAvailable("python3");
	if (!pythonPath) {
		throw new Error("Python not found");
	}
	function createVenv() {
		const venvPath = join(process.cwd(), ".venv");
		if (existsSync(venvPath)) {
			return;
		}
		execSync(`${pythonPath} -m venv --copies ${venvPath}`, { stdio: "inherit" });
		if (platform.isMacOS) {
			execSync(`${venvPath}/bin/activate`, { stdio: "inherit" });
		} else if (platform.isLinux) {
			execSync(`source ${venvPath}/bin/activate`, { stdio: "inherit" });
		}
		execSync(`${venvPath}/bin/pip install -r requirements.txt`, {
			env: { PYTHONPATH: venvPath },
			stdio: "inherit",
		});
	}

	createVenv();
}

async function createWindow() {
	// Create the browser window.
	const tray = new Tray(trayIcon);
	const trayMenu = new Menu();
	trayMenu.append(new MenuItem({ label: "Check for updates", click: () => autoUpdater.checkForUpdates() }));
	trayMenu.append(new MenuItem({ type: "separator" }));
	trayMenu.append(new MenuItem({ label: "Quit", click: () => app.quit() }));
	tray.setIgnoreDoubleClickEvents(true);
	tray.on("right-click", () => trayMenu.popup());
	tray.setContextMenu(trayMenu);
	const [defaultWidth, defaultHeight] = [960, 680];
	const mainWindow = new BrowserWindow({
		width: defaultWidth + 160,
		height: defaultHeight + 178,
		minHeight: defaultHeight,
		minWidth: defaultWidth,
		movable: true,
		minimizable: true,
		maximizable: true,
		resizable: true,
		show: false,
		...(platform.isLinux ? { icon } : { icon: iconWin }),
		...(platform.isWindows && process.argv.find((d) => d === "--use-mica")
			? {
					useContentSize: true,
					backgroundMaterial: "mica",
					titleBarStyle: "hidden",
					backgroundColor: undefined,
				}
			: {
					backgroundColor: "#09090B",
				}),
		...(platform.isMacOS
			? {
					vibrancy: "window",
					backgroundColor: undefined,
				}
			: {}),
		webPreferences: {
			preload: join(__dirname, "../preload/index.js"),
			contextIsolation: false,
			nodeIntegration: true,
			sandbox: false,
			devTools: isDevelopmentOrDebug,
			enableBlinkFeatures: "FluentScrollbars,FluentOverlayScrollbar,OverlayScrollbar",
			additionalArguments: [`--app-path=${__dirname}`, `--app-version=${app.getVersion()}`],
		},
		frame: false,
	});
	tray.on("click", (ev) => {
		mainWindow.show();
		return;
	});
	wrapWindowHandler(mainWindow, "primary", { height: defaultHeight, width: defaultWidth });
	mainWindow.setMaxListeners(100);
	mainWindow.webContents.setWindowOpenHandler((details) => {
		shell.openExternal(details.url);
		return { action: "deny" };
	});
	let isReady = false;
	const readyPromise = new Promise<void>((resolve) => {
		mainWindow.on("ready-to-show", async () => {
			isReady = true;
			if (isDevelopmentOrDebug) {
				mainWindow.webContents.openDevTools({ mode: "detach" });
			}
			app.setAccessibilitySupportEnabled(true);
			resolve();
		});
	});
	tray.setImage(trayIcon);
	trpcIpcHandler.attachWindow(mainWindow);

	// HMR for renderer base on electron-vite cli.
	// Load the remote URL for development or the local html file for production.
	await loadUrlOfWindow(mainWindow, "/");
	attachAutoUpdaterIPC(mainWindow);
	await readyPromise;
	contextMenu({
		window: mainWindow,
		showInspectElement: false,
		showSelectAll: true,
		showLearnSpelling: false,
		showLookUpSelection: true,
	});
	mainWindow.show();
	return mainWindow;
}
// This method will be called when Electron has finished
// initialization and is ready to create browser windows.
// Some APIs can only be used after this event occurs.
app.whenReady().then(async () => {
	app.commandLine.appendSwitch("disable-backgrounding-occluded-windows", "true");
	if (platform.isWindows || platform.isLinux) {
		app.commandLine.appendSwitch("enable-gpu-rasterization"); // performance feature flags
		app.commandLine.appendSwitch("enable-zero-copy");
		app.commandLine.appendSwitch(
			"enable-features",
			"CanvasOopRasterization,EnableDrDc,FluentOverlayScrollbar,OverlayScrollbar",
		); // Enables Display Compositor to use a new gpu thread. todo: testing
	}
	// Set app user model id for windows
	const appUserId = builderConfig.appId.split(".", 2).join(".");
	log.debug({ appUserId });
	electronApp.setAppUserModelId(appUserId);
	if (isProduction) electronApp.setAutoLaunch(true);
	if (!app.requestSingleInstanceLock()) app.quit();
	// Default open or close DevTools by F12 in development
	// and ignore CommandOrControl + R in production.
	// see https://github.com/alex8088/electron-toolkit/tree/master/packages/utils
	app.on("browser-window-created", (_, window) => {
		optimizer.watchWindowShortcuts(window);
		window.webContents.on("did-finish-load", () => {
			pushWindowState();
		});
	});
	await runMigrate();
	await checkBrokenLinks();
	// ytdl.initialize(); // init asynchronously

	createWindow().then((w) => {
		const clipboardWatcher = new ClipboardMonitor(w!, {
			distinct: true,
			onHttpsText(value) {
				log.debug("found https link in clipboard", { value });
				if (appStore.store.features.clipboardMonitor) {
					pushLogToClient("clipboard hit: " + value, "debug");
					if (appStore.store.features.clipboardMonitorAutoAdd) ytdlpEvents.emit("add", value);
					else ytdlpEvents.emit("autoAdd", value);
				}
			},
		});
		appStore.onDidChange("features.clipboardMonitor", (clipboardMonitor, oldClipboardMonitor) => {
			const clipEnabled = clipboardMonitor === true && oldClipboardMonitor === false;
			if (clipEnabled) clipboardWatcher.start();
			else clipboardWatcher.stop();
		});
		if (!isProduction)
			appStore.onDidAnyChange(() => {
				pushLogToClient("store change", "debug");
			});
		if (appStore.store.features.clipboardMonitor) clipboardWatcher.start();

		app.on("will-quit", () => {
			clipboardWatcher.destroy();
		});
	});

	app.on("activate", function () {
		// On macOS it's common to re-create a window in the app when the
		// dock icon is clicked and there are no other windows open.
		if (BrowserWindow.getAllWindows().length === 0) createWindow();
	});
});

// Quit when all windows are closed, except on macOS. There, it's common
// for applications and their menu bar to stay active until the user quits
// explicitly with Cmd + Q.
app.on("window-all-closed", () => {
	if (!platform.isMacOS) {
		app.quit();
	}
});

// In this file you can include the rest of your app"s specific main process
// code. You can also put them in separate files and require them here.
