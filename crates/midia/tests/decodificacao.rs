//! Decodificação de cada formato aceito pela aba, com as fixtures de `tests/fixtures`. Formatos que dependem de uma
//! extensão da Microsoft Store (Ogg, Opus, VP8) pulam onde ela falta.
mod comum;

use comum::{copiar, Tmp};
use orquestrador_midia::player::{Estado, Player};
use std::path::Path;
use std::time::{Duration, Instant};

fn esperar(player: &Player, limite: Duration, cond: impl Fn(&Estado) -> bool) -> Estado {
    let inicio = Instant::now();
    loop {
        let _ = player.quadro(false);
        let e = player.estado();
        if cond(&e) {
            return e;
        }
        assert!(inicio.elapsed() < limite, "tempo esgotado: {e:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Abre, confere duração, tamanho e o primeiro quadro, toca um pouco e confere que avança.
fn conferir(caminho: &Path, video: Option<(u32, u32)>, tem_audio: bool) {
    if !comum::decodifica(caminho) {
        return;
    }
    let p = Player::abrir(caminho);
    let e = esperar(&p, Duration::from_secs(10), |e| e.info.pronto);
    assert_eq!(e.info.erro, None, "{}", caminho.display());
    let d = e.info.duracao.expect("duração");
    assert!((d - 2.).abs() < 0.15, "{}: duração {d}", caminho.display());
    assert_eq!(e.info.video, video, "{}", caminho.display());
    assert_eq!(e.info.tem_audio, tem_audio, "{}", caminho.display());
    assert!(!e.tocando && e.posicao == 0., "não toca sozinho");
    if let Some((w, h)) = video {
        let inicio = Instant::now();
        let q = loop {
            if let Some(q) = p.quadro(true) {
                break q;
            }
            assert!(inicio.elapsed() < Duration::from_secs(10), "sem primeiro quadro");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!((q.largura, q.altura), (w, h));
        assert_eq!(q.bgra.len(), (w * h * 4) as usize, "BGRA sem preenchimento");
    }
    p.tocar();
    esperar(&p, Duration::from_secs(10), |e| e.posicao > 0.2);
    p.pausar();
    let parado = p.estado().posicao;
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(p.estado().posicao, parado, "pausado não avança");
}

#[test]
fn audio_de_cada_formato() {
    let _s = comum::serial();
    let tmp = Tmp::new("audio");
    for nome in ["tom.wav", "tom.flac", "tom.mp3", "tom.m4a", "tom.ogg", "tom.oga", "tom.opus"] {
        conferir(&copiar(&tmp, nome, nome), None, true);
    }
}

#[test]
fn video_h264_com_audio_em_cada_conteiner() {
    let _s = comum::serial();
    let tmp = Tmp::new("video");
    for nome in ["clipe.mp4", "clipe.m4v", "clipe.mov", "clipe.mkv", "clipe.avi"] {
        conferir(&copiar(&tmp, nome, nome), Some((64, 48)), true);
    }
}

#[test]
fn webm_vp8_do_mock() {
    let _s = comum::serial();
    let tmp = Tmp::new("webm");
    conferir(&copiar(&tmp, "clipe.webm", "clipe.webm"), Some((64, 48)), false);
}

#[test]
fn busca_e_fim() {
    let _s = comum::serial();
    let tmp = Tmp::new("busca");
    let c = copiar(&tmp, "clipe.mp4", "clipe.mp4");
    let p = Player::abrir(&c);
    esperar(&p, Duration::from_secs(10), |e| e.info.pronto);
    p.buscar(1.);
    let e = p.estado();
    assert!((e.posicao - 1.).abs() < 0.05, "busca parada: {}", e.posicao);
    let inicio = Instant::now();
    let q = loop {
        if let Some(q) = p.quadro(true) {
            break q;
        }
        assert!(inicio.elapsed() < Duration::from_secs(5));
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(q.pts >= 0.99, "quadro depois do alvo: {}", q.pts);
    p.buscar(1.6);
    p.tocar();
    let e = esperar(&p, Duration::from_secs(10), |e| e.terminou);
    assert!(!e.tocando && (e.posicao - e.info.duracao.unwrap()).abs() < 0.01, "{e:?}");
    // Tocar no fim recomeça do início.
    p.tocar();
    let e = p.estado();
    assert!(e.tocando && e.posicao < 0.2, "{e:?}");
}

#[test]
fn volume_mudo_e_velocidade() {
    let _s = comum::serial();
    let tmp = Tmp::new("vol");
    let c = copiar(&tmp, "tom.wav", "tom.wav");
    let p = Player::abrir(&c);
    esperar(&p, Duration::from_secs(10), |e| e.info.pronto);
    p.definir_volume(0.25);
    p.definir_mudo(true);
    let e = p.estado();
    assert!((e.volume - 0.25).abs() < 1e-6 && e.mudo);
    p.definir_velocidade(2.);
    p.tocar();
    let inicio = Instant::now();
    esperar(&p, Duration::from_secs(10), |e| e.posicao >= 1.);
    // A 2x, um segundo de mídia passa em bem menos de um segundo de relógio.
    assert!(inicio.elapsed() < Duration::from_millis(900), "velocidade 2x: {:?}", inicio.elapsed());
}

#[test]
fn erros_de_abertura() {
    let _s = comum::serial();
    let tmp = Tmp::new("erros");
    let ruim = tmp.caminho("ruim.wav");
    std::fs::write(&ruim, [0u8, 1, 2, 3]).unwrap();
    for c in [ruim, tmp.caminho("sumiu.mp4")] {
        let p = Player::abrir(&c);
        let e = esperar(&p, Duration::from_secs(5), |e| e.info.pronto);
        assert!(e.info.erro.is_some(), "{}", c.display());
        p.tocar();
        assert!(!p.estado().tocando, "com erro não toca");
    }
}

#[test]
fn renomear_e_apagar_com_o_arquivo_aberto() {
    let _s = comum::serial();
    let tmp = Tmp::new("renomear");
    let c = copiar(&tmp, "tom.wav", "tom.wav");
    let p = Player::abrir(&c);
    esperar(&p, Duration::from_secs(10), |e| e.info.pronto);
    let novo = tmp.caminho("tom-novo.wav");
    std::fs::rename(&c, &novo).expect("renomear aberto");
    std::fs::remove_file(&novo).expect("apagar aberto");
}

#[test]
fn soltar_para_a_thread() {
    let _s = comum::serial();
    let tmp = Tmp::new("soltar");
    let c = copiar(&tmp, "tom.wav", "tom.wav");
    let antes = orquestrador_midia::player::decodificadores_ativos();
    let p = Player::abrir(&c);
    p.tocar();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(orquestrador_midia::player::decodificadores_ativos(), antes + 1);
    drop(p);
    // `Drop` espera a thread: o contador volta na hora.
    assert_eq!(orquestrador_midia::player::decodificadores_ativos(), antes);
}
