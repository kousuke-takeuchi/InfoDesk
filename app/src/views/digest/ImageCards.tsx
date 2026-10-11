import type { Item } from "../../types";

const MAX_CARDS = 3;

/** カードにする記事: https の画像を持つもののうち、先頭から3件。 */
export function cardItems(items: Item[]): Item[] {
  return items.filter((i) => i.image_url !== null && i.image_url.startsWith("https:")).slice(0, MAX_CARDS);
}

/**
 * `broken`: 画像の読み込みに失敗した記事。カードにせず、呼び出し側で文字のリンクに回す。
 * `onBroken`: 画像の読み込みに失敗したときに記事の id を知らせる。
 */
export function ImageCards({ items, broken, onBroken, onOpen }: {
  items: Item[]; broken: ReadonlySet<number>; onBroken: (id: number) => void; onOpen: (id: number) => void;
}) {
  const cards = cardItems(items).filter((i) => !broken.has(i.id));
  if (cards.length === 0) return null;
  return (
    <div className="image-cards">
      {cards.map((it) => (
        <button key={it.id} className="image-card" onClick={() => onOpen(it.id)}>
          <img src={it.image_url!} alt="" loading="lazy" referrerPolicy="no-referrer" onError={() => onBroken(it.id)} />
          <span className="card-title">{it.title}</span>
          <span className="card-source">{it.source_name}</span>
        </button>
      ))}
    </div>
  );
}
