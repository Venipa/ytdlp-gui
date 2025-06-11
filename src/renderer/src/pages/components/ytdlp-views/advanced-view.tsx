import ClickableText from "@renderer/components/ui/clickable-text";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@renderer/components/ui/collapsible";
import { Form, FormDescription, FormField, FormItem, FormLabel } from "@renderer/components/ui/form";
import { SectionToggle } from "@renderer/components/ui/section-toggle";
import { SelectOption } from "@renderer/components/ui/select-option";
import { Textarea } from "@renderer/components/ui/textarea";
import { ToggleOption } from "@renderer/components/ui/toggle-option";
import { cn } from "@renderer/lib/utils";
import { debounce } from "lodash";
import { LucideChevronDown } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useForm } from "react-hook-form";
import { toast } from "sonner";
import { useApp } from "../app-context";

function AdvancedViewContent() {
	const { settings, setSetting, setSettingsObject } = useApp();
	const flags = useMemo(() => {
		return settings?.ytdlp?.flags ?? {};
	}, [settings]);
	const form = useForm({
		defaultValues: {
			flags: {
				nomtime: flags?.nomtime,
				custom: flags?.custom,
				// Section toggles
				enableYoutubeOptions: flags?.enableYoutubeOptions ?? false,
				enableStreamingOptions: flags?.enableStreamingOptions ?? false,
				enableDownloadOptions: flags?.enableDownloadOptions ?? false,
				enableMetadataOptions: flags?.enableMetadataOptions ?? false,
				enableSystemOptions: flags?.enableSystemOptions ?? false,
				// YouTube specific options
				noLiveChat: flags?.noLiveChat,
				noYoutubeChannelRedirect: flags?.noYoutubeChannelRedirect,
				noYoutubeUnavailableVideos: flags?.noYoutubeUnavailableVideos,
				noYoutubePreferUtcUploadDate: flags?.noYoutubePreferUtcUploadDate,
				// Download and merge options
				noDirectMerge: flags?.noDirectMerge,
				embedThumbnailAtomicparsley: flags?.embedThumbnailAtomicparsley,
				// Metadata options
				noCleanInfojson: flags?.noCleanInfojson,
				noKeepSubs: flags?.noKeepSubs,
				// System options
				noCertificate: flags?.noCertificate,
				filenameSanitization: flags?.filenameSanitization,
				playlistMatchFilter: flags?.playlistMatchFilter,
				// Multi-value options
				youtubeSkip: flags?.youtubeSkip || [],
				hotstarRes: flags?.hotstarRes || [],
				hotstarVcodec: flags?.hotstarVcodec || [],
				hotstarDr: flags?.hotstarDr || [],
				crunchyrollHardsub: flags?.crunchyrollHardsub || [],
			},
		},
	});

	useEffect(() => {
		const watcher = form.watch(
			debounce(async (v) => {
				const changedValues = Object.keys(v.flags)
					.filter((key) => v.flags[key] !== settings?.flags?.[key])
					.map((key) => ({ key, value: v.flags[key] }));
				if (changedValues.length > 0) {
					await setSettingsObject(changedValues.reduce((acc, { key, value }) => ({ ...acc, [`ytdlp.flags.${key}`]: value }), settings!));
					toast.success("Settings have been saved");
				}
			}, 1000),
		);
		return () => watcher.unsubscribe();
	}, [form.watch]);

	return (
		<>
			<div className='flex flex-col gap-4 my-6'>
				<Form {...form}>
					<form className='flex flex-col gap-4 lg:gap-6 lg:grid lg:grid-cols-2 lg:items-start lg:justify-start'>
						<SectionToggle control={form.control} name='flags.enableYoutubeOptions' label='YouTube Options' description='Configure YouTube-specific download options'>
							<div className='space-y-4'>
								<SelectOption
									control={form.control}
									name='flags.youtubeSkip'
									label='YouTube Skip Options'
									description='Choose which YouTube operations to skip. Multiple selections allowed.'
									options={[
										{ value: "webpage", label: "Skip initial webpage download" },
										{ value: "authcheck", label: "Allow playlist downloads without initial webpage" },
									]}
								/>
								<ToggleOption
									control={form.control}
									name='flags.noLiveChat'
									label='No Live Chat'
									description='Prevent downloading of live chat/danmaku as subtitles'
								/>
								<ToggleOption
									control={form.control}
									name='flags.noYoutubeChannelRedirect'
									label='No YouTube Channel Redirect'
									description='Prevent automatic redirection to channel uploads when using channel URLs'
								/>
								<ToggleOption
									control={form.control}
									name='flags.noYoutubeUnavailableVideos'
									label='No YouTube Unavailable Videos'
									description='Remove unavailable videos from YouTube playlists'
								/>
								<ToggleOption
									control={form.control}
									name='flags.noYoutubePreferUtcUploadDate'
									label='No YouTube UTC Upload Date'
									description='Prefer non-UTC upload dates for YouTube videos'
								/>
							</div>
						</SectionToggle>

						<SectionToggle control={form.control} name='flags.enableStreamingOptions' label='Streaming Options' description='Configure options for streaming platforms'>
							<div className='space-y-4'>
								<SelectOption
									control={form.control}
									name='flags.hotstarRes'
									label='Hotstar Resolution Ignore'
									description='Select Hotstar resolutions to ignore during download'
									options={[
										{ value: "sd", label: "SD" },
										{ value: "hd", label: "HD" },
										{ value: "fhd", label: "Full HD" },
									]}
								/>
								<SelectOption
									control={form.control}
									name='flags.hotstarVcodec'
									label='Hotstar Video Codec Ignore'
									description='Select Hotstar video codecs to ignore during download'
									options={[
										{ value: "h264", label: "H.264" },
										{ value: "h265", label: "H.265" },
										{ value: "dvh265", label: "DV H.265" },
									]}
								/>
								<SelectOption
									control={form.control}
									name='flags.hotstarDr'
									label='Hotstar Dynamic Range Ignore'
									description='Select Hotstar dynamic ranges to ignore during download'
									options={[
										{ value: "sdr", label: "SDR" },
										{ value: "hdr10", label: "HDR10" },
										{ value: "dv", label: "Dolby Vision" },
									]}
								/>
								<SelectOption
									control={form.control}
									name='flags.crunchyrollHardsub'
									label='Crunchyroll Hardsub Versions'
									description='Select Crunchyroll hardsub versions to ignore'
									options={[
										{ value: "en-US", label: "English (US)" },
										{ value: "en-GB", label: "English (UK)" },
										{ value: "es-419", label: "Spanish (Latin America)" },
										{ value: "es-ES", label: "Spanish (Spain)" },
										{ value: "pt-BR", label: "Portuguese (Brazil)" },
										{ value: "pt-PT", label: "Portuguese (Portugal)" },
										{ value: "fr-FR", label: "French (France)" },
										{ value: "de-DE", label: "German" },
										{ value: "it-IT", label: "Italian" },
										{ value: "ru-RU", label: "Russian" },
										{ value: "ar-SA", label: "Arabic" },
										{ value: "hi-IN", label: "Hindi" },
										{ value: "bn-IN", label: "Bengali" },
										{ value: "ta-IN", label: "Tamil" },
										{ value: "te-IN", label: "Telugu" },
										{ value: "ml-IN", label: "Malayalam" },
										{ value: "kn-IN", label: "Kannada" },
									]}
								/>
							</div>
						</SectionToggle>

						<SectionToggle
							control={form.control}
							name='flags.enableDownloadOptions'
							label='Download and Merge Options'
							description='Configure download and merge behavior'>
							<div className='space-y-4'>
								<ToggleOption
									control={form.control}
									name='flags.noDirectMerge'
									label='No Direct Merge'
									description='Prevent direct merging of video and audio files'
								/>
								<ToggleOption
									control={form.control}
									name='flags.embedThumbnailAtomicparsley'
									label='Embed Thumbnail with AtomicParsley'
									description='Embed thumbnail in the video file using AtomicParsley'
								/>
							</div>
						</SectionToggle>

						<SectionToggle control={form.control} name='flags.enableMetadataOptions' label='Metadata Options' description='Configure metadata handling'>
							<div className='space-y-4'>
								<ToggleOption control={form.control} name='flags.noCleanInfojson' label='No Clean Info.json' description='Prevent cleaning of info.json file' />
								<ToggleOption control={form.control} name='flags.noKeepSubs' label='No Keep Subtitles' description='Prevent keeping of subtitle files' />
							</div>
						</SectionToggle>

						<SectionToggle control={form.control} name='flags.enableSystemOptions' label='System Options' description='Configure system-level behavior'>
							<div className='space-y-4'>
								<ToggleOption control={form.control} name='flags.noCertificate' label='No Certificate' description='Prevent checking of SSL certificates' />
								<ToggleOption
									control={form.control}
									name='flags.filenameSanitization'
									label='Filename Sanitization'
									description='Enable sanitization of filenames'
								/>
								<ToggleOption control={form.control} name='flags.playlistMatchFilter' label='Playlist Match Filter' description='Enable playlist match filtering' />
							</div>
						</SectionToggle>

						<div className='space-y-4 flex flex-col'>
							<h3 className='text-sm font-medium'>Custom Options</h3>
							<FormField
								control={form.control}
								name='flags.custom'
								render={({ field }) => (
									<FormItem>
										<FormLabel>Custom Options</FormLabel>
										<Textarea placeholder='Enter custom yt-dlp options (one per line)' className='resize-none' {...field} />
										<FormDescription>Enter additional yt-dlp options that are not covered by the UI</FormDescription>
									</FormItem>
								)}
							/>
						</div>
					</form>
				</Form>
			</div>
		</>
	);
}

export default function AdvancedYTDLPView() {
	const { settings, setSetting } = useApp();
	const [isOpen, setIsOpen] = useState(!!settings?.features?.advancedView);
	useEffect(() => {
		if (isOpen !== !!settings?.features?.advancedView) setSetting("features.advancedView", isOpen);
	}, [isOpen]);
	return (
		<div className='flex flex-col gap-4'>
			<Collapsible open={isOpen} onOpenChange={setIsOpen} className='w-full'>
				<CollapsibleTrigger className='flex w-full items-center justify-between'>
					<ClickableText>Advanced Options</ClickableText>
					<LucideChevronDown className={cn("h-4 w-4 shrink-0 text-muted-foreground transition-transform duration-200", isOpen ? "transform rotate-180" : "")} />
				</CollapsibleTrigger>
				<CollapsibleContent>
					<AdvancedViewContent />
				</CollapsibleContent>
			</Collapsible>
		</div>
	);
}
