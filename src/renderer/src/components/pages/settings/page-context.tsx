import { Dispatch, PropsWithChildren, SetStateAction, createContext, useContext, useState } from "react";
type PageContextProviderType = {
	current?: {
		title: string;
		description?: string;
		tabId?: string;
	};
	setCurrent: Dispatch<SetStateAction<PageContextProviderType["current"]>>;
};
const pageContext = createContext<PageContextProviderType>({
	current: undefined,
} as any);

const PageContextProvider = ({ children }: PropsWithChildren) => {
	const [current, setCurrent] = useState<PageContextProviderType["current"]>(undefined);
	return <pageContext.Provider value={{ current, setCurrent }}>{children}</pageContext.Provider>;
};

const usePageContext = () => {
	const context = useContext(pageContext);
	if (!context) {
		throw new Error("usePageContext must be used within a PageContextProvider");
	}
	return context;
};
export { PageContextProvider, usePageContext };
