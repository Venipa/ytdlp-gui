import GroupSection from "@renderer/pages/components/group-section";
import SettingsInput from "@renderer/pages/components/settings-input";
import { useSettingsForm } from "@renderer/pages/components/settings/form";
import { LucideDownload } from "lucide-react";
import PageContent from "../page-content";
import { SectionMeta } from "../utils";

export const meta: SectionMeta = {
	title: "Download",
	icon: LucideDownload,
	index: 1,
	show: true,
	description: "Downloader settings for the application",
};
const exampleFlags = ["--concurrent-fragments 16", "--cache-dir /path/to/cache", "--no-mtime", "--embed-thumbnail", "--embed-subs", "--no-check-certificates"];
export default function DownloaderSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	const form = useSettingsForm();
	return (
		<PageContent icon={Icon} title={meta.title} description={meta.description} tabId={meta.title}>
			<div className='flex flex-col gap-0'>
				<h2 className='text-lg font-medium'>{meta.title}</h2>
				<p className='text-xs text-muted-foreground'>{meta.description}</p>
			</div>
			<GroupSection title='Download'>
				<div className='flex flex-col gap-2'>
					<SettingsInput
						name='features.concurrentDownloads'
						type='number'
						min={1}
						max={window.api.maxParallelism}
						title={"Max concurrent downloads"}
						hint={`Recommended: 2`}></SettingsInput>
				</div>
			</GroupSection>
		</PageContent>
	);
}
