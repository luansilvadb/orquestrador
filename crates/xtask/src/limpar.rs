//! Teto do `target` do workspace: acima dele, `cargo clean`. Padrão de 30 GB; `ORQUESTRADOR_TARGET_MAX_GB` troca.
//! Não bloqueia o comando seguinte: termina sempre com sucesso.
use std::path::Path;

const GB: f64 = 1024. * 1024. * 1024.;

/// Teto em bytes a partir do valor da variável (vazio, inválido ou negativo usa o padrão).
pub fn teto(valor: Option<&str>) -> f64 {
    let gb = valor.map(str::trim).filter(|v| !v.is_empty()).and_then(|v| v.parse::<f64>().ok()).filter(|g| g.is_finite() && *g >= 0.);
    gb.unwrap_or(30.) * GB
}

/// Soma os arquivos sem seguir symlinks nem junctions; erros de acesso valem 0.
pub fn tamanho(dir: &Path) -> u64 {
    let Ok(entradas) = std::fs::read_dir(dir) else { return 0 };
    entradas
        .flatten()
        .map(|e| match std::fs::symlink_metadata(e.path()) {
            // No Windows o `is_symlink` também cobre junctions (reparse point de nome substituto).
            Ok(m) if m.file_type().is_symlink() => 0,
            Ok(m) if m.is_dir() => tamanho(&e.path()),
            Ok(m) if m.is_file() => m.len(),
            _ => 0,
        })
        .sum()
}

pub fn run(root: &Path) {
    let target = root.join("target");
    if std::fs::symlink_metadata(&target).is_err() {
        return;
    }
    let limite = teto(std::env::var("ORQUESTRADOR_TARGET_MAX_GB").ok().as_deref());
    let size = tamanho(&target) as f64;
    if size > limite {
        let gb = |b: f64| (b / GB * 1000.).round() / 1000.;
        println!("target tem {} GB, acima do teto de {} GB: cargo clean", gb(size), gb(limite));
        let r = std::process::Command::new(std::env::var("CARGO").unwrap_or("cargo".into()))
            .args(["clean", "--manifest-path"])
            .arg(root.join("Cargo.toml"))
            // O medido é `target`; um CARGO_TARGET_DIR herdado limparia outra pasta.
            .arg("--target-dir")
            .arg(&target)
            .current_dir(root)
            .status();
        match r {
            Ok(s) if s.success() => {}
            Ok(s) => eprintln!("aviso: cargo clean falhou (código {}); seguindo sem limpar", s.code().unwrap_or(-1)),
            Err(e) => eprintln!("aviso: cargo clean falhou ({e}); seguindo sem limpar"),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn teto_padrao_e_variavel() {
        assert_eq!(teto(None), 30. * GB);
        assert_eq!(teto(Some("  ")), 30. * GB);
        assert_eq!(teto(Some("abc")), 30. * GB);
        assert_eq!(teto(Some("-1")), 30. * GB);
        assert_eq!(teto(Some("0")), 0.);
        assert_eq!(teto(Some(" 2.5 ")), 2.5 * GB);
    }

    #[test]
    fn soma_arquivos_sem_seguir_links() {
        let d = std::env::temp_dir().join(format!("xtask-limpar-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("a/b")).unwrap();
        std::fs::write(d.join("x"), [0u8; 10]).unwrap();
        std::fs::write(d.join("a/b/y"), [0u8; 5]).unwrap();
        let fora = d.with_extension("fora");
        std::fs::create_dir_all(&fora).unwrap();
        std::fs::write(fora.join("z"), [0u8; 100]).unwrap();
        // Junction (não pede privilégio) para fora da árvore: não entra na soma.
        #[cfg(windows)]
        let _ = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(d.join("j")).arg(&fora).output();
        assert_eq!(tamanho(&d), 15);
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&fora);
    }
}
