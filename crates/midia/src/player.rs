//! Player de áudio e vídeo: uma thread por arquivo lê e decodifica com o FFmpeg (estático, só LGPL), o áudio sai pelo
//! `cpal` e marca o relógio, e a UI pega o quadro de vídeo cuja hora chegou. Sem saída de áudio (ou sem faixa de áudio),
//! o relógio é o do sistema. Soltar o `Player` para a thread, que fecha o arquivo, o decodificador e o stream de áudio.
use ffmpeg_next as ff;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering::SeqCst};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, Once};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Quadro de vídeo já em BGRA, sem preenchimento entre linhas.
pub struct Quadro {
    pub pts: f64,
    pub largura: u32,
    pub altura: u32,
    pub bgra: Vec<u8>,
    geracao: u64,
}

/// O que se sabe do arquivo depois de aberto.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Info {
    /// Já abriu (ou falhou).
    pub pronto: bool,
    pub duracao: Option<f64>,
    /// Tamanho de exibição (com a proporção de pixel aplicada), como `videoWidth`/`videoHeight`.
    pub video: Option<(u32, u32)>,
    pub tem_audio: bool,
    pub erro: Option<String>,
}

/// Retrato do estado para a UI.
#[derive(Clone, Debug, PartialEq)]
pub struct Estado {
    pub info: Info,
    pub posicao: f64,
    pub tocando: bool,
    pub terminou: bool,
    pub volume: f32,
    pub mudo: bool,
    pub velocidade: f64,
}

enum Comando {
    Buscar(f64),
    Velocidade(f64),
}

struct Relogio {
    tocando: bool,
    /// Posição da mídia em `em`.
    pos: f64,
    em: Instant,
    velocidade: f64,
    /// O áudio marca o relógio; parado por falta de amostras não avança.
    pelo_audio: bool,
    faminto: bool,
    terminou: bool,
}

impl Relogio {
    fn agora(&self) -> f64 {
        if self.tocando && !self.faminto {
            self.pos + self.em.elapsed().as_secs_f64() * self.velocidade
        } else {
            self.pos
        }
    }

    fn fixar(&mut self, pos: f64) {
        self.pos = pos;
        self.em = Instant::now();
    }
}

struct Bloco {
    pts: f64,
    /// Segundos de mídia por quadro de saída (a velocidade dividida pela taxa do dispositivo).
    passo: f64,
    amostras: Vec<f32>,
    lido: usize,
    geracao: u64,
}

struct Comum {
    info: Mutex<Info>,
    relogio: Mutex<Relogio>,
    quadros: Mutex<VecDeque<Quadro>>,
    audio: Mutex<VecDeque<Bloco>>,
    volume: AtomicU32,
    mudo: AtomicBool,
    geracao: AtomicU64,
    /// A leitura chegou ao fim e os decodificadores esvaziaram.
    fim: AtomicBool,
    parar: AtomicBool,
    /// Tamanho em pixels de tela da área do vídeo (largura << 32 | altura); 0 enquanto não desenhou.
    alvo: AtomicU64,
}

fn travar<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Player {
    comum: Arc<Comum>,
    comandos: Sender<Comando>,
    thread: Option<JoinHandle<()>>,
}

/// Áudio decodificado sai sempre em estéreo f32; o `cpal` espalha nos canais do dispositivo.
const CANAIS: usize = 2;
/// Teto das filas: meio segundo de áudio e alguns quadros de vídeo (o vídeo cresce até `MAX_QUADROS` se o áudio passar fome).
const AUDIO_ALVO: f64 = 0.5;
const QUADROS_ALVO: usize = 4;
const MAX_QUADROS: usize = 24;

static INICIAR: Once = Once::new();
static ATIVOS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Threads de decodificação vivas no processo (cada uma segura arquivo, decodificadores e a saída de áudio).
pub fn decodificadores_ativos() -> usize {
    ATIVOS.load(SeqCst)
}

struct Viva;

impl Viva {
    fn new() -> Viva {
        ATIVOS.fetch_add(1, SeqCst);
        Viva
    }
}

