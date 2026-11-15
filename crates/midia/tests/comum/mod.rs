//! Apoio dos testes: pasta temporária e arquivos de mídia pequenos gerados pelo próprio FFmpeg da build (encoders nativos LGPL).
#![allow(dead_code)]
use ffmpeg_next as ff;
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

pub struct Audio {
    pub encoder: &'static str,
    pub formato: ff::format::Sample,
    pub taxa: i32,
}

pub struct Video {
    pub encoder: &'static str,
    pub pixel: ff::format::Pixel,
    pub largura: u32,
    pub altura: u32,
    pub fps: i32,
}

pub fn pcm() -> Audio {
    Audio { encoder: "pcm_s16le", formato: ff::format::Sample::I16(ff::format::sample::Type::Packed), taxa: 44100 }
}

pub fn planar(encoder: &'static str, taxa: i32) -> Audio {
    Audio { encoder, formato: ff::format::Sample::F32(ff::format::sample::Type::Planar), taxa }
}

/// Gera `segundos` de seno (440 Hz) e/ou de quadros de cor que muda, no contêiner `muxer` (ou pela extensão).
pub fn gerar(caminho: &Path, muxer: Option<&str>, audio: Option<Audio>, video: Option<Video>, segundos: f64) -> Result<(), String> {
    orquestrador_midia::player::iniciar_ffmpeg();
    let erro = |e: ff::Error| e.to_string();
    let mut octx = match muxer {
        Some(m) => ff::format::output_as(caminho, m),
        None => ff::format::output(caminho),
    }
    .map_err(erro)?;
    let global = octx.format().flags().contains(ff::format::Flags::GLOBAL_HEADER);
    let mut experimental = ff::Dictionary::new();
    experimental.set("strict", "experimental");

    let mut enc_video = None;
    if let Some(v) = &video {
        let codec = ff::encoder::find_by_name(v.encoder).ok_or(format!("sem encoder {}", v.encoder))?;
        let mut e = ff::codec::context::Context::new_with_codec(codec).encoder().video().map_err(erro)?;
        e.set_width(v.largura);
        e.set_height(v.altura);
        e.set_format(v.pixel);
        e.set_time_base((1, v.fps));
        e.set_frame_rate(Some((v.fps, 1)));
        e.set_bit_rate(400_000);
        if global {
            e.set_flags(ff::codec::Flags::GLOBAL_HEADER);
        }
        let e = e.open_as_with(codec, experimental.clone()).map_err(erro)?;
        let mut st = octx.add_stream(codec).map_err(erro)?;
        st.set_parameters(&e);
        st.set_time_base((1, v.fps));
        enc_video = Some((e, st.index()));
    }
    let mut enc_audio = None;
    if let Some(a) = &audio {
        let codec = ff::encoder::find_by_name(a.encoder).ok_or(format!("sem encoder {}", a.encoder))?;
        let mut e = ff::codec::context::Context::new_with_codec(codec).encoder().audio().map_err(erro)?;
        e.set_rate(a.taxa);
        e.set_format(a.formato);
        e.set_channel_layout(ff::ChannelLayout::default(2));
        e.set_time_base((1, a.taxa));
        e.set_bit_rate(96_000);
        if global {
            e.set_flags(ff::codec::Flags::GLOBAL_HEADER);
        }
        let e = e.open_as_with(codec, experimental.clone()).map_err(erro)?;
        let mut st = octx.add_stream(codec).map_err(erro)?;
        st.set_parameters(&e);
        st.set_time_base((1, a.taxa));
        enc_audio = Some((e, st.index()));
    }
    octx.write_header().map_err(erro)?;

    let base = |octx: &ff::format::context::Output, i: usize| octx.stream(i).expect("stream").time_base();
    let mut pacote = ff::Packet::empty();

    if let (Some((e, i)), Some(v)) = (enc_video.as_mut(), &video) {
        let n = (segundos * f64::from(v.fps)).round() as i64;
        let tb = base(&octx, *i);
        for k in 0..=n {
            if k < n {
                let mut q = ff::frame::Video::new(v.pixel, v.largura, v.altura);
                let tom = (k * 255 / n.max(1)) as u8;
                for p in 0..q.planes() {
                    let valor = if p == 0 { tom } else { 128 };
                    q.data_mut(p).fill(valor);
                }
                q.set_pts(Some(k));
                e.send_frame(&q).map_err(erro)?;
            } else {
                e.send_eof().map_err(erro)?;
            }
            while e.receive_packet(&mut pacote).is_ok() {
                pacote.set_stream(*i);
                pacote.rescale_ts((1, v.fps), tb);
                pacote.write_interleaved(&mut octx).map_err(erro)?;
            }
        }
    }
    if let (Some((e, i)), Some(a)) = (enc_audio.as_mut(), &audio) {
        let tb = base(&octx, *i);
        let tamanho = match e.frame_size() {
            0 => 1024,
            n => n as usize,
        };
        let total = (segundos * f64::from(a.taxa)) as usize;
        let mut feito = 0usize;
        let mut fim = false;
        loop {
            if feito < total {
                let n = tamanho.min(total - feito);
                let mut q = ff::frame::Audio::new(a.formato, n, ff::ChannelLayout::default(2));
                q.set_rate(a.taxa as u32);
                let amostra = |j: usize| (((feito + j) as f64 * 440. * std::f64::consts::TAU / f64::from(a.taxa)).sin() * 0.2) as f32;
                if q.is_planar() {
                    for c in 0..2 {
                        for (j, s) in q.plane_mut::<f32>(c).iter_mut().enumerate().take(n) {
                            *s = amostra(j);
                        }
                    }
                } else {
                    let dados = q.data_mut(0);
                    for j in 0..n {
                        let v = ((amostra(j) * 32767.) as i16).to_le_bytes();
                        dados[j * 4..j * 4 + 2].copy_from_slice(&v);
                        dados[j * 4 + 2..j * 4 + 4].copy_from_slice(&v);
                    }
                }
                q.set_pts(Some(feito as i64));
                e.send_frame(&q).map_err(erro)?;
                feito += n;
            } else {
                e.send_eof().map_err(erro)?;
                fim = true;
            }
            while e.receive_packet(&mut pacote).is_ok() {
                pacote.set_stream(*i);
                pacote.rescale_ts((1, a.taxa), tb);
                pacote.write_interleaved(&mut octx).map_err(erro)?;
            }
            if fim {
                break;
            }
        }
    }
    octx.write_trailer().map_err(erro)?;
    Ok(())
}
