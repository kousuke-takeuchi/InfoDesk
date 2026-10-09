import type { UIMessage } from "ai";
import { ChevronDown, History, PlugZap, SquarePen, X } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../api";
import { FEED_RANGES } from "../lib/filters";
import { errorMessage, type ChatThread as Thread } from "../types";
import { ErrorLine } from "../components/ErrorLine";
import { ChatThread } from "./ChatThread";
import type { ChatAttachment, ChatPeriod, ChatScope, ProviderId } from "./context";
import { ThreadMenu } from "./ThreadMenu";
import { useChatEndpoint } from "./useChatEndpoint";

interface Props {
  scope: ChatScope;
  period: ChatPeriod | null;
  attachments: ChatAttachment[];
  onRemoveAttachment(i: number): void;
  onClose(): void;
  onOpenSettings(tab: "llm"): void;
}

const PERIOD_LABEL = { day: "今日", week: "今週", month: "今月" } as const;
const TITLE_CHARS = 30;
const SUGGESTIONS: Record<string, string[]> = {
  default: ["今日の主な動きを3つにまとめて", "先週と比べて新しく出てきた話題は?", "気になる記事を3つ選んで理由を説明して"],
  github: ["今日のGitHubで伸びたリポジトリを3つ選んで理由を説明して", "エージェント関連のリポジトリだけまとめて", "先週と比べて新しく出てきた話題は?"],
  medcare: ["今週の認知症関連の動きをまとめて", "介護報酬の議論の論点を整理して", "新薬や治療法の話題だけ抜き出して"],
};

function periodChip(p: ChatPeriod | null): string | null {
  if (!p) return null;
  if (p.view === "digest") return `期間: ${PERIOD_LABEL[p.period]}`;
  return `期間: ${FEED_RANGES.find((r) => r.id === p.range)?.label ?? p.range}`;
}

function scopeChip(s: ChatScope): string {
  return s.kind === "all" ? "すべて" : s.label;
}

function firstUserText(messages: UIMessage[]): string {
  const m = messages.find((x) => x.role === "user");
  const part = m?.parts.find((p) => p.type === "text") as { text?: string } | undefined;
  return (part?.text ?? "").trim();
}

