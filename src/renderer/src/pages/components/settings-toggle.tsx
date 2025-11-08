import { FormControl, FormField, FormItem, FormMessage } from "@renderer/components/ui/form";
import { SwitchButton } from "@renderer/components/ui/switch-button";
import { useSettingsForm } from "@renderer/pages/components/settings/form";
import { PropsWithChildren } from "react";

type SettingsToggleProps = PropsWithChildren<{
	name: string;
	onChange?: (value: boolean) => void;
	disabled?: boolean;
}>;
export default function SettingsToggle({ name: key, children, ...props }: SettingsToggleProps) {
	const form = useSettingsForm();
	const isDisabled = props.disabled || form.formState.isSubmitting;
	return (
		<FormField
			control={form.control}
			name={key as any}
			render={({ field }) => (
				<FormItem>
					<FormControl>
						<SwitchButton checked={field.value} onCheckedChange={field.onChange} disabled={isDisabled}>
							{children}
						</SwitchButton>
					</FormControl>
					<FormMessage />
				</FormItem>
			)}
		/>
	);
}
