import { FormControl, FormField, FormItem, FormMessage } from "@renderer/components/ui/form";
import { CheckboxDescription, CheckboxLabel, SwitchButton } from "@renderer/components/ui/switch-button";
import GroupSection from "@renderer/pages/components/group-section";
import SettingsToggle from "@renderer/pages/components/settings-toggle";
import { logger } from "@shared/logger";
import { LucideCog } from "lucide-react";
import { useSettingsForm } from "../form";
import PageContent from "../page-content";
import { SectionMeta } from "../utils";

export const meta: SectionMeta = {
	title: "General",
	icon: LucideCog,
	index: 0,
	show: true,
	description: "General settings for the application",
};

export default function GeneralSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	const form = useSettingsForm();
	logger.child("GeneralSection").info("form", { isDirty: form.formState.isDirty });
	return (
		<PageContent icon={Icon} title='General' description={meta.description} tabId={meta.title}>
			<div className='flex flex-col gap-6'>
				<FormField
					control={form.control}
					name='beta'
					render={({ field }) => (
						<FormItem>
							<FormControl>
								<SwitchButton checked={field.value} onCheckedChange={field.onChange} disabled={field.disabled}>
									<CheckboxLabel>Beta</CheckboxLabel>
									<CheckboxDescription>Switch to beta channel, some features may be unstable or not available in the stable channel.</CheckboxDescription>
								</SwitchButton>
							</FormControl>
							<FormMessage />
						</FormItem>
					)}
				/>

				<GroupSection title='Clipboard Monitor'>
					<div className='flex flex-col gap-2'>
						<SettingsToggle name='features.clipboardMonitor'>
							<div className='flex flex-col gap-2'>
								<span>Enable Clipboard Monitor</span>
								<span className='text-muted-foreground'>Automatically adds any link to the request form.</span>
							</div>
						</SettingsToggle>
						<SettingsToggle name='features.clipboardMonitorAutoAdd'>
							<div className='flex flex-col gap-2'>
								<span>Automatically add links to queue</span>

								<span className='text-muted-foreground'>Activating this will Automatically start the download in the request form.</span>
							</div>
						</SettingsToggle>
					</div>
				</GroupSection>
			</div>
		</PageContent>
	);
}
