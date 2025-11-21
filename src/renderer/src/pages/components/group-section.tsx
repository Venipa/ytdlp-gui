import { cn } from "@renderer/lib/utils";
import { PropsWithChildren, ReactNode } from "react";
interface GroupSectionProps {
	title: string | ReactNode;
	titleRight?: ReactNode;
	className?: string;
}
export default function GroupSection({ title, titleRight, children, className }: PropsWithChildren<GroupSectionProps>) {
	return (
		<div className='flex flex-col gap-4 group/parent'>
			<div className='flex flex-col border border-border rounded-lg'>
				<div className='flex gap-3 -mt-2 mx-4'>
					<h1 className='text-xs text-muted-foreground tracking-wider select-none bg-background px-2'>{title}</h1>
					{titleRight}
				</div>
				<div className={cn("flex flex-col p-2.5 gap-2.5", className)}>{children}</div>
			</div>
		</div>
	);
}
