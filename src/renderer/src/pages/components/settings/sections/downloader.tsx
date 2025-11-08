import { Badge } from "@renderer/components/ui/badge";
import GroupSection from "@renderer/pages/components/group-section";
import SettingsInput from "@renderer/pages/components/settings-input";
import SettingsToggle from "@renderer/pages/components/settings-toggle";
import { useSettingsForm } from "@renderer/pages/components/settings/form";
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
const exampleFlags = ["--concurrent-fragments 16", "--cache-dir /path/to/cache", "--no-mtime", "--embed-thumbnail", "--embed-subs", "--no-check-certificates"];
export default function DownloaderSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	const form = useSettingsForm();
	return (
		<PageContent icon={Icon} title={meta.title} description={meta.description} tabId={meta.title}>
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
			<GroupSection title='YTDLP Flags'>
				<SettingsToggle name='ytdlp.useGlobal' disabled={window.api.platform.isWindows}>
					<div className='flex flex-col gap-2 text-pretty'>
						<span className='font-bold'>Use machine installed yt-dlp</span>
						<span className='text-muted-foreground leading-4 align-middle'>This only works if you've already installed yt-dlp globally.</span>
						<span className='text-yellow-400 text-xs'>(only supported for: MacOS, Linux)</span>
					</div>
				</SettingsToggle>
				<SettingsToggle name='ytdlp.flags.mtime'>
					<div className='flex flex-col gap-2 text-pretty'>
						<span className='font-bold'>--no-mtime</span>
						<span className='text-muted-foreground leading-4 align-middle'>
							The <code className='text-xs border border-primary/10 rounded p-px bg-input'>--no-mtime</code> flag in yt-dlp is used to prevent the program from
							setting the modification time of the downloaded file to the original upload time of the video
						</span>
					</div>
				</SettingsToggle>
			</GroupSection>
			<GroupSection title='YTDLP Commmand Flags'>
				<div className='flex flex-col gap-2'>
					<SettingsInput
						name='ytdlp.flags.custom'
						title={"Custom arguments, check yt-dlp docs for more info ..."}
						hint={
							<div className='text-muted-foreground text-xs flex flex-wrap gap-1'>
								{exampleFlags.map((flag) => (
									<Badge
										variant='pre'
										className='cursor-pointer hover:bg-muted/40'
										onClick={() =>
											form.setValue("ytdlp.flags.custom", form.getValues().ytdlp.flags.custom ? `${form.getValues().ytdlp.flags.custom} ${flag}` : flag, {
												shouldDirty: true,
												shouldTouch: true,
												shouldValidate: true,
											})
										}
										key={flag}>
										{flag}
									</Badge>
								))}
							</div>
						}
					/>
				</div>
			</GroupSection>
		</PageContent>
	);
}
