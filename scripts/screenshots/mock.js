(() => {
  const now = Date.now(), iso = (minAgo) => new Date(now - minAgo * 60000).toISOString();
  const q = new URLSearchParams(location.search);
  const scenario = q.get("s") || "normal";
  const sources = [
    { id: 1, kind: "github_trending", name: "GitHub Trending (日)", category: "github", config: "daily", enabled: true, last_fetched_at: iso(40), last_error: null },
    { id: 4, kind: "arxiv", name: "arXiv cs.AI", category: "arxiv", config: "cs.AI", enabled: true, last_fetched_at: iso(40), last_error: null },
    { id: 7, kind: "hf_models", name: "Hugging Face Trending Models", category: "huggingface", config: "trending", enabled: true, last_fetched_at: iso(40), last_error: null },
    { id: 9, kind: "hackernews", name: "Hacker News", category: "hackernews", config: "top", enabled: true, last_fetched_at: iso(40), last_error: null },
    { id: 10, kind: "rss", name: "はてなブックマーク IT", category: "tech_jp", config: "https://b.hatena.ne.jp/hotentry/it.rss", enabled: true, last_fetched_at: iso(40), last_error: null },
    { id: 13, kind: "rss", name: "Zenn", category: "tech_jp", config: "https://zenn.dev/feed", enabled: true, last_fetched_at: iso(40), last_error: "http: error sending request for url (https://zenn.dev/feed): operation timed out" },
    { id: 15, kind: "rss", name: "The Verge", category: "tech_intl", config: "https://www.theverge.com/rss/index.xml", enabled: false, last_fetched_at: iso(4000), last_error: null },
    { id: 18, kind: "rss", name: "joint-kaigo.com", category: "medcare", config: "https://www.joint-kaigo.com/feed/", enabled: true, last_fetched_at: iso(40), last_error: null },
    { id: 20, kind: "rss", name: "Google News: 認知症", category: "medcare", config: "https://news.google.com/rss/search?q=%E8%AA%8D%E7%9F%A5%E7%97%87&hl=ja&gl=JP&ceid=JP:ja", enabled: true, last_fetched_at: null, last_error: null },
  ];
  const topics = [
    { id: 1, name: "認知症", keywords: ["認知症", "dementia", "Alzheimer", "アルツハイマー"] },
    { id: 2, name: "介護", keywords: ["介護", "介護保険", "caregiving"] },
    { id: 3, name: "ローカルLLM", keywords: ["ローカルLLM", "Ollama", "llama.cpp", "量子化", "quantization"] },
  ];
  const mk = (id, title, src, min, sum, read, star, score, cat) => ({ id, source_id: 1, source_name: src, category: cat || "medcare", url: "https://example.com/" + id, title, excerpt: "", author: null, published_at: iso(min), fetched_at: iso(min), score: score ?? null, ai_summary: sum, read, starred: star, image_url: [1,2,3,5,7].includes(id) ? "https://picsum.photos/seed/infodesk" + id + "/640/360" : null });
  const items = [
    mk(1, "認知症の新薬、国内での適用拡大へ 厚労省が方針", "Google News: 認知症", 120, "軽度認知障害の段階から使える新薬について、厚生労働省が適用範囲の拡大を検討している。早ければ来年度から対象が広がる見通し。", false, false),
    mk(2, "介護報酬改定の論点整理 人材確保策が焦点に", "joint-kaigo.com", 180, "次期介護報酬改定に向けた審議会が開かれ、処遇改善加算の一本化と夜勤配置の見直しが主な論点として示された。", false, true),
    mk(3, "Blood test predicts dementia risk a decade early", "Google News: dementia", 300, "血液中のタンパク質を測定することで、発症の10年前に認知症のリスクを予測できる可能性があるとする研究が発表された。", false, false),
    mk(4, "tester-army/e2e", "GitHub Trending (日)", 360, "エージェント向けのE2Eテスト基盤。ブラウザ操作を自動で検証する。", false, false, 1430, "github"),
    mk(5, "訪問介護の事業所数が過去最少に 小規模事業者の撤退続く", "Google News: 介護", 480, "訪問介護の事業所数が統計開始以来の最少となった。人手不足と報酬引き下げの影響で小規模事業者の撤退が続いている。", true, false),
    mk(6, "認知症カフェの全国調査 継続運営の課題は担い手", "Google News: 認知症", 1500, null, false, false),
    mk(7, "AI care planning tools enter pilot in nursing homes", "Google News: dementia", 1600, "介護計画の作成を支援するAIツールの試験導入が複数の介護施設で始まった。記録業務の時間短縮が期待されている。", false, true),
  ];
  const dg = (id, cat, body, n, over) => ({ id, period: "day", period_start: "2026-10-07", scope: { category: cat }, body, item_ids: Array.from({ length: n }, (_, i) => i + 1), provider: "ollama:gemma4:26b", status: "done", attempts: 0, created_at: iso(30), last_error: null, ...over });
  const labels = { github: "GitHub", arxiv: "arXiv", huggingface: "Hugging Face", hackernews: "Hacker News", tech_jp: "国内tech", tech_intl: "海外tech", medcare: "医療介護" };
  const digests = {
    github: dg(1, "github", "見出し: エージェント向けE2E基盤が急伸\n- エージェント向けE2Eテスト基盤が急伸。1日で1,430スターを獲得し、ブラウザ操作の自動検証が注目を集めた。\n- ローカルLLMの推論サーバーが複数ランクイン。量子化モデルの配布形式を統一する動きが続く。\n- Rust製のCLIツールが引き続き上位。既存ツールの置き換えを狙う実装が多い。\n全体として、エージェント関連の開発基盤に関心が集まっている。", 25),
    arxiv: dg(2, "arxiv", "", 0, { status: "pending", attempts: 1, last_error: "ollama 500 Internal Server Error: unexpected EOF" }),
    huggingface: null,
    hackernews: dg(4, "hackernews", "- SQLiteの新しい全文検索拡張が話題。日本語を含む多言語での精度比較が共有された。\n- 小規模チームでのオンコール運用に関する議論が上位に入った。", 30),
    tech_jp: null,
    tech_intl: dg(6, "tech_intl", "", 0, { status: "failed", attempts: 3, last_error: "ollama 404 Not Found: model 'gemma4:26b' not found" }),
    medcare: dg(7, "medcare", "見出し: 認知症の新薬が国内で適用拡大へ\n- 認知症の新薬について、国内での適用拡大に関する報道が複数あった。\n- 介護報酬の改定に向けた議論が始まり、人材確保策が主な論点になっている。", 14),
  };
  const counts = { huggingface: 0, tech_jp: 21 };
  const settings = { "llm.items_provider": "ollama", "llm.digest_provider": "claude", "llm.chat_provider": "ollama", "ollama.base_url": "http://localhost:11434", "ollama.model": "gemma4:26b", "ollama.web_search_api_key": "", "claude.bin": "claude", "node.bin": "node", "summary.top_n": "20", "schedule.collect_interval_hours": "3", "schedule.daily_time": "07:00", "schedule.weekly_day": "mon", "schedule.weekly_time": "07:30", "schedule.monthly_time": "08:00", "app.launch_at_login": "false", "app.stay_resident": "true" };
  const first = scenario === "first";
  const handlers = {
    get_status: () => ({ collecting: false, summarizing: false, digesting: false, last_collect_at: first ? null : iso(40), next_collect_at: iso(-140), last_error: scenario === "err" ? "ollama 404 Not Found: model 'gemma4:26b' not found" : null }),
    sidebar_counts: () => first ? { total_unread: 0, categories: {}, topics: {} } : ({ total_unread: 128, categories: { github: 25, arxiv: 42, huggingface: 18, hackernews: 30, tech_jp: 21, tech_intl: 16, medcare: 14 }, topics: { 1: 6, 2: 9, 3: 11 } }),
    list_topics: () => topics,
    list_sources: () => sources,
    source_item_counts: () => ({ 1: 25, 4: 50, 7: 30, 9: 30, 10: 30, 18: 10 }),
    list_items: (a) => first ? [] : items.filter((i) => !a.filter.unread_only || !i.read),
    count_items: (a) => first ? 0 : (a.filter.unread_only ? 137 : 240),
    digest_stats: (a) => { const n = a.period === "day" ? 1 : a.period === "week" ? 7 : 31; const d = (b) => Array.from({ length: n }, (_, i) => Math.max(0, Math.round(b * (0.6 + 0.5 * Math.sin(i + b))))); return first ? { period: a.period, period_start: a.date, total: 0, categories: [], topics: [] } : { period: a.period, period_start: a.date, total: 156, categories: [["github",25],["arxiv",42],["huggingface",18],["hackernews",30],["tech_jp",21],["tech_intl",6],["medcare",14]].map(([c, k]) => ({ category: c, count: k * (n > 1 ? 5 : 1), daily: d(k) })), topics: [{ topic_id: 1, name: "認知症", count: 12, prev_count: 7 }, { topic_id: 2, name: "介護", count: 9, prev_count: 11 }, { topic_id: 3, name: "ローカルLLM", count: 5, prev_count: 5 }] }; },
    get_item: (a) => items.find((i) => i.id === a.id) || null,
    get_digests: (a) => a.topicId ? [{ scope: { topic: a.topicId }, label: (topics.find((t) => t.id === a.topicId) || {}).name || "", digest: null, item_count: 6 }] : Object.keys(labels).map((c) => ({ scope: { category: c }, label: labels[c], digest: first ? null : digests[c], item_count: first ? 0 : (counts[c] ?? 10) })),
    get_settings: () => settings,
  };

  const chatMsgs = [
    { id: "u1", role: "user", parts: [{ type: "text", text: "今週の認知症に関する記事をまとめて。新薬と介護現場の話題を分けて。" }] },
    { id: "a1", role: "assistant", parts: [
      { type: "step-start" },
      { type: "tool-search_articles", toolCallId: "c1", state: "output-available", input: { query: "認知症", days: 7 }, output: { items: [{ id: 1, title: "認知症の新薬、国内での適用拡大へ 厚労省が方針" }, { id: 3, title: "Blood test predicts dementia risk a decade early" }], total: 14 } },
      { type: "tool-fetch_page", toolCallId: "c2", state: "output-available", input: { url: "https://example.com/a" }, output: { page: { title: "認知症の新薬、国内での適用拡大へ" } } },
      { type: "text", text: "## 新薬\n厚生労働省が軽度認知障害の段階から使える新薬の適用拡大を検討しており、来年度からの対象拡大が見込まれています。\n\n## 介護現場\n- 介護報酬改定では処遇改善加算の一本化が論点です。\n- 見守りセンサーの導入で夜間の転倒が減ったとの調査もあります。" },
    ] },
  ];
  handlers.list_chat_threads = () => [{ id: 2, title: "認知症の今週の動き", created_at: iso(60), updated_at: iso(30) }, { id: 1, title: "GitHub の話題", created_at: iso(3000), updated_at: iso(3000) }];
  handlers.get_chat_messages = () => chatMsgs;
  handlers.chat_endpoint = () => scenario === "chaterr" ? { status: "error", kind: "node_missing", message: "Node が見つかりません (node)。チャットには Node 22 以上が必要です。" } : { status: "ready", url: "http://127.0.0.1:1", token: "t" };
  handlers.restart_chat = () => null;
  let cb = 0; const cbs = {};
  window.__TAURI_INTERNALS__ = {
    transformCallback: (f) => { cb += 1; cbs[cb] = f; return cb; },
    unregisterCallback: (id) => { delete cbs[id]; },
    invoke: async (cmd, args) => {
      if (cmd.startsWith("plugin:event|")) return 1;
      const h = handlers[cmd];
      if (!h) { console.warn("unmocked", cmd); return null; }
      return h(args || {});
    },
    metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} };
  const click = (name) => { const all = [...document.querySelectorAll("button")]; const label = (x) => (x.getAttribute("aria-label") || x.textContent || "").trim(); const b = all.find((x) => label(x) === name) || all.find((x) => label(x).startsWith(name)); if (b) { b.click(); b.scrollIntoView({ block: "nearest" }); }; return !!b; };
  const steps = (q.get("go") || "").split(",").filter(Boolean);
  window.addEventListener("load", () => {
    if (q.get("dark")) for (const sheet of document.styleSheets) for (const r of [...sheet.cssRules]) if (r.media && /dark/.test(r.conditionText)) for (const inner of r.cssRules) sheet.insertRule(inner.cssText, sheet.cssRules.length);
    let i = 0, tries = 0; const next = () => { if (i >= steps.length) return; if (click(steps[i]) || ++tries > 12) { i++; tries = 0; } setTimeout(next, 300); }; setTimeout(next, 400);
  });
})();
