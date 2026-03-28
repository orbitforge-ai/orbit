import { useEffect, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Download } from "lucide-react";
import { runsApi } from "../../api/runs";
import { StatusBadge } from "../../components/StatusBadge";
import { SplitPane } from "../../components/SplitPane";
import { TerminalPane } from "../../components/TerminalPane";
import { onRunLogChunk, onRunStateChanged } from "../../events/runEvents";
import { useLiveRunStore } from "../../store/liveRunStore";
import { useUiStore } from "../../store/uiStore";
import { formatDuration } from "../../lib/formatDuration";
import { LogLine, RunSummary } from "../../types";

export function RunHistory() {
  const { selectedRunId, selectRun } = useUiStore();
  const { appendLogChunk, updateRunState } = useLiveRunStore();
  const queryClient = useQueryClient();

  const [stateFilter, setStateFilter] = useState<string | undefined>("all");
  const [staticLogs, setStaticLogs] = useState<LogLine[]>([]);
  const [liveMode, setLiveMode] = useState(false);

  const { data: runs = [], isLoading } = useQuery({
    queryKey: ["runs", stateFilter],
    queryFn: () => runsApi.list({ limit: 200, stateFilter }),
    refetchInterval: 10_000,
  });

  const liveStore = useLiveRunStore();
  const activeLiveLogs = selectedRunId
    ? liveStore.activeRuns[selectedRunId]?.logs ?? []
    : [];

  // When a run is selected: check if it's active (use live logs) or finished (load from file)
  useEffect(() => {
    if (!selectedRunId) return;
    const run = runs.find((r) => r.id === selectedRunId);
    const isActive = run && ["pending", "queued", "running"].includes(run.state);

    if (isActive) {
      setLiveMode(true);
      setStaticLogs([]);
    } else {
      setLiveMode(false);
      runsApi
        .readLog(selectedRunId)
        .then((content) => {
          const lines = content.split("\n").filter(Boolean).map((l) => ({
            stream: "stdout" as const,
            line: l,
          }));
          setStaticLogs(lines);
        })
        .catch(() => setStaticLogs([]));
    }
  }, [selectedRunId, runs]);

  // Live event subscriptions
  useEffect(() => {
    const unlistenLog = onRunLogChunk((payload) => {
      appendLogChunk(payload.runId, payload.lines);
    });
    const unlistenState = onRunStateChanged((payload) => {
      updateRunState(payload.runId, payload.newState);
      queryClient.invalidateQueries({ queryKey: ["runs"] });
    });
    return () => {
      unlistenLog.then((fn) => fn());
      unlistenState.then((fn) => fn());
    };
  }, [appendLogChunk, updateRunState, queryClient]);

  const displayLogs: LogLine[] = liveMode ? activeLiveLogs : staticLogs;

  const STATE_OPTIONS = [
    { label: "All", value: "all" },
    { label: "Running", value: "running" },
    { label: "Success", value: "success" },
    { label: "Failed", value: "failure" },
    { label: "Cancelled", value: "cancelled" },
  ];

  return (
    <div className="flex flex-col h-full">
      <SplitPane
        top={
          <div className="flex flex-col h-full">
            {/* Toolbar */}
            <div className="flex items-center gap-2 px-4 py-3 border-b border-[#2a2d3e]">
              <h2 className="text-sm font-semibold text-white mr-2">Run History</h2>
              <div className="flex gap-1">
                {STATE_OPTIONS.map((opt) => (
                  <button
                    key={String(opt.value)}
                    onClick={() => setStateFilter(opt.value)}
                    className={`px-2.5 py-1 rounded text-xs font-medium transition-colors ${
                      stateFilter === opt.value
                        ? "bg-[#6366f1] text-white"
                        : "text-[#64748b] hover:text-white hover:bg-[#2a2d3e]"
                    }`}
                  >
                    {opt.label}
                  </button>
                ))}
              </div>
            </div>

            {/* Table */}
            <div className="flex-1 overflow-y-auto">
              {isLoading ? (
                <div className="p-8 text-center text-[#64748b] text-sm">Loading…</div>
              ) : runs.length === 0 ? (
                <div className="p-8 text-center text-[#64748b] text-sm">
                  No runs found
                </div>
              ) : (
                <table className="w-full text-sm">
                  <thead className="sticky top-0 bg-[#13151e] border-b border-[#2a2d3e]">
                    <tr>
                      {["Task", "State", "Trigger", "Started", "Duration"].map((h) => (
                        <th
                          key={h}
                          className="px-4 py-2 text-left text-xs font-medium text-[#64748b] uppercase tracking-wider"
                        >
                          {h}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody className="divide-y divide-[#2a2d3e]">
                    {runs.map((run) => (
                      <RunRow
                        key={run.id}
                        run={run}
                        selected={run.id === selectedRunId}
                        onClick={() => selectRun(run.id)}
                      />
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        }
        bottom={
          <div className="flex flex-col h-full p-3 bg-[#0f1117]">
            <div className="flex items-center justify-between mb-2">
              <span className="text-xs font-medium text-[#64748b]">
                {selectedRunId
                  ? liveMode
                    ? "Live output"
                    : "Run output"
                  : "Select a run to view logs"}
              </span>
              {selectedRunId && (
                <button
                  onClick={() => runsApi.readLog(selectedRunId).then(downloadLog)}
                  className="flex items-center gap-1 text-xs text-[#64748b] hover:text-white transition-colors"
                >
                  <Download size={12} />
                  Download
                </button>
              )}
            </div>
            <TerminalPane lines={displayLogs} live={liveMode} className="flex-1" />
          </div>
        }
      />
    </div>
  );
}

function RunRow({
  run,
  selected,
  onClick,
}: {
  run: RunSummary;
  selected: boolean;
  onClick: () => void;
}) {
  return (
    <tr
      onClick={onClick}
      className={`cursor-pointer transition-colors ${
        selected ? "bg-[#6366f1]/10" : "hover:bg-[#1a1d27]"
      }`}
    >
      <td className="px-4 py-2.5 font-medium text-white truncate max-w-[200px]">
        {run.taskName}
      </td>
      <td className="px-4 py-2.5">
        <StatusBadge state={run.state} />
      </td>
      <td className="px-4 py-2.5 text-[#64748b] capitalize">{run.trigger}</td>
      <td className="px-4 py-2.5 text-[#64748b] whitespace-nowrap">
        {run.startedAt ? new Date(run.startedAt).toLocaleString() : "—"}
      </td>
      <td className="px-4 py-2.5 text-[#64748b]">
        {formatDuration(run.durationMs)}
      </td>
    </tr>
  );
}

function downloadLog(content: string) {
  const blob = new Blob([content], { type: "text/plain" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = "orbit-run.log";
  a.click();
  URL.revokeObjectURL(url);
}
