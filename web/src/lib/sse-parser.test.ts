import { describe, expect, it } from "vitest";

import { SseFrameParser, readStream, toStreamEvent } from "./sse-parser";

/** Feed a whole payload in one push and collect the frames. */
function parseAll(payload: string) {
  const p = new SseFrameParser();
  return [...p.push(payload), ...p.flush()];
}

describe("SseFrameParser", () => {
  it("parses a simple frame", () => {
    const frames = parseAll('event: text\ndata: {"content":"hi"}\n\n');
    expect(frames).toEqual([{ event: "text", data: '{"content":"hi"}', id: undefined }]);
  });

  it("parses several frames from one chunk", () => {
    const frames = parseAll(
      'event: role\ndata: {"a":1}\n\nevent: text\ndata: {"content":"x"}\n\n',
    );
    expect(frames.map((f) => f.event)).toEqual(["role", "text"]);
  });

  // The gateway writes into a socket; chunk boundaries are arbitrary and will
  // land mid-frame in production.
  it("reassembles a frame split across chunks", () => {
    const p = new SseFrameParser();
    expect(p.push("event: te")).toEqual([]);
    expect(p.push('xt\ndata: {"con')).toEqual([]);
    expect(p.push('tent":"hello"}')).toEqual([]);
    const frames = p.push("\n\n");
    expect(frames).toEqual([
      { event: "text", data: '{"content":"hello"}', id: undefined },
    ]);
  });

  it("handles a split landing exactly on the frame boundary", () => {
    const p = new SseFrameParser();
    expect(p.push('event: done\ndata: {"stop_reason":"complete"}\n')).toEqual([]);
    expect(p.push("\n").map((f) => f.event)).toEqual(["done"]);
  });

  it("accepts CRLF line endings", () => {
    const frames = parseAll('event: text\r\ndata: {"content":"crlf"}\r\n\r\n');
    expect(frames).toEqual([
      { event: "text", data: '{"content":"crlf"}', id: undefined },
    ]);
  });

  it("joins multi-line data with newlines", () => {
    const frames = parseAll("event: text\ndata: line1\ndata: line2\ndata: line3\n\n");
    expect(frames[0].data).toBe("line1\nline2\nline3");
  });

  // Observed live: the gateway emits a bare ':' keep-alive while an approval is
  // parked. Treating it as a frame would emit a bogus event.
  it("ignores comment and keep-alive lines", () => {
    const frames = parseAll(':\n\nevent: text\ndata: {"content":"after"}\n\n');
    expect(frames.map((f) => f.event)).toEqual(["text"]);
  });

  it("ignores a comment line inside a frame", () => {
    const frames = parseAll(": keep-alive\nevent: text\ndata: ok\n\n");
    expect(frames).toEqual([{ event: "text", data: "ok", id: undefined }]);
  });

  it("strips exactly one leading space from a value", () => {
    const frames = parseAll("event: text\ndata:  two-spaces\n\n");
    expect(frames[0].data).toBe(" two-spaces");
  });

  it("handles a field with no colon", () => {
    const frames = parseAll("event: text\ndata\n\n");
    expect(frames[0].data).toBe("");
  });

  it("captures the id field", () => {
    const frames = parseAll("id: 42\nevent: text\ndata: x\n\n");
    expect(frames[0].id).toBe("42");
  });

  it("defaults the event name to message when absent", () => {
    expect(parseAll("data: bare\n\n")[0].event).toBe("message");
  });

  // A connection cut after the last frame but before its blank line would
  // otherwise silently lose a `done`, leaving the UI spinning.
  it("flushes a trailing frame with no terminating blank line", () => {
    const p = new SseFrameParser();
    expect(p.push('event: done\ndata: {"stop_reason":"complete"}')).toEqual([]);
    expect(p.flush().map((f) => f.event)).toEqual(["done"]);
  });

  it("flushes nothing when the buffer is only whitespace", () => {
    const p = new SseFrameParser();
    p.push("\n\n");
    expect(p.flush()).toEqual([]);
  });
});

describe("toStreamEvent", () => {
  it("maps a known event and parses its JSON", () => {
    const ev = toStreamEvent({ event: "text", data: '{"content":"hi"}' });
    expect(ev).toEqual({ type: "text", data: { content: "hi" } });
  });

  // Forward compatibility (spec §5): a gateway that gains an event must not go
  // unnoticed by an older UI.
  it("surfaces an unknown event name rather than dropping it", () => {
    const ev = toStreamEvent({ event: "brand_new", data: "{}" });
    expect(ev).toEqual({ type: "unknown", name: "brand_new", raw: "{}" });
  });

  it("surfaces a known event whose JSON is malformed", () => {
    const ev = toStreamEvent({ event: "text", data: "{not json" });
    expect(ev).toEqual({ type: "unknown", name: "text", raw: "{not json" });
  });
});

describe("readStream", () => {
  function responseFrom(chunks: Uint8Array[]): Response {
    const body = new ReadableStream<Uint8Array>({
      start(controller) {
        for (const c of chunks) controller.enqueue(c);
        controller.close();
      },
    });
    return new Response(body);
  }

  const enc = new TextEncoder();

  it("yields typed events from a realistic turn", async () => {
    const res = responseFrom([
      enc.encode('event: role\ndata: {"turn_id":"t1","model":"m"}\n\n'),
      enc.encode('event: text\ndata: {"content":"he"}\n\n:\n\n'),
      enc.encode('event: text\ndata: {"content":"llo"}\n\n'),
      enc.encode('event: done\ndata: {"turn_id":"t1","stop_reason":"complete"}\n\n'),
    ]);

    const seen = [];
    for await (const ev of readStream(res)) seen.push(ev);

    expect(seen.map((e) => e.type)).toEqual(["role", "text", "text", "done"]);
    expect(seen.filter((e) => e.type === "text").map((e) => e.data.content))
      .toEqual(["he", "llo"]);
  });

  // A chunk boundary can fall inside a multi-byte character; decoding each chunk
  // independently would corrupt it. This only exercises the streaming decoder if
  // the cut is genuinely mid-sequence, so assert that first — an off-by-one here
  // makes the test pass while testing nothing.
  it.each([
    { cut: 32, label: "2-byte é" },
    { cut: 38, label: "3-byte →" },
  ])("decodes a UTF-8 character split mid-sequence ($label)", async ({ cut }) => {
    const full = enc.encode('event: text\ndata: {"content":"héllo→ok"}\n\n');
    // A continuation byte is 0b10xxxxxx: the next byte belongs to the character
    // the previous chunk started.
    expect((full[cut] & 0xc0) === 0x80).toBe(true);

    const res = responseFrom([full.slice(0, cut), full.slice(cut)]);

    const seen = [];
    for await (const ev of readStream(res)) seen.push(ev);

    expect(seen).toHaveLength(1);
    expect(seen[0]).toMatchObject({ type: "text", data: { content: "héllo→ok" } });
  });

  it("yields a trailing unterminated frame", async () => {
    const res = responseFrom([
      enc.encode('event: done\ndata: {"turn_id":"t1","stop_reason":"error"}'),
    ]);
    const seen = [];
    for await (const ev of readStream(res)) seen.push(ev);
    expect(seen.map((e) => e.type)).toEqual(["done"]);
  });

  it("throws when the response has no body", async () => {
    const res = new Response(null);
    await expect(async () => {
      for await (const _ of readStream(res)) void _;
    }).rejects.toThrow(/no body/);
  });
});
