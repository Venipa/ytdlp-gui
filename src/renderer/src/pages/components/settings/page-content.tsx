import { ComponentType, PropsWithChildren, useEffect } from "react";
import { usePageContext } from "./page-context";

type PageContentProps = PropsWithChildren<{
	icon?: ComponentType<{ className?: string }>;
	title: string;
	description?: string;
	tabId?: string;
}>;
export default function PageContent({ icon: Icon, title, description, tabId, children }: PageContentProps) {
	const { current, setCurrent } = usePageContext();
	useEffect(() => {
		setCurrent({ title, description, tabId });
	}, [title, description, tabId, setCurrent]);
	return <div className='flex flex-col gap-8'>{children}</div>;
}
