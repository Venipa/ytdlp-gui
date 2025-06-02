import ClickableText from "@renderer/components/ui/clickable-text";
import { Spinner } from "@renderer/components/ui/spinner";
import { trpc } from "@renderer/lib/trpc-link";
import { logger } from "@shared/logger";
import { LucideCog } from "lucide-react";
import { toast } from "sonner";
import GroupSection from "../components/group-section";
import SettingsInput from "../components/settings-input";
import SettingsToggle from "../components/settings-toggle";

export const meta = {
	title: "Settings",
	icon: LucideCog,
	index: 10,
	show: true,
};
const Icon = meta.icon;
export default function SettingsTab() {
	const { mutateAsync: checkUpdate } = trpc.internals.checkUpdate.useMutation();
	const { mutateAsync: checkYtdlUpdate, isLoading: ytdlLoading } = trpc.ytdl.checkUpdates.useMutation();
	const handleYtdlUpdate = async () => {
		try {
			const id = toast.loading("Checking for YTDLP updates...", { duration: 0 });
			const { updated, currentVersion } = await checkYtdlUpdate();
			if (updated) toast.success("YTDLP has been updated to " + currentVersion, { id, duration: 3000 });
			else
				toast.info(`YTDLP is already the newest version (${currentVersion})`, {
					id,
					duration: 3000,
				});
		} finally {
		}
	};
	return (
		<div className='grid gap-8 p-2 h-full'>
			<div className='grid gap-6 pt-10'>
				<div className='flex items-start justify-between'>
					<div className='flex items-center gap-2'>
						<Icon className='size-5' />
						<h1 className='text-lg font-semibold'>Settings</h1>
					</div>
				</div>
				<div className='flex flex-col gap-6'>
					<SettingsToggle name='beta' onChange={() => checkUpdate().then(logger.debug.bind(logger))}>
						<span>Sign up for Beta releases.</span>
					</SettingsToggle>

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
					<GroupSection
						title='YTDLP'
						titleRight={
							<div className='flex self-start items-center mt-1.5 ml-auto'>
								{ytdlLoading ? (
									<div className='flex items-center gap-1'>
										<Spinner size={"xs"} />
										<ClickableText>Checking...</ClickableText>
									</div>
								) : (
									<ClickableText onClick={handleYtdlUpdate}>Check for update</ClickableText>
								)}
							</div>
						}>
						<div className='flex flex-col gap-6'>
							<div className='flex flex-col gap-2'>
								{
									<SettingsToggle name='ytdlp.useGlobal' disabled={window.api.platform.isWindows}>
										<div className='flex flex-col gap-2 text-pretty'>
											<span className='font-bold'>Use machine installed yt-dlp</span>
											<span className='text-muted-foreground leading-4 align-middle'>This only works if you've already installed yt-dlp globally.</span>
											<span className='text-yellow-400 text-xs'>(only supported for: MacOS, Linux)</span>
										</div>
									</SettingsToggle>
								}
								<SettingsToggle name='ytdlp.flags.mtime'>
									<div className='flex flex-col gap-2 text-pretty'>
										<span className='font-bold'>--no-mtime</span>
										<span className='text-muted-foreground leading-4 align-middle'>
											The <code className='text-xs border border-primary/10 rounded p-px bg-input'>--no-mtime</code> flag in yt-dlp is used to prevent the
											program from setting the modification time of the downloaded file to the original upload time of the video
										</span>
									</div>
								</SettingsToggle>
							</div>
							<SettingsInput
								name='ytdlp.flags.custom'
								title={
									<>
										Custom arguments, check{" "}
										<ClickableText asChild>
											<a href='https://github.com/yt-dlp/yt-dlp' target='_blank' className='cursor-pointer'>
												yt-dlp docs
											</a>
										</ClickableText>{" "}
										for more info ...
									</>
								}
							/>
						</div>
					</GroupSection>
				</div>
			</div>
		</div>
	);
}
