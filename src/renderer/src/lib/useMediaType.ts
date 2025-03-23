import { atom, useAtom } from "jotai";
export type MediaType = "video" | "audio" | "auto";
const mediaType = atom("auto" as MediaType)
export const useMediaType = () => useAtom(mediaType)
