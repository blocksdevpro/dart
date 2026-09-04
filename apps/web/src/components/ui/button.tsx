import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-2 whitespace-nowrap rounded-[10px] text-sm font-semibold outline-none transition-all disabled:pointer-events-none disabled:opacity-45 focus-visible:ring-2 focus-visible:ring-[var(--signal)]/50 [&_svg]:pointer-events-none [&_svg]:size-4",
  {
    variants: {
      variant: {
        default:
          "bg-[var(--signal)] text-[var(--signal-ink)] shadow-[0_1px_0_rgba(255,255,255,.18)_inset] hover:bg-[var(--signal-strong)]",
        secondary:
          "border border-[var(--line)] bg-[var(--panel-raised)] text-[var(--ink)] hover:border-[var(--line-strong)] hover:bg-[var(--panel-hover)]",
        ghost:
          "text-[var(--ink-muted)] hover:bg-[var(--panel-hover)] hover:text-[var(--ink)]",
        danger:
          "border border-red-400/25 bg-red-500/10 text-red-300 hover:bg-red-500/18",
        icon:
          "border border-[var(--line)] bg-[var(--panel-raised)] text-[var(--ink-muted)] hover:border-[var(--line-strong)] hover:text-[var(--ink)]",
      },
      size: {
        default: "h-10 px-4 py-2",
        sm: "h-8 rounded-lg px-3 text-xs",
        lg: "h-11 px-5",
        icon: "size-10 p-0",
        "icon-sm": "size-8 rounded-lg p-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant,
  size,
  asChild = false,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
  }) {
  const Comp = asChild ? Slot : "button";

  return (
    <Comp
      data-slot="button"
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  );
}

export { Button, buttonVariants };
