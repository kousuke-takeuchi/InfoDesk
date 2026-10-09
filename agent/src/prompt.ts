import type { ChatContext, ChatPeriod, ChatScope, FeedRange } from "./context";

const WEEKDAYS = ["日", "月", "火", "水", "木", "金", "土"];
const FEED_LABEL: Record<FeedRange, string> = { today: "今日", "3d": "直近3日", week: "直近1週間", month: "直近1ヶ月", all: "全期間" };
const PERIOD_LABEL = { day: "日次", week: "週次", month: "月次" } as const;

function localDate(d: Date): string {
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`;
}

function scopeText(s: ChatScope): string {
  switch (s.kind) {
    case "all": return "すべての記事";
    case "category": return `カテゴリー「${s.label}」 (${s.category})`;
    case "topic": return `トピック「${s.label}」`;
  }
}

function periodText(p: ChatPeriod): string {
  if (p.view === "feed") return `フィードの期間: ${FEED_LABEL[p.range]}`;
  const unit = p.period === "day" ? `${p.date}` : p.period === "week" ? `${p.date} を含む週` : `${p.date.slice(0, 7)} の月`;
  return `ダイジェストの期間: ${PERIOD_LABEL[p.period]} (${unit})`;
}

/** update_plan があるときの指示。更新は次のツール呼び出しと同じステップで送らせ、ステップの上限を食わないようにする。 */
export const PLAN_INSTRUCTION =
  "- ツールを2回以上使う見込みの質問では、最初に update_plan で手順を3〜6項目で示す。" +
  "状態 (pending / in_progress / done) の更新は次のツール呼び出しと同じステップで一緒に送り、更新だけのステップを作らない。" +
  "回答を書く直前に全項目を done にする更新は送らない (回答が終われば完了として表示される)。";

export function buildInstructions(context: ChatContext | null, now: Date, tools: string[]): string {
  const lines = [
    "あなたは InfoDesk のリサーチアシスタントです。利用者が収集した技術ニュースと医療介護ニュースについて、日本語で答えます。",
    `今日: ${localDate(now)} (${WEEKDAYS[now.getDay()]})`,
    "",
    "ツールの使い方:",
    "- 収集済みの記事は search_articles で探し、get_article で1件を詳しく読む。",
    "- 生成済みの期間要約は read_digests で読む。",
    "- 記事の詳しい中身が必要なときだけ fetch_page で本文を取る。",
  ];
  if (tools.includes("update_plan")) {
    lines.push(PLAN_INSTRUCTION);
  }
  if (tools.includes("web_search")) {
    lines.push("- 収集した記事の外の情報が必要なときだけ Web 検索 (web_search または WebSearch) を使う。");
  }
  lines.push(
    "",
    "答え方:",
    "- 推測で記事や数字を作らない。ツールで確かめた内容だけを書き、分からないことは分からないと書く。",
    "- 記事に触れるときはタイトルを書く。URL は書かなくてよい (画面が参照した記事のリンクを出す)。",
    "- 見出しと箇条書きで短くまとめる。",
  );
  if (context) {
    lines.push("", "利用者が今見ている画面:", `- ${scopeText(context.scope)}`);
    if (context.period) lines.push(`- ${periodText(context.period)}`);
    for (const a of context.attachments) {
      if (a.type === "item") lines.push(`- 質問に添えた記事 id ${a.id}「${a.title}」 (get_article で読める)`);
      else lines.push(`- 質問に添えた要約: ${a.label}の${PERIOD_LABEL[a.period]}要約 (${a.date}、scope ${a.scope})`);
    }
  }
  return lines.join("\n");
}
