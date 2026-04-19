import SuspenseLoader from "@renderer/components/ui/suspense-loader";
import { trpc } from "@renderer/lib/api/trpc-link";
import { SearchEngine, SearchItem } from "@renderer/lib/media/searchEngine";
import { cn } from "@renderer/lib/ui/utils";
import { createLogger } from "@shared/logger";
import { createColumnHelper, getCoreRowModel, useReactTable } from "@tanstack/react-table";
import { useVirtualizer } from "@tanstack/react-virtual";
import { useAtom } from "jotai";
import { useMemo, useRef, useState } from "react";
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

	const columns = useMemo(
		() => [
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
		[],
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
									/>
								</div>
							);
						})}
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
