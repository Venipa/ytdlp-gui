import ButtonLoading from "@renderer/components/ui/ButtonLoading";
import { Button } from "@renderer/components/ui/button";
import { Tab, TabNavbar } from "@renderer/components/ui/responsive-tabs";
import { Sheet, SheetContent } from "@renderer/components/ui/sheet";
import { QTooltip } from "@renderer/components/ui/tooltip";
import { cn } from "@renderer/lib/utils";
import { XIcon } from "lucide-react";
import { motion } from "motion/react";
import { HTMLProps, PropsWithChildren, ReactElement, createElement, useMemo, useState } from "react";
import { useApp } from "../app-context";
import { useSettings } from "./context";
import { SettingsFormProvider, useSettingsForm } from "./form";
import { PageContextProvider } from "./page-context";
import { SectionMeta } from "./utils";
type Element = <T extends HTMLProps<HTMLDivElement> = HTMLProps<HTMLDivElement>>(props?: T) => ReactElement<T, any>;
type Module = {
	default: Element;
	meta: SectionMeta;
};
const SETTINGS_SECTION_TABS = import.meta.glob<Module>(`./sections/*.tsx`, { eager: true });
const sectionValues = Object.values(SETTINGS_SECTION_TABS).filter((d) => d.default);
const sectionTabs = sectionValues.map(({ meta }, i) => ({ ...meta, index: meta.index !== undefined ? meta.index : i })).sort((a, b) => a.index - b.index);

function SettingsDialog() {
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
	const form = useSettingsForm();
	const closeIsDisabled = form.formState.isDirty || form.formState.isSubmitting || form.formState.isLoading;
	return (
		<Sheet
			open={open}
			onOpenChange={(_open) => {
				if (!_open) {
					if (closeIsDisabled) return;
					closeSettings();
				} else {
					showSettings();
				}
			}}
			modal>
			<SheetContent
				side='right'
				closeButton={false}
				className='flex flex-col my-auto min-h-[500px] h-[85vh] min-w-[620px] w-[860px] max-w-[100vw] rounded-l-lg border border-t-muted pb-0'>
				<div className={cn("flex flex-col px-0 h-full relative")}>
					<div className='grid grid-cols-[80px_1fr] h-full flex-auto -mt-6 -mx-6'>
						<TabNavbar defaultTab={selectedTab} onValueChange={setSelectedTab} orientation='vertical' indicatorPosition='right' className='pt-8'>
							{sectionTabs.map(({ title, icon: Icon, description }) => {
								return (
									<Tab value={title!} key={title} className='w-full py-4 justify-center items-center flex flex-col gap-2'>
										<div className='flex items-center justify-center gap-x-2'>{Icon && <Icon className='size-4' />}</div>
										<div className='text-xs text-muted-foreground'>{title}</div>
									</Tab>
								);
							})}
							<div className='flex flex-col items-center justify-end mt-auto'>
								<QTooltip content='Close settings'>
									<Button variant='outline' size='default' className='rounded-full p-2 size-10' onClick={closeSettings} disabled={closeIsDisabled}>
										<XIcon className='size-4' />
									</Button>
								</QTooltip>
							</div>
						</TabNavbar>

						<div className='px-6 overflow-auto h-full pb-24'>
							{selectedContent ?? <div className='flex flex-col items-center justify-center h-20'>Nothing here ?.?</div>}
						</div>
						<DialogFormActionControls />
					</div>
				</div>
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
	const animationState = {
		show: {
			opacity: 1,
			y: 0,
			scale: 1,
		},
		hide: {
			opacity: 0,
			y: 10,
			scale: 0.98,
		},
	};
	return (
		<motion.div
			initial={animationState.hide}
			animate={shouldShowControls ? "show" : "hide"}
			variants={animationState}
			transition={{ type: "spring", bounce: 0.18, duration: 0.36 }}
			className='absolute left-12 right-0 bottom-0 z-50 px-0 py-0'
			style={{ pointerEvents: "auto" }}>
			<div className='mx-auto w-full max-w-5xl xl:max-w-6xl 2xl:max-w-7xl px-4 pb-5'>
				<div className='flex items-center justify-between gap-x-4 bg-background border shadow-lg rounded-xl px-6 py-3'>
					<span className='text-sm text-muted-foreground'>You have unsaved changes</span>
					<div className='flex gap-x-2'>
						<Button
							type='button'
							variant='outline'
							disabled={isLoading}
							onClick={() => {
								reset();
							}}>
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
