import { zodResolver } from '@hookform/resolvers/zod'
import ButtonLoading from '@renderer/components/ui/ButtonLoading'
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle
} from '@renderer/components/ui/card'
import ClickableText from '@renderer/components/ui/clickable-text'
import {
  Form,
  FormControl,
  FormField,
  FormItem,
  FormLabel,
  FormMessage
} from '@renderer/components/ui/form'
import { Input } from '@renderer/components/ui/input'
import SuspenseLoader from '@renderer/components/ui/suspense-loader'
import { useWindowSize, useWindowTitle } from '@renderer/lib/useWindowState'
import { UserRound } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useForm } from 'react-hook-form'
import { toast } from 'sonner'
import { z } from 'zod'

export default function LicensePage() {
  useWindowTitle('')
  useWindowSize({ height: 540, width: 400 })
  const [schema] = useState(() =>
    z.object({
      email: z.string().email('Invalid email format.'),
      license: z.string().min(16)
    })
  )
  const resolver = zodResolver(schema)
  const form = useForm({
    resolver,
    defaultValues: {
      email: '',
      license: ''
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
    await state
      .activateLicense({ email: values.email, license: values.license })
      .finally(() => {
        state.loading = false
      })
      .catch(([errCode, data]) => {
        if (data?.licenseError) {
          toast.error(data.licenseError)
        }
      })
  })
  const [state] = useState(() => window.license)
  const isLoading = isSubmitting
  useEffect(() => {
    if (state.isReady) state.init()
  }, [state.isReady])
  return (
    <Form {...form}>
      {state.loading && <div className='fixed inset-0 bg-background/50 backdrop-blur z-10'><SuspenseLoader /></div>}
      <form onSubmit={handleSubmit} className="flex flex-col items-center justify-center h-full">
        <div className="container">
          <div className="flex flex-col gap-4">
            <Card className="mx-auto w-full max-w-md border-none shadow-none">
              <CardHeader className="items-center">
                <UserRound className="size-10 rounded-full bg-primary text-primary-foreground p-2.5 shadow-lg mb-2" />
                <CardTitle className="text-xl">Activate your License</CardTitle>
                <CardDescription>Enter your information and license key</CardDescription>
              </CardHeader>
              <CardContent>
                <div className="grid gap-4">
                  <FormField
                    name="email"
                    disabled={isLoading}
                    control={control}
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>E-Mail Address</FormLabel>
                        <FormControl>
                          <Input type="email" {...field} placeholder="Your email address" />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <FormField
                    name="license"
                    disabled={isLoading}
                    control={control}
                    render={({ field }) => (
                      <FormItem>
                        <FormLabel>License key</FormLabel>
                        <FormControl>
                          <Input type="password" {...field} placeholder="Your license key" />
                        </FormControl>
                        <FormMessage />
                      </FormItem>
                    )}
                  />
                  <ButtonLoading
                    loading={isLoading}
                    disabled={isLoading || state.activated}
                    type="submit"
                    className="w-full"
                  >
                    Activate
                  </ButtonLoading>
                </div>
              </CardContent>
            </Card>
            <div className="mx-auto flex flex-col items-center justify-center gap-1 text-sm leading-4">
              <p>{`Don't have a license yet?`}</p>
              <div className="flex items-baseline gap-2">
                <ClickableText disabled={isLoading} asChild className="cursor-pointer" size={'lg'}>
                  <a href="https://checkout.anystack.sh/wklack" target='_blank'>Buy now</a>
                </ClickableText>
              </div>
            </div>
          </div>
        </div>
      </form>
    </Form>
  )
}
