export interface AppStore {
	ytdlp: {
		path: string;
		version: string;
		checkForUpdate: boolean;
		flags: {
			nomtime: boolean;
			custom: string;
			outtmpl: string;
		};
	};
	updateChannel: "stable" | "beta";
	autoUpdate: "prompt" | "auto" | "manual";
	download: { paths: string[]; selected: string };
	features: {
		clipboardMonitor: boolean;
		clipboardMonitorAutoAdd: boolean;
		concurrentDownloads: number;
		advancedView: boolean;
	};
	startMinimized: boolean;
	startOnBoot: boolean;
}
