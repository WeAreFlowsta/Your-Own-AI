import { component$, type QRL, Slot } from "@builder.io/qwik";

/**
 * One liquid-metal button, three intents — all share the metal ring + rounded
 * pill shape, differing only by fill/text:
 * - `primary` (default) → full liquid-metal (the header / hero CTA).
 * - `secondary` → muted: bg-matched fill, dimmer ring, muted text.
 * - `danger` → the muted treatment but red text (destructive).
 * All light up to full metal on hover.
 */
export default component$<{
  onClick$?: QRL<() => void>;
  class?: string;
  disabled?: boolean;
  type?: 'button' | 'submit';
  variant?: 'primary' | 'secondary' | 'danger';
  title?: string;
}>(({ onClick$, class: className, disabled = false, type = "button", variant = "primary", title }) => {
  const variantClass =
    variant === "secondary" ? "btn-secondary-metal" :
    variant === "danger" ? "btn-danger-metal" :
    "";

  return (
    <button
      type={type}
      onClick$={onClick$}
      disabled={disabled}
      title={title}
      class={`btn-liquid-metal ${variantClass} ${className || ""}`}
    >
      <div class="shader-border" />
      <div class="shader-inner-fill" />
      <span class="btn-content">
        <Slot />
      </span>
    </button>
  );
});
