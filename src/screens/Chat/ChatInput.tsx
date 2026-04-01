import { useState, useRef, useCallback, useEffect } from 'react';
import { Send, Paperclip, X, Image as ImageIcon, FileText } from 'lucide-react';
import { ContentBlock } from '../../types';

interface ChatInputProps {
  onSend: (content: ContentBlock[]) => Promise<void> | void;
  disabled?: boolean;
  contextGauge?: React.ReactNode;
  textValue?: string;
  onTextChange?: (text: string) => void;
}

interface Attachment {
  id: string;
  type: 'image' | 'document';
  name: string;
  mediaType: string;
  data: string;
}

let attachId = 0;

export function ChatInput({
  onSend,
  disabled,
  contextGauge,
  textValue,
  onTextChange,
}: ChatInputProps) {
  const [internalText, setInternalText] = useState('');
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [sending, setSending] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const isControlled = textValue !== undefined;
  const text = isControlled ? textValue ?? '' : internalText;

  const setText = useCallback(
    (value: string) => {
      if (isControlled) {
        onTextChange?.(value);
        return;
      }
      setInternalText(value);
    },
    [isControlled, onTextChange]
  );

  useEffect(() => {
    if (!textareaRef.current) return;
    textareaRef.current.style.height = 'auto';
    textareaRef.current.style.height = `${Math.min(textareaRef.current.scrollHeight, 200)}px`;
  }, [text]);

  const handleSend = useCallback(() => {
    if (sending) return;

    const trimmed = text.trim();
    if (!trimmed && attachments.length === 0) return;

    const blocks: ContentBlock[] = [];

    for (const att of attachments) {
      if (att.type === 'image') {
        blocks.push({ type: 'image', media_type: att.mediaType, data: att.data });
      } else {
        blocks.push({ type: 'text', text: `[File: ${att.name}]\n${att.data}` });
      }
    }

    if (trimmed) {
      blocks.push({ type: 'text', text: trimmed });
    }

    const run = async () => {
      setSending(true);
      try {
        await Promise.resolve(onSend(blocks));
        setText('');
        setAttachments([]);
        if (textareaRef.current) {
          textareaRef.current.style.height = 'auto';
        }
      } catch {
        return;
      } finally {
        setSending(false);
      }
    };

    void run();
  }, [attachments, onSend, sending, setText, text]);

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function handleTextareaInput(e: React.ChangeEvent<HTMLTextAreaElement>) {
    setText(e.target.value);
    const el = e.target;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }

  async function handleFileSelect(e: React.ChangeEvent<HTMLInputElement>) {
    const files = e.target.files;
    if (!files) return;

    for (const file of Array.from(files)) {
      const isImage = file.type.startsWith('image/');

      if (isImage) {
        const base64 = await fileToBase64(file);
        setAttachments((prev) => [
          ...prev,
          {
            id: `att-${++attachId}`,
            type: 'image',
            name: file.name,
            mediaType: file.type,
            data: base64,
          },
        ]);
      } else {
        const fileText = await file.text();
        setAttachments((prev) => [
          ...prev,
          {
            id: `att-${++attachId}`,
            type: 'document',
            name: file.name,
            mediaType: file.type,
            data: fileText,
          },
        ]);
      }
    }

    e.target.value = '';
  }

  function removeAttachment(id: string) {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  }

  const inputDisabled = disabled || sending;
  const canSend = !inputDisabled && (text.trim().length > 0 || attachments.length > 0);

  return (
    <div className="border-t border-edge bg-panel">
      {attachments.length > 0 && (
        <div className="flex gap-2 px-4 pt-3 flex-wrap">
          {attachments.map((att) => (
            <div
              key={att.id}
              className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-surface border border-edge text-xs"
            >
              {att.type === 'image' ? (
                <ImageIcon size={12} className="text-accent-hover" />
              ) : (
                <FileText size={12} className="text-warning" />
              )}
              <span className="text-secondary max-w-[120px] truncate">{att.name}</span>
              <button
                onClick={() => removeAttachment(att.id)}
                className="text-muted hover:text-white"
              >
                <X size={10} />
              </button>
            </div>
          ))}
        </div>
      )}

      <div className="flex items-end gap-2 p-3">
        <button
          onClick={() => fileInputRef.current?.click()}
          disabled={inputDisabled}
          className="p-2 rounded-lg text-muted hover:text-white hover:bg-surface disabled:opacity-50 transition-colors shrink-0 mb-0.5"
        >
          <Paperclip size={16} />
        </button>
        <input
          ref={fileInputRef}
          type="file"
          multiple
          accept="image/*,.txt,.md,.json,.csv,.log,.xml,.yaml,.yml,.toml,.js,.ts,.py,.rs,.go,.html,.css"
          onChange={handleFileSelect}
          className="hidden"
        />

        <textarea
          ref={textareaRef}
          value={text}
          onChange={handleTextareaInput}
          onKeyDown={handleKeyDown}
          disabled={inputDisabled}
          placeholder="Type a message..."
          rows={1}
          className="flex-1 px-3 py-2 rounded-xl bg-background border border-edge text-white text-sm resize-none focus:outline-none focus:border-accent disabled:opacity-50 placeholder:text-border-hover"
          style={{ maxHeight: 200 }}
        />

        {contextGauge && <div className="shrink-0 mb-0.5">{contextGauge}</div>}

        <button
          onClick={handleSend}
          disabled={!canSend}
          className="p-2 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-30 disabled:hover:bg-accent text-white transition-colors shrink-0 mb-0.5"
        >
          <Send size={16} />
        </button>
      </div>
    </div>
  );
}

function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result as string;
      const base64 = result.split(',')[1] || result;
      resolve(base64);
    };
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
}
