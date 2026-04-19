import { useEffect, useMemo, useRef, useState } from 'react';
import {
  BaseEdge,
  EdgeLabelRenderer,
  EdgeProps,
  getBezierPath,
} from '@xyflow/react';
import { Trash2 } from 'lucide-react';

type WorkflowEdgeData = {
  onDelete?: (edgeId: string) => void;
};

export function DeletableEdge({
  id,
  sourceX,
  sourceY,
  targetX,
  targetY,
  sourcePosition,
  targetPosition,
  style,
  label,
  data,
}: EdgeProps) {
  const [hovered, setHovered] = useState(false);
  const hideTimerRef = useRef<number | null>(null);
  const edgeData = (data ?? {}) as WorkflowEdgeData;

  const [edgePath, labelX, labelY] = getBezierPath({
    sourceX,
    sourceY,
    sourcePosition,
    targetX,
    targetY,
    targetPosition,
  });

  const labelClassName = useMemo(() => {
    if (label === 'true') return 'text-emerald-300';
    if (label === 'false') return 'text-red-300';
    return 'text-muted';
  }, [label]);

  useEffect(() => {
    return () => {
      if (hideTimerRef.current !== null) {
        window.clearTimeout(hideTimerRef.current);
      }
    };
  }, []);

  const cancelHide = () => {
    if (hideTimerRef.current !== null) {
      window.clearTimeout(hideTimerRef.current);
      hideTimerRef.current = null;
    }
  };

  const showControls = () => {
    cancelHide();
    setHovered(true);
  };

  const scheduleHide = () => {
    cancelHide();
    hideTimerRef.current = window.setTimeout(() => {
      setHovered(false);
      hideTimerRef.current = null;
    }, 90);
  };

  return (
    <>
      <BaseEdge id={id} path={edgePath} style={style} />
      <path
        d={edgePath}
        fill="none"
        stroke="transparent"
        strokeWidth={24}
        className="cursor-pointer"
        onMouseEnter={showControls}
        onMouseLeave={scheduleHide}
      />
      <EdgeLabelRenderer>
        <div
          className="nodrag nopan absolute"
          style={{
            transform: `translate(-50%, -50%) translate(${labelX}px, ${labelY}px)`,
            pointerEvents: 'all',
          }}
          onMouseEnter={showControls}
          onMouseLeave={scheduleHide}
        >
          <div className="flex items-center gap-1.5">
            {label ? (
              <span
                className={
                  `rounded-full border border-edge/80 bg-background/85 px-1.5 py-1 text-[10px] ` +
                  `font-mono leading-none shadow-sm backdrop-blur-sm ${labelClassName}`
                }
              >
                {label}
              </span>
            ) : null}
            {hovered ? (
              <button
                type="button"
                aria-label="Delete connection"
                title="Delete connection"
                onClick={(event) => {
                  event.stopPropagation();
                  edgeData.onDelete?.(id);
                }}
                className="flex h-5 w-5 items-center justify-center rounded-full border border-edge/70 bg-surface/95 text-muted shadow-sm transition-colors hover:border-red-400/60 hover:text-red-300"
              >
                <Trash2 size={10} />
              </button>
            ) : null}
          </div>
        </div>
      </EdgeLabelRenderer>
    </>
  );
}

export const edgeTypes = {
  deletable: DeletableEdge,
};
