"use client";

/**
 * Form chrome for the routine editor.
 *
 * Extracted only because the editor has eighteen controls and inlining the
 * label/hint/spacing on each one buries the actual form logic. Nothing here
 * knows anything about routines.
 */

export function Field({
  label,
  hint,
  htmlFor,
  children,
}: {
  label: string;
  hint?: React.ReactNode;
  htmlFor?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="min-w-0">
      <label
        htmlFor={htmlFor}
        className="mb-1 block text-[11px] font-semibold uppercase tracking-[0.14em] text-dim"
      >
        {label}
      </label>
      {children}
      {hint && <p className="mt-1 text-[10px] leading-relaxed text-faint">{hint}</p>}
    </div>
  );
}

/** Two fields side by side above `sm`, stacked below it. */
export function Row({ children }: { children: React.ReactNode }) {
  return <div className="grid gap-3 sm:grid-cols-2">{children}</div>;
}

/**
 * A labelled box around a group of fields — the cron helper, the task
 * template. The caption is what tells you *why* these fields appeared.
 */
export function Fieldset({
  caption,
  children,
}: {
  caption: string;
  children: React.ReactNode;
}) {
  return (
    <fieldset className="rounded border border-rule bg-void/40 p-3">
      <legend className="px-1 font-mono text-[10px] uppercase tracking-[0.14em] text-faint">
        {caption}
      </legend>
      <div className="space-y-3">{children}</div>
    </fieldset>
  );
}

const CONTROL =
  "w-full min-w-0 rounded border border-rule bg-void px-2 py-1 font-mono text-[11px] text-ink outline-none placeholder:text-faint focus:border-dim disabled:opacity-40";

export function TextInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input type="text" {...props} className={CONTROL} />;
}

export function TextArea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...props} className={`${CONTROL} resize-y leading-relaxed`} />;
}

export function Select({
  options,
  ...props
}: React.SelectHTMLAttributes<HTMLSelectElement> & {
  options: { value: string; label: string; disabled?: boolean }[];
}) {
  return (
    <select {...props} className={CONTROL}>
      {options.map((o) => (
        <option key={o.value} value={o.value} disabled={o.disabled}>
          {o.label}
        </option>
      ))}
    </select>
  );
}
