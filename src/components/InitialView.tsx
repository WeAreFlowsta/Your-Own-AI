import { component$, type QRL, type Signal } from '@builder.io/qwik';
import { ChatInputBar } from './ChatInputBar';
import { SelectedAiModel, ChatAction, AttachedFile, AttachedImage } from '../types';

interface InitialViewProps {
  input: Signal<string>;
  handleSubmit$: QRL<() => void>;
  isLoading: boolean;
  currentPlaceholder: string;
  selectedAi: SelectedAiModel;
  setSelectedAi$: QRL<(ai: SelectedAiModel) => void>;
  dynamicModelOptions: SelectedAiModel[];
  getDisplayImageUrl$: QRL<(model: SelectedAiModel | undefined) => string | undefined>;
  currentSelectedOptionInListbox: SelectedAiModel | undefined;
  selectedAction: ChatAction;
  setSelectedAction$: QRL<(action: ChatAction) => void>;
  attachedFiles: Signal<AttachedFile[]>;
  attachedImages: Signal<AttachedImage[]>;
  contextWindowSize: number;
  onAttachFiles$: QRL<(paths: string[]) => void>;
  theme: 'light' | 'dark';
}

export default component$<InitialViewProps>((props) => {
  return (
    <div class="w-full max-w-4xl mx-auto">
      <ChatInputBar
        input={props.input}
        handleSubmit$={props.handleSubmit$}
        isLoading={props.isLoading}
        currentPlaceholder={props.currentPlaceholder}
        selectedAi={props.selectedAi}
        setSelectedAi$={props.setSelectedAi$}
        dynamicModelOptions={props.dynamicModelOptions}
        getDisplayImageUrl$={props.getDisplayImageUrl$}
        currentSelectedOptionInListbox={props.currentSelectedOptionInListbox}
        selectedAction={props.selectedAction}
        setSelectedAction$={props.setSelectedAction$}
        attachedFiles={props.attachedFiles}
        attachedImages={props.attachedImages}
        contextWindowSize={props.contextWindowSize}
        onAttachFiles$={props.onAttachFiles$}
        theme={props.theme}
      />
    </div>
  );
});
