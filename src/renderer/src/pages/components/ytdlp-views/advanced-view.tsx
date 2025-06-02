import ClickableText from "@renderer/components/ui/clickable-text";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@renderer/components/ui/collapsible";
import { cn } from "@renderer/lib/utils";
import { LucideChevronDown } from "lucide-react";
import { useEffect, useState } from "react";
import { useApp } from "../app-context";
function AdvancedViewContent() {
	return <h1>todo</h1>;
}
export default function AdvancedYTDLPView() {
	const { settings, setSetting } = useApp();
	const [open, setOpen] = useState(!!settings?.features?.advancedView);
	useEffect(() => {
		if (open !== !!settings?.features?.advancedView) setSetting("features.advancedView", open);
	}, [open]);
	return (
		<div className='flex flex-col gap-4'>
			<Collapsible open={open} onOpenChange={setOpen} className='flex flex-col'>
				<CollapsibleTrigger asChild>
					<ClickableText className='flex items-center gap-4'>
						<div className='h-px bg-muted flex-auto'></div>
						<div className='flex-shrink-0 flex items-center gap-1'>
							<div className='text-sm text-muted-foreground'>Advanced View</div>
							<LucideChevronDown className={cn("size-3 text-muted-foreground", open && "rotate-180")} />
						</div>
					</ClickableText>
				</CollapsibleTrigger>
				<CollapsibleContent>
					<AdvancedViewContent />
				</CollapsibleContent>
			</Collapsible>
		</div>
	);
}
