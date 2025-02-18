'use client'

import { Spinner } from './spinner'

export default function SuspenseLoader() {
  return (
    <div className="flex flex-col h-full items-center justify-center">
      <Spinner size={'sm'} />
    </div>
  )
}
