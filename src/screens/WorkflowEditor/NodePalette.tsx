import { useDraggable } from '@dnd-kit/core';
import { NODE_REGISTRY, NodeMeta } from './nodeRegistry';
import { workflowNodeDraggableId } from './dnd';

const GROUPS: NodeMeta['group'][] = ['Triggers', 'Agents', 'Logic', 'Code', 'Board', 'Integrations'];

export function NodePalette() {
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
                return <DraggablePaletteNode key={node.type} node={node} />;
              })}
            </div>
          </div>
        );
      })}
    </aside>
  );
}

function DraggablePaletteNode({ node }: { node: NodeMeta }) {
  const { attributes, listeners, setNodeRef, isDragging } = useDraggable({
    id: workflowNodeDraggableId(node.type),
  });
  const Icon = node.icon;

  return (
    <div
      ref={setNodeRef}
      {...listeners}
      {...attributes}
      className={
        'flex items-center gap-2 px-2 py-1.5 rounded-md border border-edge bg-surface ' +
        'hover:border-accent hover:bg-accent/5 cursor-grab active:cursor-grabbing ' +
        `transition-colors ${isDragging ? 'opacity-50 border-accent bg-accent/10' : ''}`
      }
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
}
