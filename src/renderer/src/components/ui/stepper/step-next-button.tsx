import { Button } from '../button'
import { useStepper } from './use-stepper'

export default function StepNext({ label }: { label?: string }) {
  const stepper = useStepper()
  const handleNext = () => stepper.nextStep()
  return <Button onClick={handleNext}>{label ?? 'Next'}</Button>
}
