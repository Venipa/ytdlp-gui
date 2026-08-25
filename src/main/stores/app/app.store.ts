import { createYmlStore } from "@shared/electron/store/createYmlStore";
import { PathsOf } from "@shared/electron/store/inferKey";
import { app } from "electron";
import { AppStore, DEFAULT_OUTTMPL, appStoreSchema } from "./AppStore";
import appStoreMigrations from "./app.migrations";
export interface AppLicense {
	code: string;
	expires: string;
}
const defaultDownloadsPath = app.getPath("downloads");
const store = createYmlStore<AppStore>("app-settings", {
	migrations: appStoreMigrations,
	defaults: appStoreSchema
		.transform((v) => {
			if (!v.download.paths.length) v.download.paths = [defaultDownloadsPath];
			if (!v.download.selected) v.download.selected = defaultDownloadsPath;
			return v;
		})
		.parse({
			ytdlp: {
				flags: { nomtime: true },
				outtmpl: DEFAULT_OUTTMPL,
				version: "internal",
				cliargs: [],
			},
			download: {
				paths: [defaultDownloadsPath],
				selected: defaultDownloadsPath,
			},
			features: {
				clipboardMonitor: true,
				clipboardMonitorAutoAdd: true,
				concurrencyMode: "global",
				concurrentDownloads: 2,
				concurrentDownloadsPerDomain: 2,
				advancedView: false,
			},
			updateChannel: "stable",
			autoUpdate: "prompt",
			startMinimized: false,
			startOnBoot: true,
		} as AppStore),
});

export type AppStoreKeys = PathsOf<AppStore, true>;

export { store as appStore };
