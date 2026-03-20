"use client";

import { LucideCheck } from "lucide-react";
import * as React from "react";

import { Button } from "@/components/ui/button";
import { Command, CommandGroup, CommandInput, CommandItem, CommandList, CommandSeparator } from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { CaretSortIcon } from "@radix-ui/react-icons";
import { type MediaType, useMediaType } from "@renderer/lib/useMediaType";
import { cn } from "@renderer/lib/utils";

type Option = {
	name: string;
	value: MediaType;
};

const autoOption: Option = {
	name: "Auto",
	value: "auto",
};

const videoOptions: Option[] = [
	{ name: "Best", value: "video-best" },
	{ name: "4K", value: "video-4k" },
	{ name: "2K", value: "video-2k" },
	{ name: "1440p", value: "video-1440p" },
	{ name: "1080p", value: "video-1080p" },
	{ name: "720p", value: "video-720p" },
	{ name: "480p", value: "video-480p" },
];

const audioOptions: Option[] = [
	{ name: "Best", value: "audio-bestaudio" },
	{ name: "MP3", value: "audio-mp3" },
	{ name: "M4A", value: "audio-m4a" },
	{ name: "Opus", value: "audio-opus" },
];

const mediaStringMap: Record<MediaType, string> = {
	auto: "Auto",
	"video-best": "Video · Best",
	"video-4k": "Video · 4K",
	"video-2k": "Video · 2K",
	"video-1440p": "Video · 1440p",
	"video-1080p": "Video · 1080p",
	"video-720p": "Video · 720p",
	"video-480p": "Video · 480p",
	"audio-bestaudio": "Audio · Best",
	"audio-mp3": "Audio · MP3",
	"audio-m4a": "Audio · M4A",
	"audio-opus": "Audio · Opus",
};

type Props = {
	onValueChange?: (value?: MediaType) => void;
	value?: MediaType;
	className?: string;
};

const POPOVER_LEFT_EXPAND_PX = 100;

export default function SelectMediaTypeBox({ className, onValueChange }: Props) {
	const [open, setOpen] = React.useState(false);
	const [type, setType] = useMediaType();
	const triggerRef = React.useRef<HTMLButtonElement | null>(null);
	const [triggerWidth, setTriggerWidth] = React.useState<number>(0);

	const selectedValue = React.useMemo(() => mediaStringMap[type], [type]);

	React.useEffect(() => {
		const triggerElement = triggerRef.current;
		if (!triggerElement) return;

		const syncTriggerWidth = () => {
			setTriggerWidth(triggerElement.getBoundingClientRect().width);
		};

		syncTriggerWidth();
		const resizeObserver = new ResizeObserver(syncTriggerWidth);
		resizeObserver.observe(triggerElement);
		window.addEventListener("resize", syncTriggerWidth);

		return () => {
			resizeObserver.disconnect();
			window.removeEventListener("resize", syncTriggerWidth);
		};
	}, []);

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverTrigger asChild>
				<Button ref={triggerRef} variant='outline' role='combobox' aria-expanded={open} className={cn(className, "select-none justify-between min-w-[140px]")}>
					<span className='truncate text-xs'>{selectedValue}</span>
					<CaretSortIcon className='opacity-50 size-4 shrink-0' />
				</Button>
			</PopoverTrigger>
			<PopoverContent
				sideOffset={6}
				style={
					triggerWidth > 0
						? {
								width: `${triggerWidth + POPOVER_LEFT_EXPAND_PX}px`,
								marginLeft: `-${POPOVER_LEFT_EXPAND_PX}px`,
							}
						: undefined
				}
				className='p-0 data-[state=open]:zoom-in-100 data-[state=closed]:zoom-out-100 data-[state=open]:fade-in-0 data-[state=closed]:fade-out-0 data-[side=bottom]:slide-in-from-top-2'>
				<Command>
					<CommandInput placeholder='Select download type...' className='h-9' />
					<CommandList>
						<CommandGroup>
							<CommandItem
								value={autoOption.value}
								onSelect={(nextValue) => {
									const selectedType = nextValue as MediaType;
									if (selectedType === type) return;
									setType(selectedType);
									onValueChange?.(selectedType);
									setOpen(false);
								}}>
								<span>{autoOption.name}</span>
								<LucideCheck className={cn("ml-auto size-4", type === autoOption.value ? "opacity-100" : "opacity-0")} />
							</CommandItem>
						</CommandGroup>
						<CommandSeparator />
						<CommandGroup heading='Video'>
							{videoOptions.map((option) => (
								<CommandItem
									key={option.value}
									value={option.value}
									onSelect={(nextValue) => {
										const selectedType = nextValue as MediaType;
										if (selectedType === type) return;
										setType(selectedType);
										onValueChange?.(selectedType);
										setOpen(false);
									}}>
									<span>{option.name}</span>
									<LucideCheck className={cn("ml-auto size-4", type === option.value ? "opacity-100" : "opacity-0")} />
								</CommandItem>
							))}
						</CommandGroup>
						<CommandSeparator />
						<CommandGroup heading='Audio'>
							{audioOptions.map((option) => (
								<CommandItem
									key={option.value}
									value={option.value}
									onSelect={(nextValue) => {
										const selectedType = nextValue as MediaType;
										if (selectedType === type) return;
										setType(selectedType);
										onValueChange?.(selectedType);
										setOpen(false);
									}}>
									<span>{option.name}</span>
									<LucideCheck className={cn("ml-auto size-4", type === option.value ? "opacity-100" : "opacity-0")} />
								</CommandItem>
							))}
						</CommandGroup>
					</CommandList>
				</Command>
			</PopoverContent>
		</Popover>
	);
}
