//! Decodificação de cada formato aceito pela aba, com arquivos gerados pelo FFmpeg da build (encoders nativos LGPL) ou pela
//! fixture do mock (WebM VP8). Formatos sem encoder LGPL nativo (MP3, VP9, H.264 sem Media Foundation) conferem o
//! decodificador e o demuxer nativos.
mod comum;

use comum::{gerar, pcm, planar, Tmp, Video};
use ffmpeg_next as ff;
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
fn conferir(caminho: &Path, duracao: f64, video: Option<(u32, u32)>, tem_audio: bool) {
    let p = Player::abrir(caminho);
    let e = esperar(&p, Duration::from_secs(10), |e| e.info.pronto);
    assert_eq!(e.info.erro, None, "{}", caminho.display());
    let d = e.info.duracao.expect("duração");
    assert!((d - duracao).abs() < 0.15, "{}: duração {d}", caminho.display());
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
fn wav_pcm() {
    let _s = comum::serial();
    let tmp = Tmp::new("wav");
    let c = tmp.caminho("tom.wav");
    gerar(&c, None, Some(pcm()), None, 2.).unwrap();
    conferir(&c, 2., None, true);
}

#[test]
fn flac() {
    let _s = comum::serial();
    let tmp = Tmp::new("flac");
    let c = tmp.caminho("tom.flac");
    let a = comum::Audio { encoder: "flac", formato: ff::format::Sample::I16(ff::format::sample::Type::Packed), taxa: 44100 };
    gerar(&c, None, Some(a), None, 2.).unwrap();
    conferir(&c, 2., None, true);
}

#[test]
fn ogg_vorbis_e_oga_opus() {
    let _s = comum::serial();
    let tmp = Tmp::new("ogg");
    let c = tmp.caminho("tom.ogg");
    gerar(&c, Some("ogg"), Some(planar("vorbis", 44100)), None, 2.).unwrap();
    conferir(&c, 2., None, true);
    let c = tmp.caminho("tom.oga");
    gerar(&c, Some("ogg"), Some(planar("opus", 48000)), None, 2.).unwrap();
    conferir(&c, 2., None, true);
}

#[test]
fn m4a_aac() {
    let _s = comum::serial();
    let tmp = Tmp::new("m4a");
    let c = tmp.caminho("tom.m4a");
    gerar(&c, Some("ipod"), Some(planar("aac", 44100)), None, 2.).unwrap();
    conferir(&c, 2., None, true);
}

fn mpeg4() -> Video {
    Video { encoder: "mpeg4", pixel: ff::format::Pixel::YUV420P, largura: 64, altura: 48, fps: 10 }
}

#[test]
fn mp4_m4v_mov_com_audio() {
    let _s = comum::serial();
    let tmp = Tmp::new("mp4");
    for (nome, muxer) in [("clipe.mp4", "mp4"), ("clipe.m4v", "mp4"), ("clipe.mov", "mov")] {
        let c = tmp.caminho(nome);
        gerar(&c, Some(muxer), Some(planar("aac", 44100)), Some(mpeg4()), 2.).unwrap();
        conferir(&c, 2., Some((64, 48)), true);
    }
}

#[test]
fn webm_vp8_do_mock() {
    let _s = comum::serial();
    let tmp = Tmp::new("webm");
    let c = comum::copiar(&tmp, "clipe.webm", "clipe.webm");
    conferir(&c, 2., Some((64, 48)), false);
}

/// H.264 pelo decodificador nativo; o arquivo vem do encoder do Media Foundation do Windows (fora do FFmpeg) quando existe.
#[test]
fn h264_nativo() {
    let _s = comum::serial();
    orquestrador_midia::player::iniciar_ffmpeg();
    let tmp = Tmp::new("h264");
    let c = tmp.caminho("clipe.mp4");
    let v = Video { encoder: "h264_mf", pixel: ff::format::Pixel::NV12, largura: 64, altura: 48, fps: 10 };
    match gerar(&c, Some("mp4"), None, Some(v), 2.) {
        Ok(()) => conferir(&c, 2., Some((64, 48)), false),
        Err(e) => eprintln!("h264_mf indisponível ({e}); só o decodificador é conferido"),
    }
}

/// Cada formato aceito tem demuxer e decodificador nativos (sem `lib*`), e a build não tem nada GPL nem nonfree.
#[test]
fn decodificadores_nativos_e_licenca() {
    let _s = comum::serial();
    orquestrador_midia::player::iniciar_ffmpeg();
    use ff::codec::Id;
    for id in [Id::MP3, Id::PCM_S16LE, Id::VORBIS, Id::OPUS, Id::FLAC, Id::AAC, Id::H264, Id::HEVC, Id::MPEG4, Id::VP8, Id::VP9, Id::AV1, Id::PRORES] {
        let d = ff::decoder::find(id);
        let nome = d.map(|d| d.name().to_string());
        // AV1 só tem decodificador por hardware ou libdav1d; fica de fora da exigência.
        if id == Id::AV1 {
            continue;
        }
        let nome = nome.unwrap_or_else(|| panic!("sem decodificador para {id:?}"));
        assert!(!nome.starts_with("lib"), "{id:?} usa biblioteca externa: {nome}");
    }
    for demuxer in ["mp3", "wav", "ogg", "flac", "aac", "mov", "matroska"] {
        let nome = std::ffi::CString::new(demuxer).unwrap();
        // SAFETY: consulta estática de tabela, sem estado.
        let achou = unsafe { !ff::ffi::av_find_input_format(nome.as_ptr()).is_null() };
        assert!(achou, "sem demuxer {demuxer}");
    }
    let licenca = unsafe { std::ffi::CStr::from_ptr(ff::ffi::avcodec_license()) }.to_string_lossy().into_owned();
    assert!(licenca.contains("LGPL"), "licença do avcodec: {licenca}");
    let conf = unsafe { std::ffi::CStr::from_ptr(ff::ffi::avcodec_configuration()) }.to_string_lossy().into_owned();
    assert!(!conf.contains("--enable-gpl") && !conf.contains("--enable-nonfree"), "{conf}");
}

#[test]
fn busca_e_fim() {
    let _s = comum::serial();
    let tmp = Tmp::new("busca");
    let c = tmp.caminho("clipe.mp4");
    gerar(&c, Some("mp4"), Some(planar("aac", 44100)), Some(mpeg4()), 2.).unwrap();
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
    let c = tmp.caminho("tom.wav");
    gerar(&c, None, Some(pcm()), None, 3.).unwrap();
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
fn soltar_para_a_thread() {
    let _s = comum::serial();
    let tmp = Tmp::new("soltar");
    let c = tmp.caminho("tom.wav");
    gerar(&c, None, Some(pcm()), None, 2.).unwrap();
    let antes = orquestrador_midia::player::decodificadores_ativos();
    let p = Player::abrir(&c);
    p.tocar();
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(orquestrador_midia::player::decodificadores_ativos(), antes + 1);
    drop(p);
    // `Drop` espera a thread: o contador volta na hora.
    assert_eq!(orquestrador_midia::player::decodificadores_ativos(), antes);
}
