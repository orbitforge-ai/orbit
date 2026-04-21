import { forwardRef, type InputHTMLAttributes } from 'react';
import { cn } from '../../lib/cn';
import { inputCls } from '../../lib/taskConstants';

export type InputProps = InputHTMLAttributes<HTMLInputElement>;

export const Input = forwardRef<HTMLInputElement, InputProps>(function Input(
  { className, type = 'text', ...rest },
  ref,
) {
  return <input ref={ref} type={type} className={cn(inputCls, className)} {...rest} />;
});
