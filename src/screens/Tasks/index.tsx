import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Play, Pencil, Trash2, ToggleLeft, ToggleRight } from "lucide-react";
import { tasksApi } from "../../api/tasks";
import { StatusBadge } from "../../components/StatusBadge";
import { useUiStore } from "../../store/uiStore";
import { Task } from "../../types";
import {info} from '@tauri-apps/plugin-log'
import { confirm } from '@tauri-apps/plugin-dialog';

export function TasksScreen() {
  const { navigate, editTask } = useUiStore();
  const queryClient = useQueryClient();

  const { data: tasks = [], isLoading } = useQuery({
    queryKey: ["tasks"],
    queryFn: tasksApi.list,
    select: (all: Task[]) => all.filter((t) => !t.tags.includes("pulse")),
  });

  async function handleTrigger(task: Task) {
    console.log(`Triggering task: ${task.name}`);
    await tasksApi.trigger(task.id);
    queryClient.invalidateQueries({ queryKey: ["runs"] });
    navigate("history");
  }

  async function handleToggle(task: Task) {
    info(`${!task.enabled ? "Enabling" : "Disabling"} task: ${task.name}`);
    await tasksApi.update(task.id, { enabled: !task.enabled });
    queryClient.invalidateQueries({ queryKey: ["tasks"] });
  }

  async function handleDelete(task: Task) {
    if (!await confirm(`Are you sure you want to delete "${task.name}"?`, { title: "Confirm Delete", })) return;
    await tasksApi.delete(task.id);
    queryClient.invalidateQueries({ queryKey: ["tasks"] });
  }

  return (
    <div className="flex flex-col h-full">
      <div className="flex items-center justify-between px-6 py-4 border-b border-[#2a2d3e]">
        <h2 className="text-lg font-semibold text-white">Tasks</h2>
        <button
          onClick={() => navigate("task-builder")}
          className="px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-sm font-medium transition-colors"
        >
          + New Task
        </button>
      </div>

      <div className="flex-1 overflow-y-auto">
        {isLoading && (
          <div className="p-8 text-center text-[#64748b] text-sm">Loading…</div>
        )}
        {!isLoading && tasks.length === 0 && (
          <div className="p-16 text-center">
            <p className="text-[#64748b] text-sm">No tasks yet</p>
            <button
              onClick={() => navigate("task-builder")}
              className="mt-3 px-4 py-2 rounded-lg bg-[#6366f1] text-white text-sm"
            >
              Create your first task
            </button>
          </div>
        )}

        <div className="divide-y divide-[#2a2d3e]">
          {tasks.map((task) => (
            <div key={task.id} className="flex items-center gap-3 px-6 py-4 hover:bg-[#1a1d27] transition-colors">
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2">
                  <p className="text-sm font-medium text-white truncate">{task.name}</p>
                  {!task.enabled && (
                    <StatusBadge state="cancelled" />
                  )}
                </div>
                <p className="text-xs text-[#64748b] mt-0.5 capitalize">
                  {task.kind.replace("_", " ")}
                </p>
              </div>

              <div className="flex items-center gap-1">
                <button
                  onClick={() => handleTrigger(task)}
                  title="Run now"
                  className="p-1.5 rounded text-[#64748b] hover:text-green-400 hover:bg-green-500/10 transition-colors"
                >
                  <Play size={14} />
                </button>
                <button
                  onClick={() => editTask(task.id)}
                  title="Edit"
                  className="p-1.5 rounded text-[#64748b] hover:text-white hover:bg-[#2a2d3e] transition-colors"
                >
                  <Pencil size={14} />
                </button>
                <button
                  onClick={() => handleToggle(task)}
                  title={task.enabled ? "Disable" : "Enable"}
                  className="p-1.5 rounded text-[#64748b] hover:text-white hover:bg-[#2a2d3e] transition-colors"
                >
                  {task.enabled ? <ToggleRight size={14} /> : <ToggleLeft size={14} />}
                </button>
                <button
                  type="button"
                  onClick={() => handleDelete(task)}
                  title="Delete"
                  className="p-1.5 rounded text-[#64748b] hover:text-red-400 hover:bg-red-500/10 transition-colors"
                >
                  <Trash2 size={14} />
                </button>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
