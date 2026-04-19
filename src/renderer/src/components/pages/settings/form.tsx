import { zodResolver } from "@hookform/resolvers/zod";
import { appStoreSchema } from "@main/stores/app/AppStore";
import { Form } from "@renderer/components/ui/form";
import { trpc } from "@renderer/lib/api/trpc-link";
import { logger } from "@shared/logger";
import { PropsWithChildren, useCallback } from "react";
import { SubmitHandler, useForm, useFormContext } from "react-hook-form";
import { z } from "zod";
import { useApp } from "../app-context";

const settingsSchema = appStoreSchema;
type SettingsValues = z.infer<typeof settingsSchema>;
const log = logger.child("SettingsFormProvider");
export function SettingsFormProvider({ children }: PropsWithChildren) {
	const { updateSettings } = useApp();
	const utils = trpc.useUtils();
	const form = useForm<SettingsValues>({
		resolver: zodResolver(settingsSchema),
		async defaultValues() {
			const settings = await utils.settings.index.fetch();
			const parsedSettings = settingsSchema.parse(settings);
			return parsedSettings;
		},
	});
	const onSubmit: SubmitHandler<SettingsValues> = useCallback(
		(data) => {
			log.child("onSubmit").info("data", { data });
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
