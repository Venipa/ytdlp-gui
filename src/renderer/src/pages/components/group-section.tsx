import { PropsWithChildren, ReactNode } from "react";
interface GroupSectionProps {
	title: string | ReactNode;
	titleRight?: ReactNode;
}
export default function GroupSection({ title, titleRight, children }: PropsWithChildren<GroupSectionProps>) {
	return (
		<div className='grid grid-cols-[18px_1fr] gap-2 group/parent'>
			<div className='grid grid-rows-[18px_1fr] gap-2 justify-items-center select-none'>
				<div className='size-1.5 bg-muted-foreground rounded-full self-end transition-colors duration-200 ease-out group-hover/parent:bg-primary'></div>
				<div className='w-px bg-muted transition-colors ease-in-out duration-200 origin-center group-hover/parent:bg-primary mask-gradient rounded-b-full'></div>
			</div>
			<div className='grid grid-rows-[40px_1fr] pb-2'>
				<div className='flex gap-3'>
					<h1 className='text-lg font-bold tracking-wider select-none'>{title}</h1>
					{titleRight}
				</div>
				<div className='flex flex-col'>{children}</div>
			</div>
		</div>
	);
}
