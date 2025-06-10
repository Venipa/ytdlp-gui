export interface AppStore {
	ytdlp: {
		path: string;
		version: string;
		checkForUpdate: boolean;
		useGlobal: boolean;
		flags: {
			nomtime: boolean;
			custom: string;
			// Section toggles
			enableYoutubeOptions: boolean;
			enableStreamingOptions: boolean;
			enableDownloadOptions: boolean;
			enableMetadataOptions: boolean;
			enableSystemOptions: boolean;
			// YouTube specific options
			noLiveChat: boolean;
			noYoutubeChannelRedirect: boolean;
			noYoutubeUnavailableVideos: boolean;
			noYoutubePreferUtcUploadDate: boolean;
			// Download and merge options
			noDirectMerge: boolean;
			embedThumbnailAtomicparsley: boolean;
			// Metadata options
			noCleanInfojson: boolean;
			noKeepSubs: boolean;
			// System options
			noCertificate: boolean;
			filenameSanitization: boolean;
			playlistMatchFilter: boolean;
			// Multi-value options
			youtubeSkip: string[];
			hotstarRes: string[];
			hotstarVcodec: string[];
			hotstarDr: string[];
			crunchyrollHardsub: string[];
		};
	};
	download: { paths: string[]; selected: string };
	features: {
		clipboardMonitor: boolean;
		clipboardMonitorAutoAdd: boolean;
		concurrentDownloads: number;
		advancedView: boolean;
	};
	startMinimized: boolean;
	startOnBoot: boolean;
	beta: boolean;
}
