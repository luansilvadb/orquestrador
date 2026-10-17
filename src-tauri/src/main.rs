#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Chamado pelos CLIs dos executores, o executável só faz o serviço do hook e sai, sem janela.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--hook") => std::process::exit(orquestrador_lib::hook::run(&args[1..])),
        Some("--ask") => std::process::exit(orquestrador_lib::hook::ask(&args[1..])),
        Some("--mcp") => std::process::exit(orquestrador_lib::mcp::serve(&args[1..])),
        _ => orquestrador_lib::run(tauri::generate_context!()),
    }
}
