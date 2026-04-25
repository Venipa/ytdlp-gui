import GroupSection from "@renderer/components/pages/group-section";
import SettingsInput from "@renderer/components/pages/settings-input";
import { useSettingsForm } from "@renderer/components/pages/settings/form";
import PageContent from "@renderer/components/pages/settings/page-content";
import { SectionMeta } from "@renderer/components/pages/settings/utils";
import { Badge } from "@renderer/components/ui/badge";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { LucideDownload } from "lucide-react";
import { useCallback } from "react";
const exampleFlags = [
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
	{
		label: "--cookies",
		hint: "The --cookies flag in yt-dlp is used to set the cookies for the downloaded file",
	},
];

export const meta: SectionMeta = {
	title: "YTDLP",
	icon: LucideDownload,
	index: 2,
	show: true,
	description: "You are using embedded yt-dlp (some settings may not be available)",
};
export default function YtdlpSection({ meta }: { meta: SectionMeta }) {
	const Icon = meta.icon;
	const form = useSettingsForm();
	const cliFormatter = useCallback((values: string[]) => values?.join(" "), []);
	const cliParser = useCallback((value: string) => value.split(" "), []);
	return (
		<PageContent icon={Icon} title={meta.title} description={meta.description} tabId={meta.title}>
			<div className='flex flex-col gap-0'>
				<h2 className='text-lg font-medium'>{meta.title}</h2>
				<p className='text-xs text-muted-foreground'>{meta.description}</p>
			</div>
			<div className='flex flex-col gap-6'>
				<GroupSection title='Flags' className='gap-6'>
					<SettingsInput
						name='ytdlp.cliargs'
						formatter={cliFormatter}
						parser={cliParser}
						title={"CLI arguments, check yt-dlp docs for more info ..."}
						hint={
							<div className='text-muted-foreground text-xs flex flex-wrap gap-1'>
								{exampleFlags.map((flag) => {
									const label = typeof flag === "string" ? flag : flag.label;
									const hint = typeof flag === "string" ? undefined : flag.hint;
									if (hint) {
										return (
											<QTooltip
												content={(<div className='text-xs w-64'>{hint}</div>) as any}
												key={label}>
												<Badge
													variant='pre'
													className='cursor-pointer hover:bg-muted/40'
													onClick={() =>
														form.setValue(
															"ytdlp.cliargs",
															[...form.getValues().ytdlp.cliargs, label],
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
													"ytdlp.cliargs",
													[...form.getValues().ytdlp.cliargs, label],
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
			</div>
		</PageContent>
	);
}
