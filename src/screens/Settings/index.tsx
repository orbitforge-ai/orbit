import { useState } from 'react';
import { Check, Key, Trash2 } from 'lucide-react';
import { llmApi } from '../../api/llm';
import { confirm } from '@tauri-apps/plugin-dialog';
import { useApiKeyStatus, useInvalidateApiKeys } from '../../hooks/useApiKeyStatus';
import { LLM_PROVIDERS, SEARCH_PROVIDERS } from '../../constants/providers';

function ProviderKeyRow({ provider, label }: { provider: string; label: string }) {
  const { data: hasKey = false } = useApiKeyStatus(provider);
  const invalidate = useInvalidateApiKeys();
  const [keyInput, setKeyInput] = useState('');
  const [editing, setEditing] = useState(false);

  async function handleSave() {
    if (!keyInput.trim()) return;
    try {
      await llmApi.setApiKey(provider, keyInput.trim());
      invalidate();
      setKeyInput('');
      setEditing(false);
    } catch (err) {
      console.error(`Failed to set ${provider} API key:`, err);
    }
  }

  async function handleRemove() {
    if (!(await confirm(`Remove ${label} API key?`))) return;
    try {
      await llmApi.deleteApiKey(provider);
      invalidate();
    } catch (err) {
      console.error(`Failed to delete ${provider} API key:`, err);
    }
  }

  return (
    <div className="rounded-lg border border-edge bg-background px-4 py-3">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium text-white">{label}</span>
        {hasKey ? (
          <div className="flex items-center gap-3">
            <div className="flex items-center gap-1.5">
              <Check size={13} className="text-emerald-400" />
              <span className="text-xs text-emerald-400">Configured</span>
            </div>
            <button
              onClick={handleRemove}
              className="flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10 transition-colors"
            >
              <Trash2 size={11} /> Remove
            </button>
          </div>
        ) : !editing ? (
          <button
            onClick={() => setEditing(true)}
            className="flex items-center gap-1.5 text-xs text-secondary hover:text-white transition-colors"
          >
            <Key size={12} />
            Add key
          </button>
        ) : null}
      </div>
      {editing && !hasKey && (
        <div className="mt-3 flex gap-2">
          <input
            type="password"
            placeholder={`Enter ${label} API key...`}
            value={keyInput}
            onChange={(e) => setKeyInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && handleSave()}
            autoFocus
            className="flex-1 px-3 py-2 rounded-lg bg-surface border border-edge text-white text-sm font-mono focus:outline-none focus:border-accent"
          />
          <button
            onClick={handleSave}
            disabled={!keyInput.trim()}
            className="px-3 py-1.5 rounded-lg bg-accent hover:bg-accent-hover disabled:opacity-50 text-white text-xs font-medium transition-colors"
          >
            Save
          </button>
          <button
            onClick={() => { setEditing(false); setKeyInput(''); }}
            className="px-3 py-1.5 rounded-lg text-muted hover:text-white text-xs transition-colors"
          >
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}

export function Settings() {
  return (
    <div className="h-full overflow-y-auto">
      <div className="max-w-2xl mx-auto p-8 space-y-8">
        <div>
          <h2 className="text-lg font-semibold text-white">Settings</h2>
          <p className="text-sm text-muted mt-1">
            Manage API keys shared across all agents.
          </p>
        </div>

        <section className="space-y-3">
          <h3 className="text-sm font-semibold text-white">Model Providers</h3>
          <p className="text-xs text-muted">
            API keys for LLM providers. These are shared by all agents that use the same provider.
          </p>
          <div className="space-y-2">
            {LLM_PROVIDERS.map((p) => (
              <ProviderKeyRow key={p.value} provider={p.value} label={p.label} />
            ))}
          </div>
        </section>

        <section className="space-y-3">
          <h3 className="text-sm font-semibold text-white">Search Providers</h3>
          <p className="text-xs text-muted">
            API keys for web search. These are shared by all agents with web search enabled.
          </p>
          <div className="space-y-2">
            {SEARCH_PROVIDERS.map((p) => (
              <ProviderKeyRow key={p.value} provider={p.value} label={p.label} />
            ))}
          </div>
        </section>
      </div>
    </div>
  );
}