impl Drop for Viva {
    fn drop(&mut self) {
        ATIVOS.fetch_sub(1, SeqCst);
    }
}

pub fn iniciar_ffmpeg() {
    INICIAR.call_once(|| {
        let _ = ff::init();
        ff::util::log::set_level(ff::util::log::Level::Quiet);
    });
}

impl Player {
    /// Abre o arquivo noutra thread; nasce parado na posição 0, com volume 1.
    pub fn abrir(caminho: &Path) -> Player {
        iniciar_ffmpeg();
        let mut p = Player::vazio();
        let (comandos, recebe) = mpsc::channel();
        p.comandos = comandos;
        let c = p.comum.clone();
        let caminho: PathBuf = caminho.to_path_buf();
        p.thread = std::thread::Builder::new()
            .name("midia-decodificador".into())
            .spawn(move || {
                let _viva = Viva::new();
                if let Err(e) = Decodificador::rodar(&caminho, &c, recebe) {
                    let mut info = travar(&c.info);
                    info.erro = Some(e);
                    info.pronto = true;
                }
            })
            .ok();
        if p.thread.is_none() {
            let mut info = travar(&p.comum.info);
            info.erro = Some("A thread de decodificação não iniciou.".into());
            info.pronto = true;
        }
        p
    }

    fn vazio() -> Player {
        let comum = Arc::new(Comum {
            info: Mutex::new(Info::default()),
            relogio: Mutex::new(Relogio {
                tocando: false,
                pos: 0.,
                em: Instant::now(),
                velocidade: 1.,
                pelo_audio: false,
                faminto: false,
                terminou: false,
            }),
            quadros: Mutex::new(VecDeque::new()),
            audio: Mutex::new(VecDeque::new()),
            volume: AtomicU32::new(1f32.to_bits()),
            mudo: AtomicBool::new(false),
            geracao: AtomicU64::new(0),
            fim: AtomicBool::new(false),
            parar: AtomicBool::new(false),
            alvo: AtomicU64::new(0),
        });
        Player { comum, comandos: mpsc::channel().0, thread: None }
    }

    /// Player que já nasce com erro (arquivo recusado antes de abrir).
    pub fn com_erro(mensagem: String) -> Player {
        let p = Player::vazio();
        {
            let mut info = travar(&p.comum.info);
            info.erro = Some(mensagem);
            info.pronto = true;
        }
        p
    }

    pub fn estado(&self) -> Estado {
        let info = travar(&self.comum.info).clone();
        let r = travar(&self.comum.relogio);
        let dur = info.duracao.unwrap_or(f64::INFINITY);
        Estado {
            posicao: r.agora().clamp(0., dur),
            tocando: r.tocando,
            terminou: r.terminou,
            velocidade: r.velocidade,
            info,
            volume: f32::from_bits(self.comum.volume.load(SeqCst)),
            mudo: self.comum.mudo.load(SeqCst),
        }
    }

    /// Toca; no fim, recomeça do início, como o `<video>`. Com erro, não faz nada.
    pub fn tocar(&self) {
        if travar(&self.comum.info).erro.is_some() {
            return;
        }
        let terminou = travar(&self.comum.relogio).terminou;
        if terminou {
            self.buscar(0.);
        }
        let mut r = travar(&self.comum.relogio);
        let pos = r.agora();
        r.fixar(pos);
        r.tocando = true;
        r.terminou = false;
    }

    pub fn pausar(&self) {
        let mut r = travar(&self.comum.relogio);
        let pos = r.agora();
        r.fixar(pos);
        r.tocando = false;
    }

    pub fn buscar(&self, segundos: f64) {
        let dur = travar(&self.comum.info).duracao.unwrap_or(f64::INFINITY);
        let alvo = segundos.clamp(0., dur.max(0.));
        let g = self.comum.geracao.fetch_add(1, SeqCst) + 1;
        {
            let mut r = travar(&self.comum.relogio);
            r.fixar(alvo);
            r.terminou = false;
            r.faminto = r.pelo_audio;
        }
        travar(&self.comum.quadros).retain(|q| q.geracao >= g);
        travar(&self.comum.audio).retain(|b| b.geracao >= g);
        self.comum.fim.store(false, SeqCst);
        let _ = self.comandos.send(Comando::Buscar(alvo));
    }

