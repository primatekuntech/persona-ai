import * as React from "react";
import { cn } from "@/lib/utils";

export interface SelectProps extends React.SelectHTMLAttributes<HTMLSelectElement> {
  label?: string;
  error?: string;
}

const Select = React.forwardRef<HTMLSelectElement, SelectProps>(
  ({ className, label, error, children, ...props }, ref) => (
    <div className="flex flex-col gap-1">
      {label && (
        <label className="text-sm font-medium text-[var(--text)]">{label}</label>
      )}
      <select
        ref={ref}
        className={cn(
          "h-9 w-full rounded border border-[var(--border)] bg-[var(--bg)] px-3 text-sm text-[var(--text)]",
          "focus:outline-none focus:ring-2 focus:ring-[var(--accent)] focus:ring-offset-1",
          "disabled:opacity-50 disabled:cursor-not-allowed",
          error && "border-[var(--danger)]",
          className,
        )}
        {...props}
      >
        {children}
      </select>
      {error && <p className="text-xs text-[var(--danger)]">{error}</p>}
    </div>
  ),
);
Select.displayName = "Select";

export { Select };
