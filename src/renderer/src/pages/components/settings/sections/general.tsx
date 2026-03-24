import GroupSection from "@renderer/pages/components/group-section";
import SettingsSelect, { SettingsSelectItem } from "@renderer/pages/components/settings-select";
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
		<PageContent icon={Icon} title={meta.title} description={meta.description} tabId={meta.title}>
			<div className='flex flex-col gap-0'>
				<h2 className='text-lg font-medium'>{meta.title}</h2>
				<p className='text-xs text-muted-foreground'>{meta.description}</p>
			</div>
			<div className='flex flex-col gap-6'>
				<GroupSection title='Updates'>
					<SettingsSelect name='updateChannel' title='Update channel'>
						<SettingsSelectItem value='stable' label='Stable' />
						<SettingsSelectItem value='nightly' label='Nightly' />
						<SettingsSelectItem value='source' label='Directly from source' />
					</SettingsSelect>
					<SettingsSelect name='autoUpdate' title='Auto update'>
						<SettingsSelectItem value='prompt' label='Prompt for update' />
						<SettingsSelectItem value='auto' label='Automatically update' />
						<SettingsSelectItem value='manual' label='Manual update' />
					</SettingsSelect>
				</GroupSection>
				<GroupSection title='Clipboard Monitor'>
					<div className='flex flex-col gap-2'>
						<SettingsToggle name='features.clipboardMonitor'>
							<div className='flex flex-col gap-1 text-sm'>
								<span>Enable Clipboard Monitor</span>
								<span className='text-muted-foreground'>Automatically adds any link to the request form.</span>
							</div>
						</SettingsToggle>
						<SettingsToggle name='features.clipboardMonitorAutoAdd'>
							<div className='flex flex-col gap-1 text-sm'>
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
