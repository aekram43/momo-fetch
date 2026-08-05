/**
 * F27 — optional audio cues. Off by default.
 *
 * Synthesised with WebAudio rather than shipped as files: three short tones cost
 * nothing to generate, and bundling audio into a static export for a feature
 * most users leave off is not a trade worth making.
 *
 * Three distinguishable cues, pitched by urgency:
 *   - `approval` rises  — you are being asked for something, act
 *   - `done` falls      — finished, nothing needed
 *   - `error` is low    — something went wrong
 */

export type Cue = "approval" | "done" | "error";

const TONES: Record<Cue, { freq: number[]; duration: number }> = {
  approval: { freq: [660, 880], duration: 0.12 },
  done: { freq: [880, 660], duration: 0.1 },
  error: { freq: [220, 180], duration: 0.18 },
};

let ctx: AudioContext | null = null;

export function playCue(cue: Cue, enabled: boolean): void {
  if (!enabled || typeof window === "undefined") return;
  // Respect the same preference that governs motion — someone who has asked the
  // OS to calm things down did not ask for beeps either.
  if (window.matchMedia?.("(prefers-reduced-motion: reduce)").matches) return;

  try {
    // Created lazily: constructing an AudioContext before a user gesture is
    // blocked by autoplay policy and logs a console warning on every load.
    ctx ??= new AudioContext();
    if (ctx.state === "suspended") void ctx.resume();

    const { freq, duration } = TONES[cue];
    const now = ctx.currentTime;

    freq.forEach((f, i) => {
      const osc = ctx!.createOscillator();
      const gain = ctx!.createGain();
      osc.type = "sine";
      osc.frequency.value = f;
      // Short attack/decay envelope — a raw gate click is unpleasant.
      const start = now + i * duration;
      gain.gain.setValueAtTime(0, start);
      gain.gain.linearRampToValueAtTime(0.06, start + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, start + duration);
      osc.connect(gain).connect(ctx!.destination);
      osc.start(start);
      osc.stop(start + duration);
    });
  } catch {
    // No audio device, blocked context, or an unsupported browser. A missing
    // beep is never worth an exception.
  }
}
