import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Item } from "../../types";
import { ImageCards } from "./ImageCards";

const item = (id: number, image_url: string | null): Item => ({
  id, source_id: 1, source_name: `ソース${id}`, category: "medcare", url: `https://e.com/${id}`, title: `記事${id}`,
  excerpt: "", author: null, published_at: null, fetched_at: "2026-10-10T00:00:00Z", score: null, ai_summary: null,
  read: false, starred: false, image_url,
});

describe("ImageCards", () => {
  it("shows up to three https-image articles as cards in order and opens on click", () => {
    const onOpen = vi.fn();
    render(<ImageCards items={[item(1, null), item(2, "https://i/2.jpg"), item(3, "http://i/3.jpg"), item(4, "https://i/4.jpg"), item(5, "https://i/5.jpg"), item(6, "https://i/6.jpg")]} onOpen={onOpen} />);
    const cards = screen.getAllByRole("button");
    expect(cards.map((c) => c.textContent)).toEqual([expect.stringContaining("記事2"), expect.stringContaining("記事4"), expect.stringContaining("記事5")]);
    const img = cards[0].querySelector("img")!;
    expect(img).toHaveAttribute("src", "https://i/2.jpg");
    expect(img).toHaveAttribute("loading", "lazy");
    expect(img).toHaveAttribute("referrerpolicy", "no-referrer");
    fireEvent.click(cards[1]);
    expect(onOpen).toHaveBeenCalledWith(4);
  });

  it("hides a card whose image fails and renders nothing without images", () => {
    const { container, rerender } = render(<ImageCards items={[item(2, "https://i/2.jpg")]} onOpen={() => {}} />);
    fireEvent.error(container.querySelector("img")!);
    expect(container.querySelector(".image-card")).toBeNull();
    rerender(<ImageCards items={[item(1, null)]} onOpen={() => {}} />);
    expect(container).toBeEmptyDOMElement();
  });
});
