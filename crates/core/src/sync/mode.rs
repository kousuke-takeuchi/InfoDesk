//! 同期の役割。`writer` は記事を集めて保存先を書き、`reader` は保存先を読むだけ
//! (書くのは自分の既読とスターの ops ファイルだけ)。アプリが使う。サーバー (CLI) はいつも writer。

use crate::store::Store;

pub const SETTING_MODE: &str = "sync.mode";
pub const MODE_WRITER: &str = "writer";
pub const MODE_READER: &str = "reader";
/// 読むだけのモードで、収集や要約を頼まれたときの文。
pub const READER_ONLY_MESSAGE: &str = "読むだけのモードでは使えません";

/// `sync.mode` が `reader` か。キーがない、ほかの値は writer として読む。
pub fn is_reader(store: &Store) -> crate::Result<bool> {
    Ok(store.get_setting(SETTING_MODE)?.as_deref() == Some(MODE_READER))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_mode_is_writer_and_only_reader_counts_as_reader() {
        assert!(crate::seed::DEFAULT_SETTINGS.contains(&(SETTING_MODE, MODE_WRITER)));
        let s = Store::open_in_memory().unwrap();
        assert!(!is_reader(&s).unwrap(), "no key");
        crate::seed::seed_defaults(&s).unwrap();
        assert!(!is_reader(&s).unwrap(), "seeded");
        s.set_setting(SETTING_MODE, "nonsense").unwrap();
        assert!(!is_reader(&s).unwrap());
        s.set_setting(SETTING_MODE, MODE_READER).unwrap();
        assert!(is_reader(&s).unwrap());
    }
}
