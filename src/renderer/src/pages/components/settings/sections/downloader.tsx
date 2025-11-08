import GroupSection from "@renderer/pages/components/group-section";
import SettingsInput from "@renderer/pages/components/settings-input";
import { LucideDownload } from "lucide-react";
import PageContent from "../page-content";
import { SectionMeta } from "../utils";

export const meta: SectionMeta = {
	title: "Downloader",
	icon: LucideDownload,
	index: 1,
	show: true,
	description: "Downloader settings for the application",
};

export default function DownloaderSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	return (
		<PageContent icon={Icon} title='Downloader' description={meta.description} tabId={meta.title}>
			<GroupSection title='Download'>
				<div className='flex flex-col gap-2'>
					<SettingsInput
						name='features.concurrentDownloads'
						type='number'
						variant={"horizontal"}
						min={1}
						max={window.api.maxParallelism}
						title={"Max concurrent downloads"}
						hint={(<div className='text-muted-foreground text-xs'>{`Recommended: 2`}</div>) as any}
					/>
				</div>
			</GroupSection>
		</PageContent>
	);
}
