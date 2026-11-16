//! Player de áudio e vídeo pelo Media Engine do Windows (o motor do `<video>`): numa thread por arquivo ele lê,
//! decodifica, sincroniza e toca o áudio; a thread copia cada quadro novo da GPU para BGRA e a UI pega o último.
//! Soltar o `Player` para a thread, que desliga o motor.
use crate::mf;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use windows::core::{implement, Interface, BSTR, HRESULT};
use windows::Win32::Foundation::{HMODULE, RECT, S_OK};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::*;
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};

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
    Tocar,
    Pausar,
    Buscar(f64),
    Volume(f32),
    Mudo(bool),
    Velocidade(f64),
}

/// Relógio da UI: os comandos o movem na hora; tocando, a thread o acerta pela posição do motor.
struct Relogio {
    tocando: bool,
    /// Posição da mídia em `em`.
    pos: f64,
    em: Instant,
    velocidade: f64,
    terminou: bool,
}

impl Relogio {
    fn agora(&self) -> f64 {
        if self.tocando {
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

struct Comum {
    info: Mutex<Info>,
    relogio: Mutex<Relogio>,
    /// Último quadro ainda não entregue à UI.
    quadro: Mutex<Option<Quadro>>,
    volume: AtomicU32,
    mudo: AtomicBool,
    geracao: AtomicU64,
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

/// Intervalo da thread entre olhadas no motor (quadro novo, posição, fim).
const PASSO: Duration = Duration::from_millis(4);
/// Depois de uma busca, quanto esperar pelo quadro do alvo antes de aceitar outro.
const PRAZO_DA_BUSCA: Duration = Duration::from_millis(500);
/// Sem saída de áudio, quanto tocar à procura do quadro do alvo antes de aceitar outro.
const PRAZO_DA_ESPIADA: Duration = Duration::from_secs(3);

static ATIVOS: AtomicUsize = AtomicUsize::new(0);

/// Threads de reprodução vivas no processo (cada uma segura o arquivo e o motor).
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

impl Player {
    /// Abre o arquivo noutra thread; nasce parado na posição 0, com volume 1.
    pub fn abrir(caminho: &Path) -> Player {
        let mut p = Player::vazio();
        let (comandos, recebe) = mpsc::channel();
        p.comandos = comandos;
        let c = p.comum.clone();
        let caminho: PathBuf = caminho.to_path_buf();
        p.thread = std::thread::Builder::new()
            .name("midia-decodificador".into())
            .spawn(move || {
                let _viva = Viva::new();
                if let Err(e) = rodar(&caminho, &c, recebe) {
                    let mut info = travar(&c.info);
                    info.erro = Some(e);
                    info.pronto = true;
                }
            })
            .ok();
        if p.thread.is_none() {
            let mut info = travar(&p.comum.info);
            info.erro = Some("A thread de reprodução não iniciou.".into());
            info.pronto = true;
        }
        p
    }

    fn vazio() -> Player {
        let comum = Arc::new(Comum {
            info: Mutex::new(Info::default()),
            relogio: Mutex::new(Relogio { tocando: false, pos: 0., em: Instant::now(), velocidade: 1., terminou: false }),
            quadro: Mutex::new(None),
            volume: AtomicU32::new(1f32.to_bits()),
            mudo: AtomicBool::new(false),
            geracao: AtomicU64::new(0),
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
        let _ = self.comandos.send(Comando::Tocar);
    }

    pub fn pausar(&self) {
        let mut r = travar(&self.comum.relogio);
        let pos = r.agora();
        r.fixar(pos);
        r.tocando = false;
        let _ = self.comandos.send(Comando::Pausar);
    }

    pub fn buscar(&self, segundos: f64) {
        let dur = travar(&self.comum.info).duracao.unwrap_or(f64::INFINITY);
        let alvo = segundos.clamp(0., dur.max(0.));
        let g = self.comum.geracao.fetch_add(1, SeqCst) + 1;
        {
            let mut r = travar(&self.comum.relogio);
            r.fixar(alvo);
            r.terminou = false;
        }
        let mut q = travar(&self.comum.quadro);
        if q.as_ref().is_some_and(|q| q.geracao < g) {
            *q = None;
        }
        drop(q);
        let _ = self.comandos.send(Comando::Buscar(alvo));
    }

    pub fn definir_volume(&self, volume: f32) {
        let v = volume.clamp(0., 1.);
        self.comum.volume.store(v.to_bits(), SeqCst);
        let _ = self.comandos.send(Comando::Volume(v));
    }

    /// Área do vídeo na tela, em pixels físicos. Vídeo bem menor que a área sai já ampliado do motor: ampliado pela
    /// GPU do GPUI, a borda do quadro borraria com o vizinho do atlas. Devolve se mudou.
    pub fn definir_alvo(&self, largura: u32, altura: u32) -> bool {
        let novo = (u64::from(largura) << 32) | u64::from(altura);
        self.comum.alvo.swap(novo, SeqCst) != novo
    }

    pub fn definir_mudo(&self, mudo: bool) {
        self.comum.mudo.store(mudo, SeqCst);
        let _ = self.comandos.send(Comando::Mudo(mudo));
    }

    /// Velocidade de reprodução; o motor mantém o tom.
    pub fn definir_velocidade(&self, velocidade: f64) {
        let v = velocidade.clamp(0.25, 4.);
        let mut r = travar(&self.comum.relogio);
        let pos = r.agora();
        r.fixar(pos);
        r.velocidade = v;
        let _ = self.comandos.send(Comando::Velocidade(v));
    }

    /// O quadro novo desde a última chamada, se houver. O motor só entrega quadro quando ele muda (tocando, ao abrir e
    /// depois de uma busca); `_sem_quadro` fica pela interface antiga.
    pub fn quadro(&self, _sem_quadro: bool) -> Option<Quadro> {
        let g = self.comum.geracao.load(SeqCst);
        travar(&self.comum.quadro).take().filter(|q| q.geracao == g)
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

/// O que o motor avisa pela `IMFMediaEngineNotify`, lido pela thread do player.
#[derive(Default)]
struct Eventos {
    metadados: AtomicBool,
    fim: AtomicBool,
    erro: Mutex<Option<HRESULT>>,
}

#[implement(IMFMediaEngineNotify)]
struct Notificacao(Arc<Eventos>);

impl IMFMediaEngineNotify_Impl for Notificacao_Impl {
    fn EventNotify(&self, evento: u32, _param1: usize, param2: u32) -> windows::core::Result<()> {
        match MF_MEDIA_ENGINE_EVENT(evento as i32) {
            MF_MEDIA_ENGINE_EVENT_LOADEDMETADATA => self.0.metadados.store(true, SeqCst),
            MF_MEDIA_ENGINE_EVENT_ENDED => self.0.fim.store(true, SeqCst),
            MF_MEDIA_ENGINE_EVENT_ERROR => *travar(&self.0.erro) = Some(HRESULT(param2 as i32)),
            _ => {}
        }
        Ok(())
    }
}

/// Device D3D11 do motor (decodificação e escala na GPU) e o contexto que copia o quadro para a CPU.
struct Gpu {
    contexto: ID3D11DeviceContext,
    device: ID3D11Device,
    gerente: IMFDXGIDeviceManager,
}

impl Gpu {
    /// Placa de vídeo; sem ela (ou sem decodificação nela), o WARP do Windows.
    fn nova() -> Result<Gpu, String> {
        let video = D3D11_CREATE_DEVICE_VIDEO_SUPPORT | D3D11_CREATE_DEVICE_BGRA_SUPPORT;
        let tentativas: [(D3D_DRIVER_TYPE, D3D11_CREATE_DEVICE_FLAG); 3] = [
            (D3D_DRIVER_TYPE_HARDWARE, video),
            (D3D_DRIVER_TYPE_WARP, video),
            (D3D_DRIVER_TYPE_WARP, D3D11_CREATE_DEVICE_BGRA_SUPPORT),
        ];
        let mut ultimo = String::new();
        for (tipo, flags) in tentativas {
            let (mut device, mut contexto) = (None, None);
            // SAFETY: saídas locais; o device vive no `Gpu`.
            let r = unsafe { D3D11CreateDevice(None, tipo, HMODULE::default(), flags, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut contexto)) };
            match (r, device, contexto) {
                (Ok(()), Some(device), Some(contexto)) => return Gpu::com(device, contexto),
                (Err(e), ..) => ultimo = e.message(),
                _ => {}
            }
        }
        Err(format!("Sem Direct3D 11 para o vídeo: {ultimo}"))
    }

    fn com(device: ID3D11Device, contexto: ID3D11DeviceContext) -> Result<Gpu, String> {
        let erro = |e: windows::core::Error| e.message();
        // SAFETY: o motor usa o device de outras threads; a proteção multithread é exigida para o gerente DXGI.
        unsafe {
            let _ = device.cast::<ID3D11Multithread>().map_err(erro)?.SetMultithreadProtected(true);
            let (mut token, mut gerente) = (0u32, None);
            MFCreateDXGIDeviceManager(&mut token, &mut gerente).map_err(erro)?;
            let gerente = gerente.ok_or("Sem gerente DXGI.")?;
            gerente.ResetDevice(&device, token).map_err(erro)?;
            Ok(Gpu { contexto, device, gerente })
        }
    }
}

/// Textura em que o motor desenha o quadro e a cópia legível pela CPU, no tamanho de saída.
struct Destino {
    tamanho: (u32, u32),
    alvo: ID3D11Texture2D,
    leitura: ID3D11Texture2D,
}

impl Destino {
    fn novo(gpu: &Gpu, (w, h): (u32, u32)) -> windows::core::Result<Destino> {
        let mut desc = D3D11_TEXTURE2D_DESC {
            Width: w,
            Height: h,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_RENDER_TARGET.0 as u32,
            CPUAccessFlags: 0,
            MiscFlags: 0,
        };
        let (mut alvo, mut leitura) = (None, None);
        // SAFETY: descrições válidas; saídas locais.
        unsafe {
            gpu.device.CreateTexture2D(&desc, None, Some(&mut alvo))?;
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            gpu.device.CreateTexture2D(&desc, None, Some(&mut leitura))?;
        }
        let falta = || windows::core::Error::from(windows::Win32::Foundation::E_FAIL);
        Ok(Destino { tamanho: (w, h), alvo: alvo.ok_or_else(falta)?, leitura: leitura.ok_or_else(falta)? })
    }

    /// Desenha o quadro atual do motor e lê os pixels.
    fn copiar(&self, gpu: &Gpu, motor: &IMFMediaEngine) -> windows::core::Result<Vec<u8>> {
        let (w, h) = self.tamanho;
        let rect = RECT { left: 0, top: 0, right: w as i32, bottom: h as i32 };
        // SAFETY: texturas do mesmo device; o mapeamento é desfeito antes de sair.
        unsafe {
            motor.TransferVideoFrame(&self.alvo, None, &rect, None)?;
            gpu.contexto.CopyResource(&self.leitura, &self.alvo);
            let mut m = D3D11_MAPPED_SUBRESOURCE::default();
            gpu.contexto.Map(&self.leitura, 0, D3D11_MAP_READ, 0, Some(&mut m))?;
            let linha = w as usize * 4;
            let mut bgra = Vec::with_capacity(linha * h as usize);
            for y in 0..h as usize {
                let origem = m.pData.cast::<u8>().add(y * m.RowPitch as usize);
                bgra.extend_from_slice(std::slice::from_raw_parts(origem, linha));
            }
            gpu.contexto.Unmap(&self.leitura, 0);
            Ok(bgra)
        }
    }
}

/// Teto da ampliação pelo motor, para o custo da cópia por quadro não crescer com a tela.
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

/// Mensagem de falha ao abrir: a extensão da Store que falta, quando é isso; senão, o erro do Windows.
fn mensagem_de_erro(caminho: &Path, detalhe: &str) -> String {
    match mf::extensao_que_falta(caminho) {
        Some(ext) => format!("O Windows não tem decodificador para este formato. Instale \u{201c}{ext}\u{201d} na Microsoft Store."),
        None => format!("Não foi possível abrir o arquivo: {detalhe}"),
    }
}

/// Thread do player: sessão MF, GPU e motor; desliga o motor antes de soltar a sessão.
fn rodar(caminho: &Path, c: &Arc<Comum>, comandos: Receiver<Comando>) -> Result<(), String> {
    let _sessao = mf::Sessao::iniciar()?;
    let gpu = Gpu::nova()?;
    let eventos = Arc::new(Eventos::default());
    let motor = criar_motor(&gpu, &eventos)?;
    let r = tocar_arquivo(caminho, c, &comandos, &gpu, &motor, &eventos);
    // SAFETY: o motor não é mais usado depois daqui.
    unsafe {
        let _ = motor.Shutdown();
    }
    r
}

fn criar_motor(gpu: &Gpu, eventos: &Arc<Eventos>) -> Result<IMFMediaEngine, String> {
    let erro = |e: windows::core::Error| format!("Media Engine indisponível: {}", e.message());
    let notificacao: IMFMediaEngineNotify = Notificacao(eventos.clone()).into();
    // SAFETY: chamadas COM com a sessão da thread.
    unsafe {
        let fabrica: IMFMediaEngineClassFactory = CoCreateInstance(&CLSID_MFMediaEngineClassFactory, None, CLSCTX_INPROC_SERVER).map_err(erro)?;
        let mut atributos = None;
        MFCreateAttributes(&mut atributos, 3).map_err(erro)?;
        let atributos = atributos.ok_or("Sem atributos.")?;
        atributos.SetUnknown(&MF_MEDIA_ENGINE_CALLBACK, &notificacao).map_err(erro)?;
        atributos.SetUnknown(&MF_MEDIA_ENGINE_DXGI_MANAGER, &gpu.gerente).map_err(erro)?;
        atributos.SetUINT32(&MF_MEDIA_ENGINE_VIDEO_OUTPUT_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM.0 as u32).map_err(erro)?;
        fabrica.CreateInstance(0, &atributos).map_err(erro)
    }
}

/// Sem saída de áudio, o arquivo só de áudio toca sem som: o relógio corre e marca o fim, como no `<audio>` mudo.
fn tocar_sem_som(c: &Comum, comandos: &Receiver<Comando>, duracao: Option<f64>) -> Result<(), String> {
    {
        let mut info = travar(&c.info);
        info.duracao = duracao;
        info.tem_audio = true;
        info.pronto = true;
    }
    loop {
        if c.parar.load(SeqCst) {
            return Ok(());
        }
        if let Err(RecvTimeoutError::Disconnected) = comandos.recv_timeout(PASSO) {
            return Ok(());
        }
        let mut r = travar(&c.relogio);
        if let Some(d) = duracao.filter(|&d| r.tocando && r.agora() >= d) {
            r.fixar(d);
            r.tocando = false;
            r.terminou = true;
        }
    }
}

/// Índices dos fluxos de áudio na numeração do motor (que não segue a ordem da fonte). Depois dos metadados.
fn fluxos_de_audio(ex: &IMFMediaEngineEx) -> Vec<u32> {
    use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
    use windows::Win32::System::Variant::VT_CLSID;
    // SAFETY: o PROPVARIANT é lido só quando traz um GUID e é liberado em seguida.
    let n = unsafe { ex.GetNumberOfStreams() }.unwrap_or(0);
    (0..n)
        .filter(|&i| unsafe {
            let Ok(mut pv) = ex.GetStreamAttribute(i, &MF_MT_MAJOR_TYPE) else { return false };
            let v = &pv.Anonymous.Anonymous;
            let audio = v.vt == VT_CLSID && !v.Anonymous.puuid.is_null() && *v.Anonymous.puuid == MFMediaType_Audio;
            let _ = PropVariantClear(&mut pv);
            audio
        })
        .collect()
}

fn tocar_arquivo(caminho: &Path, c: &Comum, comandos: &Receiver<Comando>, gpu: &Gpu, motor: &IMFMediaEngine, eventos: &Eventos) -> Result<(), String> {
    let erro = |e: windows::core::Error| format!("Não foi possível abrir o arquivo: {}", e.message());
    // Sem saída de áudio, as faixas de áudio são desligadas antes de o motor montar a reprodução; só áudio, nem passa por ele.
    // `Some(tem áudio)` quando falta a saída.
    let sem_som = if mf::tem_saida_de_audio() {
        None
    } else {
        let d = mf::descrever(caminho).map_err(|e| mensagem_de_erro(caminho, &e))?;
        let tem = |tipo| d.fluxos.iter().any(|f| f.0 == tipo);
        match (tem(MFMediaType_Video), tem(MFMediaType_Audio)) {
            (false, false) => return Err("O arquivo não tem áudio nem vídeo.".into()),
            (false, true) => {
                if !d.fluxos.iter().all(|&(p, s)| mf::decodifica(p, s)) {
                    return Err(mensagem_de_erro(caminho, "sem decodificador para o áudio."));
                }
                return tocar_sem_som(c, comandos, d.duracao);
            }
            (true, audio) => Some(audio),
        }
    };
    let bs = mf::abrir(caminho)?;
    let url = BSTR::from(caminho.to_string_lossy().as_ref());
    let ex = motor.cast::<IMFMediaEngineEx>().map_err(erro)?;
    let preload = if sem_som.is_some() { MF_MEDIA_ENGINE_PRELOAD_METADATA } else { MF_MEDIA_ENGINE_PRELOAD_AUTOMATIC };
    // SAFETY: chamadas COM com a sessão da thread.
    unsafe {
        motor.SetPreload(preload).map_err(erro)?;
        ex.SetSourceFromByteStream(&bs, &url).map_err(erro)?;
    }
    loop {
        if c.parar.load(SeqCst) {
            return Ok(());
        }
        if let Some(hr) = *travar(&eventos.erro) {
            return Err(mensagem_de_erro(caminho, &windows::core::Error::from(hr).message()));
        }
        if eventos.metadados.load(SeqCst) {
            break;
        }
        std::thread::sleep(PASSO);
    }
    if sem_som.is_some() {
        // SAFETY: chamadas COM com a sessão da thread, depois dos metadados.
        unsafe {
            for i in fluxos_de_audio(&ex) {
                ex.SetStreamSelection(i, false).map_err(erro)?;
            }
            ex.ApplyStreamSelections().map_err(erro)?;
            motor.SetPreload(MF_MEDIA_ENGINE_PRELOAD_AUTOMATIC).map_err(erro)?;
        }
    }
    // SAFETY: metadados carregados; consultas sem efeito colateral.
    let (duracao, video, tem_audio) = unsafe {
        let d = motor.GetDuration();
        let duracao = (d.is_finite() && d > 0.).then_some(d);
        let video = motor.HasVideo().as_bool().then(|| {
            let (mut w, mut h, mut px, mut py) = (0u32, 0u32, 0u32, 0u32);
            let _ = motor.GetNativeVideoSize(Some(&mut w), Some(&mut h));
            let _ = motor.GetVideoAspectRatio(Some(&mut px), Some(&mut py));
            // A proporção que o motor dá é a da imagem (4:3, 16:9), já com a do pixel aplicada.
            let largura = if px > 0 && py > 0 { ((f64::from(h) * f64::from(px) / f64::from(py)).round() as u32).max(1) } else { w };
            (largura, h)
        });
        let tem_audio = sem_som.unwrap_or_else(|| motor.HasAudio().as_bool());
        (duracao, video.filter(|v| v.0 > 0 && v.1 > 0), tem_audio)
    };
    if video.is_none() && !tem_audio {
        return Err("O arquivo não tem áudio nem vídeo.".into());
    }
    {
        let mut info = travar(&c.info);
        info.duracao = duracao;
        info.video = video;
        info.tem_audio = tem_audio;
        info.pronto = true;
    }
    // SAFETY: chamadas COM com a sessão da thread.
    unsafe {
        let _ = motor.SetVolume(f64::from(f32::from_bits(c.volume.load(SeqCst))));
        let _ = motor.SetMuted(c.mudo.load(SeqCst));
    }
    let mut geracao = c.geracao.load(SeqCst);
    let mut destino: Option<Destino> = None;
    let mut busca: Option<(f64, Instant)> = None;
    // Posição do último quadro copiado.
    let mut ultimo: Option<f64> = None;
    // Sem saída de áudio, o motor parado não desenha o quadro buscado: depois da busca, toca até ele aparecer e volta a
    // parar no alvo. `(alvo, já tocou)`.
    let mut espiar: Option<(f64, bool)> = None;
    loop {
        if c.parar.load(SeqCst) {
            return Ok(());
        }
        let mut cmd = match comandos.recv_timeout(PASSO) {
            Ok(cmd) => Some(cmd),
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => None,
        };
        while let Some(atual) = cmd {
            // SAFETY: chamadas COM com a sessão da thread.
            unsafe {
                let _ = match atual {
                    Comando::Tocar => {
                        espiar = None;
                        motor.Play()
                    }
                    Comando::Pausar => {
                        espiar = None;
                        motor.Pause()
                    }
                    Comando::Buscar(t) => {
                        geracao = c.geracao.load(SeqCst);
                        busca = Some((t, Instant::now()));
                        let r = motor.SetCurrentTime(t);
                        if sem_som.is_some() && video.is_some() && !travar(&c.relogio).tocando {
                            espiar = Some((t, false));
                        }
                        r
                    }
                    Comando::Volume(v) => motor.SetVolume(f64::from(v)),
                    Comando::Mudo(m) => motor.SetMuted(m),
                    Comando::Velocidade(v) => motor.SetDefaultPlaybackRate(v).and_then(|_| motor.SetPlaybackRate(v)),
                };
            }
            cmd = comandos.try_recv().ok();
        }
        // SAFETY: consultas ao motor com a sessão da thread.
        let (agora, parado, buscando) = unsafe { (motor.GetCurrentTime(), motor.IsPaused().as_bool(), motor.IsSeeking().as_bool()) };
        {
            let mut r = travar(&c.relogio);
            if eventos.fim.swap(false, SeqCst) {
                r.fixar(duracao.unwrap_or(agora));
                r.tocando = false;
                r.terminou = true;
            } else if r.tocando && !parado && !buscando {
                r.fixar(agora);
            }
        }
        if let Some((t, tocou @ false)) = espiar.as_mut().filter(|_| !buscando) {
            *tocou = true;
            // A busca de antes do fim do carregamento pode ter sido ignorada: repete antes de tocar.
            // SAFETY: chamadas COM com a sessão da thread.
            unsafe {
                let _ = motor.SetCurrentTime(*t);
                let _ = motor.Play();
            }
        }
        let Some(exibe) = video else { continue };
        let mut pts = 0i64;
        // SAFETY: o `windows` trata S_FALSE (sem quadro novo) como sucesso; o HRESULT cru separa os dois.
        let novo = unsafe { (Interface::vtable(motor).OnVideoStreamTick)(Interface::as_raw(motor), &mut pts) } == S_OK;
        let alvo = c.alvo.load(SeqCst);
        let tamanho = tamanho_saida(exibe, ((alvo >> 32) as u32, alvo as u32));
        let pts = if novo {
            let pts = pts as f64 / 1e7;
            // Quadro de antes da busca que chegou depois dela (o primeiro do arquivo, por exemplo): fica de fora até o
            // motor mostrar o do alvo, ou até o prazo, se a busca não cair exata.
            // Na espiada, só serve o quadro do alvo em diante.
            let fora = |pts: f64, alvo: f64| if espiar.is_some() { pts + 0.05 < alvo || pts > alvo + 0.25 } else { (pts - alvo).abs() > 0.25 };
            let prazo = if espiar.is_some() { PRAZO_DA_ESPIADA } else { PRAZO_DA_BUSCA };
            if let Some((alvo, desde)) = busca {
                if fora(pts, alvo) && desde.elapsed() < prazo {
                    continue;
                }
            }
            busca = None;
            pts
        } else {
            // Sem quadro novo, o atual é refeito quando muda o tamanho de saída ou quando a busca cai nele (o motor não
            // repete o quadro ao buscar a posição em que já está).
            let Some(ultimo) = ultimo.filter(|_| espiar.is_none()) else { continue };
            let mesma_posicao = busca.is_some_and(|(alvo, _)| (alvo - ultimo).abs() < 0.05);
            if !mesma_posicao && destino.as_ref().is_some_and(|d| d.tamanho == tamanho) {
                continue;
            }
            if mesma_posicao {
                busca = None;
            }
            ultimo
        };
        if destino.as_ref().is_none_or(|d| d.tamanho != tamanho) {
            destino = Destino::novo(gpu, tamanho).ok();
        }
        let Some(d) = destino.as_ref() else { continue };
        if let Ok(bgra) = d.copiar(gpu, motor) {
            if let Some((t, _)) = espiar.take() {
                // SAFETY: chamadas COM com a sessão da thread.
                unsafe {
                    let _ = motor.Pause();
                    let _ = motor.SetCurrentTime(t);
                }
                eventos.fim.store(false, SeqCst);
            }
            ultimo = Some(pts);
            *travar(&c.quadro) = Some(Quadro { pts, largura: tamanho.0, altura: tamanho.1, bgra, geracao });
        }
    }
}
