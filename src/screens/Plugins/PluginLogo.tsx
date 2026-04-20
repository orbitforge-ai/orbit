import { Plug } from 'lucide-react';

interface PluginLogoProps {
  name: string;
  src?: string | null;
  className?: string;
  size?: 'sm' | 'md';
}

export function PluginLogo({
  name,
  src,
  className = '',
  size = 'md',
}: PluginLogoProps) {
  const dimensions = size === 'sm' ? 'h-9 w-9' : 'h-11 w-11';
  const iconSize = size === 'sm' ? 16 : 18;
  return (
    <div
      className={`flex shrink-0 items-center justify-center overflow-hidden rounded-xl border border-edge bg-surface/80 p-1.5 ${dimensions} ${className}`.trim()}
    >
      {src ? (
        <img src={src} alt={`${name} logo`} className="h-full w-full object-contain" />
      ) : (
        <Plug size={iconSize} className="text-muted" />
      )}
    </div>
  );
}
