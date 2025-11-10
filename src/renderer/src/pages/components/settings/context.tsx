import { atom, getDefaultStore, useAtom } from "jotai";
import { createContext, useCallback, useContext, useEffect } from "react";

type SettingsContextType = {
	open: boolean;
	showSettings: () => void;
	closeSettings: () => void;
};
const settingsContext = atom<{ open: boolean }>({
	open: false,
});
const setShowSettings = (open: boolean) => {
	getDefaultStore().set(settingsContext, (prev) => ({ ...prev, open }));
	const v = getDefaultStore().get(settingsContext);
	console.log("setShowSettings", v);
};
const SettingsContext = createContext<SettingsContextType>({} as any);
const SettingsContextProvider = ({ value, ...props }) => {
	const [settings, setSettings] = useAtom(settingsContext);
	const showSettings = useCallback(() => {
		setSettings((s) => ({ ...s, open: true }));
	}, [setSettings]);
	const closeSettings = useCallback(() => {
		setSettings((s) => ({ ...s, open: false }));
	}, [setSettings]);
	useEffect(() => {
		console.log("settings", settings);
	}, [settings]);
	return <SettingsContext.Provider value={{ ...value, open: settings.open, showSettings, closeSettings }} {...props} />;
};

const useSettings = () => {
	const context = useContext(SettingsContext);
	if (!context) {
		throw new Error("useSettings must be used within a SettingsContextProvider");
	}
	return context;
};

export { SettingsContextProvider, setShowSettings, settingsContext, useSettings };
