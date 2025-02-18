import { useState } from 'react'

export default function ProUpgradeTab() {
  const [state] = useState(() => window.license)
  return (
    <div className="grid gap-8 p-2">
      <div className="grid gap-12">
        <div className="flex flex-col gap-6">
          Activated
        </div>
      </div>
    </div>
  )
}
