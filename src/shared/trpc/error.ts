import { logger } from "@shared/logger";
import { toast } from "sonner";
import { isTRPCErrorResponse } from "./utils";

export function throwErrorToast(err: any) {
	if (!isTRPCErrorResponse(err)) throw err;
	logger.error(err);
	return toast.error(err.message);
}
