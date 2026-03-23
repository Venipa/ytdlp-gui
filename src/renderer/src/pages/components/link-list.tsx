import { Button } from "@renderer/components/ui/button";
import SuspenseLoader from "@renderer/components/ui/suspense-loader";
import { SearchEngine, SearchItem } from "@renderer/lib/searchEngine";
import { trpc } from "@renderer/lib/trpc-link";
import { cn } from "@renderer/lib/utils";
import { createLogger } from "@shared/logger";
import { LucideRefreshCcw } from "lucide-react";
import { useMemo, useState } from "react";
import { VList } from "virtua";
import { YTDLItem } from "ytdlp-gui/types";
import { useSearchStore } from "./add-link.store";
import LinkListItem from "./link-list-item";
const log = createLogger("LinkList");
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
	const noResults = useMemo(() => !filteredItems.length, [filteredItems]);
	const hasSearch = useMemo(() => !!search?.length, [search]);
	return (
		<div className={cn("flex flex-col gap-2 relative", props.className)}>
			<VList className='grow relative flex flex-col pb-6' style={{ height: "100%" }}>
				{isFetching && <SuspenseLoader className='absolute bg-background inset-0' />}
				{filteredItems?.map((d) => (
					<div className='h-12' key={d.id}>
						<LinkListItem {...(d as any)} />
					</div>
				))}
			</VList>
			{noResults && (
				<div className='absolute bg-background inset-0 flex items-center justify-center text-muted-foreground gap-4 flex-col'>
					{hasSearch ? <span>No results found. Try a different search.</span> : <span>No downloads found. Add a link to start downloading.</span>}
					<pre className='flex flex-col bg-muted/20 border border-muted/40 p-2 rounded-md'>
						{["type:video|audio", "size:<100mb, <=10gb, etc", "status:success|error|active"].map((line) => (
							<span key={line} className='text-xs text-muted-foreground'>
								{line}
							</span>
						))}
					</pre>
					{hasSearch && (
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
					)}
				</div>
			)}
		</div>
	);
}
