import { forwardRef, type ReactNode } from 'react';
import * as RadixSelect from '@radix-ui/react-select';
import { Check, ChevronDown } from 'lucide-react';
import { cn } from '../../lib/cn';

export const Select = RadixSelect.Root;
export const SelectGroup = RadixSelect.Group;
export const SelectValue = RadixSelect.Value;

const triggerCls =
  'flex items-center justify-between gap-2 w-full px-3 py-2 rounded-lg bg-surface border border-edge text-white text-sm placeholder-border-hover focus:outline-none focus:border-accent data-[placeholder]:text-border-hover disabled:opacity-50';

export const SelectTrigger = forwardRef<
  React.ElementRef<typeof RadixSelect.Trigger>,
  React.ComponentPropsWithoutRef<typeof RadixSelect.Trigger>
>(function SelectTrigger({ className, children, ...rest }, ref) {
  return (
    <RadixSelect.Trigger ref={ref} className={cn(triggerCls, className)} {...rest}>
      {children}
      <RadixSelect.Icon asChild>
        <ChevronDown size={14} className="text-border-hover shrink-0" />
      </RadixSelect.Icon>
    </RadixSelect.Trigger>
  );
});

export const SelectContent = forwardRef<
  React.ElementRef<typeof RadixSelect.Content>,
  React.ComponentPropsWithoutRef<typeof RadixSelect.Content>
>(function SelectContent({ className, children, position = 'popper', sideOffset = 4, ...rest }, ref) {
  return (
    <RadixSelect.Portal>
      <RadixSelect.Content
        ref={ref}
        position={position}
        sideOffset={sideOffset}
        className={cn(
          'rounded-lg bg-surface border border-edge shadow-xl overflow-hidden z-50 min-w-[var(--radix-select-trigger-width)]',
          className,
        )}
        {...rest}
      >
        <RadixSelect.Viewport className="p-1">{children}</RadixSelect.Viewport>
      </RadixSelect.Content>
    </RadixSelect.Portal>
  );
});

export const SelectItem = forwardRef<
  React.ElementRef<typeof RadixSelect.Item>,
  React.ComponentPropsWithoutRef<typeof RadixSelect.Item>
>(function SelectItem({ className, children, ...rest }, ref) {
  return (
    <RadixSelect.Item
      ref={ref}
      className={cn(
        'flex items-center justify-between gap-2 px-2.5 py-1.5 rounded text-white text-sm cursor-pointer select-none outline-none data-[highlighted]:bg-background data-[disabled]:opacity-50 data-[disabled]:cursor-not-allowed',
        className,
      )}
      {...rest}
    >
      <RadixSelect.ItemText>{children}</RadixSelect.ItemText>
      <RadixSelect.ItemIndicator>
        <Check size={12} className="text-accent" />
      </RadixSelect.ItemIndicator>
    </RadixSelect.Item>
  );
});

export interface SimpleSelectOption<T extends string = string> {
  value: T;
  label: string;
  disabled?: boolean;
}

export interface SimpleSelectProps<T extends string = string> {
  value: T | undefined;
  onValueChange: (value: T) => void;
  options: SimpleSelectOption<T>[];
  placeholder?: string;
  className?: string;
  disabled?: boolean;
  renderValue?: (value: T | undefined) => ReactNode;
}

const EMPTY_SENTINEL = '__ui_empty__';

export function SimpleSelect<T extends string = string>({
  value,
  onValueChange,
  options,
  placeholder,
  className,
  disabled,
  renderValue,
}: SimpleSelectProps<T>) {
  const toRadix = (v: string | undefined) => (v === '' ? EMPTY_SENTINEL : v);
  const fromRadix = (v: string) => (v === EMPTY_SENTINEL ? '' : v);
  return (
    <Select
      value={toRadix(value as string | undefined)}
      onValueChange={(v) => onValueChange(fromRadix(v) as T)}
      disabled={disabled}
    >
      <SelectTrigger className={className}>
        {renderValue ? (
          <span className="truncate">{renderValue(value)}</span>
        ) : (
          <SelectValue placeholder={placeholder} />
        )}
      </SelectTrigger>
      <SelectContent>
        {options.map((o) => (
          <SelectItem
            key={o.value || EMPTY_SENTINEL}
            value={o.value === '' ? EMPTY_SENTINEL : o.value}
            disabled={o.disabled}
          >
            {o.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
