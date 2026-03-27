import { z } from "zod";

const DEFAULT_PARALLEL_DOWNLOADS = 2;
export const DEFAULT_OUTTMPL = `[%(source)s_%(id)s] %(title)s.%(ext)s`;

export const appStoreSchema = z
	.object({
		ytdlp: z
			.object({
				ffmpegPath: z.string().nullish(),
				flags: z
					.object({
						nomtime: z.boolean().fallback(true),
					})
					.fallback({}),
				outtmpl: z.string().fallback(DEFAULT_OUTTMPL),
				version: z.string().fallback("internal"),
				cliargs: z
					.union([z.array(z.string()), z.string()])
					.nullish()
					.transform((v) => {
						if (!v) return [] as string[];
						if (typeof v === "string") return v.split(" ");
						return v;
					}),
			})
			.fallback({}),
		download: z
			.object({
				paths: z.array(z.string()).fallback([]),
				selected: z.string().fallback(""),
			})
			.fallback({}),
		features: z
			.object({
				clipboardMonitor: z.boolean().fallback(false),
				clipboardMonitorAutoAdd: z.boolean().fallback(true),
				concurrentDownloads: z.coerce.number().fallback(DEFAULT_PARALLEL_DOWNLOADS),
				advancedView: z.boolean().fallback(false),
			})
			.fallback({}),
		updateChannel: z.enum(["stable", "beta"]).fallback("stable"),
		autoUpdate: z.enum(["prompt", "auto", "manual"]).fallback("prompt"),
		startMinimized: z.boolean().fallback(false),
		startOnBoot: z.boolean().fallback(true),
	})
	.fallback({});

export type AppStore = z.infer<typeof appStoreSchema>;
