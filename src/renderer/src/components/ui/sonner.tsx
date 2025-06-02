"use client";

import { LucideCheckCircle2 } from "lucide-react";
import { useTheme } from "next-themes";
import { Toaster as Sonner } from "sonner";

type ToasterProps = React.ComponentProps<typeof Sonner>;

const Toaster = ({ ...props }: ToasterProps) => {
	const { theme = "system" } = useTheme();

	return (
		<Sonner
			theme={theme as ToasterProps["theme"]}
			className='toaster group'
			richColors
			position='bottom-right'
			offset={48}
			icons={{ success: <LucideCheckCircle2 className='size-4' /> }}
			toastOptions={{
				classNames: {
					toast: "group select-none toast group-[.toaster]:bg-background group-[.toaster]:text-foreground group-[.toaster]:border-border group-[.toaster]:shadow-lg py-2",
					description: "group-[.toast]:text-muted-foreground",
					content: "-mt-px",
					actionButton: "group-[.toast]:bg-primary group-[.toast]:text-primary-foreground",
					cancelButton: "group-[.toast]:bg-muted group-[.toast]:text-muted-foreground",
				},
			}}
			{...props}
		/>
	);
};

export { Toaster };
