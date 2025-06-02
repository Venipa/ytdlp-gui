import ClickableText from "@renderer/components/ui/clickable-text";
import { Tab, TabNavbar } from "@renderer/components/ui/responsive-tabs";
import { Sheet, SheetContent, SheetTrigger } from "@renderer/components/ui/sheet";
import { cn } from "@renderer/lib/utils";
import config, { NodeEnv } from "@shared/config";
import { atom, useAtom } from "jotai";
import { createElement, HTMLProps, PropsWithChildren, ReactElement, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
type Element = <T extends HTMLProps<HTMLDivElement> = HTMLProps<HTMLDivElement>>(props?: T) => ReactElement<T, any>;
type Module = {
	default: Element;
	meta: {
		title: string;
		icon?: Element;
		index?: number;
	};
};
const SECTIONTABS = import.meta.glob<Module>(`./sections/*.tsx`, { eager: true });
const sectionValues = Object.values(SECTIONTABS).filter((d) => d.default);
const sectionTabs = sectionValues.map(({ meta }, i) => ({ ...meta, index: meta.index !== undefined ? meta.index : i })).sort((a, b) => a.index - b.index);
const getSectionContentByTitle = (title: string) => Object.values(SECTIONTABS).find((d) => d.meta.title === title)?.default;
export default function SettingsDialog({ open: value, children }: PropsWithChildren<{ open?: boolean }>) {
	const [open, setOpen] = useState(() => value ?? false);
	const [openFromCode] = useSettingsDialog();
	const [isLoading, setLoading] = useState(false);
	const [selectedTab, setSelectedTab] = useState<string>(sectionTabs[0].title);
	const selectedContent = useMemo(() => (selectedTab && createElement(getSectionContentByTitle(selectedTab) as any)) || null, [selectedTab]);
	const buildInfo = useMemo(() => `Build: ${config.git.shortHash}`, []);
	useEffect(() => {
		if (value !== undefined) setOpen(!!value);
	}, [value]);
	useEffect(() => {
		if (openFromCode !== undefined) setOpen(!!openFromCode.open);
	}, [openFromCode]);
	return (
		<Sheet
			open={open}
			modal
			onOpenChange={(_open) => {
				if (isLoading && open) return;
				if (!open) {
					setLoading(false);
				}
				setOpen(!open);
			}}>
			<SheetTrigger asChild>{children}</SheetTrigger>
			<SheetContent side='bottom' className='flex flex-col overflow-y-auto mx-auto max-w-5xl xl:max-w-6xl 2xl:max-w-7xl h-[90vh] rounded-t-lg border border-t-muted pb-0'>
				<div className={cn("flex flex-col px-0 h-full")}>
					<div className='grid grid-cols-[148px_1fr] h-full flex-auto -mt-6'>
						<TabNavbar
							defaultTab={selectedTab}
							onValueChange={setSelectedTab}
							orientation='vertical'
							indicatorPosition='right'
							className='h-full bg-background-2 -ml-6 pt-6 pb-6'>
							{sectionTabs.map(({ title, icon: Icon }) => {
								return (
									<Tab value={title!} key={title}>
										<div className='flex items-center flex-row-reverse gap-x-2'>
											{Icon && <Icon className='size-4' />}
											<div>{title}</div>
										</div>
									</Tab>
								);
							})}
							<div className='flex-auto'></div>
							<div className='flex flex-col text-xs text-muted-foreground px-2 items-end'>
								<div>{config.appInfo.name}</div>
								<ClickableText onClick={() => navigator.clipboard.writeText(buildInfo).then(() => toast("Copied build id"))}>{buildInfo}</ClickableText>
								<span>{NodeEnv}</span>
							</div>
						</TabNavbar>
						<div className='px-6 pt-6'>{selectedContent ?? <div className='flex flex-col items-center justify-center h-20'>Nothing here ?.?</div>}</div>
					</div>
				</div>
			</SheetContent>
		</Sheet>
	);
}
type SettingsConfig = {
	open: boolean;
};
const settingsConfig = atom<SettingsConfig>({
	open: false,
});
export function useSettingsDialog() {
	return useAtom(settingsConfig);
}
