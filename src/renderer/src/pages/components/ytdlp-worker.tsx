import { logger } from "@shared/logger";
import { useEventListener } from "usehooks-ts";
import { useYtdl } from "./ytdl-context";
export default function YTLDPObserver() {
	const ytdl = useYtdl();
	useEventListener("beforeunload", () => {
		logger.debug("beforeunload status", ytdl.status);
	});
	return null;
}
