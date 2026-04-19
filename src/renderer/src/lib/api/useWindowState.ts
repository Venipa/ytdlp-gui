import { createLogger } from "@shared/logger";
import { throttle } from "lodash-es";
import { useEffect, useMemo, useState } from "react";
import { useEventListener } from "usehooks-ts";
import { trpc } from "./trpc-link";
const log = createLogger("useWindowState");
export function useWindowState() {
	const [windowState] = trpc.window.getState.useSuspenseQuery(undefined, { networkMode: "always" });
	const utils = trpc.useUtils();
	trpc.window.stateChange.useSubscription(undefined, {
		enabled: true,
		onData(data) {
			utils.window.getState.setData(undefined, data, { updatedAt: Date.now() });
			utils.window.getState.invalidate();
		},
	});
	return { windowState };
}
export function useWindowConfig() {
	const [configState] = trpc.window.getConfig.useSuspenseQuery();
	const { mutateAsync } = trpc.window.setConfig.useMutation();
	return [configState, mutateAsync] as [typeof configState, typeof mutateAsync];
}
export function useWindowTitle(title: string) {
	const [configState] = trpc.window.getConfig.useSuspenseQuery(undefined, {
		refetchOnReconnect: false,
		refetchInterval: false,
		refetchOnWindowFocus: false,
		refetchIntervalInBackground: false,
	});
	const { mutateAsync: setConfig } = trpc.window.setConfig.useMutation();
	const prevTitle = useMemo(() => configState.title, []);
	useEffect(() => {
		setConfig({ title });
		return () => {
			setConfig({ title: prevTitle });
		};
	}, [title]);
}
export function useWindowSize(size: { width?: number; height?: number }) {
	const { mutateAsync: mutateSize } = trpc.window.setSize.useMutation();
	const [prevSize] = useState(() => {
		const { clientWidth: width, clientHeight: height } = document.body;
		return { width, height };
	});
	const [newSize, setNewSize] = useState(size);
	useEffect(() => {
		mutateSize(size);
		return () => {
			mutateSize(prevSize);
		};
	}, [newSize]);

	return [newSize, setNewSize];
}
export function useWindowShortcut(key: string, onKeyHit: () => void, options?: Partial<{ modifier?: "ctrl" | "command" }>) {
	const [throttledHandler] = useState(() =>
		throttle((ev: KeyboardEvent) => {
			if (ev.key.toLowerCase() === key.toLowerCase() && (options?.modifier === "ctrl" ? ev.ctrlKey : options?.modifier === "command" ? ev.metaKey : true)) onKeyHit();
		}, 50),
	);
	useEventListener("keydown", throttledHandler, undefined, { passive: true });
}
export function useWindowControls() {
	const { mutateAsync: mutateClose } = trpc.window.close.useMutation();
	const { mutateAsync: mutateHide } = trpc.window.hide.useMutation();
	const { mutateAsync: mutateMinimize } = trpc.window.minimize.useMutation();
	const { mutateAsync: mutateMaximize } = trpc.window.maximize.useMutation();
	const close = () => mutateClose();
	const minimize = () => mutateMinimize();
	const maximize = () => mutateMaximize();
	const hide = () => mutateHide();
	return { close, minimize, maximize, hide };
}
