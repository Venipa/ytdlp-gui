import { ComponentType } from "react";
export type SectionMeta = {
	title: string;
	icon?: ComponentType<{ className?: string }>;
	index?: number;
	show?: boolean;
	description?: string;
};
