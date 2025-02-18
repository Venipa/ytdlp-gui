"use client";

import * as React from "react";

import { Drawer, DrawerClose, DrawerContent, DrawerDescription, DrawerFooter, DrawerHeader, DrawerTitle, DrawerTrigger } from "@/components/ui/drawer";
import { Sheet, SheetClose, SheetContent, SheetDescription, SheetFooter, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import { useMediaQuery } from "@/hooks/use-media-query";
import { cn } from "@/lib/utils";

interface BaseProps {
  children: React.ReactNode;
}

interface RootSheetenzaProps extends BaseProps {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  preventClose?: boolean;
  preventOpen?: boolean;
}

interface SheetenzaProps extends BaseProps {
  className?: string;
  asChild?: true;
}

const desktop = "(min-width: 768px)";

const Sheetenza = ({ children, preventClose, preventOpen, open: overrideOpen, onOpenChange, ...props }: RootSheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const Sheetenza = isDesktop ? Sheet : Drawer;
  const [open, setOpen] = React.useState(false);
  return (
    <Sheetenza
      open={overrideOpen ?? open}
      onOpenChange={
        onOpenChange ??
        (() => {
          if (preventClose && open) return;
          if (preventOpen && !open) return;
          setOpen(!open);
        })
      }
      {...props}
    >
      {children}
    </Sheetenza>
  );
};

const SheetenzaTrigger = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaTrigger = isDesktop ? SheetTrigger : DrawerTrigger;

  return (
    <SheetenzaTrigger className={className} {...props}>
      {children}
    </SheetenzaTrigger>
  );
};

const SheetenzaClose = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaClose = isDesktop ? SheetClose : DrawerClose;

  return (
    <SheetenzaClose className={className} {...props}>
      {children}
    </SheetenzaClose>
  );
};

const SheetenzaContent = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaContent = isDesktop ? SheetContent : DrawerContent;

  return (
    <SheetenzaContent className={className} {...props}>
      {children}
    </SheetenzaContent>
  );
};

const SheetenzaDescription = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaDescription = isDesktop ? SheetDescription : DrawerDescription;

  return (
    <SheetenzaDescription className={className} {...props}>
      {children}
    </SheetenzaDescription>
  );
};

const SheetenzaHeader = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaHeader = isDesktop ? SheetHeader : DrawerHeader;

  return (
    <SheetenzaHeader className={className} {...props}>
      {children}
    </SheetenzaHeader>
  );
};

const SheetenzaTitle = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaTitle = isDesktop ? SheetTitle : DrawerTitle;

  return (
    <SheetenzaTitle className={className} {...props}>
      {children}
    </SheetenzaTitle>
  );
};

const SheetenzaBody = ({ className, children, ...props }: SheetenzaProps) => {
  return (
    <div className={cn("px-4 md:px-0", className)} {...props}>
      {children}
    </div>
  );
};

const SheetenzaFooter = ({ className, children, ...props }: SheetenzaProps) => {
  const isDesktop = useMediaQuery(desktop);
  const SheetenzaFooter = isDesktop ? SheetFooter : DrawerFooter;

  return (
    <SheetenzaFooter className={className} {...props}>
      {children}
    </SheetenzaFooter>
  );
};

export { Sheetenza, SheetenzaBody, SheetenzaClose, SheetenzaContent, SheetenzaDescription, SheetenzaFooter, SheetenzaHeader, SheetenzaTitle, SheetenzaTrigger };

