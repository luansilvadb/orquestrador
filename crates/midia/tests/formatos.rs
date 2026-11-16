//! Formatos fora da lista antiga: a classificação vem do `image` e do Media Foundation, e o arquivo abre de fato.
mod comum;

use comum::{copiar, Tmp};
use orquestrador_midia::player::Player;
use orquestrador_midia::{imagem, midia_de, TipoMidia};
use std::path::Path;
use std::time::{Duration, Instant};

fn abre_e_toca(caminho: &Path, video: bool) {
    let p = Player::abrir(caminho);
    let inicio = Instant::now();
    while !p.estado().info.pronto {
        assert!(inicio.elapsed() < Duration::from_secs(10), "não abriu: {}", caminho.display());
        std::thread::sleep(Duration::from_millis(5));
    }
    let e = p.estado();
    assert_eq!(e.info.erro, None, "{}", caminho.display());
    assert_eq!(e.info.video.is_some(), video, "{}", caminho.display());
    // O relógio acompanha o tempo real.
    p.tocar();
    let inicio = Instant::now();
    while inicio.elapsed() < Duration::from_millis(1200) {
        let _ = p.quadro(true);
        std::thread::sleep(Duration::from_millis(5));
    }
    let pos = p.estado().posicao;
    assert!((0.6..1.4).contains(&pos), "{}: posição {pos} depois de 1,2 s", caminho.display());
}

#[test]
fn ico_tiff_e_tga_abrem_como_imagem() {
    let tmp = Tmp::new("imagens");
    let img = image::RgbaImage::from_fn(16, 8, |x, y| image::Rgba([x as u8 * 16, y as u8 * 32, 0, 255]));
    for nome in ["a.ico", "a.tiff", "a.TIF", "a.tga"] {
        let c = tmp.caminho(nome);
        img.save(&c).unwrap();
        assert_eq!(midia_de(&c), Some(TipoMidia::Imagem), "{nome}");
        let d = imagem::carregar(&c).unwrap_or_else(|e| panic!("{nome}: {e:?}"));
        assert_eq!((d.largura, d.altura), (16, 8), "{nome}");
    }
}

#[test]
fn ogg_vorbis_e_opus_abrem_como_audio_e_tocam_no_tempo() {
    let _s = comum::serial();
    let tmp = Tmp::new("ogg-tempo");
    for nome in ["tom.ogg", "tom.oga", "tom.opus"] {
        let c = copiar(&tmp, nome, nome);
        if !comum::decodifica(&c) {
            continue;
        }
        assert_eq!(midia_de(&c), Some(TipoMidia::Audio), "{nome}");
        abre_e_toca(&c, false);
    }
}

#[test]
fn mkv_e_avi_abrem_como_video() {
    let _s = comum::serial();
    let tmp = Tmp::new("mkv");
    for nome in ["clipe.mkv", "clipe.avi"] {
        let c = copiar(&tmp, nome, nome);
        assert_eq!(midia_de(&c), Some(TipoMidia::Video), "{nome}");
        abre_e_toca(&c, true);
    }
    // Só áudio num contêiner de vídeo: o fluxo decide.
    let c = copiar(&tmp, "so-audio.mkv", "so-audio.mkv");
    assert_eq!(midia_de(&c), Some(TipoMidia::Audio));
}

#[test]
fn sem_extensao_conhecida_sonda_pelo_conteudo() {
    let _s = comum::serial();
    let tmp = Tmp::new("sem-ext");
    let audio = copiar(&tmp, "tom.m4a", "gravacao");
    assert_eq!(midia_de(&audio), Some(TipoMidia::Audio));
    abre_e_toca(&audio, false);
    let video = copiar(&tmp, "clipe.mkv", "clipe.bin");
    assert_eq!(midia_de(&video), Some(TipoMidia::Video));
    // Binário que não é mídia e texto sem extensão seguem para o editor.
    let lixo = tmp.caminho("dados.bin");
    std::fs::write(&lixo, [0u8, 1, 2, 3, 0, 9, 8, 7].repeat(512)).unwrap();
    assert_eq!(midia_de(&lixo), None);
    let texto = tmp.caminho("LICENSE");
    std::fs::write(&texto, "MIT\n").unwrap();
    assert_eq!(midia_de(&texto), None);
    // O protocolo e a IPC do core seguem a mesma regra.
    assert!(orquestrador_core::ide::files::stream_path_at(&audio).is_ok());
    assert!(orquestrador_core::ide::files::stream_path_at(&lixo).is_err());
}
