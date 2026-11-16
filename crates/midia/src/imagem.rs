//! Leitura e decodificação de imagem fora da thread da UI. Os bytes vêm de `files::read_media_at`; GIF e WebP animados viram vários quadros, que o `img` do GPUI anima.
use gpui_kit::RenderImage;
use image::{AnimationDecoder, Delay, Frame, ImageFormat, RgbaImage};
use smallvec::SmallVec;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

/// Falha da leitura (mensagem do core) ou da decodificação.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FalhaImagem {
    Leitura(String),
    Decodificacao,
}

pub struct ImagemDecodificada {
    pub imagem: Arc<RenderImage>,
    pub largura: u32,
    pub altura: u32,
}

pub fn carregar(caminho: &Path) -> Result<ImagemDecodificada, FalhaImagem> {
    crate::iniciar();
    let bytes = orquestrador_core::ide::files::read_media_at(caminho).map_err(FalhaImagem::Leitura)?;
    decodificar_com(&bytes, ImageFormat::from_path(caminho).ok()).ok_or(FalhaImagem::Decodificacao)
}

/// Decodifica pelo conteúdo (como o navegador, que não olha a extensão).
pub fn decodificar(bytes: &[u8]) -> Option<ImagemDecodificada> {
    decodificar_com(bytes, None)
}

/// Pelo conteúdo; sem assinatura reconhecida (o TGA não tem), pelo formato da extensão.
fn decodificar_com(bytes: &[u8], extensao: Option<ImageFormat>) -> Option<ImagemDecodificada> {
    let formato = image::guess_format(bytes).ok().or(extensao)?;
    let quadros: Vec<Frame> = match formato {
        ImageFormat::Gif => image::codecs::gif::GifDecoder::new(Cursor::new(bytes)).ok()?.into_frames().collect_frames().ok()?,
        ImageFormat::WebP => {
            let dec = image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
            if dec.has_animation() {
                dec.into_frames().collect_frames().ok()?
            } else {
                vec![Frame::new(image::DynamicImage::from_decoder(dec).ok()?.into_rgba8())]
            }
        }
        ImageFormat::Png => vec![Frame::new(match image::load_from_memory_with_format(bytes, formato) {
            Ok(i) => i.into_rgba8(),
            Err(_) => png_parcial(bytes)?,
        })],
        _ => vec![Frame::new(image::load_from_memory_with_format(bytes, formato).ok()?.into_rgba8())],
    };
    let primeiro = quadros.first()?;
    let (largura, altura) = primeiro.buffer().dimensions();
    if largura == 0 || altura == 0 {
        return None;
    }
    let quadros: SmallVec<[Frame; 1]> = quadros.into_iter().map(para_bgra).collect();
    Some(ImagemDecodificada { imagem: Arc::new(RenderImage::new(quadros)), largura, altura })
}

/// PNG com dados corrompidos no meio: como o Chromium, mostra as linhas que decodificaram e deixa o resto transparente.
/// Sem cabeçalho ou sem nenhuma linha, falha.
fn png_parcial(bytes: &[u8]) -> Option<RgbaImage> {
    let mut dec = png::Decoder::new(Cursor::new(bytes));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut leitor = dec.read_info().ok()?;
    let (w, h) = (leitor.info().width, leitor.info().height);
    let (cor, _) = leitor.output_color_type();
    let mut img = RgbaImage::new(w, h);
    let mut y = 0;
    while let Ok(Some(linha)) = leitor.next_row() {
        if y >= h {
            break;
        }
        for (x, px) in linha.data().chunks(cor.samples()).take(w as usize).enumerate() {
            let rgba = match px {
                [g] => [*g, *g, *g, 255],
                [g, a] => [*g, *g, *g, *a],
                [r, g, b] => [*r, *g, *b, 255],
                [r, g, b, a] => [*r, *g, *b, *a],
                _ => [0; 4],
            };
            img.put_pixel(x as u32, y, image::Rgba(rgba));
        }
        y += 1;
    }
    (y > 0).then_some(img)
}

/// O `RenderImage` do GPUI guarda BGRA no buffer RGBA.
fn para_bgra(quadro: Frame) -> Frame {
    let (esq, topo, atraso) = (quadro.left(), quadro.top(), quadro.delay());
    let mut buf: RgbaImage = quadro.into_buffer();
    for px in buf.pixels_mut() {
        px.0.swap(0, 2);
    }
    Frame::from_parts(buf, esq, topo, atraso)
}

/// Quadro único a partir de BGRA já pronto (vídeo).
pub fn quadro_bgra(largura: u32, altura: u32, bgra: Vec<u8>) -> Option<Arc<RenderImage>> {
    let buf = RgbaImage::from_raw(largura, altura, bgra)?;
    Some(Arc::new(RenderImage::new(SmallVec::from_elem(Frame::from_parts(buf, 0, 0, Delay::from_numer_denom_ms(0, 1)), 1))))
}

#[cfg(test)]
mod testes {
    #[test]
    fn png_do_mock() {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/logo.png")).unwrap();
        // O PNG do mock tem dados corrompidos depois da primeira linha; o Chromium mostra o que decodificou.
        let d = super::decodificar(&bytes).expect("PNG parcial");
        assert_eq!((d.largura, d.altura), (32, 32));
        assert!(super::decodificar(&[0, 1, 2, 3]).is_none());
        assert!(super::decodificar(&bytes[..40]).is_none(), "só o cabeçalho, sem linhas");
    }
}
