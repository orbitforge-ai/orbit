import { useState } from 'react';
import { useQuery, useQueryClient } from '@tanstack/react-query';
import {
  AlertTriangle,
  Brain,
  CheckCircle,
  ChevronDown,
  Pencil,
  Plus,
  Search,
  Trash2,
  WifiOff,
  X,
} from 'lucide-react';
import * as Select from '@radix-ui/react-select';
import { memoryApi } from '../../api/memory';
import { toast } from '../../store/toastStore';
import { MemoryEntry, MemoryType } from '../../types';
import { Input, Textarea } from '../../components/ui';

const TYPE_FILTER_OPTIONS: { value: MemoryType | 'all'; label: string }[] = [
  { value: 'all', label: 'All' },
  { value: 'user', label: 'User' },
  { value: 'feedback', label: 'Feedback' },
  { value: 'project', label: 'Project' },
  { value: 'reference', label: 'Reference' },
];

const TYPE_BADGE: Record<MemoryType, string> = {
  user: 'bg-blue-500/10 text-blue-400 border border-blue-500/20',
  feedback: 'bg-amber-500/10 text-amber-400 border border-amber-500/20',
  project: 'bg-emerald-500/10 text-emerald-400 border border-emerald-500/20',
  reference: 'bg-purple-500/10 text-purple-400 border border-purple-500/20',
};

function daysSince(iso: string): number {
  const ts = new Date(iso).getTime();
  if (Number.isNaN(ts)) return Number.NaN;
  return (Date.now() - ts) / (1000 * 60 * 60 * 24);
}

function formatMemoryDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return 'Unknown date';
  return date.toLocaleDateString();
}

