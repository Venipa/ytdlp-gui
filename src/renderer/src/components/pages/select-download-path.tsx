"use client";

import { LucideCheck, LucideFolder, LucideFolderOpen, LucidePlus, LucideX } from "lucide-react";
import * as React from "react";

import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { CaretSortIcon } from "@radix-ui/react-icons";
import { Spinner } from "@renderer/components/ui/spinner";
import { trpc } from "@renderer/lib/api/trpc-link";
import { cn } from "@renderer/lib/ui/utils";
import { CommandLoading } from "cmdk";
import { useFormContext } from "react-hook-form";
import { useApp } from "./app-context";

type Props = {
	name?: string;
	onValueChange?: (value?: any) => void;
	value?: string;
};
export default function SelectDownloadBox({ value: defaultValue, onValueChange, name: fieldName }: Props) {
	const { settings, setSetting } = useApp();
	const [open, setOpen] = React.useState(false);
	const [value, setValue] = React.useState(settings.download.selected ?? defaultValue);
	const triggerRef = React.useRef<HTMLButtonElement | null>(null);
	const [popoverWidth, setPopoverWidth] = React.useState<number>();
	const form = useFormContext();
	const { mutateAsync: addDownloadPath, isLoading: addDownloadPathLoading } = trpc.settings.addDownloadPath.useMutation();
	const { mutateAsync: deleteDownloadPath, isLoading: deleteDownloadPathLoading } = trpc.settings.deleteDownloadPath.useMutation();
	const setDownloadPath = React.useMemo(() => (path: string) => setSetting("download.selected", path), [setSetting]);
	const { mutateAsync: openPath } = trpc.internals.openPath.useMutation();

	React.useEffect(() => {
		const element = triggerRef.current;
		if (!element) return;

		const syncPopoverWidth = () => setPopoverWidth(element.getBoundingClientRect().width);
		syncPopoverWidth();

		const resizeObserver = new ResizeObserver(syncPopoverWidth);
		resizeObserver.observe(element);
		window.addEventListener("resize", syncPopoverWidth);

		return () => {
			resizeObserver.disconnect();
			window.removeEventListener("resize", syncPopoverWidth);
		};
	}, []);

	const isMutating = deleteDownloadPathLoading || addDownloadPathLoading;
	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverTrigger asChild>
				<Button ref={triggerRef} variant='outline' role='combobox' aria-expanded={open} className='flex gap-2 items-center flex-auto w-full justify-start'>
					<LucideFolder className='shrink-0 size-4 stroke-none fill-current' />
					<span className='flex-auto text-start truncate text-xs'>{value ? settings.download.selected : "Select download path..."}</span>
					<CaretSortIcon className='opacity-50 size-4' />
				</Button>
			</PopoverTrigger>
			<PopoverContent
				sideOffset={6}
				style={popoverWidth ? { width: `${popoverWidth}px` } : undefined}
				className='z-50 p-0 data-[state=open]:zoom-in-100 data-[state=closed]:zoom-out-100 data-[state=open]:fade-in-0 data-[state=closed]:fade-out-0 data-[side=bottom]:slide-in-from-top-2'>
				<Command>
					<CommandInput placeholder='Select download path...' className='h-9' />
					<CommandList>
						{isMutating && (
							<CommandLoading className='absolute inset-px bg-card rounded-lg z-10 flex items-center justify-center'>
								<Spinner />
							</CommandLoading>
						)}
						<CommandEmpty>No download path found.</CommandEmpty>
						<CommandGroup heading='Download folders'>
							{settings.download.paths?.map((p) => (
								<CommandItem
									key={p}
									value={p}
									onSelect={(newValue) => {
										if (newValue === value) return;
										setValue(newValue);
										onValueChange?.(newValue);
										setOpen(false);
										if (fieldName)
											form.setValue(fieldName, newValue, {
												shouldDirty: true,
												shouldTouch: true,
												shouldValidate: true,
											});

										setDownloadPath(newValue);
										console.log({ newValue });
									}}
									className='group/commandItem text-xs'>
									<span className='truncate'>{p}</span>
									<div className='flex items-center gap-1 ml-auto'>
										<LucideCheck className={cn("px-0.5 size-5 text-primary stroke-[2px]", value === p ? "opacity-100 cursor-pointer" : "opacity-0")} />
										<Button
											variant={"ghost"}
											className='size-6 p-1 opacity-20 group-hover/commandItem:opacity-100'
											onClick={(ev) => {
												ev.preventDefault();
												ev.stopPropagation();
												openPath({ path: p });
											}}>
											<LucideFolderOpen className='stroke-none fill-current' />
										</Button>
										{settings.download.paths?.length > 1 && (
											<Button
												variant={"ghost"}
												className='size-6 p-1 opacity-20 group-hover/commandItem:opacity-100'
												onClick={(ev) => {
													ev.preventDefault();
													ev.stopPropagation();
													deleteDownloadPath(p);
												}}>
												<LucideX />
											</Button>
										)}
									</div>
								</CommandItem>
							))}
						</CommandGroup>
						<CommandSeparator />
						<CommandGroup>
							<CommandItem
								onSelect={() => {
									addDownloadPath();
								}}
								className='cursor-pointer flex items-center gap-2'>
								<LucidePlus className='size-4' />
								<span>Add new path</span>
							</CommandItem>
						</CommandGroup>
					</CommandList>
				</Command>
			</PopoverContent>
		</Popover>
	);
}
