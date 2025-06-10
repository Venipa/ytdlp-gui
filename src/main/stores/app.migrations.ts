import { MAX_PARALLEL_DOWNLOADS } from "@main/trpc/ytdlp.core";
import { Migration } from "electron-conf";
import { AppStore } from "./AppStore";

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
			// Initialize all new flags with default values
			instance.store.ytdlp.flags = {
				...instance.store.ytdlp.flags,
				// Section toggles
				enableYoutubeOptions: false,
				enableStreamingOptions: false,
				enableDownloadOptions: false,
				enableMetadataOptions: false,
				enableSystemOptions: false,
				// YouTube specific options
				noLiveChat: false,
				noYoutubeChannelRedirect: false,
				noYoutubeUnavailableVideos: false,
				noYoutubePreferUtcUploadDate: false,
				// Download and merge options
				noDirectMerge: false,
				embedThumbnailAtomicparsley: false,
				// Metadata options
				noCleanInfojson: false,
				noKeepSubs: false,
				// System options
				noCertificate: false,
				filenameSanitization: false,
				playlistMatchFilter: false,
				// Multi-value options
				youtubeSkip: [],
				hotstarRes: [],
				hotstarVcodec: [],
				hotstarDr: [],
				crunchyrollHardsub: [],
			};
		},
	},
];

export default appStoreMigrations;
