import { useState } from "react";
import { Play, X } from "lucide-react";
import { llmApi } from "../../api/llm";

interface AgentRunDialogProps {
  agentId: string;
  agentName: string;
  open: boolean;
  onClose: () => void;
  onRunStarted: (runId: string) => void;
}

export function AgentRunDialog({
  agentId,
  agentName,
  open,
  onClose,
  onRunStarted,
}: AgentRunDialogProps) {
  const [goal, setGoal] = useState("");
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (!open) return null;

  async function handleRun() {
    if (!goal.trim()) return;
    setRunning(true);
    setError(null);
    try {
      const runId = await llmApi.triggerAgentLoop(agentId, goal.trim());
      onRunStarted(runId);
      setGoal("");
      onClose();
    } catch (err) {
      setError(String(err));
    }
    setRunning(false);
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/60">
      <div className="w-[560px] rounded-2xl border border-[#2a2d3e] bg-[#13151d] shadow-2xl">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#2a2d3e]">
          <h3 className="text-base font-semibold text-white">
            Run Agent: {agentName}
          </h3>
          <button
            onClick={onClose}
            className="p-1.5 rounded text-[#64748b] hover:text-white hover:bg-[#2a2d3e]"
          >
            <X size={16} />
          </button>
        </div>

        {/* Body */}
        <div className="px-6 py-5 space-y-4">
          <div>
            <label className="text-xs text-[#64748b] mb-1.5 block">
              What should this agent accomplish?
            </label>
            <textarea
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              placeholder="e.g., Create a Python script that scrapes weather data and saves it to a CSV file..."
              rows={5}
              autoFocus
              className="w-full px-3 py-2.5 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1] resize-none leading-relaxed"
              onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                  handleRun();
                }
              }}
            />
          </div>

          {error && (
            <div className="px-3 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-400 text-xs">
              {error}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex items-center justify-end gap-2 px-6 py-4 border-t border-[#2a2d3e]">
          <button
            onClick={onClose}
            className="px-4 py-2 rounded-lg text-[#64748b] hover:text-white text-sm"
          >
            Cancel
          </button>
          <button
            onClick={handleRun}
            disabled={running || !goal.trim()}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-sm font-medium transition-colors"
          >
            <Play size={14} />
            {running ? "Starting..." : "Run Agent"}
          </button>
        </div>
      </div>
    </div>
  );
}
