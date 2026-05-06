import { useSettingsForm } from "@renderer/components/pages/settings/form";
import { FormControl, FormDescription, FormField, FormItem, FormMessage } from "@renderer/components/ui/form";
import { Input, InputProps } from "@renderer/components/ui/input";
import { cn } from "@renderer/lib/ui/utils";
import { logger } from "@shared/logger";
import { forwardRef, useCallback, useId, useMemo } from "react";
type SettingsInputProps<TKey extends string> = {
	name: TKey;
	onChange?: (value: any) => void;
	formatter?: (value: any) => any;
	parser?: (value: any) => any;
	title: any;
	hint?: string | React.ReactNode;
} & Omit<InputProps, "title">;
export default forwardRef<HTMLInputElement, SettingsInputProps<string>>(function SettingsInput(
	{ className, name: key, title: placeholder, hint, ...props },
	ref,
) {
	const form = useSettingsForm();
	const id = useId();
	const formatter = useCallback(
		(value: any) => (props.formatter ? props.formatter(value) : value),
		[props.formatter],
	);
	const parser = useCallback((value: any) => (props.parser ? props.parser(value) : value), [props.parser]);
	const fieldValue = useMemo(
		() => formatter(form.getValues(key as any)),
		[form, key, parser, props.formatter, props.parser],
	);
	return (
		<FormField
			control={form.control}
			name={key as any}
			render={({ field: { value, ...field }, fieldState }) => (
				<>
					<FormItem>
						<FormControl>
							<div className='group relative w-full'>
								<label
									htmlFor={id}
									className='origin-start text-muted-foreground group-focus-within:text-foreground has-[+input:not(:placeholder-shown)]:text-foreground absolute top-1/2 block -translate-y-1/2 cursor-text px-2 text-sm transition-all group-focus-within:pointer-events-none group-focus-within:top-0 group-focus-within:cursor-default group-focus-within:text-xs group-focus-within:font-medium has-[+input:not(:placeholder-shown)]:pointer-events-none has-[+input:not(:placeholder-shown)]:top-0 has-[+input:not(:placeholder-shown)]:cursor-default has-[+input:not(:placeholder-shown)]:text-xs has-[+input:not(:placeholder-shown)]:font-medium'>
									<span className='bg-background inline-flex px-1'>{placeholder}</span>
								</label>
								<Input
									id={id}
									{...field}
									defaultValue={props.defaultValue ?? fieldValue}
									value={formatter(value)}
									onChange={(ev) => {
										const parsedValue = parser(ev.target.value);
										logger.debug("onChange", key, { ev: ev.target.value, parsedValue });
										form.setValue(key as any, parsedValue, {
											shouldDirty: true,
											shouldTouch: true,
											shouldValidate: form.control._options.mode === "onChange",
										});
									}}
									className={cn("dark:bg-background h-12", fieldState.error && "border-destructive")}
								/>
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