export function ChatPanel(props: Props) {
  const { endpoint, restart } = useChatEndpoint();
  const [threads, setThreads] = useState<Thread[]>([]);
  type Current = { id: number | null; key: string; messages: UIMessage[] };
  const [current, setCurrentState] = useState<Current>({ id: null, key: "new-0", messages: [] });
  // 保存コールバックが古い id を掴まないよう、最新の値を参照でも持つ。
  const currentRef = useRef(current);
  const setCurrent = useCallback((next: Current | ((c: Current) => Current)) => {
    currentRef.current = typeof next === "function" ? next(currentRef.current) : next;
    setCurrentState(currentRef.current);
  }, []);
  const [menuOpen, setMenuOpen] = useState(false);
  const [provider, setProvider] = useState<ProviderId>("ollama");
  const [model, setModel] = useState("");
  const [error, setError] = useState<string | null>(null);

  const loadThreads = useCallback(() => {
    api.listChatThreads().then(setThreads).catch((e) => setError(errorMessage(e)));
  }, []);
  useEffect(() => {
    loadThreads();
    api.getSettings()
      .then((s) => {
        setProvider(s["llm.chat_provider"] === "claude" ? "claude" : "ollama");
        setModel(s["ollama.model"] ?? "");
      })
      .catch((e) => setError(errorMessage(e)));
  }, [loadThreads]);

  const newThread = () => {
    setMenuOpen(false);
    setCurrent((c) => ({ id: null, key: `new-${Number(c.key.split("-")[1] ?? 0) + 1}`, messages: [] }));
  };
  const openThread = (id: number) => {
    setMenuOpen(false);
    api.getChatMessages(id)
      .then((messages) => setCurrent({ id, key: `thread-${id}`, messages }))
      .catch((e) => setError(errorMessage(e)));
  };
  const deleteThread = (id: number) => {
    api.deleteChatThread(id)
      .then(() => {
        if (currentRef.current.id === id) newThread();
        loadThreads();
      })
      .catch((e) => setError(errorMessage(e)));
  };

  // スレッドは最初の回答を保存するときに作る。空のスレッドを残さないため。
  const persist = useCallback(async (messages: UIMessage[]) => {
    try {
      const key = currentRef.current.key;
      let id = currentRef.current.id;
      if (id === null) {
        const title = firstUserText(messages).slice(0, TITLE_CHARS) || "新しいスレッド";
        const t = await api.createChatThread(title);
        id = t.id;
        // 作成を待つあいだに別のスレッドへ切り替わっていたら、その表示は変えない。
        setCurrent((c) => (c.key === key ? { ...c, id: t.id } : c));
      }
      await api.saveChatMessages(id, messages);
      loadThreads();
    } catch (e) {
      setError(errorMessage(e));
    }
  }, [loadThreads, setCurrent]);

  const title = threads.find((t) => t.id === current.id)?.title ?? "新しいスレッド";
  const suggestions = SUGGESTIONS[props.scope.kind === "category" ? props.scope.category : "default"] ?? SUGGESTIONS.default;
  const chips = [scopeChip(props.scope), periodChip(props.period)].filter(Boolean) as string[];

  return (
    <aside className="chat-panel" aria-label="チャット">
      <header className="chat-header">
        <button className="chat-title" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}>
          {title}<ChevronDown size={13} aria-hidden />
        </button>
        <button className="icon-btn" aria-label="新規スレッド" onClick={newThread}><SquarePen size={15} /></button>
        <button className="icon-btn" aria-label="履歴" aria-expanded={menuOpen} onClick={() => setMenuOpen((v) => !v)}><History size={15} /></button>
        <button className="icon-btn" aria-label="チャットを閉じる" onClick={props.onClose}><X size={15} /></button>
      </header>
      {menuOpen && <ThreadMenu threads={threads} currentId={current.id} onSelect={openThread} onDelete={deleteThread} />}
      <div className="chat-context" role="group" aria-label="文脈">
        <span className="text-tertiary">文脈</span>
        {chips.map((c) => <span key={c} className="chip">{c}</span>)}
        {props.attachments.map((a, i) => {
          const label = a.type === "item" ? a.title : `${a.label} の要約`;
          return (
            <span key={`${a.type}-${i}`} className="chip">
              {label}
              <button className="chip-x" aria-label={`${label} を文脈から外す`} onClick={() => props.onRemoveAttachment(i)}><X size={11} /></button>
            </span>
          );
        })}
      </div>
      <ErrorLine message={error} />
      {endpoint.status === "starting" && <div className="chat-state text-secondary">エージェントを起動しています…</div>}
      {endpoint.status === "error" && (
        <div className="chat-state-box">
          <div className="chat-error-title"><PlugZap size={15} aria-hidden />エージェントを起動できません</div>
          <p>{endpoint.message}</p>
          <p className="text-secondary">ダイジェストとフィードはこのまま使えます。</p>
          <div className="chat-error-actions">
            <button className="btn btn-primary" onClick={() => props.onOpenSettings("llm")}>Node のパスを設定</button>
            <button className="btn" onClick={restart}>再試行</button>
          </div>
        </div>
      )}
      {endpoint.status === "ready" && (
        <ChatThread
          key={`${current.key}|${endpoint.url}`}
          endpoint={{ url: endpoint.url, token: endpoint.token }}
          chatKey={current.key}
          initialMessages={current.messages}
          provider={provider}
          modelLabel={model ? `Ollama · ${model}` : "Ollama"}
          context={{ scope: props.scope, period: props.period, attachments: props.attachments }}
          onProviderChange={setProvider}
          onPersist={persist}
          onOpenSettings={() => props.onOpenSettings("llm")}
          onRestartAgent={restart}
          suggestions={suggestions}
        />
      )}
    </aside>
  );
}
