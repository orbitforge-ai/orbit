import { Extension } from '@tiptap/core';
import { PluginKey } from '@tiptap/pm/state';
import Suggestion, { SuggestionProps, SuggestionKeyDownProps } from '@tiptap/suggestion';
import { MentionToken } from '../types';

export type SuggestionTrigger = '@' | '#';

export interface SuggestionRenderState {
  open: boolean;
  trigger: SuggestionTrigger;
  query: string;
  anchorRect: (() => DOMRect | null) | null;
  submit: (token: MentionToken) => void;
}

export interface SuggestionHandlers {
  onOpen(state: SuggestionRenderState): void;
  onUpdate(state: SuggestionRenderState): void;
  onClose(): void;
  onKeyDown(event: KeyboardEvent): boolean;
}

export interface SuggestionHandlersRef {
  current: SuggestionHandlers;
}

export interface MentionSuggestionOptions {
  handlersRef: SuggestionHandlersRef | null;
}

function buildSuggestionConfig(trigger: SuggestionTrigger, handlersRef: SuggestionHandlersRef) {
  return {
    char: trigger,
    allowSpaces: false,
    startOfLine: false,
    allowedPrefixes: [' ', '\n', '\t', '(', '['],
    command: ({ editor, range, props }: { editor: any; range: any; props: MentionToken }) => {
      editor
        .chain()
        .focus()
        .insertContentAt(range, [
          { type: 'mention', attrs: { kind: props.kind, label: props.label, payload: props.payload } },
          { type: 'text', text: ' ' },
        ])
        .run();
    },
    items: () => [] as MentionToken[],
    render: () => ({
      onStart: (props: SuggestionProps<MentionToken>) => {
        handlersRef.current.onOpen({
          open: true,
          trigger,
          query: props.query,
          anchorRect: props.clientRect ?? null,
          submit: (token) => props.command(token),
        });
      },
      onUpdate: (props: SuggestionProps<MentionToken>) => {
        handlersRef.current.onUpdate({
          open: true,
          trigger,
          query: props.query,
          anchorRect: props.clientRect ?? null,
          submit: (token) => props.command(token),
        });
      },
      onKeyDown: (props: SuggestionKeyDownProps) => {
        return handlersRef.current.onKeyDown(props.event);
      },
      onExit: () => {
        handlersRef.current.onClose();
      },
    }),
  };
}

export const MentionSuggestion = Extension.create<MentionSuggestionOptions>({
  name: 'mentionSuggestion',

  addOptions() {
    return { handlersRef: null };
  },

  addProseMirrorPlugins() {
    const handlersRef = this.options.handlersRef;
    if (!handlersRef) return [];
    return [
      Suggestion({
        editor: this.editor,
        pluginKey: new PluginKey('mention-suggestion-at'),
        ...buildSuggestionConfig('@', handlersRef),
      }),
      Suggestion({
        editor: this.editor,
        pluginKey: new PluginKey('mention-suggestion-hash'),
        ...buildSuggestionConfig('#', handlersRef),
      }),
    ];
  },
});
