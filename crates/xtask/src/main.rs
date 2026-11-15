//! Tarefas do projeto em Rust, no lugar dos scripts Node: `cargo xtask <comando>`.
mod despejar;
mod limpar;
mod medir;
mod release;

use std::path::{Path, PathBuf};

const AJUDA: &str = "uso: cargo xtask <comando>
  release <subcomando> ...  regras do fluxo de release (sem argumentos lista os subcomandos)
  limpar-target             cargo clean se target passar do teto (ORQUESTRADOR_TARGET_MAX_GB, padrão 30)
  medir [opções]            medição de desempenho do orquestrador (docs/desempenho-gpui.md)
  despejar <linhas> <tag> [linhas/s] [--esperar <arquivo>]   gerador de saída para as medições";

fn raiz() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().and_then(Path::parent).map(Path::to_path_buf).unwrap_or_default()
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let resto = args.get(1..).unwrap_or(&[]);
    let r: Result<(), String> = match args.first().map(String::as_str) {
        Some("release") => release::run(&raiz(), resto).map(|m| println!("{m}")),
        Some("limpar-target") => {
            limpar::run(&raiz());
            Ok(())
        }
        Some("medir") => medir::run(&raiz(), resto),
        Some("despejar") => despejar::run(resto),
        _ => {
            println!("{AJUDA}");
            Ok(())
        }
    };
    if let Err(e) = r {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
