import { useEffect, useState } from 'react';
import { Key, Trash2, Check } from 'lucide-react';
import { llmApi } from '../../api/llm';

export function SearchKeyStatus({ provider }: { provider: string }) {
  const [hasKey, setHasKey] = useState(false);
  const [keyInput, setKeyInput] = useState('');
  const [showInput, setShowInput] = useState(false);

  useEffect(() => {
    llmApi
      .hasApiKey(provider)
      .then(setHasKey)
      .catch(() => setHasKey(false));
  }, [provider]);

  async function handleSet() {
    if (!keyInput.trim()) return;
    try {
      await llmApi.setApiKey(provider, keyInput.trim());
      setHasKey(true);
      setKeyInput('');
      setShowInput(false);
    } catch (err) {
      console.error('Failed to set search API key:', err);
    }
  }

  if (hasKey) {
    return (
      <div className="flex items-center gap-2 h-[38px]">
        <Check size={14} className="text-emerald-400" />
        <span className="text-sm text-emerald-400">Configured</span>
        <button
          onClick={async () => {
            await llmApi.deleteApiKey(provider);
            setHasKey(false);
          }}
          className="ml-auto flex items-center gap-1 px-2 py-1 rounded text-xs text-red-400 hover:bg-red-500/10"
        >
          <Trash2 size={11} /> Remove
        </button>
      </div>
    );
  }

  if (showInput) {
    return (
      <div className="flex gap-2">
        <input
          type="password"
          placeholder={`${provider} API key...`}
          value={keyInput}
          onChange={(e) => setKeyInput(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && handleSet()}
          autoFocus
          className="flex-1 px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm font-mono focus:outline-none focus:border-accent"
        />
        <button
          onClick={handleSet}
          className="px-3 py-1.5 rounded-lg bg-accent text-white text-xs font-medium"
        >
          Save
        </button>
        <button onClick={() => setShowInput(false)} className="px-2 py-1.5 text-muted text-xs">
          Cancel
        </button>
      </div>
    );
  }

  return (
    <button
      onClick={() => setShowInput(true)}
      className="flex items-center gap-2 w-full px-3 py-2 rounded-lg border border-dashed border-edge-hover text-secondary hover:text-white hover:border-accent text-sm transition-colors"
    >
      <Key size={14} />
      Set {provider} API key
    </button>
  );
}
