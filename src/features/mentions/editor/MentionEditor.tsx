import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import Document from '@tiptap/extension-document';
import Paragraph from '@tiptap/extension-paragraph';
import Text from '@tiptap/extension-text';
import HardBreak from '@tiptap/extension-hard-break';
import History from '@tiptap/extension-history';
import Placeholder from '@tiptap/extension-placeholder';
import { MentionNode } from './MentionNode';
import { MentionSuggestion, SuggestionHandlers, SuggestionRenderState } from './suggestion';
import { InputMentionPopup, InputMentionPopupHandle } from './InputMentionPopup';
import { docToString, stringToDoc } from './serialize';
import { MentionToken, PickerContext } from '../types';

export interface MentionEditorHandle {
  focus(): void;
  clear(): void;
}

interface Props {
  value: string;
  onChange(next: string): void;
  onSubmit(): void;
  placeholder?: string;
  disabled?: boolean;
  pickerContext: PickerContext | null;
  maxHeight?: number;
}

const DEFAULT_POPUP_STATE: SuggestionRenderState = {
  open: false,
  trigger: '@',
  query: '',
  anchorRect: null,
  submit: () => {},
};

export const MentionEditor = forwardRef<MentionEditorHandle, Props>(function MentionEditor(
  { value, onChange, onSubmit, placeholder, disabled, pickerContext, maxHeight = 200 },
  ref,
) {
  const [popup, setPopup] = useState<SuggestionRenderState>(DEFAULT_POPUP_STATE);
  const popupRef = useRef<InputMentionPopupHandle>(null);

  const handlersRef = useRef<SuggestionHandlers>({
    onOpen: () => {},
    onUpdate: () => {},
    onClose: () => {},
    onKeyDown: () => false,
  });
  handlersRef.current = {
    onOpen: (state) => setPopup(state),
    onUpdate: (state) => setPopup(state),
    onClose: () => setPopup((prev) => ({ ...prev, open: false })),
    onKeyDown: (event) => popupRef.current?.handleKey(event) ?? false,
  };

  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onSubmitRef = useRef(onSubmit);
  onSubmitRef.current = onSubmit;

  const editor = useEditor({
    extensions: [
      Document,
      Paragraph,
      Text,
      HardBreak,
      History,
      Placeholder.configure({ placeholder: placeholder ?? '' }),
      MentionNode,
      MentionSuggestion.configure({ handlersRef }),
    ],
    content: stringToDoc(value),
    editable: !disabled,
    editorProps: {
      attributes: {
        class:
          'tiptap-chat-input w-full px-3 py-2 rounded-xl bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent disabled:opacity-50',
      },
      handleKeyDown(_view, event) {
        if (event.key === 'Enter' && !event.shiftKey) {
          event.preventDefault();
          onSubmitRef.current();
          return true;
        }
        return false;
      },
    },
    onUpdate({ editor }) {
      onChangeRef.current(docToString(editor.getJSON()));
    },
  });

  useEffect(() => {
    if (!editor) return;
    const current = docToString(editor.getJSON());
    if (current !== value) {
      editor.commands.setContent(stringToDoc(value), { emitUpdate: false });
    }
  }, [value, editor]);

  useEffect(() => {
    if (!editor) return;
    editor.setEditable(!disabled);
  }, [disabled, editor]);

  useImperativeHandle(
    ref,
    () => ({
      focus() {
        editor?.commands.focus('end');
      },
      clear() {
        editor?.commands.setContent(stringToDoc(''), { emitUpdate: false });
      },
    }),
    [editor],
  );

  const handleSelect = useCallback(
    (token: MentionToken) => {
      popup.submit(token);
    },
    [popup],
  );

  return (
    <div className="flex-1 min-w-0">
      <div className="overflow-y-auto" style={{ maxHeight }}>
        <EditorContent editor={editor} />
      </div>
      <InputMentionPopup
        ref={popupRef}
        open={popup.open}
        trigger={popup.trigger}
        query={popup.query}
        anchorRect={popup.anchorRect}
        pickerContext={pickerContext}
        onSelect={handleSelect}
      />
    </div>
  );
});
