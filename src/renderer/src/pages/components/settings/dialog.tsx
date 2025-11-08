import ButtonLoading from "@renderer/components/ui/ButtonLoading";
import { Button } from "@renderer/components/ui/button";
import { Tab, TabNavbar } from "@renderer/components/ui/responsive-tabs";
import { Sheet, SheetContent } from "@renderer/components/ui/sheet";
import { cn } from "@renderer/lib/utils";
import { logger } from "@shared/logger";
import { XIcon } from "lucide-react";
import { motion } from "motion/react";
import { HTMLProps, PropsWithChildren, ReactElement, createElement, useMemo, useState } from "react";
import { useApp } from "../app-context";
import { useSettings } from "./context";
import { SettingsFormProvider, useSettingsForm } from "./form";
import { PageContextProvider, usePageContext } from "./page-context";
import { SectionMeta } from "./utils";
type Element = <T extends HTMLProps<HTMLDivElement> = HTMLProps<HTMLDivElement>>(props?: T) => ReactElement<T, any>;
type Module = {
	default: Element;
	meta: SectionMeta;
};
const SETTINGS_SECTION_TABS = import.meta.glob<Module>(`./sections/*.tsx`, { eager: true });
const sectionValues = Object.values(SETTINGS_SECTION_TABS).filter((d) => d.default);
const sectionTabs = sectionValues.map(({ meta }, i) => ({ ...meta, index: meta.index !== undefined ? meta.index : i })).sort((a, b) => a.index - b.index);

function SettingsDialog({ children }: PropsWithChildren) {
	const { open, showSettings, closeSettings } = useSettings();
	const [isLoading, setLoading] = useState(false);
	const [selectedTab, setSelectedTab] = useState<string>(sectionTabs[0].title);
	const selectedContent = useMemo(() => {
		const section = selectedTab && sectionValues.find((d) => d.meta.title === selectedTab);
		console.log("selectedTab", selectedTab);
		if (section) {
			const content = createElement(section.default as any, { meta: section.meta });
			return content;
		}
		return null;
	}, [selectedTab]);
	const { current } = usePageContext();
	return (
		<Sheet
			open={open}
			onOpenChange={(_open) => {
				if (!_open) {
					closeSettings();
				} else {
					showSettings();
				}
			}}
			modal>
			<SheetContent side='bottom' closeButton={false} className='flex flex-col mx-auto max-w-5xl xl:max-w-6xl 2xl:max-w-7xl h-[70vh] rounded-t-lg border border-t-muted pb-0'>
				<div className={cn("flex flex-col px-0 h-full")}>
					<div className='grid grid-rows-[80px_1fr] h-full flex-auto -mt-6 -mx-6'>
						<TabNavbar
							defaultTab={selectedTab}
							onValueChange={setSelectedTab}
							orientation='horizontal'
							indicatorPosition='bottom'
							className='bg-background-2 items-end px-4 flex-auto'>
							{sectionTabs.map(({ title, icon: Icon, description }) => {
								const richTab = title && description && true;
								if (richTab) {
									return (
										<Tab value={title!} key={title}>
											<div className='flex items-start gap-x-2'>
												{Icon && <Icon className='size-4 mt-1' />}
												<div className='flex flex-col gap-y-0'>
													<div>{title}</div>
													{description && <div className='text-xs text-muted-foreground line-clamp-2'>{description}</div>}
												</div>
											</div>
										</Tab>
									);
								}
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
							<Button variant='ghost' size='icon-circle' className='size-6 p-1 self-start' onClick={closeSettings}>
								<XIcon className='size-full' />
							</Button>
						</TabNavbar>
						<div className='px-6 pt-6'>{selectedContent ?? <div className='flex flex-col items-center justify-center h-20'>Nothing here ?.?</div>}</div>
					</div>
				</div>
				<DialogFormActionControls />
			</SheetContent>
		</Sheet>
	);
}
function DialogFormActionControls() {
	const form = useSettingsForm();
	const { closeSettings } = useSettings();
	const { formState, reset } = form;
	const { updateSettings } = useApp();
	const onSubmit = form.handleSubmit(async (data) => {
		const newSettings = await updateSettings(data);
		reset(newSettings);
	});
	const shouldShowControls = formState.isDirty || formState.isSubmitting;
	const isLoading = formState.isSubmitting;
	const isDisabled = !formState.isDirty || formState.isSubmitting || !formState.isValid || formState.isLoading;
	const values = form.watch();
	const animationState = {
		show: {
			opacity: 1,
			y: 0,
			scale: 1,
		},
		hide: {
			opacity: 0,
			y: 30,
			scale: 0.98,
		},
	};
	logger.child("DialogFormActionControls").debug("formState", { formState, values, isValid: formState.isValid });
	return (
		<motion.div
			initial={animationState.hide}
			animate={shouldShowControls ? "show" : "hide"}
			variants={animationState}
			transition={{ type: "spring", bounce: 0.18, duration: 0.36 }}
			className='fixed left-0 right-0 bottom-0 z-50 w-full px-0 py-0'
			style={{ pointerEvents: "auto" }}>
			<div className='mx-auto w-full max-w-5xl xl:max-w-6xl 2xl:max-w-7xl px-4 pb-5'>
				<div className='flex items-center justify-between gap-x-4 bg-background border shadow-lg rounded-xl px-6 py-3'>
					<span className='text-sm text-muted-foreground'>You have unsaved changes</span>
					<div className='flex gap-x-2'>
						<Button type='button' variant='outline' disabled={isLoading} onClick={() => reset()}>
							Cancel
						</Button>
						<ButtonLoading
							type='button'
							loading={isLoading}
							disabled={isDisabled}
							fixWidth
							onClickWithLoading={(ev) => {
								ev.preventDefault();
								return onSubmit(ev as any).then(() => {
									closeSettings();
								});
							}}>
							Save Changes
						</ButtonLoading>
					</div>
				</div>
			</div>
		</motion.div>
	);
}
export default function SettingsDialogWrapper({ children }: PropsWithChildren) {
	return (
		<SettingsFormProvider>
			<PageContextProvider>
				{children}
				<SettingsDialog />
			</PageContextProvider>
		</SettingsFormProvider>
	);
}
