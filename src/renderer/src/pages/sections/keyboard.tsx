import { zodResolver } from '@hookform/resolvers/zod'
import { CheckboxButton } from '@renderer/components/ui/checkbox-button'
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage
} from '@renderer/components/ui/form'
import { Input } from '@renderer/components/ui/input'
import { Label } from '@renderer/components/ui/label'
import { Switch } from '@renderer/components/ui/switch'
import { Textarea } from '@renderer/components/ui/textarea'
import { useWindowState } from '@renderer/lib/useWindowState'
import { LucideKeyboard } from 'lucide-react'
import { useEffect } from 'react'
import { useForm } from 'react-hook-form'
import { toast } from 'sonner'
import { z } from 'zod'
import SelectToneBox from '../components/select-tone'
import { useTone } from '../components/tone-context'

const schema = z.object({
  tone: z.string().describe('Keyboard Kind'),
  toneDistinct: z.boolean().default(false),
  toneAsync: z.boolean().default(false),
  exceptProcess: z.string().array().default([])
})
export const meta = {
  title: 'Keyboard',
  icon: LucideKeyboard,
  index: 0
}
const Icon = meta.icon
export default function SettingsTab() {
  const resolver = zodResolver(schema)
  const { selected: tone, setSound, setEnabled } = useTone()
  const form = useForm({
    resolver,
    defaultValues: {
      tone: tone?.key,
      toneEnabled: tone?.enabled !== false,
      toneAsync: '',
      exceptProcess: []
    },
    reValidateMode: 'onChange',
    criteriaMode: 'all'
  })
  const {
    formState: { isSubmitting, isValid, isDirty },
    control
  } = form
  const handleSubmit = form.handleSubmit(async (values) => {
    console.log(values)
  })
  const toneValue = form.watch('tone')
  const toneEnabled = form.watch('toneEnabled')
  useEffect(() => {
    if (toneValue && tone && toneValue !== tone.key)
      setSound(toneValue)
        .then(() => {
          toast.success('Updated selected tone.')
        })
        .catch(() => {
          toast.error('Failed to activate tone set.')
        })
  }, [toneValue])
  useEffect(() => {
    console.log({ toneEnabled })
    if (toneEnabled !== undefined && tone && toneEnabled !== tone.enabled)
      setEnabled(toneEnabled)
        .then(() => {
          toast.success(toneEnabled ? 'Enabled tone sounds' : 'Disabled tone sounds')
        })
        .catch(() => {
          toast.error('Failed to set tone status.')
        })
  }, [toneEnabled])
  const isLoading = isSubmitting
  const { windowState } = useWindowState()
  return (
    <div className="grid gap-8 p-2">
      <div className="grid gap-12">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-2">
            <Icon className="size-5" />
            <h1 className="text-lg font-semibold">Keyboard</h1>
          </div>
        </div>
        <Form {...form}>
          <form onSubmit={handleSubmit} className="flex flex-col gap-y-4">
            <FormField
              name="toneEnabled"
              disabled={isLoading}
              control={control}
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Enable sounds</FormLabel>
                  <FormControl>
                    <CheckboxButton
                      className="flex items-center text-sm"
                      checked={field.value}
                      onCheckedChange={(v) => field.onChange(v)}
                    >
                      <span>Enabled</span>
                    </CheckboxButton>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />

            <FormField
              name="toneDistinct"
              disabled={isLoading}
              control={control}
              render={({ field }) => (
                <FormItem>
                  <FormControl>
                    <Switch
                      className="flex flex-col text-sm"
                      checked={field.value}
                      onCheckedChange={(v) => field.onChange(v)}
                    >
                      <span className="text-semibold">Distinct down identifiers</span>
                      <span>Toggle condition of playing sound while holding a key down for a period of time.</span>
                    </Switch>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <FormField
              name="tone"
              disabled={isLoading}
              control={control}
              render={({ field }) => (
                <FormItem>
                  <FormLabel>Selected tone of your keyboard</FormLabel>
                  <FormControl>
                    <SelectToneBox name={field.name} value={field.value}></SelectToneBox>
                  </FormControl>
                  <FormMessage />
                </FormItem>
              )}
            />
            <div className="flex flex-col space-y-2">
              <FormField
                name="exceptProcess"
                disabled={isLoading}
                control={control}
                render={({ field }) => (
                  <FormItem>
                    <Label>Deactivate if process is running</Label>
                    <FormControl>
                      <Input {...field} />
                    </FormControl>
                    <FormMessage />
                  </FormItem>
                )}
              />
            </div>
            {windowState?.isFocused && (
              <div className="flex flex-col space-y-2">
                <Label>Test your new Keyboard</Label>
                <Textarea
                  className="resize-none"
                  rows={9}
                  maxLength={500}
                  placeholder="Nostrud ipsum laborum mollit amet magna reprehenderit."
                ></Textarea>
              </div>
            )}
          </form>
        </Form>
      </div>
    </div>
  )
}
