import { Slot } from "@radix-ui/react-slot";
import { type VariantProps, cva } from "class-variance-authority";
import * as React from "react";

import { cn, sn } from "@renderer/lib/utils";

const buttonVariants = cva(
	"inline-flex items-center select-none cursor-pointer disabled:cursor-default whitespace-nowrap rounded-md text-sm font-medium transition-all focus-visible:outline-none focus:outline-none ring-0 ring-secondary active:ring-4 focus-visible:ring-4 disabled:pointer-events-none disabled:opacity-50",
	{
		variants: {
			variant: {
				default: "bg-primary text-primary-foreground shadow hover:bg-primary/90 [&.active]:bg-primary",
				accent: "bg-accent text-accent-foreground shadow hover:bg-accent/90 [&.active]:bg-accent",
				destructive: "bg-destructive text-destructive-foreground shadow-sm ring-destructive/40 hover:bg-destructive/90 focus:bg-destructive/90",
				outline: "border border-secondary-foreground/10 bg-background/20 ring-secondary/40 hover:bg-secondary/50 hover:text-secondary-foreground focus:bg-secondary/50",
				secondary: "bg-secondary text-secondary-foreground shadow-sm hover:bg-secondary/80",
				ghost: "hover:bg-primary/5 hover:text-secondary-foreground ring-secondary/30 [&.active]:text-secondary-foreground [&.active]:bg-primary/10",
				link: "text-primary underline-offset-4 hover:underline",
				black: "bg-neutral-50 text-black shadow hover:bg-black/90",
				white: "bg-neutral-50 text-black shadow hover:bg-neutral/90",
				success: "bg-green-500 text-white shadow hover:bg-green-500/90 ring-green-500",
				reverse: "dark:bg-black bg-white text-primary shadow dark:hover:bg-black/80 hover:bg-white/90",
			},
			align: {
				start: "justify-start",
				center: "justify-center",
				end: "justify-end",
			},
			size: {
				default: "h-9 px-4 py-2 gap-1 [&_svg]:size-4",
				sm: "h-8 rounded-md px-3 gap-1 text-xs [&_svg]:size-4",
				lg: "h-10 rounded-md px-8 gap-2 [&_svg]:size-5",
				icon: "h-9 w-9 [&_svg]:size-4",
				"icon-lg": "h-12 w-12",
				"icon-circle": "h-10 w-10 rounded-full p-2 [&>svg]:size-5",
				"icon-circle-lg": "h-12 w-12 rounded-full p-3 [&>svg]:size-6",
			},
		},
		defaultVariants: {
			variant: "default",
			size: "default",
			align: "center",
		},
	},
);

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement>, VariantProps<typeof buttonVariants> {
	asChild?: boolean;
	width?: number;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(({ className, variant, size, asChild = false, width, ...props }, ref) => {
	const Comp = asChild ? Slot : "button";
	return (
		<Comp
			className={cn(buttonVariants({ variant, size, className: cn(className, props.disabled && "disabled") }))}
			ref={ref}
			{...props}
			style={sn(props.style, width !== undefined && width > 0 && { width: `${width}rem` })}
		/>
	);
});
Button.displayName = "Button";

export { Button, buttonVariants };
