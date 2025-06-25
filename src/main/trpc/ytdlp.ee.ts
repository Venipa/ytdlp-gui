import EventEmitter from "events";
import { appStore } from "@main/stores/app.store";
import { logger } from "@shared/logger";
import PQueue from "p-queue";
import { MAX_PARALLEL_DOWNLOADS } from "./ytdlp.core";
export const ytdlpEvents = new EventEmitter();

export const pushToastToClient = (message: string, type?: string, description?: string) => {
	return ytdlpEvents.emit("toast", { message, type, description });
};
ytdlpEvents.setMaxListeners(10000);

export const ytdlpDownloadQueue = new PQueue({
	concurrency: appStore.store.features.concurrentDownloads,
});
const log = logger.child("YTDLPQueue");
appStore.onDidChange("features", (features) => {
	if (!features) return;
	ytdlpDownloadQueue.concurrency = features.concurrentDownloads ?? MAX_PARALLEL_DOWNLOADS;
});
ytdlpDownloadQueue.on("active", () => {
	ytdlpEvents.emit("queue", { pending: ytdlpDownloadQueue.pending });
});
ytdlpDownloadQueue.on("next", () => {
	ytdlpEvents.emit("queue", { pending: ytdlpDownloadQueue.pending });
});
ytdlpDownloadQueue.on("error", (err) => {
	log.error(err);
});
ytdlpDownloadQueue.on("completed", () => {
	ytdlpEvents.emit("queue", { pending: ytdlpDownloadQueue.pending });
});
export {};