    pub fn definir_volume(&self, volume: f32) {
        self.comum.volume.store(volume.clamp(0., 1.).to_bits(), SeqCst);
    }

    /// Área do vídeo na tela, em pixels físicos. Vídeo bem menor que a área sai já ampliado do swscale (bilinear, como o
    /// Chromium): ampliado pela GPU, a borda do quadro borraria com o vizinho do atlas. Devolve se mudou.
    pub fn definir_alvo(&self, largura: u32, altura: u32) -> bool {
        let novo = (u64::from(largura) << 32) | u64::from(altura);
        self.comum.alvo.swap(novo, SeqCst) != novo
    }

    pub fn definir_mudo(&self, mudo: bool) {
        self.comum.mudo.store(mudo, SeqCst);
    }

    /// Velocidade de reprodução (o `atempo` do FFmpeg mantém o tom). Recomeça a decodificação na posição atual.
    pub fn definir_velocidade(&self, velocidade: f64) {
        let v = velocidade.clamp(0.25, 4.);
        let pos = {
            let mut r = travar(&self.comum.relogio);
            let pos = r.agora();
            r.fixar(pos);
            r.velocidade = v;
            pos
        };
        let _ = self.comandos.send(Comando::Velocidade(v));
        let terminou = travar(&self.comum.relogio).terminou;
        if !terminou {
            self.buscar(pos);
        }
    }

    /// Avança a máquina: marca o fim e devolve o quadro que deve estar na tela agora, se mudou. `sem_quadro` pede o primeiro
    /// quadro disponível mesmo parado (abertura e busca).
    pub fn quadro(&self, sem_quadro: bool) -> Option<Quadro> {
        let g = self.comum.geracao.load(SeqCst);
        let (agora, tocando) = {
            let r = travar(&self.comum.relogio);
            (r.agora(), r.tocando)
        };
        let mut fila = travar(&self.comum.quadros);
        while fila.front().is_some_and(|q| q.geracao != g) {
            fila.pop_front();
        }
        let mut escolhido = None;
        if tocando {
            while fila.front().is_some_and(|q| q.pts <= agora) {
                escolhido = fila.pop_front();
            }
        }
        if escolhido.is_none() && sem_quadro {
            escolhido = fila.pop_front();
        }
        let vazia = fila.is_empty();
        drop(fila);
        self.verificar_fim(vazia, agora);
        escolhido
    }

