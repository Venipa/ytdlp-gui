import { SelectProps } from "@radix-ui/react-select";
import { FormControl, FormDescription, FormField, FormItem, FormMessage } from "@renderer/components/ui/form";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@renderer/components/ui/select";
import { cn } from "@renderer/lib/utils";
import { useSettingsForm } from "@renderer/pages/components/settings/form";
import { PropsWithChildren, createContext, forwardRef, useCallback, useContext, useEffect, useId, useState } from "react";
const settingsSelectContext = createContext<{
	value: string;
	onChange: (value: string) => void;
	options: { value: string; label: string }[];
	pushOption: (option: { value: string; label: string }) => void;
}>({
	value: "",
	onChange: () => {},
	options: [],
	pushOption: () => {},
});
function SettingsSelectProvider({ children }: PropsWithChildren) {
	const [options, setOptions] = useState<{ value: string; label: string }[]>([]);
	const [value, setValue] = useState<string>("");
	const pushOption = useCallback(
		(option: { value: string; label: string }) => {
			setOptions((s) => [...s, option]);
		},
		[options, setOptions],
	);
	const onChange = useCallback(
		(value: string) => {
			setValue(value);
		},
		[setValue],
	);
	return <settingsSelectContext.Provider value={{ value, onChange, options, pushOption }}>{children}</settingsSelectContext.Provider>;
}
function useSettingsSelect() {
	return useContext(settingsSelectContext);
}
export function SettingsSelectItem({ value, label }: { value: string; label: string }) {
	const { onChange, pushOption } = useContext(settingsSelectContext);
	useEffect(() => {
		pushOption({ value, label });
	}, []);
	return (
		<SelectItem value={value} onSelect={() => onChange(value)}>
			{label}
		</SelectItem>
	);
}
type SettingsSelectProps = {
	name: string;
	onChange?: (value: any) => void;
	title: any;
	hint?: string | React.ReactNode;
} & Omit<SelectProps, "title"> &
	React.PropsWithChildren;
export default forwardRef<HTMLSelectElement, SettingsSelectProps>(function SettingsSelect({ name: key, title: placeholder, hint, children, ...props }, ref) {
	const form = useSettingsForm();
	const { options, onChange } = useSettingsSelect();
	const id = useId();
	return (
		<FormField
			control={form.control}
			name={key as any}
			render={({ field, fieldState }) => (
				<>
					<FormItem>
						<FormControl>
							<div className='group relative w-full'>
								<Select value={field.value} onValueChange={field.onChange} {...props}>
									<SelectTrigger className={cn("dark:bg-background h-12", fieldState.error && "border-destructive")}>
										<SelectValue placeholder={placeholder} className={cn("text-sm", (field.value && "text-foreground") || "text-muted")} />
									</SelectTrigger>
									<SelectContent>{children}</SelectContent>
								</Select>
							</div>
						</FormControl>
						<FormMessage />
						{hint && typeof hint === "string" ? (
							<FormDescription className='text-muted-foreground text-xs ml-2 relative flex items-center gap-2'>
								<div className='size-1 bg-muted-foreground rounded-full'></div>
								{hint}
							</FormDescription>
						) : (
							hint
						)}
					</FormItem>
				</>
			)}
		/>
	);
});
