import { component$, useSignal, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { LuChevronDown } from '@qwikest/icons/lucide';
import type { SelectedAiModel } from '../types';

const GENERIC_AI_PLACEHOLDER_IMG = '/generic-ai-placeholder.svg';

interface AiSelectorProps {
  selectedAi: SelectedAiModel;
  setSelectedAi$: QRL<(ai: SelectedAiModel) => void>;
  dynamicModelOptions: SelectedAiModel[];
  getDisplayImageUrl$: QRL<(model: SelectedAiModel | undefined) => string | undefined>;
  currentSelectedOptionInListbox: SelectedAiModel | undefined;
  isLoading: boolean;
  positionClass: string;
}

export const AiSelector = component$<AiSelectorProps>((props) => {
  const aiDropdownOpen = useSignal(false);
  const optionImageUrls = useSignal<Record<string, string | undefined>>({});

  const handleImageError = $((e: Event) => {
    const target = e.target as HTMLImageElement;
    target.src = GENERIC_AI_PLACEHOLDER_IMG;
  });

  // Reactive: recompute dropdown option image URLs when AI options change
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => props.dynamicModelOptions);
    const urls: Record<string, string | undefined> = {};
    Promise.all(
      props.dynamicModelOptions.map(async (opt) => {
        const url = await props.getDisplayImageUrl$(opt);
        urls[opt.id] = url;
      })
    ).then(() => {
      optionImageUrls.value = { ...urls };
    });
  });

  return (
    <div class="relative">
      <button
        type="button"
        onClick$={() => { if (!props.isLoading) aiDropdownOpen.value = !aiDropdownOpen.value; }}
        disabled={props.isLoading}
        class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-1.5 pl-2 sm:pl-3 pr-10 text-left text-[var(--text-primary)] focus:outline-none text-xs sm:text-base gradient-border-target"
      >
        <span class="flex items-center">
          <img
            src={props.currentSelectedOptionInListbox?.imageUrl || GENERIC_AI_PLACEHOLDER_IMG}
            alt={props.currentSelectedOptionInListbox?.label || 'AI thumbnail'}
            class="h-5 w-5 sm:h-7 sm:w-7 rounded-full mr-2 object-cover flex-shrink-0"
            width={28}
            height={28}
            onError$={handleImageError}
          />
          <span class="block truncate">{props.currentSelectedOptionInListbox?.label || 'Select AI'}</span>
        </span>
        <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2">
          <LuChevronDown class="h-5 w-5 text-[var(--text-muted)]" aria-hidden="true" />
        </span>
      </button>
      {aiDropdownOpen.value && (
        <>
          <div class="fixed inset-0 z-[5]" onClick$={() => { aiDropdownOpen.value = false; }} />
          <div class={`${props.positionClass} absolute max-h-60 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none z-10 text-xs sm:text-sm`}>
            {props.dynamicModelOptions.map((option) => {
              const displayOptionImageUrl = optionImageUrls.value[option.id];
              const isUserAiLoading = !displayOptionImageUrl || displayOptionImageUrl === GENERIC_AI_PLACEHOLDER_IMG;
              return (
                <div
                  key={option.id}
                  onClick$={() => {
                    props.setSelectedAi$(option);
                    aiDropdownOpen.value = false;
                  }}
                  class="relative cursor-default select-none py-2 pl-2 sm:pl-3 pr-4 text-[var(--text-dropdown)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-dropdown-active)]"
                >
                  <div class="flex items-center">
                    {isUserAiLoading ? (
                      <div class="h-5 w-5 sm:h-7 sm:w-7 rounded-md mr-2 flex-shrink-0 animate-pulse bg-gray-300 dark:bg-zinc-700" />
                    ) : (
                      <img
                        src={displayOptionImageUrl || GENERIC_AI_PLACEHOLDER_IMG}
                        alt={option.label}
                        class="h-5 w-5 sm:h-7 sm:w-7 rounded-full mr-2 object-cover flex-shrink-0"
                        width={28}
                        height={28}
                        onError$={handleImageError}
                      />
                    )}
                    <span class={`block truncate ${props.selectedAi.id === option.id ? 'font-medium' : 'font-normal'}`}>
                      {option.label}
                    </span>
                  </div>
                </div>
              );
            })}
          </div>
        </>
      )}
    </div>
  );
});
