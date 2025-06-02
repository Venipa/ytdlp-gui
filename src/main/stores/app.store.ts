import { MAX_PARALLEL_DOWNLOADS } from "@main/trpc/ytdlp.core";
import { createYmlStore } from "@shared/electron/store/createYmlStore";
import { PathsOf } from "@shared/electron/store/inferKey";
import { app } from "electron";
import { AppStore } from "./AppStore";
import appStoreMigrations from "./app.migrations";
export interface AppLicense {
	code: string;
	expires: string;
}
const defaultDownloadsPath = app.getPath("downloads");
const store = createYmlStore<AppStore>("app-settings", {
	migrations: appStoreMigrations,
	defaults: {
		ytdlp: {
			checkForUpdate: true,
			useGlobal: false,
			flags: { nomtime: true },
		} as AppStore["ytdlp"],
		download: {
			paths: [defaultDownloadsPath],
			selected: defaultDownloadsPath,
		},
		features: {
			clipboardMonitor: true,
			clipboardMonitorAutoAdd: true,
			concurrentDownloads: MAX_PARALLEL_DOWNLOADS,
			advancedView: false,
		},
		startMinimized: false,
		startOnBoot: true,
		beta: false,
	},
});

export type AppStoreKeys = PathsOf<AppStore, true>;

export { store as appStore };
