import { useAtom } from "jotai";
import { atomWithStorage } from "jotai/utils";
export type MediaType =
	| "auto"
	| "video-best"
	| "video-4k"
	| "video-2k"
	| "video-1440p"
	| "video-1080p"
	| "video-720p"
	| "video-480p"
	| "audio-bestaudio"
	| "audio-mp3"
	| "audio-m4a"
	| "audio-opus";
const mediaType = atomWithStorage("mediaType", "auto" as MediaType);
export const useMediaType = () => useAtom(mediaType);
