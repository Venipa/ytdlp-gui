import { cn } from "@renderer/lib/utils";
import { type VariantProps, cva } from "class-variance-authority";
import React from "react";

// Inset input variants
const insetInputVariants = cva(
	"flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm ring-0 ring-offset-0 ring-offset-secondary ring-secondary/0 transition-all placeholder:text-muted-foreground read-only:focus-visible:ring-0 read-only:focus-visible:border-input focus-visible:ring-4 focus-visible:ring-secondary focus-visible:border-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus:outline-none",
	{
		variants: {
			inset: {
				true: "pl-9", // Extra padding for inset icon/element on the left
				false: "",
			},
			size: {
				default: "h-9 text-sm",
				sm: "h-8 px-2 text-xs",
				lg: "h-10 px-4 text-base",
			},
		},
		defaultVariants: {
			inset: false,
			size: "default",
		},
	},
);

export type InputInsetProps = React.InputHTMLAttributes<HTMLInputElement> &
	VariantProps<typeof insetInputVariants> & {
		insetElement?: React.ReactNode; // Icon or element for the inset
	};

export const InputInset = React.forwardRef<HTMLInputElement, InputInsetProps>(({ className, inset, size, insetElement, ...props }, ref) => {
	return (
		<div className='relative w-full'>
			{insetElement && <span className='pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 flex h-5 w-5 items-center text-muted-foreground'>{insetElement}</span>}
			<input type={props.type} className={cn(insetInputVariants({ inset: !!insetElement || inset, size, className }))} ref={ref} {...props} />
		</div>
	);
});
InputInset.displayName = "InputInset";
