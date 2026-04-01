import { useCallback, useRef, useState } from 'react';
import { cn } from '../lib/cn';

interface SplitPaneProps {
  top: React.ReactNode;
  bottom: React.ReactNode;
  defaultSplit?: number; // 0–1, fraction for top
  className?: string;
}

export function SplitPane({ top, bottom, defaultSplit = 0.55, className }: SplitPaneProps) {
  const [split, setSplit] = useState(defaultSplit);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const onMouseDown = useCallback(() => {
    dragging.current = true;
    document.body.style.cursor = 'row-resize';
    document.body.style.userSelect = 'none';
  }, []);

  const onMouseMove = useCallback((e: React.MouseEvent) => {
    if (!dragging.current || !containerRef.current) return;
    const rect = containerRef.current.getBoundingClientRect();
    const frac = (e.clientY - rect.top) / rect.height;
    setSplit(Math.max(0.2, Math.min(0.85, frac)));
  }, []);

  const onMouseUp = useCallback(() => {
    if (!dragging.current) return;
    dragging.current = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
  }, []);

  return (
    <div
      ref={containerRef}
      className={cn('flex flex-col h-full', className)}
      onMouseMove={onMouseMove}
      onMouseUp={onMouseUp}
      onMouseLeave={onMouseUp}
    >
      {/* Top pane */}
      <div className="overflow-hidden" style={{ flex: `0 0 ${split * 100}%` }}>
        {top}
      </div>

      {/* Drag handle */}
      <div
        className="h-1.5 flex-shrink-0 cursor-row-resize flex items-center justify-center group"
        onMouseDown={onMouseDown}
      >
        <div className="w-12 h-0.5 rounded bg-edge group-hover:bg-accent transition-colors" />
      </div>

      {/* Bottom pane */}
      <div className="flex-1 overflow-hidden min-h-0">{bottom}</div>
    </div>
  );
}
