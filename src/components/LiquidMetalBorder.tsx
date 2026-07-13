import { component$, Slot } from "@builder.io/qwik";

export const LiquidMetalBorder = component$<{
  class?: string;
  borderRadius?: string;
  theme?: 'light' | 'dark';
  style?: Record<string, string>;
}>(({ class: className, borderRadius = "9999px", style }) => {
  return (
    <div
      class={`liquid-metal-border ${className || ""}`}
      style={{ borderRadius, ...style }}
    >
      <div
        class="liquid-metal-border-shader"
        style={{ borderRadius }}
      />
      <div
        class="liquid-metal-border-inner"
        style={{ borderRadius }}
      >
        <Slot />
      </div>
    </div>
  );
});
