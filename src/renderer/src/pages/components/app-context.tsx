import { AppStore } from "@main/stores/AppStore";
import { trpc } from "@renderer/lib/trpc-link";
import { logger } from "@shared/logger";
import { uniq } from "lodash-es";
import { Context, Provider, createContext, useContext, useMemo } from "react";
import { toast } from "sonner";
import { useDebounceCallback } from "usehooks-ts";
import { z } from "zod";
import { useLinkBoxStore } from "./add-link.store";
type AppContext = {
	settings: AppStore & Record<string, any>;
	setSetting<T = any>(key: string, value: any, showToast?: boolean): Promise<{ key: string; value: T }>;
	getSetting<T = any>(key?: string): Promise<T>;
};
type AppContextType = Context<AppContext>;
const appContext: AppContextType = createContext({} as any);
const useApp = () => useContext(appContext);

const AppContextProvider: Provider<AppContext> = (({ value, ...props }) => {
	const utils = trpc.useUtils();
	const [settings] = trpc.settings.index.useSuspenseQuery(undefined);
	trpc.settings.onChange.useSubscription(undefined, {
		onData(data) {
			console.log("settings", { newData: data });
			utils.settings.index.setData(undefined, data);
			utils.settings.index.invalidate();
		},
	});
	trpc.ytdl.onAutoAddCapture.useSubscription(undefined, {
		onData(data) {
			toast.info("A new url has been captured.", { description: data.url });
		},
	});
	trpc.ytdl.onToast.useSubscription(undefined, {
		onData([message, description, type = "default"]) {
			const messageType = z.enum(["warning", "error", "success", "default", "info"]).parse(type);
			if (!messageType) return;
			if (messageType === "default") toast(message, { description });
			else toast[messageType](message, { description });
		},
	});
	const [, setAddLinkContent] = useLinkBoxStore();
	trpc.ytdl.onAutoAdd.useSubscription(undefined, {
		onData(data) {
			setAddLinkContent((s) => {
				const newLinks = uniq((s ?? "").split("\n").filter(Boolean).concat(data));
				logger.debug({ newLinks });
				return newLinks.join("\n");
			});
		},
	});
	const getSetting = useMemo(() => (key?: string) => (!key ? utils.internals.getAll.fetch() : utils.settings.key.fetch(key)), [utils.internals.get]);
	const onUpdateCallback = useDebounceCallback((res) => {
		toast.success("Settings have been saved.");
		return res;
	}, 500);
	const { mutateAsync: _setSetting } = trpc.settings.update.useMutation();
	const setSetting = useMemo(
		() => (key: string, value: any, showToast?: boolean) =>
			_setSetting({ key, value }).then((s) => {
				if (showToast) onUpdateCallback(s);
				return s;
			}) as Promise<{ key: string; value: any }>,
		[],
	);

	return <appContext.Provider value={{ getSetting, setSetting, settings }} {...props}></appContext.Provider>;
}) as any;
export { AppContextProvider, useApp };
