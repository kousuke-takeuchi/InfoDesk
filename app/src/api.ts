import type { UIMessage } from "ai";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AppStatus, Category, ChatEndpoint, ChatThread, DigestScope, DigestStats, DigestView, Item, ItemFilter, Period,
  ProbeResult, Settings, SidebarCounts, Source, SyncStatus, Topic,
} from "./types";

export const EVENTS = {
  status: "status-changed",
  items: "items-changed",
  digests: "digests-changed",
  openSettings: "open-settings",
  chat: "chat-endpoint-changed",
  reader: "reader-state",
  sync: "sync-changed",
} as const;

export const api = {
  getStatus: () => invoke<AppStatus>("get_status"),
  listItems: (filter: ItemFilter) => invoke<Item[]>("list_items", { filter }),
  countItems: (filter: ItemFilter) => invoke<number>("count_items", { filter }),
  getItem: (id: number) => invoke<Item | null>("get_item", { id }),
  setRead: (id: number, read: boolean) => invoke<void>("set_read", { id, read }),
  setStarred: (id: number, starred: boolean) => invoke<void>("set_starred", { id, starred }),
  markAllRead: (filter: ItemFilter) => invoke<number>("mark_all_read", { filter }),
  openItem: (id: number) => invoke<void>("open_item", { id }),
  openLink: (url: string) => invoke<void>("open_link", { url }),
  openInReader: (url: string) => invoke<void>("open_in_reader", { url }),
  readerBack: () => invoke<void>("reader_back"),
  readerForward: () => invoke<void>("reader_forward"),
  readerReload: () => invoke<void>("reader_reload"),
  readerOpenExternal: () => invoke<void>("reader_open_external"),
  readerClose: () => invoke<void>("reader_close"),
  readerState: () => invoke<{ url: string; title: string }>("reader_state"),
  sidebarCounts: () => invoke<SidebarCounts>("sidebar_counts"),
  listSources: () => invoke<Source[]>("list_sources"),
  addRssSource: (url: string, name: string | null, category: Category) =>
    invoke<Source>("add_rss_source", { url, name, category }),
  updateSource: (id: number, name: string, category: Category, config: string) =>
    invoke<void>("update_source", { id, name, category, config }),
  setSourceEnabled: (id: number, enabled: boolean) => invoke<void>("set_source_enabled", { id, enabled }),
  deleteSource: (id: number) => invoke<void>("delete_source", { id }),
  /** ソース ID (文字列キー) ごとの保存済み記事数。記事のないソースは含まれない。 */
  sourceItemCounts: () => invoke<Record<string, number>>("source_item_counts"),
  listTopics: () => invoke<Topic[]>("list_topics"),
  addTopic: (name: string, keywords: string[]) => invoke<Topic>("add_topic", { name, keywords }),
  updateTopic: (id: number, name: string, keywords: string[]) => invoke<void>("update_topic", { id, name, keywords }),
  deleteTopic: (id: number) => invoke<void>("delete_topic", { id }),
  getDigests: (period: Period, date: string, topicId: number | null) =>
    invoke<DigestView[]>("get_digests", { period, date, topicId }),
  digestStats: (period: Period, date: string, topicId: number | null) =>
    invoke<DigestStats>("digest_stats", { period, date, topicId }),
  /** 指定した scope を1つのジョブで順に再生成する。 */
  regenerateDigest: (period: Period, date: string, scopes: DigestScope[]) =>
    invoke<void>("regenerate_digest", { period, date, scopes }),
  collectNow: () => invoke<void>("collect_now"),
  retryPendingDigests: () => invoke<void>("retry_pending_digests"),
  getSettings: () => invoke<Settings>("get_settings"),
  setSetting: (key: string, value: string) => invoke<void>("set_setting", { key, value }),
  testOllama: () => invoke<ProbeResult>("test_ollama"),
  testClaude: () => invoke<ProbeResult>("test_claude"),
  listChatThreads: () => invoke<ChatThread[]>("list_chat_threads"),
  createChatThread: (title: string) => invoke<ChatThread>("create_chat_thread", { title }),
  renameChatThread: (id: number, title: string) => invoke<void>("rename_chat_thread", { id, title }),
  deleteChatThread: (id: number) => invoke<void>("delete_chat_thread", { id }),
  getChatMessages: (threadId: number) => invoke<UIMessage[]>("get_chat_messages", { threadId }),
  saveChatMessages: (threadId: number, messages: UIMessage[]) =>
    invoke<void>("save_chat_messages", { threadId, messages }),
  chatEndpoint: () => invoke<ChatEndpoint>("chat_endpoint"),
  restartChat: () => invoke<void>("restart_chat"),
  syncStatus: () => invoke<SyncStatus>("sync_status"),
  syncLogin: () => invoke<SyncStatus>("sync_login"),
  syncLogout: () => invoke<SyncStatus>("sync_logout"),
  syncSetEnabled: (enabled: boolean) => invoke<SyncStatus>("sync_set_enabled", { enabled }),
  syncSetDeviceName: (name: string) => invoke<SyncStatus>("sync_set_device_name", { name }),
  syncSetBackend: (backend: string) => invoke<SyncStatus>("sync_set_backend", { backend }),
  syncSetNas: (url: string, user: string, password: string | null) => invoke<SyncStatus>("sync_set_nas", { url, user, password }),
  tailnetLogin: () => invoke<SyncStatus>("tailnet_login"),
  tailnetLogout: () => invoke<SyncStatus>("tailnet_logout"),
  tailnetForget: () => invoke<SyncStatus>("tailnet_forget"),
  syncTestNas: () => invoke<string>("sync_test_nas"),
  syncNow: () => invoke<void>("sync_now"),
  setLaunchAtLogin: (enabled: boolean) => invoke<void>("set_launch_at_login", { enabled }),
};

export function onEvent(name: string, handler: () => void): Promise<UnlistenFn> {
  return listen(name, () => handler());
}
