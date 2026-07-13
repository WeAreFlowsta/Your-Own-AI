import { component$, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { toSvg } from 'jdenticon';

interface UserAvatarProps {
  userId: string | undefined | null;
  imageUrl?: string | null;
  size?: number;
  class?: string; // Allow passing classes for border, shadow etc.
}

export default component$<UserAvatarProps>(({
  userId,
  imageUrl,
  size = 32,
  class: className = '',
}) => {
  const svgRef = useSignal<HTMLDivElement>();

  useVisibleTask$(({ track }) => {
    track(() => imageUrl);
    track(() => userId);
    track(() => size);

    if (!imageUrl && svgRef.value && userId) {
      const config = {
        padding: 0,
      };
      const svgString = toSvg(userId, size, config);
      svgRef.value.innerHTML = svgString;
    } else if (svgRef.value) {
      // Clear SVG if image URL is present or user ID is null/undefined
      svgRef.value.innerHTML = '';
    }
  });

  // Base classes for the outer container: Shape, overflow, display behavior
  // Let passed class handle borders, shadows, specific avatar system classes
  const baseClasses = `rounded-full overflow-hidden inline-block align-middle`;

  // These effects now apply to both default and uploaded avatars
  const hoverEffectClasses = 'transition-all hover:ring-2 hover:ring-offset-2 hover:ring-[var(--border-primary-hover)] hover:ring-offset-[var(--bg-header-footer)]';

  return (
    <div
      class={`${baseClasses} ${hoverEffectClasses} ${className}`}
      style={{ width: `${size}px`, height: `${size}px`, verticalAlign: 'middle' }}
      aria-label={imageUrl ? "User profile picture" : "User identicon"}
    >
      {imageUrl ? (
        <img
          src={imageUrl}
          alt="Profile"
          width={size}
          height={size}
          class="object-cover w-full h-full"
        />
      ) : (
        <div
          ref={svgRef}
          class="w-full h-full bg-transparent hover:bg-[var(--bg-button-primary-hover)] border border-transparent hover:border-[var(--border-primary-hover)] transition-colors flex items-center justify-center"
        >
          {/* Jdenticon SVG will be rendered here and should fill this div */}
        </div>
      )}
    </div>
  );
});
