import { useChat } from "@ai-sdk/react";
import { AssistantRuntimeProvider, ComposerPrimitive, MessagePrimitive, ThreadPrimitive } from "@assistant-ui/react";
import { MarkdownTextPrimitive } from "@assistant-ui/react-markdown";
import { useAISDKRuntime } from "@assistant-ui/react-ai-sdk";
import { DefaultChatTransport, type UIMessage } from "ai";
import { AlertCircle, ArrowUp, CornerDownRight, Sparkles } from "lucide-react";
import { useMemo, useRef } from "react";
import { api } from "../api";
import type { ChatContext, ProviderId } from "./context";
import { parseChatError } from "./errors";
import { referencedItems } from "./references";
import { ToolCall } from "./ToolCall";

export interface ChatThreadProps {
  endpoint: { url: string; token: string };
  chatKey: string;
  initialMessages: UIMessage[];
  provider: ProviderId;
  modelLabel: string;
  context: ChatContext;
  onProviderChange(p: ProviderId): void;
  onPersist(messages: UIMessage[]): Promise<void>;
  onOpenSettings(): void;
  /** サイドカーに接続できないときの「エージェントを再起動」。 */
  onRestartAgent?: () => void;
  suggestions: string[];
}

const OTHER: Record<ProviderId, ProviderId> = { ollama: "claude", claude: "ollama" };
const PROVIDER_NAME: Record<ProviderId, string> = { ollama: "Ollama", claude: "Claude" };

function MarkdownText() {
  return <MarkdownTextPrimitive className="chat-markdown" />;
}

function References({ message }: { message: UIMessage }) {
  const refs = referencedItems(message);
  if (refs.length === 0) return null;
  return (
    <div className="chat-refs">
      {refs.map((r) => (
        <button key={r.id} className="chat-ref" onClick={() => void api.openItem(r.id)}>{r.title}</button>
      ))}
    </div>
  );
}

export function ChatThread(props: ChatThreadProps) {
  const { endpoint, chatKey, initialMessages, provider, context, onPersist } = props;
  // 送信時の値を、transport を作り直さずに読むための参照。
  const latest = useRef({ provider, context });
  latest.current = { provider, context };
  const transport = useMemo(
    () => new DefaultChatTransport<UIMessage>({
      api: `${endpoint.url}/chat`,
      headers: { Authorization: `Bearer ${endpoint.token}` },
      body: () => ({ provider: latest.current.provider, context: latest.current.context }),
    }),
    [endpoint.url, endpoint.token],
  );
  const chat = useChat<UIMessage>({
    id: chatKey,
    messages: initialMessages,
    transport,
    onFinish: ({ messages, isError, isAbort }) => {
      if (!isError && !isAbort) void onPersist(messages);
    },
  });
  const runtime = useAISDKRuntime(chat);
  const error = parseChatError(chat.error);
  const lastAssistant = [...chat.messages].reverse().find((m) => m.role === "assistant");

  const retry = () => { chat.clearError(); void chat.regenerate(); };
  const switchAndRetry = (p: ProviderId) => {
    props.onProviderChange(p);
    latest.current = { ...latest.current, provider: p };
    retry();
  };

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      <ThreadPrimitive.Root className="chat-thread">
        <ThreadPrimitive.Viewport className="chat-viewport">
          <ThreadPrimitive.Empty>
            <div className="chat-empty">
              <Sparkles size={22} aria-hidden />
              <h3>収集した記事について質問できます</h3>
              <p className="text-secondary">表示中のカテゴリーと期間を文脈にして答えます。例:</p>
              {props.suggestions.map((s) => (
                <button key={s} className="chat-suggestion" onClick={() => void chat.sendMessage({ text: s })}>
                  <CornerDownRight size={13} aria-hidden />{s}
                </button>
              ))}
            </div>
          </ThreadPrimitive.Empty>
          <ThreadPrimitive.Messages
            components={{
              UserMessage: () => (
                <MessagePrimitive.Root className="chat-msg user"><MessagePrimitive.Parts /></MessagePrimitive.Root>
              ),
              AssistantMessage: () => (
                <MessagePrimitive.Root className="chat-msg assistant">
                  <MessagePrimitive.Parts components={{ Text: MarkdownText, tools: { Fallback: ToolCall } }} />
                </MessagePrimitive.Root>
              ),
            }}
          />
          {lastAssistant && chat.status === "ready" && <References message={lastAssistant} />}
          {error && (
            <div className="chat-error" role="alert">
              <div className="chat-error-title"><AlertCircle size={15} aria-hidden />{error.title}</div>
              <p><span>{error.message}</span> 質問はそのまま残っています。</p>
              <div className="chat-error-actions">
                <button className="btn btn-primary" onClick={retry}>再試行</button>
                {error.kind === "network" ? (
                  props.onRestartAgent && <button className="btn" onClick={props.onRestartAgent}>エージェントを再起動</button>
                ) : (
                  <button className="btn" onClick={() => switchAndRetry(OTHER[error.provider ?? provider])}>
                    {PROVIDER_NAME[OTHER[error.provider ?? provider]]} で送る
                  </button>
                )}
                <button className="btn" onClick={props.onOpenSettings}>LLM設定</button>
              </div>
            </div>
          )}
        </ThreadPrimitive.Viewport>
        <ComposerPrimitive.Root className="chat-composer">
          <ComposerPrimitive.Input aria-label="質問" placeholder="記事や要約について質問…" className="chat-input" />
          <div className="chat-composer-row">
            <select aria-label="LLM" value={provider} onChange={(e) => props.onProviderChange(e.target.value as ProviderId)}>
              <option value="ollama">{provider === "ollama" ? props.modelLabel : "Ollama"}</option>
              <option value="claude">Claude</option>
            </select>
            <ComposerPrimitive.Send aria-label="送信" className="chat-send"><ArrowUp size={15} /></ComposerPrimitive.Send>
          </div>
        </ComposerPrimitive.Root>
      </ThreadPrimitive.Root>
    </AssistantRuntimeProvider>
  );
}
