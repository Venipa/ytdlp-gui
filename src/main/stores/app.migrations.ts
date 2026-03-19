import { MAX_PARALLEL_DOWNLOADS } from "@main/trpc/ytdlp.core";
import { Migration } from "electron-conf";
import { AppStore } from "./AppStore";
const removeProperty = (instance: any, property: string) => {
	if (property in instance) delete (instance as any)[property];
};

const appStoreMigrations: Migration<AppStore>[] = [
	{
		version: 0,
		hook(instance, currentVersion) {
			instance.store.features.concurrentDownloads = MAX_PARALLEL_DOWNLOADS;
		},
	},
	{
		version: 1,
		hook(instance, currentVersion) {
			instance.store.startMinimized = false;
			instance.store.startOnBoot = true;
		},
	},
	{
		version: 2,
		hook(instance, currentVersion) {
			instance.store.features.advancedView = false;
		},
	},
	{
		version: 3,
		hook(instance, currentVersion) {
			instance.store.ytdlp.path = "internal";
			removeProperty(instance.store.ytdlp, "checkForUpdate");
		},
	},
];

export default appStoreMigrations;
