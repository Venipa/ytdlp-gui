import { MAX_PARALLEL_DOWNLOADS } from "@main/trpc/ytdlp/ytdlp.core";
import { DEFAULT_OUTTMPL } from "@main/trpc/ytdlp/ytdlp.utils";
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
			removeProperty(instance.store.ytdlp, "path");
			removeProperty(instance.store.ytdlp, "checkForUpdate");
		},
	},
	{
		version: 4,
		hook(instance, currentVersion) {
			instance.store.updateChannel = "stable";
			instance.store.autoUpdate = "prompt";
			removeProperty(instance.store, "checkForUpdate");
			removeProperty(instance.store.ytdlp, "checkForUpdate");
		},
	},
	{
		version: 5,
		hook(instance, currentVersion) {
			instance.set("ytdlp.flags.outtmpl", DEFAULT_OUTTMPL);
		},
	},
	{
		version: 6,
		hook(instance, currentVersion) {
			removeProperty(instance.store.ytdlp, "custom");
		},
	},
];

export default appStoreMigrations;
