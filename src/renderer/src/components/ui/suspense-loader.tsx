"use client";

import { Progress } from "@renderer/components/ui/progress";
import { cn } from "@renderer/lib/utils";
import { ReactElement, ReactNode } from "react";
import { Spinner } from "./spinner";

export default function SuspenseLoader({ className }: { className?: string }) {
	return (
		<div className={cn("flex flex-col h-full items-center justify-center", className)}>
			<Spinner size={"sm"} />
		</div>
	);
}
type SuspenseLoaderOptionsProps = {
	className?: string;
	content?: ReactNode | ReactElement | ReactNode;
	progress?: number | null;
	progressLabel?: string | null;
} & React.HTMLAttributes<HTMLDivElement>;
export function SuspenseLoaderOptions({ className, content, progressLabel, progress }: SuspenseLoaderOptionsProps) {
	return (
		<div className={cn("flex flex-col h-full items-center justify-center gap-2", className)}>
			{progress ? (
				<>
					<div className='flex items-center gap-2'>
						<span className='text-muted-foreground text-sm'>{progressLabel ?? "Downloading dependencies..."}</span>
						<span className='tabular-nums text-sm text-muted-foreground'>{progress.toFixed(2)}%</span>
					</div>
					<Progress value={progress} className='w-40' />
				</>
			) : (
				<>
					<span className='text-muted-foreground text-sm mb-2'>{progressLabel ?? "Downloading dependencies..."}</span>
					<Spinner size={"sm"} />
				</>
			)}
		</div>
	);
}
