import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
import { HTMLProps, ReactElement } from "react";

type Element = <T extends HTMLProps<HTMLDivElement> = HTMLProps<HTMLDivElement>>(props?: T) => ReactElement<T, any>;
type Module = {
	default: Element;
	meta: {
		title: string;
		icon?: Element;
		index?: number;
		show?: boolean;
		customLayout?: boolean;
	};
};
export const SECTIONTABS = import.meta.glob<Module>(`./sections/*.tsx`, { eager: true });
export const sectionValues = Object.values(SECTIONTABS).filter((d) => d.default);
export const sectionTabs = sectionValues
	.map(({ meta }, i) => ({ ...meta, index: meta.index !== undefined ? meta.index : i }))
	.filter((d) => d.show !== false)
	.sort((a, b) => a.index - b.index);

export const getSectionContentByTitle = (title: string) => sectionValues.find((d) => d.meta.title === title)?.default;
export const getSectionMetaByTitle = (title: string) => sectionValues.find((d) => d.meta.title === title)?.meta;

export const selectedTabTitle = atomWithStorage<string>("selectedTabTitle", sectionTabs[0].title, undefined, { getOnInit: true });
export const useSelectedTabTitle = () => useAtom(selectedTabTitle);
export const useResetSelectedTab = () => () => useSelectedTabTitle()[1](sectionTabs[0].title);
