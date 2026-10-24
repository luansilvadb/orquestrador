#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Chamado pelos CLIs dos executores, o executável só faz o serviço do hook e sai, sem janela.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--hook") => std::process::exit(orquestrador_lib::agents::hook::run(&args[1..])),
        Some("--allow") => {
            // O pedido vem por stdin e é lido inteiro, para o CLI não escrever num pipe já fechado.
            let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut Vec::new());
            println!("{}", orquestrador_lib::agents::cli::ALLOW);
        }
        Some("--mcp") => std::process::exit(orquestrador_lib::agents::mcp::serve(&args[1..])),
        _ => {
            let cwd = std::env::current_dir().unwrap_or_default();
            let paths = orquestrador_lib::open_paths::extract_paths(&args, &cwd);
            orquestrador_lib::run(tauri::generate_context!(), paths);
        }
    }
}
