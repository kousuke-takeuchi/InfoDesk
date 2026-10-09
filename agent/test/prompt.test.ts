import { describe, expect, it } from "vitest";
import { buildInstructions } from "../src/prompt";

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
    expect(s).toContain("update_plan");
    expect(s).toContain("done");
  });
});
