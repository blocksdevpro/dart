import * as React from "react";

import { cn } from "@/lib/utils";

export function Badge({
  className,
  tone = "neutral",
  ...props
}: React.ComponentProps<"span"> & {
  tone?: "neutral" | "success" | "warning" | "danger" | "signal";
}) {
  return (
    <span
      className={cn(
        "inline-flex h-6 items-center gap-1.5 rounded-full border px-2.5 text-[11px] font-semibold tracking-[-0.01em]",
        tone === "neutral" &&
          "border-[var(--line)] bg-[var(--panel-raised)] text-[var(--ink-muted)]",
        tone === "success" &&
          "border-emerald-400/20 bg-emerald-400/10 text-emerald-300",
        tone === "warning" &&
          "border-amber-400/20 bg-amber-400/10 text-amber-300",
        tone === "danger" && "border-red-400/20 bg-red-400/10 text-red-300",
        tone === "signal" &&
          "border-[var(--signal)]/25 bg-[var(--signal)]/10 text-[var(--signal)]",
        className,
      )}
      {...props}
    />
  );
}
