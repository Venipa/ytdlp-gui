'use client'
import { AnimatePresence, motion } from "motion/react"
import * as React from 'react'

export function RotateWords({
  words = [],
  height = 20,
  className,
  delay = 3000
}: {
  words: string[]
  height?: number
  className?: string
  delay?: number
}) {
  const [index, setIndex] = React.useState(0)

  React.useEffect(() => {
    const interval = setInterval(() => {
      setIndex((prevIndex) => (prevIndex + 1) % words.length)
    }, delay)
    // Clean up interval on unmount
    return () => clearInterval(interval)
  }, [])
  return (
    <AnimatePresence mode="wait">
      <motion.p
        key={words[index]}
        initial={({ opacity: 0, y: -height })}
        animate={{ opacity: 1, y: 0 }}
        exit={{ opacity: 0, y: height }}
        transition={{ duration: 0.35 }}
        {...{ className }}
      >
        {words[index]}
      </motion.p>
    </AnimatePresence>
  )
}
