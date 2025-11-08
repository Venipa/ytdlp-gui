import { zodResolver } from "@hookform/resolvers/zod";
import { Form } from "@renderer/components/ui/form";
import { trpc } from "@renderer/lib/trpc-link";
import { logger } from "@shared/logger";
import { PropsWithChildren, useCallback } from "react";
import { SubmitHandler, useForm, useFormContext } from "react-hook-form";
import { z } from "zod";
import { useApp } from "../app-context";

const settingsSchema = z.object({
	beta: z.boolean(),
	startMinimized: z.boolean(),
	features: z.object({
		concurrentDownloads: z.coerce.number().min(1).max(window.api.maxParallelism),
		clipboardMonitor: z.coerce.boolean(),
		clipboardMonitorAutoAdd: z.coerce.boolean(),
	}),
	ytdlp: z.object({
		flags: z.object({
			custom: z
				.string()
				.nullish()
				.transform((val) => val?.trim() ?? ""),
		}),
	}),
});
type SettingsValues = z.infer<typeof settingsSchema>;
const log = logger.child("SettingsFormProvider");
export function SettingsFormProvider({ children }: PropsWithChildren) {
	const { updateSettings } = useApp();
	const utils = trpc.useUtils();
	const form = useForm<SettingsValues>({
		resolver: zodResolver(settingsSchema),
		async defaultValues() {
			const settings = await utils.settings.index.fetch();
			return {
				beta: !!settings.beta,
				startMinimized: !!settings.startMinimized,
				features: {
					concurrentDownloads: settings.features.concurrentDownloads ?? 2,
					clipboardMonitor: !!settings.features.clipboardMonitor,
					clipboardMonitorAutoAdd: !!settings.features.clipboardMonitorAutoAdd,
				},
				ytdlp: {
					flags: {
						custom: settings.ytdlp.flags?.custom ?? "",
					},
				},
			};
		},
	});
	const onSubmit: SubmitHandler<SettingsValues> = useCallback(
		(data) => {
			logger.child("onSubmit").info("data", { data });
			updateSettings(data).then((newSettings) => {
				form.reset(newSettings);
			});
		},
		[form, updateSettings],
	);
	return (
		<Form {...form}>
			<form onSubmit={form.handleSubmit(onSubmit)}>{children}</form>
		</Form>
	);
}

export function useSettingsForm() {
	return useFormContext<SettingsValues>();
}
