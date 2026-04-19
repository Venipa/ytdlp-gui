import { SortingState } from "@tanstack/react-table";
import { atomWithStorage } from "jotai/utils";

const defaultSorting: SortingState = [{ id: "created", desc: true }];

export const linkListSortingStore = atomWithStorage<SortingState>("linkListSorting", defaultSorting, undefined, { getOnInit: true });
