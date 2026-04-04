import * as DropdownMenu from '@radix-ui/react-dropdown-menu';
import { Globe, ChevronDown, Check, AlertTriangle, ExternalLink } from "lucide-react";
import * as Select from '@radix-ui/react-select';
import { SEARCH_PROVIDERS } from '../../../constants/providers';
import { useApiKeyStatus } from '../../../hooks/useApiKeyStatus';
import { useUiStore } from '../../../store/uiStore';


export function WebSearchChip({
  allowedTools,
  webSearchProvider,
  onToggle,
  onProviderChange,
}: {
  allowedTools: string[];
  webSearchProvider: string;
  onToggle: () => void;
  onProviderChange: (provider: string) => void;
}) {
  const { data: hasKey = false } = useApiKeyStatus(webSearchProvider);
  const { navigate } = useUiStore();
  const enabled = allowedTools.length === 0 || allowedTools.includes('web_search');
  return (
    <DropdownMenu.Root>
      <div className={`inline-flex items-center rounded-full border text-[11px] transition-colors ${
        enabled ? 'border-accent/50 bg-accent/10 text-accent-hover' : 'border-edge bg-surface text-secondary'
      }`}>
        <button
          onClick={onToggle}
          className="flex items-center gap-1 pl-2.5 pr-1.5 py-1 rounded-l-full hover:bg-white/5 transition-colors"
          title={enabled ? 'Disable web search' : 'Enable web search'}
        >
          <Globe size={10} className={enabled ? 'text-accent-hover' : 'text-muted'} />
          <span>Web Search</span>
        </button>
        <span className="w-px h-3 bg-current opacity-20" />
        <DropdownMenu.Trigger asChild>
          <button
            className="flex items-center pl-1 pr-2 py-1 rounded-r-full hover:bg-white/5 transition-colors"
            title="Configure web search"
          >
            <ChevronDown size={9} className="opacity-60" />
          </button>
        </DropdownMenu.Trigger>
      </div>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="start"
          sideOffset={6}
          className="z-50 w-64 rounded-xl border border-edge bg-surface p-3 shadow-xl space-y-3"
          onCloseAutoFocus={(e) => e.preventDefault()}
        >
          <p className="text-[10px] uppercase tracking-wide text-muted font-semibold">Web Search</p>
          <div className="space-y-1">
            <label className="text-xs text-muted block">Provider</label>
            <Select.Root value={webSearchProvider} onValueChange={onProviderChange}>
              <Select.Trigger className="flex items-center justify-between w-full px-3 py-2 rounded-lg bg-background border border-edge text-white text-sm focus:outline-none focus:border-accent">
                <Select.Value />
                <Select.Icon>
                  <ChevronDown size={14} className="text-muted" />
                </Select.Icon>
              </Select.Trigger>
              <Select.Portal>
                <Select.Content className="rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-[60]">
                  <Select.Viewport className="p-1">
                    {SEARCH_PROVIDERS.map((p) => (
                      <Select.Item
                        key={p.value}
                        value={p.value}
                        className="px-3 py-2 text-sm text-white rounded-md outline-none cursor-pointer data-[highlighted]:bg-accent/20"
                      >
                        <Select.ItemText>{p.label}</Select.ItemText>
                      </Select.Item>
                    ))}
                  </Select.Viewport>
                </Select.Content>
              </Select.Portal>
            </Select.Root>
          </div>
          <div className="rounded-lg border border-edge bg-background px-3 py-2">
            {hasKey ? (
              <div className="flex items-center gap-1.5">
                <Check size={12} className="text-emerald-400" />
                <span className="text-xs text-emerald-400">API key configured</span>
              </div>
            ) : (
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-1.5">
                  <AlertTriangle size={12} className="text-amber-400" />
                  <span className="text-xs text-amber-400">No API key</span>
                </div>
                <button
                  onClick={() => navigate('settings')}
                  className="flex items-center gap-1 text-xs text-accent-hover hover:underline transition-colors"
                >
                  Open Settings <ExternalLink size={10} />
                </button>
              </div>
            )}
          </div>
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}