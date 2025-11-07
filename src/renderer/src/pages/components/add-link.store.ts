import { atomWithDebounce } from "@renderer/lib/atom";
import { atom, useAtom, useAtomValue, useSetAtom } from "jotai";

export const linkBoxStore = atom("");
export const useLinkBoxStore = () => useAtom(linkBoxStore);

export const searchStore = atomWithDebounce<string>("");
export const useSearchStore = () => {
	const search = useAtomValue(searchStore.currentValueAtom);
	const setSearch = useSetAtom(searchStore.debouncedValueAtom);
	return [search, setSearch] as const;
};
export interface AddLinkStore {
	showAddLink: boolean;
	type: "video" | "audio" | "auto";
	path: string | null;
}
export const addLinkStore = atom({
	showAddLink: false,
	type: "auto",
	path: null,
} as AddLinkStore);
export const useAddLinkStore = () => useAtom(addLinkStore);
