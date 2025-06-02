import * as React from "react";
import type { StepperProps } from "./types";

interface StepperContextValue extends StepperProps {
	clickable?: boolean;
	isError?: boolean;
	isLoading?: boolean;
	isVertical?: boolean;
	stepCount?: number;
	expandVerticalSteps?: boolean;
	activeStep: number;
	initialStep: number;
}

type StepperContextProviderProps = {
	value: Omit<StepperContextValue, "activeStep">;
	children: React.ReactNode;
};

const StepperContext = React.createContext<
	StepperContextValue & {
		nextStep: () => void;
		prevStep: () => void;
		resetSteps: () => void;
		setStep: (step: number) => void;
		setLoading: (value: boolean) => void;
	}
>({
	steps: [],
	activeStep: 0,
	initialStep: 0,
	onStepperFinish: () => {},
	nextStep: () => {},
	prevStep: () => {},
	resetSteps: () => {},
	setStep: () => {},
	setLoading: () => {},
});

const StepperProvider = ({ value, children }: StepperContextProviderProps) => {
	const isError = value.state === "error";
	const isLoading = value.state === "loading";

	const [activeStep, setActiveStep] = React.useState(value.initialStep);

	const nextStep = () => {
		if (value.stepCount === activeStep + 1) value.onStepperFinish?.(activeStep, setStep);
		setActiveStep((prev) => prev + 1);
	};

	const prevStep = () => {
		setActiveStep((prev) => prev - 1);
	};

	const resetSteps = () => {
		setActiveStep(value.initialStep);
	};

	const setStep = (step: number) => {
		setActiveStep(step);
	};

	const setLoading = (loading: boolean) => (value.state = loading ? "loading" : undefined);

	return (
		<StepperContext.Provider
			value={{
				...value,
				isError,
				isLoading,
				activeStep,
				nextStep,
				prevStep,
				resetSteps,
				setStep,
				setLoading,
			}}>
			{children}
		</StepperContext.Provider>
	);
};

export { StepperContext, StepperProvider };