export function Memory() {
  const queryClient = useQueryClient();
  const [filterType, setFilterType] = useState<MemoryType | 'all'>('all');
  const [searchQuery, setSearchQuery] = useState('');
  const [searching, setSearching] = useState(false);
  const [searchResults, setSearchResults] = useState<MemoryEntry[] | null>(null);
  const [showAddForm, setShowAddForm] = useState(false);
  const [newText, setNewText] = useState('');
  const [newType, setNewType] = useState<MemoryType>('project');
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editText, setEditText] = useState('');

  const { data: health } = useQuery({
    queryKey: ['memory-health'],
    queryFn: () => memoryApi.getHealth(),
    refetchInterval: 30_000,
  });

  const { data: memories, isLoading } = useQuery({
    queryKey: ['memories', filterType],
    queryFn: () => memoryApi.list(filterType === 'all' ? undefined : filterType, 200),
    enabled: health === true,
  });

  const { data: allMemories } = useQuery({
    queryKey: ['memories', 'all'],
    queryFn: () => memoryApi.list(undefined, 200),
    enabled: health === true,
  });

  async function handleSearch() {
    if (!searchQuery.trim()) {
      setSearchResults(null);
      return;
    }
    setSearching(true);
    try {
      setSearchResults(await memoryApi.search(searchQuery, undefined, 20));
    } finally {
      setSearching(false);
    }
  }

  function clearSearch() {
    setSearchQuery('');
    setSearchResults(null);
  }

  async function handleAdd() {
    if (!newText.trim()) return;
    setAdding(true);
    try {
      const savedText = newText.trim();
      const created = await memoryApi.add(savedText, newType);
      setNewText('');
      setShowAddForm(false);
      queryClient.invalidateQueries({ queryKey: ['memories'] });
      if (created.length > 0) {
        toast.success('Memory saved');
      } else {
        toast.info('Memory queued. It may take a moment to appear.');
        window.setTimeout(() => queryClient.invalidateQueries({ queryKey: ['memories'] }), 1500);
        window.setTimeout(() => queryClient.invalidateQueries({ queryKey: ['memories'] }), 4000);
      }
    } catch (error) {
      toast.error('Failed to save memory', error);
    } finally {
      setAdding(false);
    }
  }

  async function handleDelete(memoryId: string) {
    await memoryApi.delete(memoryId);
    queryClient.invalidateQueries({ queryKey: ['memories'] });
    if (searchResults) setSearchResults(searchResults.filter((m) => m.id !== memoryId));
  }

  async function handleSaveEdit(memoryId: string) {
    if (!editText.trim()) return;
    await memoryApi.update(memoryId, editText.trim());
    setEditingId(null);
    queryClient.invalidateQueries({ queryKey: ['memories'] });
  }

  const displayMemories = searchResults ?? memories ?? [];

  return (
    <div className="p-6 space-y-5 h-full overflow-y-auto">
      {/* Header */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <h3 className="text-sm font-semibold text-white">Long-term Memory</h3>
          {health === true ? (
            <span className="flex items-center gap-1 text-[10px] text-emerald-400">
              <CheckCircle size={10} /> Online
            </span>
          ) : health === false ? (
            <span className="flex items-center gap-1 text-[10px] text-red-400">
              <WifiOff size={10} /> Offline
            </span>
          ) : null}
        </div>
        <button
          onClick={() => setShowAddForm((v) => !v)}
          className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-accent hover:bg-accent-hover text-white text-xs font-medium transition-colors"
        >
          {showAddForm ? <X size={12} /> : <Plus size={12} />}
          {showAddForm ? 'Cancel' : 'Add Memory'}
        </button>
      </div>

      {/* Add form */}
      {showAddForm && (
        <div className="space-y-3 p-4 rounded-lg border border-edge bg-surface">
          <Textarea
            value={newText}
            onChange={(e) => setNewText(e.target.value)}
            placeholder="Enter information to remember..."
            rows={3}
            className="bg-background px-3 py-2 resize-none"
          />
          <div className="flex items-center gap-2">
            <Select.Root value={newType} onValueChange={(v) => setNewType(v as MemoryType)}>
              <Select.Trigger className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-background border border-edge text-white text-xs focus:outline-none focus:border-accent">
                <Select.Value />
                <Select.Icon>
                  <ChevronDown size={12} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50">
                  <Select.Viewport className="p-1">
                    {TYPE_FILTER_OPTIONS.filter((o) => o.value !== 'all').map((o) => (
                      <Select.Item
                        key={o.value}
                        value={o.value}
                        className="px-3 py-1.5 text-xs text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Select.ItemText>{o.label}</Select.ItemText>
                      </Select.Item>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
            <button
              onClick={handleAdd}
              disabled={adding || !newText.trim()}
              className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
            >
              {adding ? 'Saving\u2026' : 'Save'}
            </button>
          </div>
        </div>
      )}

      {/* Search */}
      <div className="flex gap-2">
        <div className="relative flex-1">
          <Search
            size={13}
            className="absolute left-3 top-1/2 -translate-y-1/2 text-muted pointer-events-none"
          />
          <Input
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSearch()}
            placeholder="Semantic search..."
            className="bg-background pl-8 pr-8 py-2"
          />
          {searchQuery && (
            <button
              onClick={clearSearch}
              className="absolute right-2.5 top-1/2 -translate-y-1/2 text-muted hover:text-white"
            >
              <X size={12} />
            </button>
          )}
        </div>
        <button
          onClick={handleSearch}
          disabled={searching || !searchQuery.trim()}
          className="px-3 py-2 rounded-lg bg-surface border border-edge text-secondary hover:text-white disabled:opacity-50 text-xs transition-colors"
        >
          {searching ? 'Searching\u2026' : 'Search'}
        </button>
      </div>

      {/* Type filters (hidden during search) */}
      {!searchResults && (
        <div className="flex gap-1.5 flex-wrap">
          {TYPE_FILTER_OPTIONS.map((opt) => (
            <button
              key={opt.value}
              onClick={() => setFilterType(opt.value)}
              className={`px-2.5 py-1 rounded-lg border text-xs font-medium transition-colors ${
                filterType === opt.value
                  ? 'border-accent/40 bg-accent/10 text-accent-light'
                  : 'border-edge bg-surface text-muted hover:border-edge-hover hover:text-white'
              }`}
            >
              {opt.label}
              {opt.value !== 'all' && memories && (
                <span className="ml-1.5 text-[10px] opacity-60">
                  {(allMemories ?? []).filter((m) => m.memoryType === opt.value).length}
                </span>
              )}
            </button>
          ))}
        </div>
      )}

      {/* Search result header */}
      {searchResults && (
        <div className="flex items-center gap-2 text-xs text-muted">
          <Search size={11} />
          {searchResults.length} result{searchResults.length !== 1 ? 's' : ''} for &ldquo;
          {searchQuery}&rdquo;
          <button
            onClick={clearSearch}
            className="ml-auto flex items-center gap-1 text-muted hover:text-white transition-colors"
          >
            <X size={11} /> Clear
          </button>
        </div>
      )}

      {/* Body */}
      {health === false ? (
        <div className="flex flex-col items-center justify-center py-16 gap-3 text-center">
          <WifiOff size={28} className="text-muted opacity-40" />
          <p className="text-sm text-muted">Memory service is offline.</p>
          <p className="text-xs text-muted opacity-60">
            Agents will continue to work. Check logs if this persists.
          </p>
        </div>
      ) : isLoading ? (
        <div className="py-10 text-center text-xs text-muted">Loading memories&hellip;</div>
      ) : displayMemories.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-16 gap-3 text-center">
          <Brain size={28} className="text-muted opacity-40" />
          <p className="text-sm text-muted">No memories yet.</p>
          <p className="text-xs text-muted opacity-60">
            Memories are added automatically at the end of sessions, or add them manually above.
          </p>
        </div>
      ) : (
        <div className="space-y-2">
          {displayMemories.map((memory) => (
            <MemoryRow
              key={memory.id}
              memory={memory}
              isEditing={editingId === memory.id}
              editText={editText}
              onStartEdit={() => {
                setEditingId(memory.id);
                setEditText(memory.text);
              }}
              onEditTextChange={setEditText}
              onSaveEdit={() => handleSaveEdit(memory.id)}
              onCancelEdit={() => setEditingId(null)}
              onDelete={() => handleDelete(memory.id)}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function MemoryRow({
  memory,
  isEditing,
  editText,
  onStartEdit,
  onEditTextChange,
  onSaveEdit,
  onCancelEdit,
  onDelete,
}: {
  memory: MemoryEntry;
  isEditing: boolean;
  editText: string;
  onStartEdit: () => void;
  onEditTextChange: (v: string) => void;
  onSaveEdit: () => void;
  onCancelEdit: () => void;
  onDelete: () => void;
}) {
  const ageInDays = daysSince(memory.createdAt);
  const stale = Number.isFinite(ageInDays) && ageInDays > 30;

  return (
    <div
      className={`rounded-lg border p-3 ${
        stale ? 'border-amber-500/20 bg-amber-500/5' : 'border-edge bg-surface'
      }`}
    >
      <div className="flex items-start gap-2">
        <div className="flex-1 min-w-0">
          {isEditing ? (
            <Textarea
              value={editText}
              onChange={(e) => onEditTextChange(e.target.value)}
              rows={3}
              autoFocus
              className="bg-background rounded px-2 py-1.5 text-xs resize-none"
            />
          ) : (
            <p className="text-sm text-white leading-relaxed break-words">{memory.text}</p>
          )}
          <div className="mt-1.5 flex items-center gap-2 flex-wrap">
            <span
              className={`inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium ${TYPE_BADGE[memory.memoryType]}`}
            >
              {memory.memoryType}
            </span>
            <span className="text-[10px] text-muted">{formatMemoryDate(memory.createdAt)}</span>
            {stale && (
              <span className="flex items-center gap-1 text-[10px] text-amber-400">
                <AlertTriangle size={9} /> Stale
              </span>
            )}
            {typeof memory.score === 'number' && (
              <span className="text-[10px] text-muted opacity-60">
                score {memory.score.toFixed(2)}
              </span>
            )}
          </div>
        </div>

        <div className="flex items-center gap-1 shrink-0">
          {isEditing ? (
            <>
              <button
                onClick={onSaveEdit}
                className="px-2 py-1 rounded text-xs bg-accent/20 text-accent-hover hover:bg-accent/30 transition-colors"
              >
                Save
              </button>
              <button
                onClick={onCancelEdit}
                className="px-2 py-1 rounded text-xs text-muted hover:text-white transition-colors"
              >
                Cancel
              </button>
            </>
          ) : (
            <>
              <button
                onClick={onStartEdit}
                className="p-1.5 rounded text-muted hover:text-white hover:bg-surface-hover transition-colors"
                title="Edit"
              >
                <Pencil size={12} />
              </button>
              <button
                onClick={onDelete}
                className="p-1.5 rounded text-muted hover:text-red-400 hover:bg-red-500/10 transition-colors"
                title="Delete"
              >
                <Trash2 size={12} />
              </button>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
