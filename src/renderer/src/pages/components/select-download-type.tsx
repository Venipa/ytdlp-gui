"use client";

import * as React from "react";

import { Select, SelectContent, SelectItem, SelectTrigger } from "@renderer/components/ui/select";
import { type MediaType, useMediaType } from "@renderer/lib/useMediaType";
import { cn } from "@renderer/lib/utils";
const mediaStringMap: Record<MediaType, string> = {
	audio: "Audio",
	video: "Video",
	auto: "Auto",
};
type Props = {
	onValueChange?: (value?: MediaType) => void;
	value?: MediaType;
	className?: string;
};
export default function SelectMediaTypeBox({ className, onValueChange }: Props) {
	const [open, setOpen] = React.useState(false);
	const [value, setValue] = useMediaType();
	const selectedValue = React.useMemo(() => mediaStringMap[value], [value]);
	const availableValues = React.useMemo(() => {
		const values = Object.entries(mediaStringMap);
		return values.map(([value, name]) => ({ name, value }));
	}, [value]);
	return (
		<Select
			open={open}
			onOpenChange={setOpen}
			value={value}
			onValueChange={(v: MediaType) => {
				setValue(v);
				onValueChange?.(v);
			}}>
			<SelectTrigger className={cn(className, "select-none")}>{selectedValue}</SelectTrigger>
			<SelectContent>
				{availableValues.map((d) => (
					<SelectItem key={d.value} value={d.value}>
						{d.name}
					</SelectItem>
				))}
			</SelectContent>
		</Select>
	);
}
