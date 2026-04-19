import { cn } from "@renderer/lib/ui/utils";
import { HeaderGroup, flexRender } from "@tanstack/react-table";
import { cva } from "class-variance-authority";
import { ArrowDown, ArrowUp, ArrowUpDown } from "lucide-react";
import { LINK_LIST_GRID_COLUMNS } from "./constants";
import { LinkListTableItem } from "./link-list.types";

interface LinkListHeaderProps {
	headerGroups: HeaderGroup<LinkListTableItem>[];
}

const headerVariants = cva("truncate flex items-center gap-1 px-0", {
	variants: {
		canSort: {
			true: "cursor-pointer select-none relative hover:text-primary/90",
		},
	},
});

export default function LinkListHeader({ headerGroups }: LinkListHeaderProps): JSX.Element {
	return (
		<div
			className={cn(
				"grid",
				LINK_LIST_GRID_COLUMNS,
				"gap-2 px-2 h-10 shrink-0 items-center border-b border-border/80 bg-background/95 text-xs text-muted-foreground font-medium sticky top-0 z-10",
			)}>
			{headerGroups.map((headerGroup) =>
				headerGroup.headers.map((header) => {
					const canSort = header.column.getCanSort();
					const isSorted = header.column.getIsSorted();
					return (
						<div
							key={header.id}
							className={cn("flex items-center h-full", headerVariants({ canSort }))}
							onClick={header.column.getToggleSortingHandler()}>
							{header.isPlaceholder
								? null
								: flexRender(header.column.columnDef.header, header.getContext())}
							{canSort &&
								(isSorted === "asc" ? (
									<ArrowUp className='size-3.5 shrink-0' />
								) : isSorted === "desc" ? (
									<ArrowDown className='size-3.5 shrink-0' />
								) : (
									<ArrowUpDown className='size-3.5 shrink-0 opacity-50' />
								))}
						</div>
					);
				}),
			)}
		</div>
	);
}
