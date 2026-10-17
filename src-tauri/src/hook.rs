//! O modo `--hook` do executável, sem janela, chamado pelos CLIs dos executores no fim de cada rodada.
use std::io::Read;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// `orquestrador.exe --hook <pasta> [json]`: o sinal de fim de rodada. O Claude Code entrega o
/// JSON do hook `Stop` por stdin; o Codex, como último argumento do `notify`. Grava o que recebeu,
/// sem interpretar, num arquivo novo da pasta (publicado por renomeação); quem lê é o app.
/// Um arquivo por chamada: medido no Codex 0.162, o `notify` também dispara para a rodada interna
/// que gera o título da conversa, e uma chamada não pode apagar a outra.
pub fn run(args: &[String]) -> i32 {
    let Some(dir) = args.first() else { return 2 };
    let payload = match args.get(1) {
        Some(json) => json.clone().into_bytes(),
        None => {
            let mut buf = Vec::new();
            let _ = std::io::stdin().read_to_end(&mut buf);
            buf
        }
    };
    if put(Path::new(dir), &format!("{}.json", unique()), &payload).is_err() { 1 } else { 0 }
}

/// Nome de um arquivo novo da pasta: os nanossegundos com zeros à esquerda (a ordem alfabética é a
/// cronológica) e o pid, para duas chamadas no mesmo instante não se confundirem.
pub fn unique() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{nanos:032}-{}", std::process::id())
}

/// Publica `bytes` como `dir\name`, por renomeação: quem lê a pasta nunca pega o arquivo pela metade.
fn put(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = dir.join(format!("{name}.tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dir.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::fresh;

    #[test]
    fn hook_grava_um_arquivo_por_chamada() {
        let dir = fresh("orq-hook", "hook");
        let d = dir.to_string_lossy().to_string();
        assert_eq!(run(&[d.clone(), r#"{"a":"ação"}"#.into()]), 0);
        assert_eq!(run(&[d.clone(), r#"{"b":2}"#.into()]), 0);
        let mut files: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
        files.sort();
        assert_eq!(files.len(), 2);
        assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), r#"{"a":"ação"}"#);
        assert_eq!(run(&[]), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
