import { useState } from 'react';
import { ChevronRight, CheckCircle, XCircle, Hammer } from 'lucide-react';

interface ToolUseBlockProps {
  name: string;
  input: Record<string, unknown>;
  result?: { content: string; isError: boolean };
}

export function ToolUseBlock({ name, input, result }: ToolUseBlockProps) {
  const [expanded, setExpanded] = useState(false);
  const inputStr = JSON.stringify(input, null, 2);

  return (
    <div className="rounded-lg border border-warning/30 bg-warning/5 overflow-hidden">
      {/* Collapsed header — always visible */}
      <button
        onClick={() => setExpanded(!expanded)}
        className="flex items-center gap-2 px-3 py-2 w-full text-left hover:bg-warning/10 transition-colors"
      >
        <ChevronRight
          size={12}
          className={`text-warning shrink-0 transition-transform ${expanded ? 'rotate-90' : ''}`}
        />
        <Hammer size={12} className="text-warning shrink-0" />
        <span className="text-xs text-muted">Tool Used</span>
        <span className="text-xs font-medium text-warning">{name}</span>
        {result && !result.isError && (
          <CheckCircle size={11} className="text-emerald-400 ml-auto shrink-0" />
        )}
        {result && result.isError && (
          <XCircle size={11} className="text-red-400 ml-auto shrink-0" />
        )}
      </button>

      {/* Expanded details */}
      {expanded && (
        <>
          {/* Input */}
          <div className="border-t border-warning/10">
            <div className="px-3 py-1.5 text-[10px] uppercase tracking-wider text-muted">Input</div>
            <pre className="px-3 pb-2 text-xs font-mono text-secondary whitespace-pre-wrap break-all overflow-x-auto">
              {inputStr}
            </pre>
          </div>

          {/* Result */}
          {result && (
            <div
              className={`border-t ${
                result.isError
                  ? 'border-red-500/20 bg-red-500/5'
                  : 'border-emerald-500/20 bg-emerald-500/5'
              }`}
            >
              <div className="px-3 py-1.5 text-[10px] uppercase tracking-wider text-muted">
                Result
              </div>
              <pre
                className={`px-3 pb-2 text-xs font-mono whitespace-pre-wrap break-all overflow-x-auto ${
                  result.isError ? 'text-red-400' : 'text-secondary'
                }`}
              >
                {result.content}
              </pre>
            </div>
          )}
        </>
      )}
    </div>
  );
}
