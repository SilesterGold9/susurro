import * as React from "react";
import { cn } from "@/lib/utils";

export interface InputProps
  extends React.InputHTMLAttributes<HTMLInputElement> {
  mono?: boolean;
}

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, mono, ...props }, ref) => (
    <input
      type={type}
      ref={ref}
      className={cn(
        "flex h-9 w-full rounded-lg border border-ink/20 bg-card px-3 py-1 text-[13px] text-ink shadow-none transition-colors outline-none placeholder:text-ink/40 focus-visible:border-pine disabled:cursor-not-allowed disabled:opacity-45",
        mono && "font-mono text-xs",
        className,
      )}
      {...props}
    />
  ),
);
Input.displayName = "Input";

export { Input };
