import ButtonLoading from "@renderer/components/ui/ButtonLoading";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@renderer/components/ui/alert-dialog";
import { Button } from "@renderer/components/ui/button";
import ClickableText from "@renderer/components/ui/clickable-text";
import { ProgressCircle } from "@renderer/components/ui/progress-circle";
import { Spinner } from "@renderer/components/ui/spinner";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { TrimSubdomainRegex } from "@renderer/lib/regex";
import { trpc } from "@renderer/lib/trpc-link";
import { DotIcon, LucideArrowDownToDot, LucideCheck, LucideFileX, LucideFolderOpen, LucideGlobe, LucideListPlus, LucideRedo2, LucideSquare, LucideX } from "lucide-react";
import prettyBytes from "pretty-bytes";
import { useMemo, useState } from "react";
import { YTDLDownloadStatus, YTDLItem } from "ytdlp-gui/types";
import FileSheet from "./file-sheet";

const padStatusItem = (status: string | number, type: "percent" | "speed" | "eta") => {
	const statusString = String(status);
	if (type === "percent") return statusString.padStart(6, " ") + "%";
	if (type === "speed") return statusString.padStart(12, " ");
	if (type === "eta") return statusString.padStart(8, " ") + "ETA";
	return statusString;
};

export function LinkListItem(props: YTDLItem & { key: any }) {
	const { id, error: ytderror, state, filesize: fsize, type, source, title, filepath, url } = props;
	const error = useMemo(() => ytderror, [ytderror]);
	const [showDeleteDialog, setShowDeleteDialog] = useState<boolean>(false);
	const [status, setDownloadStatus] = useState<YTDLDownloadStatus>();
	const { mutateAsync: openPath } = trpc.internals.openPath.useMutation();
	const { mutateAsync: retryFromId } = trpc.ytdl.retry.useMutation();
	const { mutateAsync: cancelFromId } = trpc.ytdl.cancel.useMutation();
	const { mutateAsync: deleteFromId, isLoading: deleteLoading } = trpc.ytdl.delete.useMutation();
	trpc.ytdl.onIdDownload.useSubscription(id, {
		onData(data) {
			if (data) setDownloadStatus(data as any);
		},
	});
	const filesize = useMemo(() => (fsize && fsize > 0 ? prettyBytes(fsize) : null), [fsize, setDownloadStatus]);
	const completed = useMemo(() => state === "completed", [state, status]);
	const cancelled = useMemo(() => state === "cancelled", [state, status]);
	const downloading = useMemo(() => state === "downloading", [state, status]);
	const processingMeta = useMemo(() => state === "fetching_meta", [state, status]);
	const queued = useMemo(() => state === "queued", [state, status]);
	const isDownloadedItem = useMemo(() => completed && !!filepath, [completed, filepath]);
	const faviconUrl = useMemo(() => source && `https://icons.duckduckgo.com/ip3/${source.replace(TrimSubdomainRegex, "")}.ico`, [source]);
	const handleDeleteDbOnly = () => deleteFromId({ id, deleteFile: false });
	const handleDeleteWithFile = () => deleteFromId({ id, deleteFile: true });
	const handleDeleteClick = () => {
		if (isDownloadedItem) {
			setShowDeleteDialog(true);
			return;
		}
		handleDeleteDbOnly();
	};
	const downloadStatus = useMemo(() => {
		if (queued) return null;
		if (downloading && status)
			return {
				percent: padStatusItem(status.percent?.toFixed(0) ?? "", "percent"),
				speed: padStatusItem(status.speed ?? "", "speed"),
				eta: padStatusItem(status.eta ?? "", "eta").split("ETA")[0],
			};
		return null;
	}, [queued, downloading, status]);
	return (
		<div className='h-16 hover:bg-muted/60 grid grid-cols-[40px_1fr_minmax(100px,auto)] gap-2 items-center relative cursor-default group/item shrink-0 select-none'>
			<div className='flex flex-col size-10 items-center justify-center'>
				{error ? (
					<QTooltip side='right' content={"An error occurred while downloading."}>
						<div className='size-5 p-1 flex flex-col items-center justify-center border-2 border-destructive/40 bg-destructive rounded-full'>
							<LucideX className='stroke-[4px] stroke-destructive-foreground' />
						</div>
					</QTooltip>
				) : cancelled ? (
					<div className='size-5 p-1 flex flex-col items-center justify-center border-2 border-muted rounded-full'>
						<LucideSquare className='stroke-none fill-current' />
					</div>
				) : completed ? (
					<div className='size-5 p-1 flex flex-col items-center justify-center bg-green-500 text-white rounded-full'>
						<LucideCheck className='stroke-[4px]' />
					</div>
				) : queued ? (
					<QTooltip className='cursor-default' content={"Queued"} side='right'>
						<div className='flex flex-col items-center justify-center size-10 relative'>
							<LucideListPlus className='size-5 text-secondary-foreground' />
						</div>
					</QTooltip>
				) : downloading && status ? (
					<QTooltip className='cursor-default' content={"Download Progress"} side='right'>
						<div className='flex flex-col items-center justify-center size-10 relative'>
							<ProgressCircle
								min={0}
								max={100}
								value={queued ? 0 : (status?.percent ?? 0)}
								className='h-6'
								gaugePrimaryColor='rgb(225 225 225)'
								gaugeSecondaryColor='rgba(120, 120, 120, 0.1)'
								showValue={false}
							/>
							<LucideArrowDownToDot className='absolute size-3.5 text-secondary-foreground animate-pulse' />
						</div>
					</QTooltip>
				) : (
					<Spinner />
				)}
			</div>
			<FileSheet item={props as any}>
				<div className='h-full pt-2 pb-1 grid grid-rows-[20px_12px_auto] items-center cursor-pointer'>
					<div className='text-sm truncate leading-none'>{title}</div>
					<div className='flex gap-1 items-center text-xs text-muted-foreground leading-none'>
						{downloadStatus && (
							<>
								<span className='w-[40px] text-right tabular-nums'>{downloadStatus.percent}</span>
								<DotIcon className='size-2' />
								{downloadStatus.speed && (
									<>
										<span className='w-[68px] text-right tabular-nums'>{downloadStatus.speed}</span>
										<DotIcon className='size-2' />
									</>
								)}
								{downloadStatus.eta && (
									<>
										<span className='w-[40px] text-right tabular-nums'>{downloadStatus.eta}</span>
										<DotIcon className='size-2' />
									</>
								)}
							</>
						)}
						{!error && filesize && (
							<>
								<span className='w-[60px] tabular-nums'>{filesize}</span>
								<DotIcon className='size-2' />
							</>
						)}
						{type && <span className='w-[60px] text-center'>{type}</span>}
					</div>
					<div className='flex items-center gap-1'>
						{!url ? (
							<div className='flex cursor-pointer items-center gap-1'>
								{!faviconUrl ? <LucideGlobe className='size-2.5' /> : <img src={faviconUrl} className='size-2.5' />}
								<span>{source}</span>
							</div>
						) : (
							<ClickableText asChild>
								<a className='cursor-pointer flex items-center gap-1' href={url} target='_blank'>
									{!faviconUrl ? <LucideGlobe className='size-2.5' /> : <img src={faviconUrl} className='size-2.5' />}
									<span>{source}</span>
								</a>
							</ClickableText>
						)}
					</div>
				</div>
			</FileSheet>
			<div className='flex justify-end items-center gap-1 px-2 opacity-20 group-hover/item:opacity-100'>
				{(error || cancelled) && (
					<Button
						variant={"ghost"}
						size={"sm"}
						className='px-2'
						onClick={(ev) => {
							ev.stopPropagation();
							ev.preventDefault();
							retryFromId(id);
						}}>
						<LucideRedo2 className='stroke-[3px]' />
					</Button>
				)}
				{(downloading || processingMeta) && (
					<Button
						variant={"ghost"}
						size={"sm"}
						className='px-2'
						onClick={(ev) => {
							ev.stopPropagation();
							ev.preventDefault();
							cancelFromId(id);
						}}>
						<LucideSquare className='fill-current stroke-none' />
					</Button>
				)}
				{completed && filepath && (
					<Button
						variant={"ghost"}
						size={"sm"}
						className='px-2'
						onClick={(ev) => {
							ev.stopPropagation();
							ev.preventDefault();
							openPath({ path: filepath, openParent: true });
						}}>
						<LucideFolderOpen className='fill-current stroke-none' />
					</Button>
				)}
				{completed && (
					<ButtonLoading
						variant={"ghost"}
						size={"sm"}
						className='px-2 text-red-500 hover:text-red-400 opacity-0 group-hover/item:opacity-100'
						onClick={handleDeleteClick}
						loading={deleteLoading}
						fixWidth>
						<LucideFileX className='stroke-current' />
					</ButtonLoading>
				)}
				{(error || cancelled || queued) && (
					<ButtonLoading
						variant={"ghost"}
						size={"sm"}
						className='px-2 text-red-500 hover:text-red-400 opacity-0 group-hover/item:opacity-100'
						onClick={handleDeleteClick}
						loading={deleteLoading}
						fixWidth>
						<LucideX className='stroke-current' />
					</ButtonLoading>
				)}
			</div>
			<AlertDialog open={showDeleteDialog} onOpenChange={setShowDeleteDialog}>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>Delete downloaded file too?</AlertDialogTitle>
						<AlertDialogDescription>
							Choose whether to also remove the file from disk. If you skip this, only the download entry is removed from the list.
						</AlertDialogDescription>
					</AlertDialogHeader>
					<AlertDialogFooter>
						<AlertDialogCancel
							className='h-10'
							onClick={(ev) => {
								ev.preventDefault();
								setShowDeleteDialog(false);
							}}>
							Cancel
						</AlertDialogCancel>
						<AlertDialogAction
							className='h-10'
							onClick={(ev) => {
								ev.preventDefault();
								handleDeleteWithFile().finally(() => setShowDeleteDialog(false));
							}}>
							Delete File & Entry
						</AlertDialogAction>
						<AlertDialogAction
							className='h-10'
							onClick={(ev) => {
								ev.preventDefault();
								handleDeleteDbOnly().finally(() => setShowDeleteDialog(false));
							}}>
							Keep File
						</AlertDialogAction>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>
		</div>
	);
}

export default LinkListItem;
