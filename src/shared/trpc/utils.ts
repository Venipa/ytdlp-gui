import { TRPCClientError } from "@trpc/client";
import isObject from "lodash-es/isObject";

export function isTRPCErrorResponse(obj: any): obj is TRPCClientError<any> {
	return (isObject(obj) as any) && obj.name === "TRPCClientError";
}
