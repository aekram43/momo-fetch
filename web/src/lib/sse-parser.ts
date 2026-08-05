/**
 * SSE frame parser for the V2 stream.
 *
 * **Why hand-rolled.** `EventSource` is GET-only and cannot set headers;
 * `/v2/chat/stream` is a POST with a JSON body and an optional bearer token
 * (spec C3). So: `fetch` + `ReadableStream` + this.
 *
 * **This file is the most likely source of silent breakage in the frontend.**
 * Its failure mode is losing events, not throwing — a dropped `done` leaves the
 * UI spinning forever, a dropped `approval_required` strands a turn until it
 * times out. Every branch below is covered in `sse-parser.test.ts`; keep it that
 * way.
 *
 * Handles, because the real gateway emits all of them:
 *   - frames split across arbitrary chunk boundaries (a frame can even be split
 *     mid-UTF-8-sequence, hence the streaming TextDecoder)
 *   - `\r\n` as well as `\n`
 *   - multi-line `data:` (joined with `\n`, per the SSE spec)
 *   - `:` comment / keep-alive lines — observed live during approval waits
 *   - unknown `event:` names, which are surfaced rather than dropped
 */

import { sseEventNames, type SseEventName, type StreamEvent } from "./types";

/** One decoded SSE frame, before it is mapped onto a typed event. */
export interface RawFrame {
  event: string;
  data: string;
  id?: string;
}

const knownEvents = new Set<string>(sseEventNames);

/**
 * Incremental SSE frame decoder.
 *
 * Feed it string chunks; it returns whichever frames completed. Bytes that do
 * not yet form a whole frame stay buffered.
 */
export class SseFrameParser {
  private buffer = "";

  /** Push a chunk of decoded text; get back any frames it completed. */
  push(chunk: string): RawFrame[] {
    // Normalise line endings up front so the split below only has one case to
    // consider. Doing this per-chunk is safe: a `\r\n` split across two chunks
    // leaves a trailing `\r` in the buffer, which the next push resolves.
    this.buffer += chunk;

    const frames: RawFrame[] = [];

    // Frames are separated by a blank line. Tolerate \r\n\r\n and \n\n, and a
    // stray \r before the boundary.
    const boundary = /\r?\n\r?\n/;
    let match: RegExpExecArray | null;

    while ((match = boundary.exec(this.buffer)) !== null) {
      const block = this.buffer.slice(0, match.index);
      this.buffer = this.buffer.slice(match.index + match[0].length);
      const frame = parseBlock(block);
      if (frame) frames.push(frame);
    }

    return frames;
  }

  /**
   * Flush a trailing frame that arrived without its terminating blank line.
   *
   * A well-behaved server always terminates, but a connection cut mid-stream can
   * leave one behind, and silently discarding it loses a `done`.
   */
  flush(): RawFrame[] {
    const rest = this.buffer.trim();
    this.buffer = "";
    if (!rest) return [];
    const frame = parseBlock(rest);
    return frame ? [frame] : [];
  }
}

/** Parse one frame block (no trailing blank line) into a frame. */
function parseBlock(block: string): RawFrame | null {
  let event = "message";
  const dataLines: string[] = [];
  let id: string | undefined;
  let sawField = false;

  for (const rawLine of block.split(/\r?\n/)) {
    // A line starting with ':' is a comment. The gateway sends bare ':' as a
    // keep-alive during approval waits — dropping these is required, and
    // treating one as a frame would emit a bogus event.
    if (rawLine.startsWith(":")) continue;

    const colon = rawLine.indexOf(":");
    let field: string;
    let value: string;

    if (colon === -1) {
      // Field with no value, e.g. a bare `data`.
      field = rawLine;
      value = "";
    } else {
      field = rawLine.slice(0, colon);
      value = rawLine.slice(colon + 1);
      // Exactly one leading space is stripped, per the SSE spec. Not trimmed —
      // further spaces are part of the value.
      if (value.startsWith(" ")) value = value.slice(1);
    }

    switch (field) {
      case "event":
        event = value;
        sawField = true;
        break;
      case "data":
        dataLines.push(value);
        sawField = true;
        break;
      case "id":
        id = value;
        sawField = true;
        break;
      default:
        // Unknown field (including `retry`) — ignore, per spec.
        break;
    }
  }

  if (!sawField) return null;
  // Multi-line data is joined with newlines, not concatenated.
  return { event, data: dataLines.join("\n"), id };
}

/**
 * Map a raw frame onto a typed {@link StreamEvent}.
 *
 * Unknown event names and unparseable JSON both resolve to an `unknown` event
 * rather than being discarded, so a gateway/frontend type drift is visible in
 * the UI instead of manifesting as an event that never arrives.
 */
export function toStreamEvent(frame: RawFrame): StreamEvent {
  if (!knownEvents.has(frame.event)) {
    return { type: "unknown", name: frame.event, raw: frame.data };
  }
  try {
    const data = JSON.parse(frame.data);
    return { type: frame.event as SseEventName, data } as StreamEvent;
  } catch {
    return { type: "unknown", name: frame.event, raw: frame.data };
  }
}

/**
 * Consume a `fetch` response body as typed stream events.
 *
 * Usage:
 * ```ts
 * for await (const ev of readStream(res)) { ... }
 * ```
 * Cancel by aborting the `AbortController` passed to `fetch`; the loop ends.
 */
export async function* readStream(
  response: Response,
): AsyncGenerator<StreamEvent, void, unknown> {
  if (!response.body) {
    throw new Error("Response has no body — cannot stream.");
  }

  const reader = response.body.getReader();
  // `stream: true` matters: a multi-byte character can straddle a chunk
  // boundary, and decoding each chunk independently would corrupt it.
  const decoder = new TextDecoder("utf-8");
  const parser = new SseFrameParser();

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      for (const frame of parser.push(decoder.decode(value, { stream: true }))) {
        yield toStreamEvent(frame);
      }
    }
    // Trailing bytes, then any frame left without its blank-line terminator.
    for (const frame of parser.push(decoder.decode())) {
      yield toStreamEvent(frame);
    }
    for (const frame of parser.flush()) {
      yield toStreamEvent(frame);
    }
  } finally {
    // Releasing matters on the abort path: without it the connection can be
    // held open after the consumer walks away.
    reader.releaseLock();
  }
}
