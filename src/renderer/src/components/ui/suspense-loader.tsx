"use client";

import { cn } from "@renderer/lib/utils";
import { Spinner } from "./spinner";

export default function SuspenseLoader({ className }: { className?: string }) {
	return (
		<div className={cn("flex flex-col h-full items-center justify-center", className)}>
			<Spinner size={"sm"} />
		</div>
	);
}

export function SuspenseLoaderOptions({ className, content }: { className?: string; content?: React.ReactNode }) {
	return (
		<div className={cn("flex flex-col h-full items-center justify-center gap-2", className)}>
			<Spinner size={"sm"} />
			{content}
		</div>
	);
}
