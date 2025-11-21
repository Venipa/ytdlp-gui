"use client";

import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import * as React from "react";

import { Slot } from "@radix-ui/react-slot";
import { cn } from "@renderer/lib/utils";

function TooltipProvider({ delayDuration = 0, ...props }: React.ComponentProps<typeof TooltipPrimitive.Provider>) {
	return <TooltipPrimitive.Provider data-slot='tooltip-provider' delayDuration={delayDuration} {...props} />;
}

function Tooltip({ ...props }: React.ComponentProps<typeof TooltipPrimitive.Root>) {
	return (
		<TooltipProvider>
			<TooltipPrimitive.Root data-slot='tooltip' {...props} />
		</TooltipProvider>
	);
}

function TooltipTrigger({ ...props }: React.ComponentProps<typeof TooltipPrimitive.Trigger>) {
	return <TooltipPrimitive.Trigger data-slot='tooltip-trigger' {...props} />;
}

function TooltipContent({ className, sideOffset = 0, children, ...props }: React.ComponentProps<typeof TooltipPrimitive.Content>) {
	return (
		<TooltipPrimitive.Portal>
			<TooltipPrimitive.Content
				data-slot='tooltip-content'
				sideOffset={sideOffset}
				className={cn(
					"bg-foreground text-background animate-in fade-in-0 zoom-in-95 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:zoom-out-95 data-[side=bottom]:slide-in-from-top-2 data-[side=left]:slide-in-from-right-2 data-[side=right]:slide-in-from-left-2 data-[side=top]:slide-in-from-bottom-2 z-50 w-fit origin-(--radix-tooltip-content-transform-origin) rounded-md px-3 py-1.5 text-xs text-balance",
					className,
				)}
				{...props}>
				{children}
			</TooltipPrimitive.Content>
		</TooltipPrimitive.Portal>
	);
}

const QTooltip = React.forwardRef<
	React.ElementRef<typeof Tooltip>,
	React.ComponentPropsWithoutRef<typeof TooltipContent> & React.PropsWithChildren<{ content: string | React.ReactElement | React.ReactNode | React.ComponentType }>
>(({ className, children, content, asChild, side, ...props }, ref) => {
	const TriggerSlot = asChild ? Slot : "button";
	return (
		<Tooltip delayDuration={350} defaultOpen={false} {...props}>
			<TooltipTrigger disabled={!content} asChild={asChild} className='cursor-auto'>
				<TriggerSlot>{children}</TriggerSlot>
			</TooltipTrigger>
			<TooltipContent
				align='center'
				{...{ side, ...props }}
				sideOffset={4}
				updatePositionStrategy='optimized'
				ref={ref}
				className='bg-white dark:bg-background border border-muted fill-muted dark:border-muted/60 shadow-md text-primary'>
				{typeof content === "string" ? <div>{content}</div> : content}
			</TooltipContent>
		</Tooltip>
	);
});
QTooltip.displayName = "QTooltip";
export { QTooltip, Tooltip, TooltipContent, TooltipProvider, TooltipTrigger };
