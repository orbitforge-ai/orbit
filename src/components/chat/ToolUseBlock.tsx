import { useState } from "react";
import { Terminal, ChevronRight, CheckCircle, XCircle } from "lucide-react";

interface ToolUseBlockProps {
  name: string;
  input: Record<string, unknown>;
  result?: { content: string; isError: boolean };
}

export function ToolUseBlock({ name, input, result }: ToolUseBlockProps) {
  const [inputExpanded, setInputExpanded] = useState(false);
  const [resultExpanded, setResultExpanded] = useState(true);
  const inputStr = JSON.stringify(input, null, 2);
  const isLongInput = inputStr.length > 120;
  const isLongResult = result && result.content.length > 300;

  return (
    <div className="rounded-lg border border-[#f59e0b]/30 bg-[#f59e0b]/5 overflow-hidden">
      {/* Tool header */}
      <div className="flex items-center gap-2 px-3 py-2">
        <Terminal size={12} className="text-[#f59e0b] shrink-0" />
        <span className="text-xs font-medium text-[#f59e0b]">{name}</span>
        {result && !result.isError && (
          <CheckCircle size={11} className="text-emerald-400 ml-auto shrink-0" />
        )}
        {result && result.isError && (
          <XCircle size={11} className="text-red-400 ml-auto shrink-0" />
        )}
      </div>

      {/* Input */}
      <div className="border-t border-[#f59e0b]/10">
        {isLongInput ? (
          <>
            <button
              onClick={() => setInputExpanded(!inputExpanded)}
              className="flex items-center gap-1 px-3 py-1.5 text-xs text-[#64748b] hover:text-[#94a3b8] w-full"
            >
              <ChevronRight
                size={10}
                className={`transition-transform ${inputExpanded ? "rotate-90" : ""}`}
              />
              <span>Input</span>
            </button>
            {inputExpanded && (
              <pre className="px-3 pb-2 text-xs font-mono text-[#94a3b8] whitespace-pre-wrap overflow-x-auto">
                {inputStr}
              </pre>
            )}
          </>
        ) : (
          <pre className="px-3 py-1.5 text-xs font-mono text-[#94a3b8] whitespace-pre-wrap overflow-x-auto">
            {inputStr}
          </pre>
        )}
      </div>

      {/* Result */}
      {result && (
        <div
          className={`border-t ${
            result.isError
              ? "border-red-500/20 bg-red-500/5"
              : "border-emerald-500/20 bg-emerald-500/5"
          }`}
        >
          {isLongResult ? (
            <>
              <button
                onClick={() => setResultExpanded(!resultExpanded)}
                className="flex items-center gap-1 px-3 py-1.5 text-xs text-[#64748b] hover:text-[#94a3b8] w-full"
              >
                <ChevronRight
                  size={10}
                  className={`transition-transform ${resultExpanded ? "rotate-90" : ""}`}
                />
                <span>Result</span>
              </button>
              {resultExpanded && (
                <pre
                  className={`px-3 pb-2 text-xs font-mono whitespace-pre-wrap overflow-x-auto ${
                    result.isError ? "text-red-400" : "text-[#94a3b8]"
                  }`}
                >
                  {result.content}
                </pre>
              )}
            </>
          ) : (
            <pre
              className={`px-3 py-1.5 text-xs font-mono whitespace-pre-wrap overflow-x-auto ${
                result.isError ? "text-red-400" : "text-[#94a3b8]"
              }`}
            >
              {result.content}
            </pre>
          )}
        </div>
      )}
    </div>
  );
}
