// Copia as DLLs do FFmpeg compartilhado (`FFMPEG_DIR\bin`) para `target\<perfil>`, `deps` e `examples`, onde o app, os
// testes e os exemplos as acham sem PATH. Só copia o que mudou.
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=FFMPEG_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let Ok(dir) = std::env::var("FFMPEG_DIR") else { return };
    let bin = Path::new(&dir).join("bin");
    println!("cargo:rerun-if-changed={}", bin.display());
    // OUT_DIR = target\<perfil>\build\<crate>-<hash>\out
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let perfil = out.ancestors().nth(3).unwrap().to_path_buf();
    let Ok(entradas) = std::fs::read_dir(&bin) else { return };
    for entrada in entradas.flatten() {
        let origem = entrada.path();
        if origem.extension().is_none_or(|e| !e.eq_ignore_ascii_case("dll")) {
            continue;
        }
        for alvo in [perfil.clone(), perfil.join("deps"), perfil.join("examples")] {
            let destino = alvo.join(entrada.file_name());
            let igual = match (std::fs::metadata(&origem), std::fs::metadata(&destino)) {
                (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() <= b.modified().ok(),
                _ => false,
            };
            if !igual {
                std::fs::create_dir_all(&alvo).unwrap();
                std::fs::copy(&origem, &destino).unwrap_or_else(|e| panic!("copiar {}: {e}", origem.display()));
            }
        }
    }
}
