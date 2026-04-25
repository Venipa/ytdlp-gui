import { Button } from "@renderer/components/ui/button";
import { Checkbox } from "@renderer/components/ui/checkbox";
import { Spinner } from "@renderer/components/ui/spinner";
import SuspenseLoader from "@renderer/components/ui/suspense-loader";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { trpc } from "@renderer/lib/api/trpc-link";
import { SearchEngine, SearchItem } from "@renderer/lib/media/searchEngine";
import { cn } from "@renderer/lib/ui/utils";
import { createLogger } from "@shared/logger";
import { createColumnHelper, getCoreRowModel, useReactTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useAtom } from "jotai";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSearchStore } from "../add-link.store";
import LinkListProvider from "./link-list-context";
import LinkListEmptyState from "./link-list-empty-state";
import LinkListHeader from "./link-list-header";
import LinkListRow from "./link-list-row";
import { linkListSortingStore } from "./link-list.store";
import { LinkListTableItem } from "./link-list.types";

const log = createLogger("LinkList");
const columnHelper = createColumnHelper<LinkListTableItem>();

interface LinkListProps {
	className?: string;
}

export default function LinkList({ className }: LinkListProps): JSX.Element {
	const [sorting, setSorting] = useAtom(linkListSortingStore);
	const sortInput = useMemo(
		() => ({
			sortBy:
				(sorting[0]?.id as "created" | "title" | "source" | "state" | "filesize" | "type" | undefined) ??
				"created",
			sortDir: sorting[0]?.desc ? ("desc" as const) : ("asc" as const),
		}),
		[sorting],
	);
	const { data: items, isFetching } = trpc.ytdl.list.useQuery(sortInput);
	const {
		ytdl: { list },
	} = trpc.useUtils();
	const [search, setSearch] = useSearchStore();
	const [searchEngine] = useState(() => new SearchEngine());
	const [selectedIds, setSelectedIds] = useState<Set<string>>(() => new Set());
	const [lastInteractedSelectionId, setLastInteractedSelectionId] = useState<string | null>(null);
	const [hoveredRowId, setHoveredRowId] = useState<string | null>(null);
	const [bulkDeleteMode, setBulkDeleteMode] = useState<"db" | "file" | null>(null);
	const { mutateAsync: deleteFromId } = trpc.ytdl.delete.useMutation();

	const filteredItems = useMemo(() => {
		const safeItems = (items ?? []) as LinkListTableItem[];
		const activeItems = safeItems.filter(
			(item) => item.state === "downloading" || item.state === "fetching_meta" || item.state === "queued",
		);
		const results = searchEngine.filterResults(
			safeItems as unknown as SearchItem<LinkListTableItem>[],
			search ?? "",
			["title", "source", "url"],
		);
		if (!search?.trim()) return results;

		const merged = [...activeItems, ...results] as LinkListTableItem[];
		const byId = new Map<string, LinkListTableItem>();
		for (const item of merged) byId.set(String(item.id), item);
		return [...byId.values()];
	}, [items, search, searchEngine]);
	const rowIdsForSelection = useMemo(() => filteredItems.map((item) => String(item.id)), [filteredItems]);
	const selectedVisibleCount = useMemo(() => {
		let count = 0;
		for (const id of rowIdsForSelection) {
			if (selectedIds.has(id)) count += 1;
		}
		return count;
	}, [rowIdsForSelection, selectedIds]);
	const allVisibleSelected = rowIdsForSelection.length > 0 && selectedVisibleCount === rowIdsForSelection.length;
	const selectAllState: boolean | "indeterminate" = allVisibleSelected
		? true
		: selectedVisibleCount > 0
			? "indeterminate"
			: false;

	const handleSelectAllVisible = useCallback(
		(checked: boolean): void => {
			setSelectedIds((previousIds) => {
				if (checked) {
					const nextIds = new Set(previousIds);
					for (const id of rowIdsForSelection) nextIds.add(id);
					return nextIds;
				}
				const nextIds = new Set(previousIds);
				for (const id of rowIdsForSelection) nextIds.delete(id);
				return nextIds;
			});
		},
		[rowIdsForSelection],
	);

	const columns = useMemo(
		() => [
			columnHelper.display({
				id: "selection",
				header: () => (
					<div className='flex items-center justify-center w-full'>
						<QTooltip content='Select all visible items'>
							<Checkbox
								checked={selectAllState}
								disabled={bulkDeleteMode !== null || rowIdsForSelection.length === 0}
								aria-label='Select all visible items'
								variant='ghost'
								size='sm'
								onCheckedChange={(checked) => {
									handleSelectAllVisible(checked === true);
								}}
								onClick={(event) => {
									event.stopPropagation();
								}}
							/>
						</QTooltip>
					</div>
				),
				cell: () => null,
				enableSorting: false,
			}),
			columnHelper.display({
				id: "status",
				header: "Status",
				cell: () => null,
				enableSorting: false,
			}),
			columnHelper.accessor("title", {
				header: "Title",
				cell: () => null,
				enableSorting: true,
			}),
			columnHelper.display({
				id: "source",
				header: "Source",
				cell: () => null,
				enableSorting: true,
			}),
			columnHelper.accessor("filesize", {
				header: "Size",
				cell: () => null,
				enableSorting: true,
			}),
			columnHelper.accessor("type", {
				header: "Type",
				cell: () => null,
				enableSorting: true,
			}),
			columnHelper.accessor("created", {
				id: "created",
				header: "Created",
				cell: () => null,
				enableSorting: true,
			}),
			columnHelper.display({
				id: "actions",
				header: "Actions",
				cell: () => null,
				enableSorting: false,
			}),
		],
		[bulkDeleteMode, handleSelectAllVisible, rowIdsForSelection.length, selectAllState],
	);

	const table = useReactTable({
		columns,
		data: filteredItems as LinkListTableItem[],
		getRowId: (item) => String(item.id),
		getCoreRowModel: getCoreRowModel(),
		manualSorting: true,
		state: { sorting },
		onSortingChange: setSorting,
	});

	trpc.ytdl.listSync.useSubscription(undefined, {
		onData(data: LinkListTableItem[]) {
			if (data?.length) {
				log.debug("listSync", { data });
				list.setData(sortInput, (state) => {
					const nextState = [...(state ?? [])];
					data.forEach((item) => {
						const itemId = String(item.id);
						const index = nextState.findIndex((entry) => String(entry.id) === itemId);
						if (item.state === "deleted" && index !== -1) {
							nextState.splice(index, 1);
							log.debug("operation", "removed item from list", { item });
						} else if (index !== -1) {
							nextState.splice(index, 1, { ...nextState[index], ...item } as any);
							log.debug("operation", "updated item in list", { item });
						} else {
							nextState.push(item as any);
							log.debug("operation", "added item to list", { item });
						}
					});
					log.debug("listSync complete", { state: nextState });
					return nextState;
				});
				list.invalidate(sortInput);
			}
		},
	});

	const rows = table.getRowModel().rows;
	const scrollRef = useRef<HTMLDivElement | null>(null);
	const rowVirtualizer = useVirtualizer({
		count: rows.length,
		getScrollElement: () => scrollRef.current,
		estimateSize: () => 48,
		overscan: 10,
		getItemKey: (index) => String(rows[index]?.original.id ?? index),
	});
	const virtualRows = rowVirtualizer.getVirtualItems();
	const totalSize = rowVirtualizer.getTotalSize();
	const hasSearch = Boolean(search?.length);
	const noResults = rows.length === 0;
	const selectedCount = selectedIds.size;
	const hasSelection = selectedCount > 0;

	useEffect(() => {
		if (!selectedIds.size) return;
		const currentRowIds = new Set(rows.map((row) => String(row.original.id)));
		setSelectedIds((previousIds) => {
			const nextIds = new Set<string>();
			for (const id of previousIds) {
				if (currentRowIds.has(id)) nextIds.add(id);
			}
			return nextIds.size === previousIds.size ? previousIds : nextIds;
		});
	}, [rows, selectedIds.size]);

	useEffect(() => {
		if (!lastInteractedSelectionId) return;
		if (!rowIdsForSelection.includes(lastInteractedSelectionId)) {
			setLastInteractedSelectionId(null);
		}
	}, [lastInteractedSelectionId, rowIdsForSelection]);

	const handleSelectRow = useCallback(
		(id: string, checked: boolean, shiftKey: boolean): void => {
			setSelectedIds((previousIds) => {
				const nextIds = new Set(previousIds);
				if (shiftKey && lastInteractedSelectionId) {
					const fromIndex = rowIdsForSelection.indexOf(lastInteractedSelectionId);
					const toIndex = rowIdsForSelection.indexOf(id);
					if (fromIndex !== -1 && toIndex !== -1) {
						const rangeStart = Math.min(fromIndex, toIndex);
						const rangeEnd = Math.max(fromIndex, toIndex);
						for (let index = rangeStart; index <= rangeEnd; index += 1) {
							const targetId = rowIdsForSelection[index];
							if (checked) nextIds.add(targetId);
							else nextIds.delete(targetId);
						}
						return nextIds;
					}
				}
				if (checked) nextIds.add(id);
				else nextIds.delete(id);
				return nextIds;
			});
			setLastInteractedSelectionId(id);
		},
		[lastInteractedSelectionId, rowIdsForSelection],
	);

	const clearSelection = useCallback((): void => {
		setSelectedIds(new Set());
		setLastInteractedSelectionId(null);
		setHoveredRowId(null);
	}, []);

	const handleBulkDelete = useCallback(
		async (deleteFile: boolean): Promise<void> => {
			const idsToDelete = [...selectedIds];
			if (!idsToDelete.length) return;
			setBulkDeleteMode(deleteFile ? "file" : "db");
			try {
				await Promise.all(
					idsToDelete.map(async (id) => {
						await deleteFromId({ id: Number(id), deleteFile });
					}),
				);
				clearSelection();
			} finally {
				setBulkDeleteMode(null);
			}
		},
		[clearSelection, deleteFromId, selectedIds],
	);

	return (
		<LinkListProvider>
			<div className={cn("flex flex-col relative h-full", className)}>
				<LinkListHeader headerGroups={table.getHeaderGroups()} />
				<div ref={scrollRef} className='grow relative overflow-auto shrink'>
					{isFetching && <SuspenseLoader className='absolute bg-background inset-0' />}
					<div className='relative w-full'>
						{virtualRows.map((virtualRow) => {
							const row = rows[virtualRow.index];
							if (!row) return null;

							return (
								<div
									key={virtualRow.key}
									data-index={virtualRow.index}
									ref={rowVirtualizer.measureElement}
									className='absolute left-0 top-0 w-full'
									style={{ transform: `translateY(${virtualRow.start}px)` }}>
									<LinkListRow
										className={cn(virtualRow.index === rows.length - 1 && "border-b-0")}
										row={row}
										isSelected={selectedIds.has(String(row.original.id))}
										showCheckbox={hasSelection || hoveredRowId === String(row.original.id)}
										disableSelection={bulkDeleteMode !== null}
										onSelectChange={(checked, shiftKey) => {
											handleSelectRow(String(row.original.id), checked, shiftKey);
										}}
										onHoverChange={(isHovering) => {
											setHoveredRowId(isHovering ? String(row.original.id) : null);
										}}
									/>
								</div>
							);
						})}
					</div>
				</div>
				<div
					aria-hidden={!hasSelection}
					className={cn(
						"absolute bottom-4 left-1/2 -translate-x-1/2 z-20 transition-all duration-200 ease-out",
						hasSelection ? "opacity-100 translate-y-0" : "opacity-0 translate-y-2 pointer-events-none",
					)}>
					<div className='flex items-center gap-2 rounded-lg border border-border bg-background/95 shadow-lg px-2 py-2 backdrop-blur'>
						<span className='text-xs text-muted-foreground px-2 tabular-nums min-w-24 text-right'>
							{selectedCount} selected
						</span>
						<Button
							size='sm'
							variant='outline'
							disabled={!hasSelection || bulkDeleteMode !== null}
							onClick={clearSelection}>
							Cancel
						</Button>
						<Button
							size='sm'
							variant='secondary'
							disabled={!hasSelection || bulkDeleteMode !== null}
							onClick={() => {
								void handleBulkDelete(false);
							}}>
							{bulkDeleteMode === "db" ? <Spinner size={"sm"} /> : "Delete Entries"}
						</Button>
						<Button
							size='sm'
							variant='destructive'
							disabled={!hasSelection || bulkDeleteMode !== null}
							onClick={() => {
								void handleBulkDelete(true);
							}}>
							{bulkDeleteMode === "file" ? <Spinner size={"sm"} /> : "Delete Entries + Files"}
						</Button>
					</div>
				</div>
				{noResults && (
					<LinkListEmptyState
						hasSearch={hasSearch}
						onClearSearch={() => {
							setSearch("");
						}}
					/>
				)}
			</div>
		</LinkListProvider>
	);
}
