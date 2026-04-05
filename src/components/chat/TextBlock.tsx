import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import { StreamingCursor } from './StreamingCursor';

interface TextBlockProps {
  text: string;
  isStreaming: boolean;
}

export function TextBlock({ text, isStreaming }: TextBlockProps) {
  return (
    <div className="text-sm text-primary leading-relaxed chat-markdown">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{text}</ReactMarkdown>
      {isStreaming && (
        <div className="mt-2">
          <StreamingCursor />
        </div>
      )}
    </div>
  );
}
