// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // チャットのサイドカーは、アプリ本体を `--cli` 付きで起動して infodesk CLI として使う。
    if args.get(1).map(String::as_str) == Some("--cli") {
        let cli_args = std::iter::once("infodesk".to_string()).chain(args.into_iter().skip(2));
        std::process::exit(infodesk_cli::run_from(cli_args));
    }
    infodesk_app_lib::run()
}
