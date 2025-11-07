import ButtonLoading from "@renderer/components/ui/ButtonLoading";
import { Button } from "@renderer/components/ui/button";
import ClickableText from "@renderer/components/ui/clickable-text";
import { ProgressCircle } from "@renderer/components/ui/progress-circle";
import { Spinner } from "@renderer/components/ui/spinner";
import SuspenseLoader from "@renderer/components/ui/suspense-loader";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { TrimSubdomainRegex } from "@renderer/lib/regex";
import { SearchEngine, SearchItem } from "@renderer/lib/searchEngine";
import { trpc } from "@renderer/lib/trpc-link";
import { cn } from "@renderer/lib/utils";
import { createLogger } from "@shared/logger";
import {
	DotIcon,
	LucideArrowDownToDot,
	LucideCheck,
	LucideFileX,
	LucideFolderOpen,
	LucideGlobe,
	LucideListPlus,
	LucideRedo2,
	LucideRefreshCcw,
	LucideSquare,
	LucideX,
} from "lucide-react";
import prettyBytes from "pretty-bytes";
import { useMemo, useState } from "react";
import { VList } from "virtua";
import { YTDLDownloadStatus, YTDLItem } from "ytdlp-gui/types";
import { useSearchStore } from "./add-link.store";
import FileSheet from "./file-sheet";
const log = createLogger("LinkListItem");
export function LinkListItem(props: YTDLItem & { key: any }) {
	const { id, error: ytderror, state, filesize: fsize, type, source, title, filepath, url } = props;
	const error = useMemo(() => ytderror, [ytderror]);
	const [status, setDownloadStatus] = useState<YTDLDownloadStatus>();
	const { mutateAsync: openPath } = trpc.internals.openPath.useMutation();
	const { mutateAsync: retryFromId } = trpc.ytdl.retry.useMutation();
	const { mutateAsync: cancelFromId } = trpc.ytdl.cancel.useMutation();
	const { mutateAsync: deleteFromId, isLoading: deleteLoading } = trpc.ytdl.delete.useMutation();
	const filesize = useMemo(() => prettyBytes(fsize), [fsize]);
	trpc.ytdl.onIdDownload.useSubscription(id, {
		onData(data) {
			if (data) setDownloadStatus(data as any);
		},
	});
	const completed = useMemo(() => state === "completed", [state, status]);
	const cancelled = useMemo(() => state === "cancelled", [state, status]);
	const downloading = useMemo(() => state === "downloading", [state, status]);
	const processingMeta = useMemo(() => state === "fetching_meta", [state, status]);
	const queued = useMemo(() => state === "queued", [state, status]);
	const faviconUrl = useMemo(() => source && `https://icons.duckduckgo.com/ip3/${source.replace(TrimSubdomainRegex, "")}.ico`, [source]);
	return (
		<div className='h-10 rounded-md hover:bg-muted/60 grid grid-cols-[40px_1fr_minmax(100px,_auto)] gap-2 items-center relative cursor-default group/item flex-shrink-0 select-none'>
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
				<div className='grid grid-rows-[20px_12px] items-center cursor-pointer'>
					<div className='text-sm truncate leading-none'>{title}</div>
					<div className='flex gap-1 items-center text-xs text-muted-foreground leading-none'>
						{!error && filesize && (
							<>
								<span className='w-[60px]'>{filesize}</span>
								<DotIcon className='size-2' />
							</>
						)}
						{(type && <span className='w-[60px] text-center'>{type}</span>) || <span className='w-[60px] text-center'>unknown</span>}
						<DotIcon className='size-2' />
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
						onClick={() => deleteFromId(id)}
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
						onClick={() => deleteFromId(id)}
						loading={deleteLoading}
						fixWidth>
						<LucideX className='stroke-current' />
					</ButtonLoading>
				)}
			</div>
		</div>
	);
}
export default function LinkList(props: { className?: string }) {
	const { data: items, isFetching } = trpc.ytdl.list.useQuery(undefined);
	const {
		ytdl: { list },
	} = trpc.useUtils();
	const [search, setSearch] = useSearchStore();
	const [searchEngine] = useState(() => new SearchEngine());
	const filteredItems = useMemo(() => {
		const activeItems = items?.filter((d) => d.state === "downloading" || d.state === "fetching_meta" || d.state === "queued") ?? [];
		const results = searchEngine.filterResults((items ?? []) as unknown as SearchItem<YTDLItem>[], search ?? "", ["title", "source", "url"]);
		if (search) {
			return activeItems?.reduce((acc, item) => {
				if (!results.find((d) => d.id === item.id)) acc.push(item as YTDLItem);
				return acc;
			}, [] as YTDLItem[]);
		}
		return results;
	}, [items, search, list]);
	trpc.ytdl.listSync.useSubscription(undefined, {
		onData(data: YTDLItem[]) {
			if (data?.length) {
				log.debug("listSync", { data });
				list.setData(undefined, (state) => {
					if (!state) state = [];
					data.forEach((item) => {
						const idx = state.findIndex((d) => d.id === item.id);
						if (item.state === "deleted" && idx !== -1) {
							state.splice(idx, 1);
							log.debug("operation", "removed item from list", { item });
						} else if (idx !== -1) {
							state.splice(idx, 1, item as any);
							log.debug("operation", "updated item in list", { item });
						} else {
							state.insert(idx, item as any);
							log.debug("operation", "added item to list", { item });
						}
					});
					log.debug("listSync complete", { state });
					return [...state];
				});
				list.invalidate(undefined);
			}
		},
	});
	return (
		<div className={cn("flex flex-col gap-2 relative", props.className)}>
			<VList className='flex-grow relative flex flex-col py-2.5 px-2' style={{ height: "100%" }}>
				{isFetching && <SuspenseLoader className='absolute bg-background inset-0' />}
				{filteredItems?.map((d) => (
					<div className='h-12' key={d.id}>
						<LinkListItem {...(d as any)} />
					</div>
				))}
			</VList>
			{!filteredItems.length && (
				<div className='absolute bg-background inset-0 flex items-center justify-center text-muted-foreground gap-4 flex-col'>
					<span>No results found. Try a different search.</span>
					<pre className='flex flex-col bg-muted/20 border border-muted/40 p-2 rounded-md'>
						{["type:video|audio", "size:<100mb, <=10gb, etc", "status:success|error|active"].map((line) => (
							<span key={line} className='text-xs text-muted-foreground'>
								{line}
							</span>
						))}
					</pre>
					<Button
						variant={"ghost"}
						size={"sm"}
						className='px-2'
						onClick={() => {
							setSearch("");
						}}>
						<LucideRefreshCcw />
						<span>Clear search</span>
					</Button>
				</div>
			)}
		</div>
	);
}
