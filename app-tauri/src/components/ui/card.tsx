import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cn } from "@/lib/utils";

// Layout shells stay .flow-card in CSS; this Card is for interactive and
// content cards that need selected/disabled states the CSS cannot express.
const Card = React.forwardRef<
  HTMLDivElement,
  React.HTMLAttributes<HTMLDivElement> & { selected?: boolean; asChild?: boolean }
>(({ className, selected, asChild, ...props }, ref) => {
  const Comp = asChild ? Slot : "div";
  return (
    <Comp
      ref={ref}
      data-selected={selected || undefined}
      className={cn(
        "rounded-[18px] border border-ink/10 bg-card p-5 text-ink",
        "data-[selected]:border-2 data-[selected]:border-pine",
        className,
      )}
      {...props}
    />
  );
});
Card.displayName = "Card";

const CardTitle = React.forwardRef<
  HTMLHeadingElement,
  React.HTMLAttributes<HTMLHeadingElement>
>(({ className, ...props }, ref) => (
  <h3
    ref={ref}
    className={cn("mb-3 font-serif text-[22px] font-medium", className)}
    {...props}
  />
));
CardTitle.displayName = "CardTitle";

const CardDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p
    ref={ref}
    className={cn("text-[13px] leading-relaxed text-ink/65", className)}
    {...props}
  />
));
CardDescription.displayName = "CardDescription";

export { Card, CardTitle, CardDescription };
