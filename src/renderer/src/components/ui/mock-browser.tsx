import { cn } from "@renderer/lib/utils";

export const BrowserComponent: React.FC<{ children?: React.ReactNode; className?: string; title?: string }> = ({ className, children, title }) => (
	<div
		className={cn(
			"relative text-sm dark:text-neutral-400 text-neutral-950 border dark:border-neutral-800 rounded-lg w-full h-[400px] dark:shadow-none shadow-lg shadow-gray-200 dark:dots-neutral-800 dots-gray-300 dark:bg-neutral-950 bg-white",
			className,
		)}>
		<div className={"border-b border-inherit flex items-center justify-between w-full py-2 px-4 bg-inherit rounded-t-lg"}>
			<div className={"flex gap-2"}>
				<div className={"w-3 h-3 rounded-full dark:bg-neutral-800 bg-neutral-300"} />
				<div className={"w-3 h-3 rounded-full dark:bg-neutral-800 bg-neutral-300"} />
				<div className={"w-3 h-3 rounded-full dark:bg-neutral-800 bg-neutral-300"} />
			</div>
			<div className={"border border-inherit rounded-md flex gap-2 px-1.5 py-1 font-sans w-fit min-w-1/3"}>
				<svg
					xmlns='http://www.w3.org/2000/svg'
					width='24'
					height='24'
					viewBox='0 0 24 24'
					fill='none'
					strokeWidth='2'
					className={"dark:stroke-neutral-700 stroke-neutral-300 w-4 max-w-5"}
					strokeLinecap='round'
					strokeLinejoin='round'>
					<rect width='18' height='11' x='3' y='11' rx='2' ry='2' />
					<path d='M7 11V7a5 5 0 0 1 10 0v4' />
				</svg>
				<span className={"text-sm flex items-center justify-center"}>{title ?? "yourwebsite.com/admin/sales"}</span>
			</div>
			<div />
		</div>
		<div className={"w-full h-full absolute top-0 left-0 pt-12"}>{children}</div>
	</div>
);