    fn verificar_fim(&self, quadros_vazios: bool, agora: f64) {
        if !self.comum.fim.load(SeqCst) || !quadros_vazios {
            return;
        }
        let audio_vazio = travar(&self.comum.audio).is_empty();
        let dur = travar(&self.comum.info).duracao;
        let mut r = travar(&self.comum.relogio);
        if !r.tocando || !audio_vazio {
            return;
        }
        // Sem áudio, o relógio do sistema corre até a duração antes de parar no último quadro.
        if !r.pelo_audio && dur.is_some_and(|d| agora < d) {
            return;
        }
        let fim = dur.unwrap_or(agora);
        r.fixar(fim);
        r.tocando = false;
        r.terminou = true;
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.comum.parar.store(true, SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Saida {
    _stream: cpal::Stream,
    taxa: u32,
}

/// Abre a saída padrão; o callback consome a fila de blocos, aplica o volume e marca o relógio.
fn abrir_saida(comum: &Arc<Comum>) -> Option<Saida> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let disp = cpal::default_host().default_output_device()?;
    let conf = disp.default_output_config().ok()?;
    let formato = conf.sample_format();
    let conf: cpal::StreamConfig = conf.into();
    let taxa = conf.sample_rate;
    let canais = conf.channels as usize;
    let c = comum.clone();
    let erro = |e: cpal::Error| eprintln!("áudio: {e}");
    macro_rules! stream {
        ($t:ty) => {
            disp.build_output_stream(
                conf.clone(),
                move |dados: &mut [$t], _: &cpal::OutputCallbackInfo| {
                    preencher(&c, canais, dados.len() / canais.max(1), |i, v| {
                        dados[i] = <$t as cpal::FromSample<f32>>::from_sample_(v)
                    })
                },
                erro,
                None,
            )
        };
    }
    let stream = match formato {
        cpal::SampleFormat::F32 => stream!(f32),
        cpal::SampleFormat::I16 => stream!(i16),
        cpal::SampleFormat::U16 => stream!(u16),
        cpal::SampleFormat::I32 => stream!(i32),
        cpal::SampleFormat::F64 => stream!(f64),
        _ => return None,
    }
    .ok()?;
    stream.play().ok()?;
    Some(Saida { _stream: stream, taxa })
}

fn preencher(c: &Comum, canais: usize, quadros: usize, mut escrever: impl FnMut(usize, f32)) {
    let tocando = travar(&c.relogio).tocando;
    let ganho = if c.mudo.load(SeqCst) { 0. } else { f32::from_bits(c.volume.load(SeqCst)) };
    let g = c.geracao.load(SeqCst);
    let mut pos = None;
    let mut faltou = false;
    {
        let mut fila = travar(&c.audio);
        for i in 0..quadros {
            let mut lr = [0f32; CANAIS];
            if tocando {
                while fila.front().is_some_and(|b| b.geracao != g || b.lido >= b.amostras.len()) {
                    fila.pop_front();
                }
                match fila.front_mut() {
                    Some(b) => {
                        lr.copy_from_slice(&b.amostras[b.lido..b.lido + CANAIS]);
                        b.lido += CANAIS;
                        pos = Some(b.pts + (b.lido / CANAIS) as f64 * b.passo);
                    }
                    None => faltou = true,
                }
            }
            for k in 0..canais {
                let v = match (canais, k) {
                    (1, _) => (lr[0] + lr[1]) * 0.5,
                    (_, 0) => lr[0],
                    (_, 1) => lr[1],
                    _ => 0.,
                };
                escrever(i * canais + k, v * ganho);
            }
        }
    }
    if !tocando {
        return;
    }
    let mut r = travar(&c.relogio);
    if !r.tocando || c.geracao.load(SeqCst) != g {
        return;
    }
    if let Some(p) = pos {
        r.fixar(p);
        r.faminto = false;
    } else if faltou {
        let p = r.agora();
        r.fixar(p);
        r.faminto = true;
    }
}

struct Decodificador {
    entrada: ff::format::context::Input,
    video: Option<Video>,
    audio: Option<Audio>,
    descartar_ate: f64,
    geracao: u64,
}

struct Video {
    indice: usize,
    base: f64,
    dec: ff::decoder::Video,
    escala: Option<(ff::software::scaling::Context, ChaveEscala)>,
    saida: (u32, u32),
    convertido: ff::frame::Video,
    quadro: ff::frame::Video,
}

/// Formato e tamanho de entrada e tamanho de saída que o contexto do swscale atende.
type ChaveEscala = (ff::format::Pixel, u32, u32, u32, u32);

struct Audio {
    indice: usize,
    base: f64,
    dec: ff::decoder::Audio,
    taxa: u32,
    reamostra: Option<ff::software::resampling::Context>,
    tempo: Option<Tempo>,
    velocidade: f64,
    quadro: ff::frame::Audio,
    convertido: ff::frame::Audio,
}

/// `atempo` com a posição de mídia da saída contada à parte (a saída do filtro não carrega a hora da mídia).
struct Tempo {
    grafo: ff::filter::Graph,
    pos: Option<f64>,
}

impl Decodificador {
    fn rodar(caminho: &Path, comum: &Arc<Comum>, comandos: Receiver<Comando>) -> Result<(), String> {
        // O `File` do Rust abre com FILE_SHARE_DELETE: renomear ou apagar o arquivo com a aba aberta funciona, como no
        // protocolo `orq-media`, que abria o arquivo a cada pedaço. O nome só ajuda o FFmpeg a adivinhar o formato.
        let erro = |e: &dyn std::fmt::Display| format!("Não foi possível abrir o arquivo: {e}");
        let arquivo = std::fs::File::open(caminho).map_err(|e| erro(&e))?;
        let io = ff::format::context::StreamIo::from_read_seek(arquivo).map_err(|e| erro(&e))?;
        let nome = caminho.file_name().map(|n| n.to_string_lossy().into_owned());
        let entrada = ff::format::input_from_stream(io, nome.as_deref(), None).map_err(|e| erro(&e))?;
        let duracao = (entrada.duration() > 0).then(|| entrada.duration() as f64 / f64::from(ff::ffi::AV_TIME_BASE));
        let video = match entrada.streams().best(ff::media::Type::Video) {
            Some(s) if s.disposition().contains(ff::format::stream::Disposition::ATTACHED_PIC) => None,
            Some(s) => Some(Video::novo(&s)?),
            None => None,
        };
        let saida = match entrada.streams().best(ff::media::Type::Audio) {
            Some(_) => abrir_saida(comum),
            None => None,
        };
        let audio = match (entrada.streams().best(ff::media::Type::Audio), &saida) {
            (Some(s), Some(saida)) => Some(Audio::novo(&s, saida.taxa)?),
            _ => None,
        };
        let tem_audio = entrada.streams().best(ff::media::Type::Audio).is_some();
        if video.is_none() && !tem_audio {
            return Err("O arquivo não tem áudio nem vídeo.".into());
        }
        {
            let mut info = travar(&comum.info);
            info.duracao = duracao;
            info.video = video.as_ref().map(|v| v.saida);
            info.tem_audio = tem_audio;
            info.pronto = true;
        }
        travar(&comum.relogio).pelo_audio = audio.is_some();
        let mut d = Decodificador { entrada, video, audio, descartar_ate: 0., geracao: 0 };
        let mut decodificou = false;
        loop {
            if comum.parar.load(SeqCst) {
                return Ok(());
            }
            let mut espera = Duration::ZERO;
            if d.cheio(comum) || comum.fim.load(SeqCst) {
                espera = Duration::from_millis(5);
            }
            let cmd = if espera.is_zero() {
                comandos.try_recv().map_err(|_| RecvTimeoutError::Timeout)
            } else {
                comandos.recv_timeout(espera)
            };
            match cmd {
                Ok(Comando::Buscar(t)) => {
                    d.buscar(t, comum);
                    continue;
                }
                Ok(Comando::Velocidade(v)) => {
                    if let Some(a) = d.audio.as_mut() {
                        a.velocidade = v;
                        a.tempo = None;
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
                Err(RecvTimeoutError::Timeout) => {}
            }
            if !espera.is_zero() {
                continue;
            }
            match d.ler(comum) {
                Ok(true) => decodificou = true,
                Ok(false) => {}
                Err(e) if !decodificou => return Err(e),
                Err(_) => {}
            }
        }
    }

    fn cheio(&self, c: &Comum) -> bool {
        let segundos_audio: f64 = travar(&c.audio).iter().map(|b| (b.amostras.len() - b.lido) as f64 / CANAIS as f64 * b.passo).sum();
        let quadros = travar(&c.quadros).len();
        let audio_cheio = self.audio.is_none() || segundos_audio >= AUDIO_ALVO;
        let video_cheio = self.video.is_none() || quadros >= QUADROS_ALVO;
        // Fila de vídeo cheia com o áudio com fome: lê mais (até o teto) para o relógio do áudio não parar.
        (audio_cheio && video_cheio) || quadros >= MAX_QUADROS || (self.audio.is_none() && video_cheio)
    }

    fn buscar(&mut self, t: f64, c: &Comum) {
        self.geracao = c.geracao.load(SeqCst);
        self.descartar_ate = t;
        let ts = (t * f64::from(ff::ffi::AV_TIME_BASE)) as i64;
        let _ = self.entrada.seek(ts, ..ts);
        if let Some(v) = self.video.as_mut() {
            v.dec.flush();
        }
        if let Some(a) = self.audio.as_mut() {
            a.dec.flush();
            a.tempo = None;
        }
        c.fim.store(false, SeqCst);
    }

    /// Lê um pacote e decodifica. `Ok(true)` quando saiu algum quadro ou amostra; no fim, esvazia os decodificadores.
    fn ler(&mut self, c: &Comum) -> Result<bool, String> {
        let pacote = self.entrada.packets().next();
        let Some((stream, pacote)) = pacote else {
            if let Some(v) = self.video.as_mut() {
                let _ = v.dec.send_eof();
            }
            if let Some(a) = self.audio.as_mut() {
                let _ = a.dec.send_eof();
            }
            self.colher(c)?;
            c.fim.store(true, SeqCst);
            return Ok(false);
        };
        let i = stream.index();
        if let Some(v) = self.video.as_mut().filter(|v| v.indice == i) {
            v.dec.send_packet(&pacote).map_err(|e| format!("Vídeo ilegível: {e}"))?;
        } else if let Some(a) = self.audio.as_mut().filter(|a| a.indice == i) {
            a.dec.send_packet(&pacote).map_err(|e| format!("Áudio ilegível: {e}"))?;
        } else {
            return Ok(false);
        }
        self.colher(c)
    }

    fn colher(&mut self, c: &Comum) -> Result<bool, String> {
        let mut saiu = false;
        let g = self.geracao;
        let alvo = c.alvo.load(SeqCst);
        let alvo = ((alvo >> 32) as u32, alvo as u32);
        if let Some(v) = self.video.as_mut() {
            while v.dec.receive_frame(&mut v.quadro).is_ok() {
                saiu = true;
                let pts = v.quadro.timestamp().map(|t| t as f64 * v.base).unwrap_or(self.descartar_ate);
                if pts + 1e-3 < self.descartar_ate {
                    continue;
                }
                if let Some(q) = v.converter(pts, g, alvo)? {
                    travar(&c.quadros).push_back(q);
                }
            }
        }
        if let Some(a) = self.audio.as_mut() {
            while a.dec.receive_frame(&mut a.quadro).is_ok() {
                saiu = true;
                let pts = a.quadro.timestamp().map(|t| t as f64 * a.base).unwrap_or(self.descartar_ate);
                for mut bloco in a.converter(pts, g)? {
                    // Corta o começo que fica antes do alvo da busca.
                    if bloco.pts < self.descartar_ate {
                        let pular = (((self.descartar_ate - bloco.pts) / bloco.passo) as usize).min(bloco.amostras.len() / CANAIS);
                        bloco.pts += pular as f64 * bloco.passo;
                        bloco.amostras.drain(..pular * CANAIS);
                    }
                    if !bloco.amostras.is_empty() {
                        travar(&c.audio).push_back(bloco);
                    }
                }
            }
        }
        Ok(saiu)
    }
}

/// Teto da ampliação pelo swscale, para o custo por quadro não crescer com a tela.
const MAX_AMPLIADO: (u32, u32) = (1920, 1080);

/// Ampliado para caber no alvo (sem passar de `MAX_AMPLIADO`) quando o alvo é ao menos 1,25 vez maior; senão, o tamanho de exibição.
fn tamanho_saida(exibe: (u32, u32), alvo: (u32, u32)) -> (u32, u32) {
    let (w, h) = (f64::from(exibe.0), f64::from(exibe.1));
    let caixa = (f64::from(alvo.0.min(MAX_AMPLIADO.0)), f64::from(alvo.1.min(MAX_AMPLIADO.1)));
    let fator = (caixa.0 / w).min(caixa.1 / h);
    if fator < 1.25 {
        return exibe;
    }
    (((w * fator).round() as u32).max(1), ((h * fator).round() as u32).max(1))
}

fn base(s: &ff::format::stream::Stream) -> f64 {
    f64::from(s.time_base())
}

impl Video {
    fn novo(s: &ff::format::stream::Stream) -> Result<Video, String> {
        let ctx = ff::codec::context::Context::from_parameters(s.parameters()).map_err(|e| e.to_string())?;
        let dec = ctx.decoder().video().map_err(|e| format!("Formato de vídeo não suportado: {e}"))?;
        let (w, h) = (dec.width(), dec.height());
        if w == 0 || h == 0 {
            return Err("Vídeo sem dimensões.".into());
        }
        let sar = dec.aspect_ratio();
        let largura = if sar.numerator() > 0 && sar.denominator() > 0 && sar.numerator() != sar.denominator() {
            ((f64::from(w) * f64::from(sar)).round() as u32).max(1)
        } else {
            w
        };
        Ok(Video {
            indice: s.index(),
            base: base(s),
            dec,
            escala: None,
            saida: (largura, h),
            convertido: ff::frame::Video::empty(),
            quadro: ff::frame::Video::empty(),
        })
    }

    /// Converte para BGRA no tamanho de exibição; o contexto do swscale só é refeito se a entrada mudar.
    fn converter(&mut self, pts: f64, geracao: u64, alvo: (u32, u32)) -> Result<Option<Quadro>, String> {
        let q = &self.quadro;
        if q.width() == 0 || q.height() == 0 {
            return Ok(None);
        }
        // Tamanho de exibição: a proporção de pixel aplicada à largura (se o quadro muda de tamanho no meio, vale o novo).
        let exibe = if q.width() == self.dec.width() && q.height() == self.dec.height() { self.saida } else { (q.width(), q.height()) };
        let (w, h) = tamanho_saida(exibe, alvo);
        let chave = (q.format(), q.width(), q.height(), w, h);
        if self.escala.as_ref().map(|e| e.1) != Some(chave) {
            let ctx = ff::software::scaling::Context::get(
                chave.0,
                chave.1,
                chave.2,
                ff::format::Pixel::BGRA,
                w,
                h,
                ff::software::scaling::Flags::BILINEAR
                    | ff::software::scaling::Flags::ACCURATE_RND
                    | ff::software::scaling::Flags::FULL_CHR_H_INT,
            )
            .map_err(|e| e.to_string())?;
            self.escala = Some((ctx, chave));
            // O destino reaproveitado tem o tamanho velho; o swscale recusa e o quadro se perderia.
            self.convertido = ff::frame::Video::empty();
        }
        let (ctx, _) = self.escala.as_mut().expect("escala criada acima");
        ctx.run(q, &mut self.convertido).map_err(|e| e.to_string())?;
        let passo = self.convertido.stride(0);
        let dados = self.convertido.data(0);
        let linha = w as usize * 4;
        let bgra = if passo == linha {
            dados[..linha * h as usize].to_vec()
        } else {
            let mut bgra = Vec::with_capacity(linha * h as usize);
            for y in 0..h as usize {
                bgra.extend_from_slice(&dados[y * passo..y * passo + linha]);
            }
            bgra
        };
        Ok(Some(Quadro { pts, largura: w, altura: h, bgra, geracao }))
    }
}

impl Audio {
    fn novo(s: &ff::format::stream::Stream, taxa: u32) -> Result<Audio, String> {
        let ctx = ff::codec::context::Context::from_parameters(s.parameters()).map_err(|e| e.to_string())?;
        let dec = ctx.decoder().audio().map_err(|e| format!("Formato de áudio não suportado: {e}"))?;
        Ok(Audio {
            indice: s.index(),
            base: base(s),
            dec,
            taxa,
            reamostra: None,
            tempo: None,
            velocidade: 1.,
            quadro: ff::frame::Audio::empty(),
            convertido: ff::frame::Audio::empty(),
        })
    }

    fn converter(&mut self, pts: f64, geracao: u64) -> Result<Vec<Bloco>, String> {
        let q = &mut self.quadro;
        let mut layout = q.channel_layout();
        if layout.is_empty() || layout.channels() != i32::from(q.channels()) {
            layout = ff::ChannelLayout::default(i32::from(q.channels()));
            q.set_channel_layout(layout);
        }
        let estereo = ff::ChannelLayout::default(CANAIS as i32);
        let refazer = self.reamostra.as_ref().is_none_or(|r| {
            let e = r.input();
            e.format != q.format() || e.rate != q.rate() || e.channel_layout != layout
        });
        if refazer {
            let r = ff::software::resampling::Context::get(
                q.format(),
                layout,
                q.rate(),
                ff::format::Sample::F32(ff::format::sample::Type::Packed),
                estereo,
                self.taxa,
            )
            .map_err(|e| e.to_string())?;
            self.reamostra = Some(r);
        }
        let r = self.reamostra.as_mut().expect("reamostrador criado acima");
        r.run(q, &mut self.convertido).map_err(|e| e.to_string())?;
        let n = self.convertido.samples() * CANAIS;
        if n == 0 {
            return Ok(Vec::new());
        }
        let amostras: Vec<f32> = bytemuck_f32(self.convertido.data(0), n);
        let passo_normal = 1. / f64::from(self.taxa);
        if (self.velocidade - 1.).abs() < 1e-6 {
            return Ok(vec![Bloco { pts, passo: passo_normal, amostras, lido: 0, geracao }]);
        }
        if self.tempo.is_none() {
            self.tempo = Some(Tempo::novo(self.taxa, self.velocidade)?);
        }
        let t = self.tempo.as_mut().expect("tempo criado acima");
        if t.pos.is_none() {
            t.pos = Some(pts);
        }
        let mut entrada = ff::frame::Audio::new(
            ff::format::Sample::F32(ff::format::sample::Type::Packed),
            amostras.len() / CANAIS,
            ff::ChannelLayout::default(CANAIS as i32),
        );
        entrada.set_rate(self.taxa);
        entrada.data_mut(0)[..amostras.len() * 4].copy_from_slice(f32_bytes(&amostras));
        t.grafo.get("in").expect("abuffer").source().add(&entrada).map_err(|e| e.to_string())?;
        let mut blocos = Vec::new();
        let mut saida = ff::frame::Audio::empty();
        let passo = self.velocidade / f64::from(self.taxa);
        while t.grafo.get("out").expect("abuffersink").sink().frame(&mut saida).is_ok() {
            let n = saida.samples() * CANAIS;
            let pos = t.pos.unwrap_or(pts);
            blocos.push(Bloco { pts: pos, passo, amostras: bytemuck_f32(saida.data(0), n), lido: 0, geracao });
            t.pos = Some(pos + saida.samples() as f64 * passo);
        }
        Ok(blocos)
    }
}

impl Tempo {
    fn novo(taxa: u32, velocidade: f64) -> Result<Tempo, String> {
        let mut grafo = ff::filter::Graph::new();
        let args = format!("time_base=1/{taxa}:sample_rate={taxa}:sample_fmt=flt:channel_layout=stereo");
        let erro = |e: ff::Error| format!("atempo: {e}");
        grafo.add(&ff::filter::find("abuffer").ok_or("abuffer ausente")?, "in", &args).map_err(erro)?;
        grafo.add(&ff::filter::find("abuffersink").ok_or("abuffersink ausente")?, "out", "").map_err(erro)?;
        let spec = format!("atempo={velocidade},aformat=sample_fmts=flt:sample_rates={taxa}:channel_layouts=stereo");
        grafo.output("in", 0).map_err(erro)?.input("out", 0).map_err(erro)?.parse(&spec).map_err(erro)?;
        grafo.validate().map_err(erro)?;
        Ok(Tempo { grafo, pos: None })
    }
}

fn bytemuck_f32(bytes: &[u8], n: usize) -> Vec<f32> {
    bytes[..n * 4].chunks_exact(4).map(|b| f32::from_ne_bytes([b[0], b[1], b[2], b[3]])).collect()
}

fn f32_bytes(v: &[f32]) -> &[u8] {
    // SAFETY: f32 não tem padding e qualquer padrão de bits é válido como u8.
    unsafe { std::slice::from_raw_parts(v.as_ptr().cast::<u8>(), std::mem::size_of_val(v)) }
}
