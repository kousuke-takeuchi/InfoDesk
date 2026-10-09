import { describe, expect, it } from "vitest";
import { buildInstructions, PLAN_INSTRUCTION } from "../src/prompt";

const NOW = new Date("2026-10-09T10:00:00+09:00");
const ALL = ["search_articles", "get_article", "read_digests", "fetch_page"];

describe("buildInstructions", () => {
  it("states the role, the local date and the tool rules", () => {
    const s = buildInstructions(null, NOW, ALL);
    expect(s).toContain("日本語");
    expect(s).toContain("今日: 2026-10-09 (金)");
    expect(s).toContain("search_articles");
    expect(s).toContain("推測で");
    expect(s).not.toContain("web_search");
  });

  it("mentions web_search only when the tool is available", () => {
    expect(buildInstructions(null, NOW, [...ALL, "web_search"])).toContain("web_search");
  });

  it("describes the scope, the period and the attachments", () => {
    const s = buildInstructions(
      {
        scope: { kind: "topic", topicId: 3, label: "認知症" },
        period: { view: "digest", period: "week", date: "2026-10-09" },
        attachments: [
          { type: "item", id: 12, title: "認知症の新薬、国内での適用拡大へ" },
          { type: "digest", period: "day", date: "2026-10-09", scope: "category:medcare", label: "医療介護" },
        ],
      },
      NOW,
      ALL,
    );
    expect(s).toContain("トピック「認知症」");
    expect(s).toContain("週次 (2026-10-09 を含む週)");
    expect(s).toContain("記事 id 12「認知症の新薬、国内での適用拡大へ」");
    expect(s).toContain("医療介護の日次要約 (2026-10-09、scope category:medcare)");
  });

  it("describes a feed range and the whole collection", () => {
    const s = buildInstructions({ scope: { kind: "all" }, period: { view: "feed", range: "3d" }, attachments: [] }, NOW, ALL);
    expect(s).toContain("すべての記事");
    expect(s).toContain("直近3日");
  });

  it("asks for a plan on multi-step questions", () => {
    const s = buildInstructions(null, NOW, [...ALL, "update_plan"]);
    expect(PLAN_INSTRUCTION).toBe(
      "- ツールを2回以上使う見込みの質問では、最初に update_plan で手順を3〜6項目で示す。" +
        "状態 (pending / in_progress / done) の更新は次のツール呼び出しと同じステップで一緒に送り、更新だけのステップを作らない。" +
        "回答を書く直前に全項目を done にする更新は送らない (回答が終われば完了として表示される)。",
    );
    expect(s.split("\n")).toContain(PLAN_INSTRUCTION);
  });

  it("leaves out the plan instruction without update_plan", () => {
    const s = buildInstructions(null, NOW, ALL);
    expect(s).not.toContain("update_plan");
    expect(s.split("\n")).not.toContain(PLAN_INSTRUCTION);
  });
});
