import { FormControl, FormDescription, FormField, FormItem, FormLabel } from "@renderer/components/ui/form";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@renderer/components/ui/select";
import { Control } from "react-hook-form";

interface SelectOptionProps {
	control: Control<any>;
	name: string;
	label?: string;
	description?: string;
	options: { value: string; label: string }[];
}

export function SelectOption({ control, name, label, description, options }: SelectOptionProps) {
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem>
					{label && <FormLabel>{label}</FormLabel>}
					<Select onValueChange={field.onChange} defaultValue={field.value}>
						<FormControl>
							<SelectTrigger>
								<SelectValue placeholder='Select an option' />
							</SelectTrigger>
						</FormControl>
						<SelectContent>
							{options.map((option) => (
								<SelectItem key={option.value} value={option.value}>
									{option.label}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
					{description && <FormDescription>{description}</FormDescription>}
				</FormItem>
			)}
		/>
	);
}
