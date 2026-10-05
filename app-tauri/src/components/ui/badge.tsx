import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

// Provider/state badges. Colors carry exactly one meaning each:
// pine healthy/fast, ember degraded/error, stone idle, sky cloud.
const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] font-semibold whitespace-nowrap",
  {
    variants: {
      variant: {
        local: "bg-pine/15 text-pine",
        cloud: "bg-sky-500/15 text-sky-700",
        degraded: "bg-ember/15 text-ember-deep",
        idle: "bg-ink/8 text-ink/70",
        destructive: "bg-ember text-white",
        outline: "border border-ink/20 text-inherit",
      },
    },
    defaultVariants: { variant: "idle" },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {}

function Badge({ className, variant, ...props }: BadgeProps) {
  return (
    <span className={cn(badgeVariants({ variant }), className)} {...props} />
  );
}

export { Badge, badgeVariants };
