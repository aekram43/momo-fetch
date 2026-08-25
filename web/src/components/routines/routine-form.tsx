"use client";

import { useState } from "react";

import {
  Field,
  Fieldset,
  Row,
  Select,
  TextArea,
  TextInput,
} from "@/components/routines/form-controls";
import type {
  AssigneeOption,
  CatchUp,
  Concurrency,
  PermissionMode,
  Routine,
  RoutineAssignee,
  RoutineUpsert,
  TaskPriority,
} from "@/lib/types";

/**
 * Create or edit one routine.
 *
 * Two decisions worth naming:
 *
 * 1. **The cron helper is a helper, not a wrapper.** The presets write into the
 *    expression field; the field is always the truth. A schedule builder that
 *    hides the expression cannot express what crontab can, and the people most
 *    likely to want a routine are the people who already know the syntax.
 * 2. **The timezone defaults to the browser's, not to UTC.** Somebody typing
 *    "09:00" means nine in the morning where they are. Storing that as UTC is
 *    the bug this field exists to prevent.
 *
 * Draft state is initialised once from `routine`; the parent gives this
 * component a `key` so switching rows remounts it rather than syncing props
 * into state in an effect.
 */

interface Draft {
  name: string;
  assignee: string;
  triggerKind: "cron" | "every" | "manual";
  expression: string;
  timezone: string;
  catchUp: CatchUp;
  everyValue: string;
  everyUnit: "s" | "m" | "h";
  concurrency: Concurrency;
  title: string;
  priority: TaskPriority;
  description: string;
  permission: PermissionMode;
}

const PRESETS: { label: string; expression: string }[] = [
  { label: "Every 15 minutes", expression: "*/15 * * * *" },
  { label: "Every hour", expression: "0 * * * *" },
  { label: "Every day at 03:00", expression: "0 3 * * *" },
  { label: "Every day at 09:00", expression: "0 9 * * *" },
  { label: "Every weekday at 09:00", expression: "0 9 * * 1-5" },
  { label: "Every Monday at 09:00", expression: "0 9 * * mon" },
  { label: "First of the month at 00:00", expression: "0 0 1 * *" },
];

const CUSTOM = "custom";

/** The IANA zone the browser is in, or UTC when it will not say. */
function localZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
  } catch {
    return "UTC";
  }
}

function emptyDraft(): Draft {
  return {
    name: "",
    assignee: "lead",
    triggerKind: "cron",
    expression: "0 * * * *",
    timezone: localZone(),
    catchUp: "skip",
    everyValue: "5",
    everyUnit: "m",
    concurrency: "queue",
    title: "",
    priority: "medium",
    description: "",
    permission: "auto",
  };
}

function toDraft(routine: Routine | null): Draft {
  if (!routine) return emptyDraft();
  const base = emptyDraft();
  const draft: Draft = {
    ...base,
    name: routine.name,
    assignee: routine.assignee,
    concurrency: routine.concurrency,
    title: routine.task.title,
    priority: routine.task.priority,
    description: routine.task.description,
    permission: routine.permission,
  };
  if (routine.trigger.type === "cron") {
    draft.triggerKind = "cron";
    draft.expression = routine.trigger.expression;
    draft.timezone = routine.trigger.timezone;
    draft.catchUp = routine.trigger.catch_up;
  } else if (routine.trigger.type === "every") {
    draft.triggerKind = "every";
    const [value, unit] = splitInterval(routine.trigger.seconds);
    draft.everyValue = String(value);
    draft.everyUnit = unit;
  } else {
    draft.triggerKind = "manual";
  }
  return draft;
}

/** Show 3600s as "1 h" rather than "3600 s". */
function splitInterval(seconds: number): [number, "s" | "m" | "h"] {
  if (seconds % 3600 === 0) return [seconds / 3600, "h"];
  if (seconds % 60 === 0) return [seconds / 60, "m"];
  return [seconds, "s"];
}

function intervalSeconds(draft: Draft): number {
  const value = Number(draft.everyValue);
  if (!Number.isFinite(value) || value <= 0) return 0;
  const factor = draft.everyUnit === "h" ? 3600 : draft.everyUnit === "m" ? 60 : 1;
  return Math.round(value * factor);
}

function parseAssignee(value: string): RoutineAssignee {
  if (value === "lead") return { kind: "lead" };
  const [kind, ...rest] = value.split(":");
  const name = rest.join(":");
  return kind === "worker" ? { kind: "worker", name } : { kind: "agent", name };
}

/** Everything the gateway would reject, said in the form instead. */
function problemWith(draft: Draft): string | null {
  if (!draft.name.trim()) return "Give the routine a name.";
  if (!draft.title.trim()) return "Give the task a title — it is what each run is called.";
  if (!draft.description.trim())
    return "Describe the task. It is the prompt the assignee receives.";
  if (draft.triggerKind === "cron") {
    const fields = draft.expression.trim().split(/\s+/).filter(Boolean);
    if (fields.length !== 5)
      return `A cron expression has five fields (min hour dom month dow) — this has ${fields.length}.`;
    if (!draft.timezone.trim()) return "Give a timezone, e.g. UTC or Asia/Bangkok.";
  }
  if (draft.triggerKind === "every" && intervalSeconds(draft) < 30)
    return "An interval trigger has to be at least 30 seconds.";
  return null;
}

