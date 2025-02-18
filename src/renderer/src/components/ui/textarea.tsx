import * as React from "react"

import { cn } from "@renderer/lib/utils"

export interface TextareaProps
  extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {}

const Textarea = React.forwardRef<HTMLTextAreaElement, TextareaProps>(
  ({ className, ...props }, ref) => {
    return (
      <textarea
        className={cn(
          "flex min-h-[60px] w-full rounded-lg border border-input bg-transparent px-3 py-2 text-sm ring-0 ring-offset-0 ring-offset-secondary ring-secondary/0 transition-all placeholder:text-muted-foreground read-only:focus-visible:ring-0 read-only:focus-visible:border-input focus-visible:ring-4 focus-visible:ring-secondary focus-visible:border-muted-foreground disabled:cursor-not-allowed disabled:opacity-50 focus-visible:outline-none focus:outline-none",
          className
        )}
        ref={ref}
        {...props}
      />
    )
  }
)
Textarea.displayName = "Textarea"

export { Textarea }

