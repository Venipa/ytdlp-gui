import { forwardRef } from 'react'
import { BsCommand, BsWindows } from 'react-icons/bs'

export const Kbd = forwardRef<
  HTMLElement,
  React.HTMLAttributes<HTMLElement> & {
    showCommand?: 'win' | 'mac' | undefined | true
  }
>(({ className, children: label, showCommand, ...props }, ref) => {
  return (
    <kbd
      ref={ref}
      className="pointer-events-none inline-flex h-5 select-none items-center gap-1 rounded border border-muted px-1.5 font-mono text-[10px] font-medium text-muted-foreground opacity-100"
      {...props}
    >
      {showCommand && <span className="mr-1 [&_svg]:size-[12px]">{showCommand === "win" ? <BsWindows /> : <BsCommand />}</span>}
      <span className='uppercase'>{label}</span>
    </kbd>
  )
})
