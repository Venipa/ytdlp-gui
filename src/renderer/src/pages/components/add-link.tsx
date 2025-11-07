import { Badge } from "@renderer/components/ui/badge";
import { Button } from "@renderer/components/ui/button";
import { Input } from "@renderer/components/ui/input";
import { Textarea } from "@renderer/components/ui/textarea";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { trpc } from "@renderer/lib/trpc-link";
import { useMediaType } from "@renderer/lib/useMediaType";
import { cn } from "@renderer/lib/utils";
import { logger } from "@shared/logger";
import { isTRPCErrorResponse } from "@shared/trpc/utils";
import { LucideFlame } from "lucide-react";
import { useCallback, useMemo } from "react";
import { toast } from "sonner";
import { useAddLinkStore, useLinkBoxStore, useSearchStore } from "./add-link.store";
import { useApp } from "./app-context";
import SelectDownloadBox from "./select-download-path";
import SelectMediaTypeBox from "./select-download-type";
const httpsRegex = /^https?/i;
export default function AddLink({ showDownloadPath, children }: { showDownloadPath?: boolean; children?: React.ReactNode }) {
	const [mediaType] = useMediaType();
	const { settings, setSetting } = useApp();
	const { mutateAsync: queueDownloadFromUrl, isLoading: isDownloading } = trpc.ytdl.downloadMedia.useMutation({
		onError(error, variables, context) {
			toast.error(error.data!.code, { description: error.message });
		},
	});
	const [search, setSearch] = useSearchStore();
	const [mediaUrl, setMediaUrl] = useLinkBoxStore();
	const linkCount = useMemo(
		() =>
			mediaUrl
				.split("\n")
				.filter((url) => url && httpsRegex.test(url))
				.clampMax(999),
		[mediaUrl],
	);
	const handleSubmit = useCallback(() => {
		if (!mediaUrl) return;
		const queueUrls = [
			...mediaUrl.split("\n").filter((s) => {
				logger.debug({ regexTestSource: s });
				return s && httpsRegex.test(s);
			}),
		];
		logger.debug("download requested for ", { url: queueUrls, mediaUrl, mediaType });
		queueDownloadFromUrl({ url: queueUrls, type: mediaType }).catch((err) => {
			if (isTRPCErrorResponse(err)) toast.error(err.message);
		});
		setMediaUrl("");
	}, [mediaUrl, mediaType]);
	const [{ showAddLink }, setAddConfig] = useAddLinkStore();
	logger.debug("add-link", { mediaUrl });
	return (
		<>
			<div className={cn("flex flex-col gap-4 pt-4", showAddLink && " bg-muted/20 border-b border-b-muted")}>
				<div className='flex justify-start items-center px-4 gap-2'>
					<Input placeholder='Search...' value={search ?? ""} onChange={(ev) => setSearch(ev.target.value)} />
					<div className='w-px h-[80%] bg-muted/60'></div>
					<Button variant={"outline"} onClick={() => setAddConfig((s) => ({ ...s, showAddLink: !s.showAddLink }))} className='w-[100px] relative'>
						{showAddLink ? "Close" : "Add Link"}
						{linkCount > 0 && !showAddLink && <Badge className='absolute -top-2.5 right-0 h-5 px-1.5 flex items-center justify-center'>{linkCount}</Badge>}
					</Button>
				</div>
				{showAddLink && (
					<>
						<div className='flex flex-col gap-6 px-4 py-4'>
							<div className='flex flex-col gap-0.5'>
								<Textarea
									placeholder='https://youtube.com/watch?v=xyz'
									className='placeholder:text-xs text-[0.775rem]'
									value={mediaUrl}
									onChange={(ev) => setMediaUrl(ev.target.value)}
									rows={5}
									role='textbox'
								/>
								<div className='flex items-end justify-end text-xs text-muted-foreground mr-2'>{linkCount} links captured</div>
							</div>
							<div className='flex items-center gap-2'>
								<SelectDownloadBox></SelectDownloadBox>
								<SelectMediaTypeBox className='w-[200px]'></SelectMediaTypeBox>
								<div className='flex-auto'></div>
								<QTooltip content='Enable/Disable clipboard monitoring'>
									<Button variant={"ghost"} onClick={() => setSetting("features.clipboardMonitor", !settings.features.clipboardMonitor)}>
										<LucideFlame
											className={cn(
												settings.features.clipboardMonitor ? "fill-yellow-300 stroke-yellow-600" : "stroke-primary",
												"transition-colors duration-200 ease-out",
											)}
										/>
									</Button>
								</QTooltip>

								<Button disabled={!mediaUrl} onClick={handleSubmit}>
									Add & Start Download
								</Button>
							</div>
						</div>
					</>
				)}
			</div>
		</>
	);
}
