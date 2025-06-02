"use client";

import { cva, type VariantProps } from "class-variance-authority";
import { Laptop, Moon, Sun } from "lucide-react";
import { motion } from "motion/react";
import { useTheme } from "next-themes";
import * as React from "react";

import { Button } from "@/components/ui/button";

const toggleVariants = cva("inline-flex items-center justify-center rounded-full transition-colors hover:opacity-90", {
	variants: {
		theme: {
			light: "",
			dark: "",
			system: "",
		},
		size: {
			sm: "h-8 w-8",
			md: "h-10 w-10",
			lg: "h-12 w-12",
		},
		variant: {
			default: "",
			ghost: "",
			primary: "",
		},
	},
	defaultVariants: {
		theme: "light",
		size: "md",
		variant: "ghost",
	},
});

interface ThemeToggleProps extends VariantProps<typeof toggleVariants> {}

export function ThemeToggle({ size, variant }: ThemeToggleProps) {
	const { setTheme, theme } = useTheme();
	const [mounted, setMounted] = React.useState(false);

	React.useEffect(() => setMounted(true), []);

	if (!mounted) {
		return null;
	}

	const toggleTheme = () => {
		if (theme === "light") {
			setTheme("dark");
		} else if (theme === "dark") {
			setTheme("system");
		} else {
			setTheme("light");
		}
	};

	const iconSize = {
		sm: "h-4 w-4",
		md: "h-5 w-5",
		lg: "h-6 w-6",
	}[size || "md"];

	return (
		<Button
			variant={(variant || "ghost") as any}
			size='icon'
			onClick={toggleTheme}
			className={toggleVariants({ theme: theme as "light" | "dark" | "system", size, variant })}
			tabIndex={0}>
			<motion.div
				initial={false}
				animate={{
					scale: theme === "light" ? 1 : 0,
					rotate: theme === "light" ? 0 : -180,
				}}
				transition={{ duration: 0.3, ease: "easeInOut" }}>
				<Sun className={iconSize} />
			</motion.div>
			<motion.div
				initial={false}
				animate={{
					scale: theme === "dark" ? 1 : 0,
					rotate: theme === "dark" ? 0 : 180,
				}}
				transition={{ duration: 0.3, ease: "easeInOut" }}
				style={{ position: "absolute" }}>
				<Moon className={iconSize} />
			</motion.div>
			<motion.div
				initial={false}
				animate={{
					scale: theme === "system" ? 1 : 0,
					rotate: theme === "system" ? 0 : -180,
				}}
				transition={{ duration: 0.3, ease: "easeInOut" }}
				style={{ position: "absolute" }}>
				<Laptop className={iconSize} />
			</motion.div>
			<span className='sr-only'>Toggle theme</span>
		</Button>
	);
}
