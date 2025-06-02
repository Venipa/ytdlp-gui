import { Button } from "@renderer/components/ui/button";
import { Sheet, SheetContent, SheetTrigger } from "@renderer/components/ui/sheet";
import { Textarea } from "@renderer/components/ui/textarea";
import { random } from "lodash";
import { CookieIcon, LucideCookie, LucideEdit2, LucideImport } from "lucide-react";
import { useMemo, useState } from "react";
import { YTDLCookie } from "ytdlp-gui/types";

export const meta = {
	title: "Cookies",
	icon: LucideCookie,
	index: 5,
	show: import.meta.env.DEV,
};
const Icon = meta.icon;
type CookiesTabItemProps = { cookie: YTDLCookie };
export function CookiesTabItem({ cookie }: CookiesTabItemProps) {
	const amountOfCookies = useMemo(() => random(1, 1000), []);
	const amountOfDomains = useMemo(() => random(1, 100), []);
	const [open, setOpen] = useState(false);
	const Icon = useMemo(() => (cookie.type === "imported" ? LucideImport : CookieIcon), [cookie.type]);
	return (
		<Sheet open={open} onOpenChange={setOpen} modal>
			<SheetTrigger>
				<div className='h-8 border border-border rounded hover:bg-muted truncate grid grid-cols-[20px_1fr] gap-6 text-sm items-center px-4 cursor-pointer select-none group/cookie'>
					<Icon className='size-4 self-center' />
					<div className='flex'>
						<div className='grid grid-cols-[100px_10px_100px] flex-shrink-0 truncate items-center gap-2 justify-items-end'>
							<div className='truncate'>{amountOfCookies} cookies</div>
							<div className='w-px h-4 bg-muted group-hover/cookie:bg-muted-foreground'></div>
							<div className='truncate'>{amountOfDomains} domains</div>
						</div>
						<div className='flex items-center ml-auto opacity-20 group-hover/cookie:opacity-100'>
							<LucideEdit2 fill='currentColor' className='size-4' />
						</div>
					</div>
				</div>
			</SheetTrigger>
			<SheetContent className='flex flex-col gap-6 sm:max-w-[85vw]' side={"right"}>
				<h1 className='font-bold tracking-wide'>Cookies</h1>
				<Textarea defaultValue={cookie.cookie} className='-mx-6 rounded-none flex w-auto border-x-0 flex-auto resize-none whitespace-pre' plain></Textarea>
				<div className='flex justify-end'>
					<Button onClick={() => setOpen(!open)}>Save</Button>
				</div>
			</SheetContent>
		</Sheet>
	);
}
export default function CookiesTab() {
	return (
		<div className='grid gap-8 p-2 h-full'>
			<div className='grid gap-6 pt-10'>
				<div className='flex items-start justify-between'>
					<div className='flex items-center gap-2'>
						<Icon className='size-5' />
						<h1 className='text-lg font-semibold'>Cookies</h1>
					</div>
					<Button variant={"outline"}>Import from file</Button>
				</div>
				<div className='flex flex-col gap-2'>
					{new Array(100).fill(0).map((_, i) => (
						<CookiesTabItem key={i} cookie={{ cookie: "todo", type: i % 5 ? "imported" : "cookie", domain: ".youtube.com" }} />
					))}
				</div>
			</div>
		</div>
	);
}
