import { cn } from "@renderer/lib/ui/utils";
import { HeaderGroup, flexRender } from "@tanstack/react-table";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import { LINK_LIST_GRID_COLUMNS } from "./constants";
import { LinkListTableItem } from "./link-list.types";

interface LinkListHeaderProps {
	headerGroups: HeaderGroup<LinkListTableItem>[];
}

export default function LinkListHeader({ headerGroups }: LinkListHeaderProps): JSX.Element {
	return (
		<div
			className={cn(
				"grid",
				LINK_LIST_GRID_COLUMNS,
				"gap-2 px-2 h-10 shrink-0 items-center border-b border-border/80 bg-background/95 text-xs text-muted-foreground font-medium sticky top-0 z-10",
			)}>
			{headerGroups.map((headerGroup) =>
				headerGroup.headers.map((header) => (
					<div
						key={header.id}
						className={cn("truncate flex items-center gap-1", header.column.getCanSort() && "cursor-pointer select-none")}
						onClick={header.column.getToggleSortingHandler()}>
						{header.isPlaceholder ? null : flexRender(header.column.columnDef.header, header.getContext())}
						{header.column.getCanSort() &&
							(header.column.getIsSorted() === "asc" ? (
								<ArrowUp className='size-3.5 shrink-0' />
							) : header.column.getIsSorted() === "desc" ? (
								<ArrowDown className='size-3.5 shrink-0' />
							) : (
								<ArrowUpDown className='size-3.5 shrink-0 opacity-50' />
							))}
					</div>
				)),
			)}
		</div>
	);
}
