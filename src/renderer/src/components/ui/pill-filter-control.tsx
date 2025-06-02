"use client";

import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Form, FormControl, FormField, FormItem, FormLabel } from "@/components/ui/form";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { zodResolver } from "@hookform/resolvers/zod";
import { ChevronDown, X } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { createContext, useContext, useEffect } from "react";
import { useForm, useFormContext } from "react-hook-form";
import * as z from "zod";

interface Filter {
	id: string;
	label: string;
	category: string;
}

interface FilterContextType {
	filters: Filter[];
	selectedFilters: string[];
	toggleFilter: (filterId: string) => void;
}

const FilterContext = createContext<FilterContextType | undefined>(undefined);

const useFilterContext = () => {
	const context = useContext(FilterContext);
	if (!context) {
		throw new Error("useFilterContext must be used within a FilterProvider");
	}
	return context;
};

interface FilterButtonProps {
	category: string;
	count: number;
	isSelected: boolean;
}

const FilterButton: React.FC<FilterButtonProps> = ({ category, count, isSelected }) => (
	<motion.div
		layout
		initial={false}
		animate={{
			backgroundColor: isSelected ? "hsl(var(--primary))" : "white",
			color: isSelected ? "hsl(var(--primary-foreground))" : "hsl(var(--foreground))",
		}}
		transition={{ duration: 0.2 }}
		className='rounded-full inline-block'>
		<Button variant={isSelected ? "default" : "outline"} className='relative px-4 py-2 h-auto rounded-full transition-none'>
			<span className='flex items-center gap-2 pr-6'>{isSelected ? `${category}: ${count}` : `Select ${category}`}</span>
			<span className='absolute right-2 top-1/2 -translate-y-1/2'>
				<ChevronDown className='h-4 w-4' />
			</span>
		</Button>
	</motion.div>
);

interface FilterOptionProps {
	filter: Filter;
}

const FilterOption: React.FC<FilterOptionProps> = ({ filter }) => {
	const { selectedFilters, toggleFilter } = useFilterContext();
	const isSelected = selectedFilters.includes(filter.id);

	return (
		<div className='flex items-center space-x-2'>
			<Checkbox id={filter.id} checked={isSelected} onCheckedChange={() => toggleFilter(filter.id)} />
			<Label htmlFor={filter.id} className='text-sm leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70'>
				{filter.label}
			</Label>
		</div>
	);
};

interface FilterCategoryProps {
	category: string;
}

const FilterCategory: React.FC<FilterCategoryProps> = ({ category }) => {
	const { filters, selectedFilters } = useFilterContext();
	const categoryFilters = filters.filter((f) => f.category === category);
	const selectedCategoryFilters = selectedFilters.filter((id) => categoryFilters.some((f) => f.id === id));

	return (
		<Popover>
			<PopoverTrigger asChild>
				<FilterButton category={category} count={selectedCategoryFilters.length} isSelected={selectedCategoryFilters.length > 0} />
			</PopoverTrigger>
			<PopoverContent className='w-56 p-4'>
				<div className='space-y-4'>
					{categoryFilters.map((filter) => (
						<FilterOption key={filter.id} filter={filter} />
					))}
				</div>
			</PopoverContent>
		</Popover>
	);
};

interface SelectedFilterProps {
	filter: Filter;
}

const SelectedFilter: React.FC<SelectedFilterProps> = ({ filter }) => {
	const { toggleFilter } = useFilterContext();

	return (
		<motion.div
			layout
			initial={{ scale: 0.8, opacity: 0 }}
			animate={{ scale: 1, opacity: 1 }}
			exit={{ scale: 0.8, opacity: 0 }}
			className='bg-primary text-primary-foreground px-3 py-1 rounded-full flex items-center text-sm'>
			{filter.label}
			<button onClick={() => toggleFilter(filter.id)} className='ml-2 focus:outline-none'>
				<X className='h-4 w-4' />
			</button>
		</motion.div>
	);
};

const FilterList: React.FC = () => {
	const { filters } = useFilterContext();
	const categories = Array.from(new Set(filters.map((f) => f.category)));

	return (
		<div className='flex flex-wrap gap-2 mb-4'>
			{categories.map((category) => (
				<FilterCategory key={category} category={category} />
			))}
		</div>
	);
};

const SelectedFilterList: React.FC = () => {
	const { filters, selectedFilters } = useFilterContext();

	return (
		<AnimatePresence>
			{selectedFilters.length > 0 && (
				<motion.div initial={{ opacity: 0, y: 10 }} animate={{ opacity: 1, y: 0 }} exit={{ opacity: 0, y: 10 }} className='mt-4 flex flex-wrap gap-2'>
					{filters
						.filter((filter) => selectedFilters.includes(filter.id))
						.map((filter) => (
							<SelectedFilter key={filter.id} filter={filter} />
						))}
				</motion.div>
			)}
		</AnimatePresence>
	);
};

interface FilterComponentProps {
	filters: Filter[];
	name: string;
}

const formSchema = z.object({
	selectedFilters: z.array(z.string()),
});

export default function FilterComponent({ filters = [], name }: FilterComponentProps) {
	const parentForm = useFormContext();
	const form = useForm<z.infer<typeof formSchema>>({
		resolver: zodResolver(formSchema),
		defaultValues: {
			selectedFilters: [],
		},
	});

	const toggleFilter = (filterId: string) => {
		const currentFilters = form.getValues().selectedFilters;
		const newFilters = currentFilters.includes(filterId) ? currentFilters.filter((id) => id !== filterId) : [...currentFilters, filterId];
		form.setValue("selectedFilters", newFilters);
	};

	useEffect(() => {
		const subscription = form.watch((value, { name }) => {
			if (name === "selectedFilters") {
				parentForm.setValue(name, value.selectedFilters);
			}
		});
		return () => subscription.unsubscribe();
	}, [form, parentForm, name]);

	if (filters.length === 0) {
		return <div className='p-4'>No filters available</div>;
	}

	return (
		<FilterContext.Provider
			value={{
				filters,
				selectedFilters: form.watch("selectedFilters"),
				toggleFilter,
			}}>
			<Form {...form}>
				<form className='p-4'>
					<FormField
						control={form.control}
						name='selectedFilters'
						render={({}) => (
							<FormItem>
								<FormLabel>Filters</FormLabel>
								<FormControl>
									<FilterList />
								</FormControl>
							</FormItem>
						)}
					/>
					<SelectedFilterList />
				</form>
			</Form>
		</FilterContext.Provider>
	);
}
