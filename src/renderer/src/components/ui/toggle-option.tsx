import { FormControl, FormDescription, FormField, FormItem, FormLabel } from "@renderer/components/ui/form";
import { Switch } from "@renderer/components/ui/switch";
import { Control } from "react-hook-form";

interface ToggleOptionProps {
	control: Control<any>;
	name: string;
	label?: string;
	description?: string;
}

export function ToggleOption({ control, name, label, description }: ToggleOptionProps) {
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className='flex items-center justify-between'>
					<div className='space-y-0.5'>
						{label && <FormLabel>{label}</FormLabel>}
						{description && <FormDescription>{description}</FormDescription>}
					</div>
					<FormControl>
						<Switch checked={field.value} onCheckedChange={field.onChange} />
					</FormControl>
				</FormItem>
			)}
		/>
	);
}