function toUpsert(draft: Draft): RoutineUpsert {
  return {
    name: draft.name.trim(),
    assignee: parseAssignee(draft.assignee),
    trigger:
      draft.triggerKind === "cron"
        ? {
            type: "cron",
            expression: draft.expression.trim().replace(/\s+/g, " "),
            timezone: draft.timezone.trim(),
            catch_up: draft.catchUp,
          }
        : draft.triggerKind === "every"
          ? { type: "every", seconds: intervalSeconds(draft) }
          : { type: "manual" },
    concurrency: draft.concurrency,
    task: {
      title: draft.title.trim(),
      priority: draft.priority,
      description: draft.description.trim(),
    },
    permission: draft.permission,
  };
}

export function RoutineForm({
  routine,
  assignees,
  busy,
  error,
  onSubmit,
  onCancel,
}: {
  /** `null` for a new routine. */
  routine: Routine | null;
  assignees: AssigneeOption[];
  busy: boolean;
  error: string | null;
  onSubmit: (body: RoutineUpsert) => void;
  onCancel: () => void;
}) {
  const [draft, setDraft] = useState<Draft>(() => toDraft(routine));
  const [showedProblem, setShowedProblem] = useState<string | null>(null);
  const set = <K extends keyof Draft>(key: K, value: Draft[K]) =>
    setDraft((d) => ({ ...d, [key]: value }));

  const matchedPreset =
    PRESETS.find((p) => p.expression === draft.expression.trim())?.label ?? CUSTOM;

  // An assignee that has gone away — a worker whose team stopped — must stay
  // visible, or editing an unrelated field would silently reassign the routine.
  const assigneeOptions = assignees.map((a) => ({
    value: a.value,
    label: a.available ? a.label : `${a.label} — unavailable`,
  }));
  if (!assignees.some((a) => a.value === draft.assignee)) {
    assigneeOptions.push({
      value: draft.assignee,
      label: `${draft.assignee} — not currently available`,
    });
  }

  function submit(e: React.FormEvent) {
    e.preventDefault();
    const problem = problemWith(draft);
    setShowedProblem(problem);
    if (problem) return;
    onSubmit(toUpsert(draft));
  }

  return (
    <form onSubmit={submit} className="space-y-4">
      <h3 className="text-sm font-medium text-ink">
        {routine ? `Edit ${routine.name}` : "New routine"}
      </h3>

      <Row>
        <Field label="Name" htmlFor="routine-name">
          <TextInput
            id="routine-name"
            value={draft.name}
            autoFocus
            placeholder="Nightly digest"
            onChange={(e) => set("name", e.target.value)}
          />
        </Field>
        <Field
          label="Assignee"
          hint="Workers appear while a team is running and only if they are standby."
          htmlFor="routine-assignee"
        >
          <Select
            id="routine-assignee"
            value={draft.assignee}
            options={assigneeOptions}
            onChange={(e) => set("assignee", e.target.value)}
          />
        </Field>
      </Row>

      <Row>
        <Field label="Trigger" htmlFor="routine-trigger">
          <Select
            id="routine-trigger"
            value={draft.triggerKind}
            options={[
              { value: "cron", label: "Cron schedule" },
              { value: "every", label: "Every N minutes (heartbeat)" },
              { value: "manual", label: "Manual only" },
            ]}
            onChange={(e) => set("triggerKind", e.target.value as Draft["triggerKind"])}
          />
        </Field>
        <Field
          label="Concurrency"
          hint={
            draft.concurrency === "queue"
              ? "A window that comes due mid-run waits its turn."
              : draft.concurrency === "skip"
                ? "A window that comes due mid-run is dropped."
                : "Runs may overlap."
          }
          htmlFor="routine-concurrency"
        >
          <Select
            id="routine-concurrency"
            value={draft.concurrency}
            options={[
              { value: "queue", label: "Queue" },
              { value: "skip", label: "Skip" },
              { value: "parallel", label: "Parallel" },
            ]}
            onChange={(e) => set("concurrency", e.target.value as Concurrency)}
          />
        </Field>
      </Row>

      {draft.triggerKind === "cron" && (
        <Fieldset caption="Cron helper">
          <Row>
            <Field label="Schedule" htmlFor="routine-preset">
              <Select
                id="routine-preset"
                value={matchedPreset}
                options={[
                  ...PRESETS.map((p) => ({ value: p.label, label: p.label })),
                  { value: CUSTOM, label: "Custom…" },
                ]}
                onChange={(e) => {
                  const preset = PRESETS.find((p) => p.label === e.target.value);
                  if (preset) set("expression", preset.expression);
                }}
              />
            </Field>
            <Field
              label="Timezone (IANA)"
              hint="Where the clock times above are read. Defaults to this machine's zone."
              htmlFor="routine-timezone"
            >
              <TextInput
                id="routine-timezone"
                value={draft.timezone}
                placeholder="UTC"
                onChange={(e) => set("timezone", e.target.value)}
              />
            </Field>
          </Row>

          <Field
            label="Cron expression"
            hint={
              <>
                Five fields: <code className="font-mono">min hour dom month dow</code>. A
                preset above writes into this; edit it directly for anything else.
              </>
            }
            htmlFor="routine-cron"
          >
            <TextInput
              id="routine-cron"
              value={draft.expression}
              placeholder="0 * * * *"
              spellCheck={false}
              onChange={(e) => set("expression", e.target.value)}
            />
          </Field>

          <Field
            label="Catch-up after downtime"
            hint="What a window that passed while nothing was running means."
            htmlFor="routine-catchup"
          >
            <Select
              id="routine-catchup"
              value={draft.catchUp}
              options={[
                { value: "skip", label: "Skip missed windows" },
                { value: "run_once", label: "Run once when back up" },
              ]}
              onChange={(e) => set("catchUp", e.target.value as CatchUp)}
            />
          </Field>
        </Fieldset>
      )}

      {draft.triggerKind === "every" && (
        <Fieldset caption="Heartbeat">
          <Field
            label="Fire every"
            hint="Counted from the last fire. An interval that elapsed while the machine was down is due as soon as it comes back."
            htmlFor="routine-every"
          >
            <div className="flex gap-2">
              <TextInput
                id="routine-every"
                value={draft.everyValue}
                inputMode="numeric"
                onChange={(e) => set("everyValue", e.target.value)}
              />
              <Select
                aria-label="Interval unit"
                value={draft.everyUnit}
                options={[
                  { value: "s", label: "seconds" },
                  { value: "m", label: "minutes" },
                  { value: "h", label: "hours" },
                ]}
                onChange={(e) => set("everyUnit", e.target.value as Draft["everyUnit"])}
              />
            </div>
          </Field>
        </Fieldset>
      )}

      {draft.triggerKind === "manual" && (
        <p className="text-[11px] leading-relaxed text-faint">
          This routine never fires on its own. Use <em>Run now</em>, or{" "}
          <code className="font-mono">momo-fetch routine run</code>.
        </p>
      )}

      <Fieldset caption="Task template">
        <Row>
          <Field label="Task title" htmlFor="routine-title">
            <TextInput
              id="routine-title"
              value={draft.title}
              placeholder="Task each firing creates"
              onChange={(e) => set("title", e.target.value)}
            />
          </Field>
          <Field label="Task priority" htmlFor="routine-priority">
            <Select
              id="routine-priority"
              value={draft.priority}
              options={[
                { value: "low", label: "Low" },
                { value: "medium", label: "Medium" },
                { value: "high", label: "High" },
                { value: "urgent", label: "Urgent" },
              ]}
              onChange={(e) => set("priority", e.target.value as TaskPriority)}
            />
          </Field>
        </Row>
        <Field
          label="Task description"
          hint="This is the prompt. Write it as an instruction the assignee can act on cold — it arrives with no conversation around it."
          htmlFor="routine-description"
        >
          <TextArea
            id="routine-description"
            rows={5}
            value={draft.description}
            placeholder="What the assignee should do each time this fires"
            onChange={(e) => set("description", e.target.value)}
          />
        </Field>
      </Fieldset>

      <Field
        label="Permission"
        hint={
          draft.permission === "yolo"
            ? "Nobody is watching an unattended run. yolo removes the destructive-command guard from it."
            : draft.permission === "strict"
              ? "A scheduled run has no one to confirm tools — in strict it will stop at the first write."
              : "Mutating tools run without asking; destructive shell commands are still refused."
        }
        htmlFor="routine-permission"
      >
        <Select
          id="routine-permission"
          value={draft.permission}
          options={[
            { value: "auto", label: "auto — recommended" },
            { value: "strict", label: "strict" },
            { value: "yolo", label: "yolo" },
          ]}
          onChange={(e) => set("permission", e.target.value as PermissionMode)}
        />
      </Field>

      {(showedProblem || error) && (
        <p role="alert" className="text-[11px] leading-relaxed text-halt">
          {showedProblem ?? error}
        </p>
      )}

      <div className="flex gap-2">
        <button
          type="submit"
          disabled={busy}
          className="rounded bg-signal px-3 py-1 font-mono text-[11px] font-semibold text-void disabled:opacity-40"
        >
          {routine ? "Save" : "Create routine"}
        </button>
        <button
          type="button"
          onClick={onCancel}
          disabled={busy}
          className="rounded border border-rule px-3 py-1 font-mono text-[11px] text-dim transition-colors hover:text-ink disabled:opacity-40"
        >
          Cancel
        </button>
      </div>
    </form>
  );
}
