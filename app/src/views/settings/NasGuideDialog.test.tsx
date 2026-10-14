import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { NasGuideDialog } from "./NasGuideDialog";
import { setupIpc } from "../../test/ipc";

describe("NasGuideDialog", () => {
  it("shows the five steps and the tip", () => {
    setupIpc({});
    render(<NasGuideDialog onClose={() => {}} />);
    const dialog = screen.getByRole("dialog", { name: "NAS の準備" });
    expect(dialog).toHaveAttribute("aria-modal", "true");
    for (const t of [
      "DSM で WebDAV Server を入れる", "共有フォルダを作る", "InfoDesk 用のユーザーを作る", "NAS に Tailscale を入れる", "InfoDesk でつなぐ",
      "パッケージセンターで「WebDAV Server」を入れ、HTTP (ポート 5005) を有効にします。",
      "コントロールパネル → 共有フォルダで「infodesk」を作ります。",
      "コントロールパネル → ユーザーで新しいユーザーを作り、infodesk だけ「読み取り/書き込み」、ほかの共有フォルダは「アクセス不可」にします。",
      "パッケージセンターで「Tailscale」を入れてログインし、NAS の名前 (例: takeuchi-nas) を控えます。",
      "保存先に「NAS (WebDAV)」を選び、Tailscale にログイン (NAS と同じアカウント)。URL に http://NAS の名前:5005/infodesk/、ユーザー名とパスワードを入れて「確かめる」を押します。",
      "Mac や iPhone に Tailscale のアプリは要りません。InfoDesk の中で Tailscale を使うので、ルーターのポートを開けず、ほかのアプリの通信にも影響しません。",
    ]) expect(screen.getByText(t)).toBeInTheDocument();
  });

  it("opens the GitHub guide in the default browser", async () => {
    const ipc = setupIpc({ open_link: () => undefined });
    render(<NasGuideDialog onClose={() => {}} />);
    await userEvent.click(screen.getByRole("button", { name: "詳しい手順 (docs/13-nas-setup)" }));
    expect(ipc.last("open_link")).toEqual({ url: "https://github.com/kousuke-takeuchi/InfoDesk/blob/master/docs/13-nas-setup.ja.md" });
  });

  it("closes with the button and with Escape", async () => {
    setupIpc({});
    const onClose = vi.fn();
    render(<NasGuideDialog onClose={onClose} />);
    await userEvent.click(screen.getByRole("button", { name: "閉じる" }));
    expect(onClose).toHaveBeenCalledTimes(1);
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
