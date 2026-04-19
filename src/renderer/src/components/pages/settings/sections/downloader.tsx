import { DEFAULT_OUTTMPL } from "@main/stores/app/AppStore";
import GroupSection from "@renderer/components/pages/group-section";
import SettingsInput from "@renderer/components/pages/settings-input";
import { useSettingsForm } from "@renderer/components/pages/settings/form";
import { Badge } from "@renderer/components/ui/badge";
import { QTooltip } from "@renderer/components/ui/tooltip";
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
type OuttmplExample = {
	label: string;
	value: string;
	hint?: string;
};
const OUTTMPL_EXAMPLES: OuttmplExample[] = [
	{
		label: "Default",
		value: DEFAULT_OUTTMPL,
		hint: "The default filename template is used when no other template is specified. It is a combination of the source, id, and title of the video.",
	},
	{
		label: "Only title",
		value: "%(title)s.%(ext)s",
		hint: "The filename template is only the title of the video.",
	},
	{
		label: "Title with id",
		value: "%(title)s [%(id)s].%(ext)s",
		hint: "The filename template is the title of the video with the id of the video.",
	},
	{
		label: "Title with uploader",
		value: "%(title)s [%(uploader)s].%(ext)s",
		hint: "The filename template is the title of the video with the uploader of the video.",
	},
];

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
				<div className='flex flex-col gap-6'>
					<SettingsInput
						name='features.concurrentDownloads'
						type='number'
						min={1}
						max={window.api.maxParallelism}
						title={"Max concurrent downloads"}
						hint={`Recommended: 2`}></SettingsInput>
					<SettingsInput
						name='ytdlp.outtmpl'
						defaultValue={DEFAULT_OUTTMPL}
						title='Filename template'
						hint={
							<div className='text-muted-foreground text-xs flex flex-wrap gap-1'>
								<div className='text-muted-foreground text-xs flex flex-wrap gap-1'>
									{OUTTMPL_EXAMPLES.map((flag) => {
										const label = typeof flag === "string" ? flag : flag.label;
										const hint = typeof flag === "string" ? undefined : flag.hint;
										if (hint) {
											return (
												<QTooltip
													content={
														(
															<div className='text-xs w-64 flex flex-col gap-1'>
																<span>{hint}</span>
																<code>{flag.value}</code>
															</div>
														) as any
													}
													key={label}>
													<Badge
														variant='pre'
														className='cursor-pointer hover:bg-muted/40'
														onClick={() =>
															form.setValue("ytdlp.outtmpl", flag.value, {
																shouldDirty: true,
																shouldTouch: true,
																shouldValidate: true,
															})
														}>
														{label}
													</Badge>
												</QTooltip>
											);
										}
										return (
											<Badge
												variant='pre'
												className='cursor-pointer hover:bg-muted/40'
												onClick={() =>
													form.setValue("ytdlp.outtmpl", flag.value, {
														shouldDirty: true,
														shouldTouch: true,
														shouldValidate: true,
													})
												}
												key={label}>
												{label}
											</Badge>
										);
									})}
								</div>
							</div>
						}
					/>
				</div>
			</GroupSection>
		</PageContent>
	);
}
