/**
 * ThemeAwareLottie - Lottie animation component for status indicators (Qwik version)
 *
 * This component displays Lottie animations for various status states.
 * Uses lottie-web directly instead of lottie-react.
 * The animations use the app's metallic palette for a cohesive design.
 */

import { component$, useSignal, useVisibleTask$, noSerialize } from '@builder.io/qwik';
import lottie from 'lottie-web';

// Import the Lottie animations with metallic palette colors
import thinkingAnimation from '../assets/wired-gradient-426-brain-hover-pinch.json';
import codingAnimation from '../assets/wired-gradient-743-web-code-hover-pinch.json';
import loadingAnimation from '../assets/wired-gradient-1373-load-cargo-hover-pointing.json';

type AnimationType = 'thinking' | 'coding' | 'loading';

interface ThemeAwareLottieProps {
  type: AnimationType;
  theme?: 'light' | 'dark'; // Kept for API compatibility
  size?: number;
  class?: string;
}

export default component$<ThemeAwareLottieProps>(({
  type,
  size = 16,
  class: className = ''
}) => {
  const containerRef = useSignal<HTMLDivElement>();

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const currentType = track(() => type);
    if (!containerRef.value) return;

    // Select the appropriate animation based on type
    const animationData = currentType === 'coding'
      ? codingAnimation
      : currentType === 'loading'
        ? loadingAnimation
        : thinkingAnimation;

    const anim = noSerialize(
      lottie.loadAnimation({
        container: containerRef.value,
        renderer: 'svg',
        loop: true,
        autoplay: true,
        animationData,
        rendererSettings: {
          preserveAspectRatio: 'xMidYMid slice',
        },
      })
    );

    cleanup(() => {
      anim?.destroy();
    });
  });

  return (
    <div
      class={`inline-flex items-center justify-center flex-shrink-0 ${className}`}
      style={{ width: `${size}px`, height: `${size}px` }}
    >
      <div
        ref={containerRef}
        style={{ width: '100%', height: '100%' }}
      />
    </div>
  );
});
