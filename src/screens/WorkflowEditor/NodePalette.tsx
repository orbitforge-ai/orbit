import { DragEvent } from 'react';
import { NODE_REGISTRY, NodeMeta } from './nodeRegistry';

const GROUPS: NodeMeta['group'][] = ['Triggers', 'Agents', 'Logic', 'Integrations'];

export function NodePalette() {
  function onDragStart(event: DragEvent<HTMLDivElement>, type: string) {
    event.dataTransfer.setData('application/orbit-node-type', type);
    event.dataTransfer.effectAllowed = 'move';
  }

  return (
    <aside className="w-56 border-r border-edge bg-background/50 overflow-y-auto">
      <div className="px-3 py-3 border-b border-edge">
        <h2 className="text-xs font-semibold uppercase tracking-wider text-muted">Nodes</h2>
        <p className="text-[10px] text-muted mt-0.5">Drag onto canvas</p>
      </div>
      {GROUPS.map((group) => {
        const items = NODE_REGISTRY.filter((n) => n.group === group);
        return (
          <div key={group} className="px-2 py-2">
            <div className="px-2 text-[10px] uppercase tracking-wider text-muted mb-1.5">
              {group}
            </div>
            <div className="space-y-1">
              {items.map((node) => {
                const Icon = node.icon;
                return (
                  <div
                    key={node.type}
                    draggable
                    onDragStart={(e) => onDragStart(e, node.type)}
                    className="flex items-center gap-2 px-2 py-1.5 rounded-md border border-edge bg-surface hover:border-accent hover:bg-accent/5 cursor-grab active:cursor-grabbing transition-colors"
                  >
                    <Icon size={12} className="text-muted" />
                    <span className="text-xs text-white flex-1 truncate">{node.label}</span>
                    {node.comingSoon && (
                      <span className="text-[9px] uppercase tracking-wider text-muted px-1 rounded bg-muted/15">
                        Soon
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}
    </aside>
  );
}
