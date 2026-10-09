import { createUIMessageStream, createUIMessageStreamResponse, type UIMessageChunk } from "ai";
import { vi } from "vitest";

/** サイドカーの /chat の応答を、与えた chunk の列で作る。 */
export function streamResponse(chunks: UIMessageChunk[]): Response {
  return createUIMessageStreamResponse({
    stream: createUIMessageStream({ execute: ({ writer }) => { for (const c of chunks) writer.write(c); } }),
  });
}

/** 前半の chunk を流したあと release() まで止まる応答。回答中の画面を確かめるのに使う。 */
export function pausedStreamResponse(before: UIMessageChunk[], after: UIMessageChunk[]): { response: () => Response; release: () => void } {
  let release!: () => void;
  const gate = new Promise<void>((r) => { release = r; });
  const response = () => createUIMessageStreamResponse({
    stream: createUIMessageStream({
      execute: async ({ writer }) => {
        for (const c of before) writer.write(c);
        await gate;
        for (const c of after) writer.write(c);
      },
    }),
  });
  return { response, release };
}

export const answerChunks = (text: string, extra: UIMessageChunk[] = []): UIMessageChunk[] => [
  { type: "start" },
  ...extra,
  { type: "text-start", id: "t1" },
  { type: "text-delta", id: "t1", delta: text },
  { type: "text-end", id: "t1" },
  { type: "finish" },
] as UIMessageChunk[];

/** fetch を差し替え、呼ばれた要求を記録する。 */
export function mockFetch(...responses: Array<() => Response | Promise<Response>>) {
  const calls: { url: string; init: RequestInit; body: any }[] = [];
  let i = 0;
  const fn = vi.fn(async (url: string, init: RequestInit) => {
    calls.push({ url: String(url), init, body: init.body ? JSON.parse(String(init.body)) : null });
    const make = responses[Math.min(i++, responses.length - 1)];
    return make();
  });
  vi.stubGlobal("fetch", fn);
  return calls;
}
