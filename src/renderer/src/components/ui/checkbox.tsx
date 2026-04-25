"use client";

import * as CheckboxPrimitive from "@radix-ui/react-checkbox";
import { CheckIcon } from "@radix-ui/react-icons";
import * as React from "react";

import { cn } from "@renderer/lib/ui/utils";
import { VariantProps, cva } from "class-variance-authority";

const checkboxVariants = cva(
	"peer shrink-0 rounded-sm border border-muted shadow focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50 data-[state=checked]:bg-primary data-[state=checked]:text-primary-foreground data-[state=checked]:border-primary",
	{
		variants: {
			variant: {
				default: "bg-primary text-primary-foreground border-primary",
				ghost: "data-[state=checked]:bg-muted data-[state=checked]:text-primary data-[state=checked]:border-muted-foreground/60",
				accent: "data-[state=checked]:bg-accent data-[state=checked]:text-accent-foreground data-[state=checked]:border-accent",
			},
			size: {
				default: "h-4.5 w-4.5 px-0 [&_svg]:size-3",
				sm: "h-4 w-4 px-0 [&_svg]:size-3",
				lg: "h-5 w-5 px-0 [&_svg]:size-4",
			},
		},
	},
);

interface CheckboxProps
	extends React.ComponentPropsWithoutRef<typeof CheckboxPrimitive.Root>,
		VariantProps<typeof checkboxVariants> {}

const Checkbox = React.forwardRef<React.ElementRef<typeof CheckboxPrimitive.Root>, CheckboxProps>(
	({ className, variant = "default", size = "default", ...props }, ref) => (
		<CheckboxPrimitive.Root ref={ref} className={cn(checkboxVariants({ variant, size }), className)} {...props}>
			<CheckboxPrimitive.Indicator className={cn("flex items-center justify-center text-current")}>
				<CheckIcon className='h-4 w-4' />
			</CheckboxPrimitive.Indicator>
		</CheckboxPrimitive.Root>
	),
);
Checkbox.displayName = CheckboxPrimitive.Root.displayName;

export { Checkbox };
