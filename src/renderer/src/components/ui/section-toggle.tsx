import { FormControl, FormDescription, FormField, FormItem, FormLabel } from "@renderer/components/ui/form";
import { Switch } from "@renderer/components/ui/switch";
import { Control } from "react-hook-form";

interface SectionToggleProps {
	control: Control<any>;
	name: string;
	label?: string;
	description?: string;
	children: React.ReactNode;
}

export function SectionToggle({ control, name, label, description, children }: SectionToggleProps) {
	return (
		<FormField
			control={control}
			name={name}
			render={({ field }) => (
				<FormItem className='space-y-4'>
					<div className='flex items-center justify-between'>
						<div className='space-y-0.5'>
							{label && <FormLabel className='text-secondary-foreground'>{label}</FormLabel>}
							{description && <FormDescription>{description}</FormDescription>}
						</div>
						<FormControl>
							<Switch checked={field.value} onCheckedChange={field.onChange} />
						</FormControl>
					</div>
					{field.value && <div className='pl-4 border-l-2 border-muted'>{children}</div>}
				</FormItem>
			)}
		/>
	);
}
