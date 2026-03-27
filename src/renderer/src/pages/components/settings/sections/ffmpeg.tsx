import GroupSection from "@renderer/pages/components/group-section";
import DependenciesForm from "@renderer/pages/components/settings/dependencies";
import PageContent from "@renderer/pages/components/settings/page-content";
import { SectionMeta } from "@renderer/pages/components/settings/utils";
import { LucideDownload } from "lucide-react";

export const meta: SectionMeta = {
	title: "FFmpeg",
	icon: LucideDownload,
	index: 3,
	show: true,
	description: "Manage the FFmpeg dependency for the application",
};
export default function YtdlpSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	return (
		<PageContent icon={Icon} title={meta.title} description={meta.description} tabId={meta.title}>
			<div className='flex flex-col gap-0'>
				<h2 className='text-lg font-medium'>{meta.title}</h2>
				<p className='text-xs text-muted-foreground'>{meta.description}</p>
			</div>
			<div className='flex flex-col gap-6'>
				<GroupSection title='Media processing' className='gap-6'>
					<DependenciesForm dependencyKey='ffmpeg' />
				</GroupSection>
			</div>
		</PageContent>
	);
}
