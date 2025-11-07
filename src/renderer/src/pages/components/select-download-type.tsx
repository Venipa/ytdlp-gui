"use client";

import * as React from "react";

import { Select, SelectContent, SelectItem, SelectTrigger } from "@renderer/components/ui/select";
import { type MediaType } from "@renderer/lib/useMediaType";
import { cn } from "@renderer/lib/utils";
import { useAddLinkStore } from "./add-link.store";
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
	const [{ type }, setValue] = useAddLinkStore();

	const selectedValue = React.useMemo(() => mediaStringMap[type], [type]);
	const availableValues = React.useMemo(() => {
		const values = Object.entries(mediaStringMap);
		return values.map(([value, name]) => ({ name, value }));
	}, [type]);
	return (
		<Select
			open={open}
			onOpenChange={setOpen}
			value={type}
			onValueChange={(v: MediaType) => {
				setValue((s) => ({ ...s, type: v }));
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
