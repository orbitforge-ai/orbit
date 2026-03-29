import { useState, useRef, useCallback } from "react";
import { Send, Paperclip, X, Image as ImageIcon, FileText } from "lucide-react";
import { ContentBlock } from "../../types";

interface ChatInputProps {
  onSend: (content: ContentBlock[]) => void;
  disabled?: boolean;
}

interface Attachment {
  id: string;
  type: "image" | "document";
  name: string;
  mediaType: string;
  data: string; // base64 for images, text content for documents
}

let attachId = 0;

export function ChatInput({ onSend, disabled }: ChatInputProps) {
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleSend = useCallback(() => {
    const trimmed = text.trim();
    if (!trimmed && attachments.length === 0) return;

    const blocks: ContentBlock[] = [];

    // Add image attachments
    for (const att of attachments) {
      if (att.type === "image") {
        blocks.push({ type: "image", media_type: att.mediaType, data: att.data });
      } else {
        blocks.push({ type: "text", text: `[File: ${att.name}]\n${att.data}` });
      }
    }

    // Add text
    if (trimmed) {
      blocks.push({ type: "text", text: trimmed });
    }

    onSend(blocks);
    setText("");
    setAttachments([]);

    // Reset textarea height
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, [text, attachments, onSend]);

  function handleKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSend();
    }
  }

  function handleTextareaInput(e: React.ChangeEvent<HTMLTextAreaElement>) {
    setText(e.target.value);
    // Auto-resize
    const el = e.target;
    el.style.height = "auto";
    el.style.height = Math.min(el.scrollHeight, 200) + "px";
  }

  async function handleFileSelect(e: React.ChangeEvent<HTMLInputElement>) {
    const files = e.target.files;
    if (!files) return;

    for (const file of Array.from(files)) {
      const isImage = file.type.startsWith("image/");

      if (isImage) {
        const base64 = await fileToBase64(file);
        setAttachments((prev) => [
          ...prev,
          {
            id: `att-${++attachId}`,
            type: "image",
            name: file.name,
            mediaType: file.type,
            data: base64,
          },
        ]);
      } else {
        // Read as text
        const text = await file.text();
        setAttachments((prev) => [
          ...prev,
          {
            id: `att-${++attachId}`,
            type: "document",
            name: file.name,
            mediaType: file.type,
            data: text,
          },
        ]);
      }
    }

    // Reset input
    e.target.value = "";
  }

  function removeAttachment(id: string) {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  }

  const canSend = !disabled && (text.trim().length > 0 || attachments.length > 0);

  return (
    <div className="border-t border-[#2a2d3e] bg-[#13151e]">
      {/* Attachment previews */}
      {attachments.length > 0 && (
        <div className="flex gap-2 px-4 pt-3 flex-wrap">
          {attachments.map((att) => (
            <div
              key={att.id}
              className="flex items-center gap-2 px-2.5 py-1.5 rounded-lg bg-[#1a1d27] border border-[#2a2d3e] text-xs"
            >
              {att.type === "image" ? (
                <ImageIcon size={12} className="text-[#818cf8]" />
              ) : (
                <FileText size={12} className="text-[#f59e0b]" />
              )}
              <span className="text-[#94a3b8] max-w-[120px] truncate">{att.name}</span>
              <button
                onClick={() => removeAttachment(att.id)}
                className="text-[#64748b] hover:text-white"
              >
                <X size={10} />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Input row */}
      <div className="flex items-end gap-2 p-3">
        <button
          onClick={() => fileInputRef.current?.click()}
          disabled={disabled}
          className="p-2 rounded-lg text-[#64748b] hover:text-white hover:bg-[#1a1d27] disabled:opacity-50 transition-colors shrink-0 mb-0.5"
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
          disabled={disabled}
          placeholder="Type a message..."
          rows={1}
          className="flex-1 px-3 py-2 rounded-xl bg-[#0f1117] border border-[#2a2d3e] text-white text-sm resize-none focus:outline-none focus:border-[#6366f1] disabled:opacity-50 placeholder:text-[#4a4d6e]"
          style={{ maxHeight: 200 }}
        />

        <button
          onClick={handleSend}
          disabled={!canSend}
          className="p-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-30 disabled:hover:bg-[#6366f1] text-white transition-colors shrink-0 mb-0.5"
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
      // Strip the data:...;base64, prefix
      const base64 = result.split(",")[1] || result;
      resolve(base64);
    };
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
}
