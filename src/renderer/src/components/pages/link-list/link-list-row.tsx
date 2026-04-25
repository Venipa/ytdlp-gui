import ButtonLoading from "@renderer/components/ui/ButtonLoading";
import { Button } from "@renderer/components/ui/button";
import { Checkbox } from "@renderer/components/ui/checkbox";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { trpc } from "@renderer/lib/api/trpc-link";
import { TrimSubdomainRegex } from "@renderer/lib/media/regex";
import { cn } from "@renderer/lib/ui/utils";
import { Row } from "@tanstack/react-table";
import { formatDate } from "date-fns";
import { DotIcon, LucideFolderOpen, LucideGlobe, LucideRedo2, LucideSquare, LucideX } from "lucide-react";
import prettyBytes from "pretty-bytes";
import { useCallback, useMemo, useState } from "react";
import { YTDLDownloadStatus, YTDLItem } from "ytdlp-gui/types";
import FileSheet from "../file-sheet";
import { LINK_LIST_GRID_COLUMNS } from "./constants";
import { useLinkListActions } from "./link-list-context";
import LinkListDeletePopover from "./link-list-delete-popover";
import LinkListStatusIndicator from "./link-list-status-indicator";
import { LinkListTableItem } from "./link-list.types";
function formatSpeed(speed: number): string {
	const mib = speed / (1024 * 1024);
	return `${mib.toFixed(2)} MiB/s`;
}
const padStatusItem = (status: string | number, type: "percent" | "speed" | "eta"): string => {
	const statusString = String(status);
	if (type === "percent") return statusString.padStart(6, " ") + "%";
	if (type === "speed") return statusString.padStart(12, " ");
	if (type === "eta") return statusString.padStart(8, " ") + "ETA";
	return statusString;
};

