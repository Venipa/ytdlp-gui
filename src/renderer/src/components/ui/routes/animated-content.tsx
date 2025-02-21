import { HTMLMotionProps, motion } from "motion/react"
import { forwardRef, ReactNode } from 'react'

export const AnimatedContent = forwardRef<HTMLDivElement, HTMLMotionProps<'div'>>(
  ({ ...props }, ref) => {
    return (
      <motion.div
        initial={{ scale: 0.96, opacity: 0 }}
        animate={{ scale: 1, opacity: 1 }}
        exit={{ scale: 0.94, opacity: 0 }}
        transition={{ duration: 0.225, ease: 'circInOut' }}
        ref={ref}
        {...props}
      />
    )
  }
)
export function withAnimatedTransition(Comp: () => ReactNode) {
  return (props: any) => (
    <AnimatedContent>
      <Comp {...props} />
    </AnimatedContent>
  )
}

export const Appear = forwardRef<HTMLDivElement, HTMLMotionProps<'div'>>(({ ...props }, ref) => {
  return (
    <motion.div
      initial={{ scale: 0.97, opacity: 0, transformOrigin: 'center' }}
      animate={{ scale: 1, opacity: 1 }}
      transition={{ duration: 0.275, ease: 'circInOut' }}
      ref={ref}
      {...props}
    />
  )
})
