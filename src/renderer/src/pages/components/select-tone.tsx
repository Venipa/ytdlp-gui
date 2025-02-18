'use client'

import { CheckCircle2, ChevronsUpDown, FileVolumeIcon } from 'lucide-react'
import * as React from 'react'

import { Button } from '@/components/ui/button'
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList
} from '@/components/ui/command'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { cn } from '@/lib/utils'
import { trpc } from '@renderer/lib/trpc-link'
import { useFormContext } from 'react-hook-form'

type Props = {
  name?: string
  onValueChange?: (value?: any) => void
  value?: string
}
export default function SelectToneBox({
  value: defaultValue,
  onValueChange,
  name: fieldName
}: Props) {
  const [open, setOpen] = React.useState(false)
  const [value, setValue] = React.useState(defaultValue)
  const form = useFormContext()
  const [options] = trpc.sounds.getAllSoundSets.useSuspenseQuery()
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <Button
          variant="outline"
          role="combobox"
          aria-expanded={open}
          className="flex gap-2 items-center flex-auto w-full justify-start"
        >
          <FileVolumeIcon />
          <span className="flex-auto text-start">
            {value ? options.find((tone) => tone.key === value)?.caption : 'Select tone...'}
          </span>
          <ChevronsUpDown className="opacity-50" />
        </Button>
      </PopoverTrigger>
      <PopoverContent sideOffset={-64} className="p-0 w-[390px]">
        <Command>
          <CommandInput placeholder="Select tone..." className="h-9" />
          <CommandList>
            <CommandEmpty>No tone found.</CommandEmpty>
            <CommandGroup>
              {options?.map((tone) => (
                <CommandItem
                  key={tone.key}
                  value={tone.key}
                  onSelect={(newValue) => {
                    if (newValue === value) return
                    setValue(newValue)
                    onValueChange?.(newValue)
                    setOpen(false)
                    if (fieldName)
                      form.setValue(fieldName, newValue, {
                        shouldDirty: true,
                        shouldTouch: true,
                        shouldValidate: true
                      })
                    console.log({ newValue })
                  }}
                >
                  {tone.caption}
                  <CheckCircle2
                    className={cn(
                      'ml-auto',
                      value === tone.key ? 'opacity-100 cursor-pointer' : 'opacity-0'
                    )}
                  />
                </CommandItem>
              ))}
            </CommandGroup>
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  )
}