const getStateLabel = (state: YTDLItem["state"]): string => {
	switch (state) {
		case "fetching_meta":
			return "Fetching";
		case "downloading":
			return "Downloading";
		case "queued":
			return "Queued";
		case "completed":
			return "Completed";
		case "cancelled":
			return "Cancelled";
		default:
			return "Idle";
	}
};
type LinkListRowProps = {
	className?: string;
	row: Row<LinkListTableItem>;
	isSelected: boolean;
	showCheckbox: boolean;
	disableSelection?: boolean;
	onSelectChange: (checked: boolean) => void;
	onHoverChange: (isHovering: boolean) => void;
};
export function LinkListRow({
	className,
	row,
	isSelected,
	showCheckbox,
	disableSelection,
	onSelectChange,
	onHoverChange,
}: LinkListRowProps): JSX.Element {
	const props = row.original;
	const { id, error, state, filesize: rawFilesize, type, source, title, filepath, url, created } = props;
	const [status, setDownloadStatus] = useState<YTDLDownloadStatus>();
	const { cancelById, deleteById, deletingId, openParentPath, retryById } = useLinkListActions();

	trpc.ytdl.onIdDownload.useSubscription(id, {
		onData(data) {
			if (data) setDownloadStatus(data as YTDLDownloadStatus);
		},
	});

	const filesize = useMemo(() => (rawFilesize && rawFilesize > 0 ? prettyBytes(rawFilesize) : null), [rawFilesize]);
	const completed = state === "completed";
	const cancelled = state === "cancelled";
	const downloading = state === "downloading";
	const processingMeta = state === "fetching_meta";
	const queued = state === "queued";
	const isDownloadedItem = completed && !!filepath;
	const stateLabel = getStateLabel(state);
	const deleteLoading = deletingId === id;

	const faviconUrl = useMemo(() => {
		if (!source) return null;
		return `https://icons.duckduckgo.com/ip3/${source.replace(TrimSubdomainRegex, "")}.ico`;
	}, [source]);

	const handleDeleteDbOnly = useCallback(() => deleteById(id, false), [deleteById, id]);
	const handleDeleteWithFile = useCallback(() => deleteById(id, true), [deleteById, id]);

	const downloadStatus = useMemo(() => {
		if (queued) return null;
		if (!downloading || !status) return null;
		return {
			percent: padStatusItem(status.percent?.toFixed(0) ?? "", "percent"),
			speed: padStatusItem(formatSpeed(status.speed ?? 0), "speed"),
			eta: padStatusItem(status.eta ?? "", "eta").split("ETA")[0],
		};
	}, [queued, downloading, status]);
	const createdAt = useMemo(() => {
		if (state !== "completed" || !created) return null;
		const parsedDate = new Date(created);
		if (Number.isNaN(parsedDate.getTime())) return null;
		return {
			date: formatDate(parsedDate, "MM/dd/yyyy"),
			time: formatDate(parsedDate, "HH:mm:ss"),
		};
	}, [state, created]);
	const isSingleLine = completed || cancelled || error || queued;
	return (
		<div
			onMouseEnter={() => {
				onHoverChange(true);
			}}
			onMouseLeave={() => {
				onHoverChange(false);
			}}
			className={cn(
				"h-12 grid min-w-full w-fit",
				LINK_LIST_GRID_COLUMNS,
				"gap-2 items-center px-2 border-b border-border/60 hover:bg-muted/50 relative cursor-default group/item shrink-0 select-none",
				className,
			)}>
			{row.getVisibleCells().map((cell) => {
				if (cell.column.id === "selection") {
					return (
						<div key={cell.id} className='flex items-center justify-center'>
							<Checkbox
								checked={isSelected}
								disabled={disableSelection}
								aria-label={isSelected ? "Deselect item" : "Select item"}
								className={cn(
									"transition-opacity",
									showCheckbox ? "opacity-100" : "opacity-0 pointer-events-none",
								)}
								onCheckedChange={(checked) => {
									onSelectChange(checked === true);
								}}
								onClick={(event) => {
									event.stopPropagation();
								}}
								variant={"ghost"}
								size='sm'
							/>
						</div>
					);
				}

				if (cell.column.id === "status") {
					return (
						<div key={cell.id} className='flex items-center justify-center'>
							<QTooltip content={stateLabel} side='right' asChild>
								<div className='flex items-center justify-center'>
									<LinkListStatusIndicator
										cancelled={cancelled}
										completed={completed}
										downloading={downloading}
										error={error}
										queued={queued}
										status={status}
									/>
								</div>
							</QTooltip>
						</div>
					);
				}

				if (cell.column.id === "title") {
					return (
						<FileSheet key={cell.id} item={props as any}>
							<div
								className={cn(
									"h-full py-1 items-center cursor-pointer min-w-0",
									isSingleLine ? "flex" : "grid grid-rows-[24px_12px]",
								)}>
								<div className='text-sm truncate leading-none'>{title || "-"}</div>
								<div className='flex gap-1 items-center text-xs text-muted-foreground leading-none min-w-0'>
									{downloadStatus && (
										<>
											<span className='w-[40px] text-right tabular-nums'>
												{downloadStatus.percent}
											</span>
											<DotIcon className='size-2' />
											{downloadStatus.speed && (
												<>
													<span className='w-[68px] text-right tabular-nums'>
														{downloadStatus.speed}
													</span>
													<DotIcon className='size-2' />
												</>
											)}
											{downloadStatus.eta && (
												<>
													<span className='w-[40px] text-right tabular-nums'>
														{downloadStatus.eta}
													</span>
													<DotIcon className='size-2' />
												</>
											)}
										</>
									)}
								</div>
							</div>
						</FileSheet>
					);
				}

				if (cell.column.id === "source") {
					return (
						<div key={cell.id} className='text-xs text-muted-foreground flex items-center gap-1 min-w-0'>
							{source ? (
								<>
									{!faviconUrl ? (
										<LucideGlobe className='size-3 shrink-0' />
									) : (
										<img src={faviconUrl} className='size-3 shrink-0' />
									)}
									<span className='truncate'>{source}</span>
								</>
							) : (
								<span>-</span>
							)}
						</div>
					);
				}

				if (cell.column.id === "state") {
					return (
						<div key={cell.id} className='text-xs font-medium text-muted-foreground tabular-nums'>
							{stateLabel}
						</div>
					);
				}

				if (cell.column.id === "filesize") {
					return (
						<div key={cell.id} className='text-xs text-muted-foreground tabular-nums'>
							{filesize || "-"}
						</div>
					);
				}

				if (cell.column.id === "type") {
					return (
						<div key={cell.id} className='text-xs text-muted-foreground truncate'>
							{type || "-"}
						</div>
					);
				}

				if (cell.column.id === "created") {
					return (
						<div key={cell.id} className='text-xs text-muted-foreground flex flex-col gap-0.5'>
							{createdAt ? (
								<>
									<span className='text-xs'>{createdAt.date}</span>
									<span className='text-xs text-muted-foreground/80'>{createdAt.time}</span>
								</>
							) : (
								"-"
							)}
						</div>
					);
				}
				if (cell.column.id === "actions")
					return (
						<div
							key={cell.id}
							className='flex justify-end items-center gap-1 px-2 opacity-25 group-hover/item:opacity-100 transition-opacity'>
							{(error || cancelled) && (
								<Button
									variant={"ghost"}
									size={"sm"}
									className='px-2'
									onClick={(event) => {
										event.stopPropagation();
										event.preventDefault();
										void retryById(id);
									}}>
									<LucideRedo2 className='stroke-[3px]' />
								</Button>
							)}
							{(downloading || processingMeta) && (
								<Button
									variant={"ghost"}
									size={"sm"}
									className='px-2'
									onClick={(event) => {
										event.stopPropagation();
										event.preventDefault();
										void cancelById(id);
									}}>
									<LucideSquare className='fill-current stroke-none' />
								</Button>
							)}
							{completed && filepath && (
								<Button
									variant={"ghost"}
									size={"sm"}
									className='px-2'
									onClick={(event) => {
										event.stopPropagation();
										event.preventDefault();
										void openParentPath(filepath);
									}}>
									<LucideFolderOpen className='fill-current stroke-none' />
								</Button>
							)}
							{isDownloadedItem && (
								<LinkListDeletePopover
									loading={deleteLoading}
									onDeleteEntry={() => {
										void handleDeleteDbOnly();
									}}
									onDeleteEntryAndFile={() => {
										void handleDeleteWithFile();
									}}
								/>
							)}
							{(error || cancelled || queued) && (
								<ButtonLoading
									variant={"ghost"}
									size={"sm"}
									className='px-2 text-red-500 hover:text-red-400 opacity-0 group-hover/item:opacity-100'
									onClick={(event) => {
										event.stopPropagation();
										event.preventDefault();
										void handleDeleteDbOnly();
									}}
									loading={deleteLoading}
									fixWidth>
									<LucideX className='stroke-current' />
								</ButtonLoading>
							)}
						</div>
					);
				return null;
			})}
		</div>
	);
}

export default LinkListRow;
