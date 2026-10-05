#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("compositor {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    compositor::run()
}
