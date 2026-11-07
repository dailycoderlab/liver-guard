import { ChevronDown, ChevronUp } from 'lucide-react';
import { forwardRef, useState, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes, type TextareaHTMLAttributes } from 'react';
import { cn } from '@/lib/cn';
import { clampTo, isNumberDraft, parseDraft, stepNumber } from '@/lib/number';

type Variant = 'default' | 'primary' | 'ghost' | 'danger';

const VARIANTS: Record<Variant, string> = {
  default: 'bg-raised border-line hover:bg-[#263043] text-ink',
  primary: 'bg-accent border-accent hover:brightness-110 text-white',
  ghost: 'bg-transparent border-transparent hover:bg-raised text-ink',
  danger: 'bg-red-600/90 border-red-600 hover:bg-red-600 text-white',
};

export const Button = forwardRef<HTMLButtonElement, ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; small?: boolean }>(
  ({ variant = 'default', small, className, ...rest }, ref) => (
    <button
      ref={ref}
      {...rest}
      className={cn(
        'inline-flex items-center justify-center gap-1.5 rounded-md border whitespace-nowrap transition-colors disabled:opacity-40 disabled:pointer-events-none',
        small ? 'h-6 px-2 text-xs' : 'h-8 px-3',
        VARIANTS[variant],
        className,
      )}
    />
  ),
);
Button.displayName = 'Button';

const FIELD = 'w-full rounded-md border border-line bg-canvas px-2 text-ink outline-none focus:border-accent placeholder:text-mute/70';

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(({ className, ...rest }, ref) => (
  <input ref={ref} {...rest} className={cn(FIELD, 'h-8', className)} />
));
Input.displayName = 'Input';

export const TextArea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(({ className, ...rest }, ref) => (
  <textarea ref={ref} {...rest} className={cn(FIELD, 'py-1.5 resize-y min-h-16', className)} />
));
TextArea.displayName = 'TextArea';

export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(({ className, ...rest }, ref) => (
  <select ref={ref} {...rest} className={cn(FIELD, 'h-8', className)} />
));
Select.displayName = 'Select';

export function Field({ label, hint, children, className }: { label: string; hint?: string; children: ReactNode; className?: string }) {
  return (
    <label className={cn('flex flex-col gap-1', className)}>
      <span className="text-xs text-mute">{label}</span>
      {children}
      {hint && <span className="text-[11px] text-mute/80">{hint}</span>}
    </label>
  );
}

/**
 * A number field that keeps what is typed (so it can be emptied and retyped), reports
 * only complete numbers, and draws its own steppers: WebKit shows the native ones only
 * on hover, and not at all in the packaged desktop window.
 */
export function NumberInput({ value, onChange, onBlur, min, max, step = 1, className }: { value: number; onChange: (v: number) => void; /** Editing finished (focus left the field). */ onBlur?: () => void; min?: number; max?: number; step?: number; className?: string }) {
  const [draft, setDraft] = useState<string | null>(null);
  const shown = draft ?? (Number.isFinite(value) ? String(value) : '');

  const commit = (next: number) => {
    onChange(clampTo(next, min, max));
  };
  const bump = (direction: 1 | -1) => {
    const next = stepNumber(Number.isFinite(value) ? value : 0, direction, step, min, max);
    setDraft(null);
    commit(next);
  };

  return (
    <div className="relative">
      <Input
        type="text"
        inputMode="decimal"
        className={cn('pr-6', className)}
        value={shown}
        onChange={(e) => {
          const text = e.target.value;
          if (!isNumberDraft(text)) return;
          setDraft(text);
          const parsed = parseDraft(text);
          if (parsed !== null) commit(parsed);
        }}
        onBlur={() => {
          setDraft(null);
          onBlur?.();
        }}
        onKeyDown={(e) => {
          if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            bump(e.key === 'ArrowUp' ? 1 : -1);
          }
        }}
      />
      <div className="absolute inset-y-px right-px flex w-5 flex-col overflow-hidden rounded-r-md border-l border-line">
        {([1, -1] as const).map((direction) => (
          <button
            key={direction}
            type="button"
            tabIndex={-1}
            aria-label={direction === 1 ? '增加' : '减少'}
            // Keep focus (and the draft) in the text field.
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => bump(direction)}
            className="flex flex-1 items-center justify-center text-mute transition-colors hover:bg-raised hover:text-ink disabled:pointer-events-none disabled:opacity-40"
          >
            {direction === 1 ? <ChevronUp size={10} /> : <ChevronDown size={10} />}
          </button>
        ))}
      </div>
    </div>
  );
}

export function Section({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="border-b border-line px-3 py-3">
      <div className="mb-2 flex items-center justify-between">
        <h3 className="text-xs font-medium tracking-wide text-mute">{title}</h3>
        {action}
      </div>
      <div className="flex flex-col gap-2.5">{children}</div>
    </section>
  );
}

export function Empty({ children }: { children: ReactNode }) {
  return <div className="flex h-full items-center justify-center p-6 text-center text-mute">{children}</div>;
}
