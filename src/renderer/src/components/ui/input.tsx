import * as React from "react";

import { cn } from "@renderer/lib/utils";

export interface InputProps extends React.InputHTMLAttributes<HTMLInputElement> {}

const Input = React.forwardRef<HTMLInputElement, InputProps>(({ className, type, ...props }, ref) => {
	return (
		<input
			type={type}
			className={cn(
				"flex h-9 w-full rounded-md border border-input bg-transparent px-3 py-1 text-sm ring-0 ring-offset-0 ring-offset-secondary ring-secondary/0 transition-all placeholder:text-muted-foreground read-only:focus-visible:ring-0 read-only:focus-visible:border-input focus-visible:ring-4 focus-visible:ring-secondary focus-visible:border-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus:outline-none",
				className,
			)}
			ref={ref}
			{...props}
		/>
	);
});
Input.displayName = "Input";

export { Input };
