import { Button } from "@renderer/components/ui/button";
import { LucideRefreshCcw } from "lucide-react";

interface LinkListEmptyStateProps {
	hasSearch: boolean;
	onClearSearch: () => void;
}

export default function LinkListEmptyState({ hasSearch, onClearSearch }: LinkListEmptyStateProps): JSX.Element {
	return (
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
				<Button variant={"ghost"} size={"sm"} className='px-2' onClick={onClearSearch}>
					<LucideRefreshCcw />
					<span>Clear search</span>
				</Button>
			)}
		</div>
	);
}
