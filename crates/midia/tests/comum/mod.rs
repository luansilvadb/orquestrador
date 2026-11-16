//! Apoio dos testes: pasta temporária e as fixtures de mídia (2 s, 64×48, tom de 440 Hz), em `tests/fixtures`.
#![allow(dead_code)]
use orquestrador_midia::player::Player;
use std::path::{Path, PathBuf};

/// Serializa os testes do arquivo: um player por vez na saída de áudio, e o contador de threads fica exato.
pub fn serial() -> std::sync::MutexGuard<'static, ()> {
    static TRAVA: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TRAVA.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Tmp(pub PathBuf);

impl Tmp {
    pub fn new(tag: &str) -> Tmp {
        let dir = std::env::temp_dir().join(format!("orq-midia-teste-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("pasta temporária");
        Tmp(dir)
    }

    pub fn caminho(&self, nome: &str) -> PathBuf {
        self.0.join(nome)
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn fixture(nome: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(nome)
}

/// Copia uma fixture para a pasta temporária com outro nome.
pub fn copiar(tmp: &Tmp, fixture_nome: &str, nome: &str) -> PathBuf {
    let destino = tmp.caminho(nome);
    std::fs::copy(fixture(fixture_nome), &destino).expect("copiar fixture");
    destino
}

/// Abre o arquivo e diz se o Windows decodifica o formato; sem a extensão da Microsoft Store que ele exige, avisa e
/// devolve `false` para o teste pular.
pub fn decodifica(caminho: &Path) -> bool {
    let p = Player::abrir(caminho);
    let inicio = std::time::Instant::now();
    while !p.estado().info.pronto {
        assert!(inicio.elapsed() < std::time::Duration::from_secs(10), "não abriu: {}", caminho.display());
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    match p.estado().info.erro {
        Some(e) if e.contains("Microsoft Store") => {
            eprintln!("pulado, sem decodificador: {}: {e}", caminho.display());
            false
        }
        _ => true,
    }
}
