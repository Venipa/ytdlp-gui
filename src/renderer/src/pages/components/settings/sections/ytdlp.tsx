import { Badge } from "@renderer/components/ui/badge";
import { QTooltip } from "@renderer/components/ui/tooltip";
import GroupSection from "@renderer/pages/components/group-section";
import SettingsInput from "@renderer/pages/components/settings-input";
import SettingsSelect, { SettingsSelectItem } from "@renderer/pages/components/settings-select";
import SettingsToggle from "@renderer/pages/components/settings-toggle";
import { useSettingsForm } from "@renderer/pages/components/settings/form";
import PageContent from "@renderer/pages/components/settings/page-content";
import { SectionMeta } from "@renderer/pages/components/settings/utils";
import { LucideDownload } from "lucide-react";
const exampleFlags = [
	"--concurrent-fragments 16",
	"--cache-dir /path/to/cache",
	{
		label: "--no-mtime",
		hint: "The --no-mtime flag in yt-dlp is used to prevent the program from setting the modification time of the downloaded file to the original upload time of the video",
	},
	{
		label: "--no-check-certificates",
		hint: "The --no-check-certificates flag in yt-dlp is used to prevent the program from checking the certificates of the downloaded file",
	},
	{
		label: "--embed-subs",
		hint: "The --embed-subs flag in yt-dlp is used to embed the subtitles of the downloaded file",
	},
	{
		label: "--embed-thumbnail",
		hint: "The --embed-thumbnail flag in yt-dlp is used to embed the thumbnail of the downloaded file",
	},
	{
		label: "--concurrent-fragments 16",
		hint: "The --concurrent-fragments 16 flag in yt-dlp is used to set the number of concurrent fragments to 16",
	},
];

export const meta: SectionMeta = {
	title: "YTDLP",
	icon: LucideDownload,
	index: 2,
	show: true,
	description: "YTDLP settings",
};
export default function YtdlpSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	const form = useSettingsForm();
	return (
		<PageContent icon={Icon} title={meta.title} description={meta.description} tabId={meta.title}>
			<div className='flex flex-col gap-6'>
				<div className='flex flex-col gap-2'>
					<h2 className='text-lg font-medium'>YTDLP</h2>
				</div>

				<GroupSection title='Flags' className='gap-6'>
					<SettingsToggle name='ytdlp.useGlobal' disabled={window.api.platform.isWindows}>
						<div className='flex flex-col gap-2 text-pretty'>
							<span className='font-bold'>Use machine installed yt-dlp</span>
							<span className='text-muted-foreground leading-4 align-middle'>This only works if you've already installed yt-dlp globally.</span>
							<span className='text-yellow-400 text-xs'>(only supported for: MacOS, Linux)</span>
						</div>
					</SettingsToggle>

					<SettingsInput
						name='ytdlp.flags.custom'
						title={"Custom arguments, check yt-dlp docs for more info ..."}
						hint={
							<div className='text-muted-foreground text-xs flex flex-wrap gap-1'>
								{exampleFlags.map((flag) => {
									const label = typeof flag === "string" ? flag : flag.label;
									const hint = typeof flag === "string" ? undefined : flag.hint;
									if (hint) {
										return (
											<QTooltip content={<div className='text-xs w-64'>{hint}</div>} key={label}>
												<Badge
													variant='pre'
													className='cursor-pointer hover:bg-muted/40'
													onClick={() =>
														form.setValue(
															"ytdlp.flags.custom",
															form.getValues().ytdlp.flags.custom ? `${form.getValues().ytdlp.flags.custom} ${label}` : label,
															{
																shouldDirty: true,
																shouldTouch: true,
																shouldValidate: true,
															},
														)
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
												form.setValue(
													"ytdlp.flags.custom",
													form.getValues().ytdlp.flags.custom ? `${form.getValues().ytdlp.flags.custom} ${label}` : label,
													{
														shouldDirty: true,
														shouldTouch: true,
														shouldValidate: true,
													},
												)
											}
											key={label}>
											{label}
										</Badge>
									);
								})}
							</div>
						}
					/>
				</GroupSection>
				<GroupSection title='Updates'>
					<SettingsSelect name='ytdlp.updateChannel' title='Update channel'>
						<SettingsSelectItem value='stable' label='Stable' />
						<SettingsSelectItem value='nightly' label='Nightly' />
						<SettingsSelectItem value='source' label='Directly from source' />
					</SettingsSelect>
					<SettingsSelect name='ytdlp.autoUpdate' title='Auto update'>
						<SettingsSelectItem value='prompt' label='Prompt for update' />
						<SettingsSelectItem value='auto' label='Automatically update' />
						<SettingsSelectItem value='manual' label='Manual update' />
					</SettingsSelect>
					<SettingsToggle name='ytdlp.checkForUpdate'>
						<div className='flex flex-col gap-2 text-pretty'>
							<span className='font-bold'>Check for updates</span>
							<span className='text-muted-foreground leading-4 align-middle'>Automatically check for updates for yt-dlp.</span>
						</div>
					</SettingsToggle>
				</GroupSection>
			</div>
		</PageContent>
	);
}
