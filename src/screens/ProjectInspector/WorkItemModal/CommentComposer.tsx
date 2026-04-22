import { forwardRef, useCallback, useEffect, useImperativeHandle, useRef, useState } from 'react';
import { EditorContent, useEditor } from '@tiptap/react';
import Document from '@tiptap/extension-document';
import Paragraph from '@tiptap/extension-paragraph';
import Text from '@tiptap/extension-text';
import HardBreak from '@tiptap/extension-hard-break';
import History from '@tiptap/extension-history';
import Placeholder from '@tiptap/extension-placeholder';
import Link from '@tiptap/extension-link';
import { MentionNode } from '../../../features/mentions/editor/MentionNode';
import {
  MentionSuggestion,
  SuggestionHandlers,
  SuggestionRenderState,
} from '../../../features/mentions/editor/suggestion';
import {
  InputMentionPopup,
  InputMentionPopupHandle,
} from '../../../features/mentions/editor/InputMentionPopup';
import { docToString, stringToDoc } from '../../../features/mentions/editor/serialize';
import type { MentionToken, PickerContext } from '../../../features/mentions/types';

export interface CommentComposerHandle {
  focus(): void;
  clear(): void;
}

interface Props {
  value: string;
  onChange: (next: string) => void;
  onSubmit: () => void;
  placeholder?: string;
  disabled?: boolean;
  /** Required for the @-mention popup to know which agent to scope file/skill lookups to. */
  pickerContext: PickerContext | null;
  maxHeight?: number;
}

const DEFAULT_POPUP: SuggestionRenderState = {
  open: false,
  trigger: '@',
  query: '',
  anchorRect: null,
  submit: () => {},
};

/**
 * Multi-line comment composer with @-mentions, reusing the chat
 * MentionNode/MentionSuggestion primitives. Differs from the chat input by
 * allowing bare Enter to insert a newline — only Cmd/Ctrl+Enter submits.
 */
export const CommentComposer = forwardRef<CommentComposerHandle, Props>(function CommentComposer(
  { value, onChange, onSubmit, placeholder, disabled, pickerContext, maxHeight = 180 },
  ref,
) {
  const [popup, setPopup] = useState<SuggestionRenderState>(DEFAULT_POPUP);
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
      Link.configure({ openOnClick: false, autolink: true, linkOnPaste: true }),
      MentionNode,
      MentionSuggestion.configure({ handlersRef }),
    ],
    content: stringToDoc(value),
    editable: !disabled,
    editorProps: {
      attributes: {
        class:
          'tiptap-chat-input w-full px-3 py-2 rounded-lg bg-background/60 border border-edge text-white text-xs focus:outline-none focus:border-accent disabled:opacity-50',
      },
      handleKeyDown(_view, event) {
        if ((event.metaKey || event.ctrlKey) && event.key === 'Enter') {
          event.preventDefault();
          onSubmitRef.current();
          return true;
        }
        return false;
      },
    },
    onUpdate({ editor: ed }) {
      onChangeRef.current(docToString(ed.getJSON()));
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
