import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

// One button language for the whole app. Variants map the old dialects:
// primary = old button.primary (teal), ink = old .flow-dark (near-black
// pill action), paper = old .flow-light (cream on dark heroes), outline
// and ghost = secondary actions, destructive = old .flow-mini wipe/erase
// grown up (coral), link = inline text actions.
const buttonVariants = cva(
  "inline-flex cursor-pointer items-center justify-center gap-2 rounded-xl font-semibold whitespace-nowrap transition-colors select-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-pine disabled:pointer-events-none disabled:opacity-45 [&_svg]:size-4 [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-pine text-white hover:bg-pine-deep",
        ink: "bg-ink text-cream hover:bg-coal",
        paper: "bg-cream text-ink hover:bg-parchment",
        outline:
          "border border-ink/20 bg-transparent text-inherit hover:bg-ink/5",
        ghost: "bg-transparent text-inherit hover:bg-ink/8",
        destructive: "bg-ember text-white hover:bg-ember-deep",
        link: "text-pine underline-offset-4 hover:underline",
      },
      size: {
        xs: "h-7 rounded-lg px-2.5 text-xs",
        sm: "h-8 rounded-lg px-3 text-[13px]",
        md: "h-10 px-5 text-sm",
        lg: "h-12 rounded-xl px-7 text-[15px]",
        icon: "size-9",
      },
    },
    defaultVariants: { variant: "primary", size: "md" },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
  loading?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant,
      size,
      asChild = false,
      loading = false,
      disabled,
      children,
      ...props
    },
    ref,
  ) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        ref={ref}
        disabled={disabled || loading}
        aria-busy={loading || undefined}
        className={cn(buttonVariants({ variant, size, className }))}
        {...props}
      >
        {loading && (
          <svg
            aria-hidden="true"
            viewBox="0 0 16 16"
            className="animate-spin"
            fill="none"
          >
            <circle
              cx="8"
              cy="8"
              r="6.5"
              stroke="currentColor"
              strokeOpacity="0.25"
              strokeWidth="2"
            />
            <path
              d="M14.5 8a6.5 6.5 0 0 0-6.5-6.5"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            />
          </svg>
        )}
        {children}
      </Comp>
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
