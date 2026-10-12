import { Button as BaseButton } from "@base-ui/react/button";
import type { ComponentProps } from "react";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

const variants: Record<Variant, string> = {
  primary: "bg-ink text-on-ink shadow-chip hover:bg-ink-soft",
  secondary: "bg-page text-ink shadow-raised hover:bg-sheet",
  ghost: "text-ink-soft hover:bg-sand",
  danger: "bg-page text-exception shadow-raised hover:bg-exception-tint",
};

const sizes: Record<Size, string> = {
  sm: "h-8 px-2.5 text-[13px] gap-1.5",
  md: "h-9 px-3.5 text-sm gap-2",
};

export function buttonClass(variant: Variant = "secondary", size: Size = "md", extra = ""): string {
  return `inline-flex shrink-0 items-center justify-center rounded-md font-medium whitespace-nowrap transition-colors select-none disabled:pointer-events-none disabled:opacity-50 data-[disabled]:pointer-events-none data-[disabled]:opacity-50 ${variants[variant]} ${sizes[size]} ${extra}`;
}

export function Button({
  variant = "secondary",
  size = "md",
  className = "",
  type = "button",
  ...props
}: ComponentProps<typeof BaseButton> & { variant?: Variant; size?: Size }) {
  return <BaseButton type={type} className={buttonClass(variant, size, typeof className === "string" ? className : "")} {...props} />;
}
