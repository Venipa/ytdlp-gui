import { logger } from "@shared/logger";
import type { IpcRendererEvent } from "electron/renderer";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useIsomorphicLayoutEffect } from "usehooks-ts";
import usePromise from "./use-promise";

interface IpcOptions<T = any> {
	defaultValue: T;
	getOnInit?: boolean;
}
export function useIPC<T = any>(eventName: string, options: IpcOptions<T> = { getOnInit: false } as IpcOptions<T>) {
	const [state, setState] = useState(options.defaultValue);
	const handle = useCallback(
		(ev: IpcRendererEvent, data: any) => {
			setState(data);
		},
		[eventName],
	);
	useIsomorphicLayoutEffect(() => {
		if (options.getOnInit)
			window.api
				.invoke(`action:${eventName}`)
				.then((initialData) => setState(initialData))
				.catch((err) => {
					logger.error(err);
				});
	}, [eventName]);
	useEffect(() => {
		window.api.on(eventName, handle);
		return () => {
			window.api.off(eventName, handle);
		};
	}, [handle]);
	return [state, setState] as const;
}
export function useInvoke<T = any>(actionName: string, data: any[] = [], options: IpcOptions<T> = {} as any) {
	const state = usePromise(() => window.api.invoke<T>(actionName, ...data));
	const returnValue = useMemo(() => state ?? options.defaultValue, [state, options.defaultValue]);
	return returnValue;
}
