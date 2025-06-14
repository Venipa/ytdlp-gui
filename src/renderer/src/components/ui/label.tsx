"use client";

import * as LabelPrimitive from "@radix-ui/react-label";
import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";

import { cn } from "@renderer/lib/utils";

const labelVariants = cva("text-xs font-medium leading-none peer-disabled:cursor-not-allowed peer-disabled:opacity-70 select-none text-muted-foreground pb-0 mb-0");

const Label = React.forwardRef<React.ElementRef<typeof LabelPrimitive.Root>, React.ComponentPropsWithoutRef<typeof LabelPrimitive.Root> & VariantProps<typeof labelVariants>>(
	({ className, ...props }, ref) => <LabelPrimitive.Root ref={ref} className={cn(labelVariants(), className)} {...props} />,
);
Label.displayName = LabelPrimitive.Root.displayName;

export { Label };
