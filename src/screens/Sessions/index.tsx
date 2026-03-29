import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Boxes, Edit2, Plus, Save, Trash2, X, Variable } from "lucide-react";
import { sessionsApi } from "../../api/sessions";
import { Session, CreateSession, UpdateSession } from "../../types";
import { confirm } from "@tauri-apps/plugin-dialog";

export function SessionsScreen() {
  const queryClient = useQueryClient();
  const [showCreate, setShowCreate] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const { data: sessions = [], isLoading } = useQuery({
    queryKey: ["sessions"],
    queryFn: sessionsApi.list,
    refetchInterval: 10_000,
  });

  const selected = sessions.find(s => s.id === selectedId) ?? null;

  async function handleDelete(session: Session) {
    if (!await confirm(`Delete session "${session.name}"?`)) return;
    await sessionsApi.delete(session.id);
    queryClient.invalidateQueries({ queryKey: ["sessions"] });
    if (selectedId === session.id) setSelectedId(null);
  }

  return (
    <div className="flex h-full">
      {/* Left: Session list */}
      <div className="w-[380px] flex flex-col border-r border-[#2a2d3e]">
        <div className="flex items-center justify-between px-6 py-4 border-b border-[#2a2d3e]">
          <h2 className="text-lg font-semibold text-white">Sessions</h2>
          <button
            onClick={() => setShowCreate(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] text-white text-xs font-medium transition-colors"
          >
            <Plus size={12} /> New Session
          </button>
        </div>

        <div className="flex-1 overflow-y-auto p-4 space-y-3">
          {isLoading && <p className="text-center py-8 text-[#64748b] text-sm">Loading...</p>}
          {!isLoading && sessions.length === 0 && !showCreate && (
            <div className="text-center py-16 text-[#64748b] text-sm">
              Sessions group tasks and share environment variables across them. Create one to get started.
            </div>
          )}

          {showCreate && (
            <SessionForm
              onSave={async (payload) => {
                await sessionsApi.create(payload as CreateSession);
                queryClient.invalidateQueries({ queryKey: ["sessions"] });
                setShowCreate(false);
              }}
              onCancel={() => setShowCreate(false)}
            />
          )}

          {sessions.map(session =>
            editingId === session.id ? (
              <SessionForm
                key={session.id}
                initial={session}
                onSave={async (payload) => {
                  await sessionsApi.update(session.id, payload as UpdateSession);
                  queryClient.invalidateQueries({ queryKey: ["sessions"] });
                  setEditingId(null);
                }}
                onCancel={() => setEditingId(null)}
              />
            ) : (
              <div
                key={session.id}
                onClick={() => setSelectedId(session.id)}
                className={`flex items-center gap-4 px-5 py-4 rounded-xl border cursor-pointer transition-colors ${
                  selectedId === session.id
                    ? "border-[#6366f1] bg-[#6366f1]/10"
                    : "border-[#2a2d3e] bg-[#1a1d27] hover:border-[#4a4d6e]"
                }`}
              >
                <div className="w-10 h-10 rounded-full bg-[#6366f1]/20 flex items-center justify-center flex-shrink-0">
                  <Boxes size={18} className="text-[#818cf8]" />
                </div>
                <div className="flex-1 min-w-0">
                  <p className="text-sm font-semibold text-white">{session.name}</p>
                  <p className="text-xs text-[#64748b] mt-0.5">
                    {Object.keys(session.environment).length} env vars
                    {session.description && <> &middot; {session.description}</>}
                  </p>
                </div>
                <div className="flex items-center gap-1.5 flex-shrink-0">
                  <button onClick={(e) => { e.stopPropagation(); setEditingId(session.id); }}
                    className="p-1.5 rounded text-[#64748b] hover:text-[#818cf8] hover:bg-[#6366f1]/10 transition-colors">
                    <Edit2 size={13} />
                  </button>
                  <button onClick={(e) => { e.stopPropagation(); handleDelete(session); }}
                    className="p-1.5 rounded text-[#64748b] hover:text-red-400 hover:bg-red-500/10 transition-colors">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>
            )
          )}
        </div>
      </div>

      {/* Right: Session detail */}
      <div className="flex-1 overflow-y-auto">
        {selected ? (
          <SessionDetail session={selected} />
        ) : (
          <div className="flex items-center justify-center h-full text-[#64748b] text-sm">
            Select a session to view its environment variables
          </div>
        )}
      </div>
    </div>
  );
}

function SessionForm({
  initial,
  onSave,
  onCancel,
}: {
  initial?: Session;
  onSave: (payload: CreateSession | UpdateSession) => Promise<void>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [envPairs, setEnvPairs] = useState<{ k: string; v: string }[]>(
    initial
      ? Object.entries(initial.environment).map(([k, v]) => ({ k, v }))
      : []
  );
  const [saving, setSaving] = useState(false);

  async function handleSubmit() {
    if (!name.trim()) return;
    setSaving(true);
    try {
      const environment: Record<string, string> = {};
      for (const pair of envPairs) {
        if (pair.k.trim()) environment[pair.k.trim()] = pair.v;
      }
      await onSave({
        name: name.trim(),
        description: description.trim() || undefined,
        environment,
      });
    } catch {
      setSaving(false);
    }
  }

  return (
    <div className="rounded-xl border border-[#6366f1] bg-[#1a1d27] p-4 space-y-3">
      <input type="text" placeholder="Session name" value={name}
        onChange={e => setName(e.target.value)} autoFocus
        className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />
      <input type="text" placeholder="Description (optional)" value={description}
        onChange={e => setDescription(e.target.value)}
        className="w-full px-3 py-2 rounded-lg bg-[#0f1117] border border-[#2a2d3e] text-white text-sm focus:outline-none focus:border-[#6366f1]" />

      <div className="space-y-2">
        <p className="text-xs text-[#64748b] font-medium">Environment Variables</p>
        {envPairs.map((pair, i) => (
          <div key={i} className="flex gap-2 items-center">
            <input type="text" placeholder="KEY" value={pair.k}
              onChange={e => setEnvPairs(prev => prev.map((p, j) => j === i ? { ...p, k: e.target.value } : p))}
              className="flex-1 px-2 py-1.5 rounded bg-[#0f1117] border border-[#2a2d3e] text-white text-xs font-mono focus:outline-none focus:border-[#6366f1]" />
            <input type="text" placeholder="value" value={pair.v}
              onChange={e => setEnvPairs(prev => prev.map((p, j) => j === i ? { ...p, v: e.target.value } : p))}
              className="flex-1 px-2 py-1.5 rounded bg-[#0f1117] border border-[#2a2d3e] text-white text-xs font-mono focus:outline-none focus:border-[#6366f1]" />
            <button onClick={() => setEnvPairs(prev => prev.filter((_, j) => j !== i))}
              className="p-1 text-[#64748b] hover:text-red-400">
              <X size={12} />
            </button>
          </div>
        ))}
        <button onClick={() => setEnvPairs(prev => [...prev, { k: "", v: "" }])}
          className="flex items-center gap-1.5 text-xs text-[#6366f1] hover:text-[#818cf8]">
          <Plus size={12} /> Add variable
        </button>
      </div>

      <div className="flex gap-2 pt-1">
        <button onClick={handleSubmit} disabled={saving || !name.trim()}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-[#6366f1] hover:bg-[#818cf8] disabled:opacity-50 text-white text-xs font-medium">
          <Save size={12} /> {saving ? "Saving..." : initial ? "Save" : "Create"}
        </button>
        <button onClick={onCancel}
          className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-[#64748b] hover:text-white text-xs">
          <X size={12} /> Cancel
        </button>
      </div>
    </div>
  );
}

function SessionDetail({ session }: { session: Session }) {
  const envEntries = Object.entries(session.environment);

  return (
    <div className="p-6 space-y-6">
      <div className="flex items-center gap-4">
        <div className="w-14 h-14 rounded-full bg-[#6366f1]/20 flex items-center justify-center">
          <Boxes size={24} className="text-[#818cf8]" />
        </div>
        <div>
          <h3 className="text-lg font-semibold text-white">{session.name}</h3>
          {session.description && (
            <p className="text-sm text-[#64748b] mt-0.5">{session.description}</p>
          )}
        </div>
      </div>

      <div>
        <h4 className="text-sm font-semibold text-white mb-3 flex items-center gap-2">
          <Variable size={14} className="text-[#818cf8]" />
          Environment Variables ({envEntries.length})
        </h4>

        {envEntries.length === 0 ? (
          <p className="text-sm text-[#64748b]">No environment variables configured.</p>
        ) : (
          <div className="rounded-xl border border-[#2a2d3e] bg-[#0a0c12] overflow-hidden">
            {envEntries.map(([key, value], i) => (
              <div key={key}
                className={`flex items-center gap-4 px-4 py-2.5 ${i > 0 ? "border-t border-[#2a2d3e]" : ""}`}>
                <span className="text-sm font-mono text-[#818cf8] w-48 flex-shrink-0 truncate">{key}</span>
                <span className="text-sm font-mono text-green-400 flex-1 truncate">{value}</span>
              </div>
            ))}
          </div>
        )}
      </div>

      <div className="text-xs text-[#64748b]">
        Created {new Date(session.createdAt).toLocaleString()}
        {session.tags.length > 0 && (
          <span> &middot; Tags: {session.tags.join(", ")}</span>
        )}
      </div>
    </div>
  );
}
