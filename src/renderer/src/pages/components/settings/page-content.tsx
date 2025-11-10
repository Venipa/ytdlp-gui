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
	return (
		<div className='flex flex-col flex-auto gap-8 pt-6'>
			{/* <div className='flex flex-col gap-4 border-b border-muted pb-4 -mx-6 pt-4 z-10 px-6 sticky top-0 bg-background'>
				<div className='flex items-center gap-2'>
					{Icon && <Icon className='size-4' />}
					<h1 className='text-lg font-medium'>{title}</h1>
				</div>
				{description && <p className='text-sm text-muted-foreground'>{description}</p>}
			</div> */}
			{children}
		</div>
	);
}
