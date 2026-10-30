//! O explorador de arquivos do projeto: lista pastas, lê e grava conteúdo de arquivos, e renomeia, manda à Lixeira
//! e mostra no Explorer do Windows. Ações do projeto recebem caminho relativo; o editor também lê e grava caminhos
//! absolutos dos arquivos que o usuário abriu ou escolheu.
use crate::platform::{self, ErroShell, Snapshot};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Component, Path, PathBuf};

/// Arquivo com mais que isto não é lido: a tela mostra só o aviso.
pub(crate) const MAX_FILE: u64 = 1_048_576;
/// Quantos bytes do começo de um arquivo olhar para decidir se é binário (a mesma regra do Git).
pub(crate) const BINARY_SNIFF: usize = 8000;
/// Bytes do BOM UTF-8.
pub(crate) const BOM: [u8; 3] = [0xEF, 0xBB, 0xBF];

/// Diz se os bytes são binários: NUL nos primeiros `BINARY_SNIFF` bytes, como o Git decide.
pub(crate) fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_SNIFF)].contains(&0)
}

/// Impressão digital (FNV-1a de 64 bits) dos bytes. Serve de revisão: se o conteúdo mudar, a revisão muda.
pub(crate) fn fingerprint(bytes: &[u8]) -> String {
    let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3));
    format!("{hash:016x}")
}

/// Uma entrada da pasta listada: só o nome, o caminho é o da pasta pedida mais esse nome. `ignored`: o `.gitignore`
/// a ignora (o explorador a mostra apagada); sempre falso fora de um repositório.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub ignored: bool,
}

/// O conteúdo de um arquivo. `text` vai sem BOM e vazio quando o arquivo é binário ou grande demais. `bom` diz se o
/// arquivo começa com BOM UTF-8: a gravação verificada precisa saber disso para devolver o mesmo BOM.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Content {
    pub binary: bool,
    pub too_large: bool,
    pub read_only: bool,
    pub bom: bool,
    pub text: String,
}

/// Um nível da pasta `rel` do projeto (com barras "/", vazio é a raiz). Pastas primeiro, cada grupo em
/// ordem alfabética sem diferenciar maiúsculas. Na raiz, o `.git` fica de fora.
pub fn list(project: &Path, rel: &str) -> Result<Vec<Entry>, String> {
    let dir = inside(project, rel, true)?;
    let root = rel.is_empty();
    let read = std::fs::read_dir(&dir).map_err(|e| format!("Não foi possível ler a pasta: {e}"))?;
    let mut entries: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if root && name == ".git" {
                return None;
            }
            // `is_dir` segue link simbólico e junction; o que não dá para saber conta como arquivo.
            let dir = e.path().is_dir();
            Some(Entry { name, dir, ignored: false })
        })
        .collect();
    entries.sort_by_key(|e| (!e.dir, e.name.to_lowercase()));
    // Fora de um repositório (ou sem Git) o check-ignore falha, e nada fica apagado.
    let paths: Vec<String> = entries.iter().map(|e| if root { e.name.clone() } else { format!("{rel}/{}", e.name) }).collect();
    if paths.is_empty() {
        return Ok(entries);
    }
    if let Ok(ignored) = crate::proc::git_ignorados(project, &paths) {
        entries.iter_mut().zip(&paths).for_each(|(e, p)| e.ignored = ignored.contains(p));
    }
    Ok(entries)
}

/// O arquivo `rel` do projeto (com barras "/"). Arquivo grande ou binário volta sem `text`.
pub fn read(project: &Path, rel: &str) -> Result<Content, String> {
    let file = inside(project, rel, false)?;
    read_path(&file)
}

/// O arquivo absoluto que o usuário abriu ou escolheu para o editor. O caminho precisa ser absoluto e apontar para arquivo.
pub fn read_at(path: &Path) -> Result<Content, String> {
    if !path.is_absolute() {
        return Err("O caminho do arquivo precisa ser absoluto.".into());
    }
    read_path(path)
}

fn read_path(file: &Path) -> Result<Content, String> {
    let erro = |e: std::io::Error| format!("Não foi possível ler o arquivo: {e}");
    // O `take` limita a leitura mesmo que o arquivo cresça depois de aberto.
    let mut bytes = Vec::new();
    File::open(file).map_err(erro)?.take(MAX_FILE + 1).read_to_end(&mut bytes).map_err(erro)?;
    if bytes.len() as u64 > MAX_FILE {
        return Ok(Content { binary: false, too_large: true, read_only: false, bom: false, text: String::new() });
    }
    if is_binary(&bytes) {
        return Ok(Content { binary: true, too_large: false, read_only: false, bom: false, text: String::new() });
    }
    let text = bytes.strip_prefix(&BOM).unwrap_or(&bytes[..]);
    Ok(Content { binary: false, too_large: false, read_only: std::str::from_utf8(text).is_err(), bom: text.len() != bytes.len(), text: String::from_utf8_lossy(text).into_owned() })
}

/// Extensões (minúsculas) de imagem que a aba de mídia lê pela IPC. SVG e HTML ficam de fora de propósito, porque viram
/// documento ativo; eles seguem como texto. Os tipos MIME ficam em `src/media-view.tsx`, que é quem monta o blob.
/// Áudio e vídeo não passam por aqui: ver `AUDIO_EXTS` e `VIDEO_EXTS`.
const MEDIA_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];
/// Imagem acima disto não é lida: a aba mostra o erro. Os bytes inteiros passam pela memória do app.
pub(crate) const MAX_MEDIA: u64 = 32 * 1_048_576;

/// Os bytes do arquivo de imagem `rel` do projeto, para a aba de mídia. A extensão precisa estar em `MEDIA_EXTS`;
/// o caminho passa por `plain` (sem `.git`, sem link em nenhuma parte, sem sair do projeto); e o arquivo não pode passar
/// de `MAX_MEDIA`. Não há sniff de conteúdo: o navegador é quem decodifica (e recusa o que não for imagem).
pub fn read_media(project: &Path, rel: &str) -> Result<Vec<u8>, String> {
    let ext = Path::new(rel).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !MEDIA_EXTS.contains(&ext.as_str()) {
        return Err(format!("Tipo de arquivo não aberto como mídia: {rel}"));
    }
    let file = plain(project, rel, false)?;
    read_media_path(&file, rel)
}

/// Imagem aberta pelo caminho absoluto que o usuário escolheu.
pub fn read_media_at(path: &Path) -> Result<Vec<u8>, String> {
    if !path.is_absolute() {
        return Err("O caminho do arquivo precisa ser absoluto.".into());
    }
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !MEDIA_EXTS.contains(&ext.as_str()) {
        return Err(format!("Tipo de arquivo não aberto como mídia: {}", path.display()));
    }
    read_media_path(path, &path.display().to_string())
}

fn read_media_path(file: &Path, label: &str) -> Result<Vec<u8>, String> {
    let grande = || format!("Arquivo grande demais para abrir aqui (limite de 32 MiB): {label}");
    let erro = |e: std::io::Error| format!("Não foi possível ler o arquivo: {e}");
    // Um handle só: o tamanho checado é o do arquivo que se lê. O `take` corta o que crescer depois do `metadata`.
    let handle = File::open(file).map_err(erro)?;
    if handle.metadata().map_err(erro)?.len() > MAX_MEDIA {
        return Err(grande());
    }
    let mut bytes = Vec::new();
    handle.take(MAX_MEDIA + 1).read_to_end(&mut bytes).map_err(erro)?;
    if bytes.len() as u64 > MAX_MEDIA {
        return Err(grande());
    }
    Ok(bytes)
}

/// Extensões (minúsculas) de vídeo. Vídeo e áudio não passam pela IPC: o protocolo `orq-media` serve pedaços (HTTP Range)
/// lidos do disco sob demanda, porque costumam passar de `MAX_MEDIA`.
pub(crate) const VIDEO_EXTS: &[&str] = &["mp4", "m4v", "webm", "mov"];
/// Extensões (minúsculas) de áudio, servidas pelo mesmo protocolo que o vídeo.
pub(crate) const AUDIO_EXTS: &[&str] = &["mp3", "wav", "ogg", "oga", "flac", "m4a"];
/// Maior pedaço de vídeo ou áudio numa resposta: intervalo aberto ou maior é cortado aqui, e o arquivo nunca vai inteiro para a memória.
pub(crate) const MAX_STREAM_CHUNK: u64 = 4 * 1_048_576;

/// Recusa (403) a extensão que não é de vídeo nem de áudio.
fn stream_ext(path: &Path, label: &str) -> Result<(), (u16, String)> {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if VIDEO_EXTS.contains(&ext.as_str()) || AUDIO_EXTS.contains(&ext.as_str()) {
        Ok(())
    } else {
        Err((403, format!("Tipo de arquivo não aberto como vídeo ou áudio: {label}")))
    }
}

/// O arquivo de vídeo ou áudio `rel` do projeto, com as regras de `plain` (sem `.git`, sem link, sem sair do projeto). O erro leva
/// o status HTTP: 404 se o arquivo não existe, 403 para o resto.
pub fn stream_path(project: &Path, rel: &str) -> Result<PathBuf, (u16, String)> {
    stream_ext(Path::new(rel), rel)?;
    plain(project, rel, false).map_err(|e| {
        let lexical = no_git(rel).is_ok() && Path::new(rel).components().all(|c| matches!(c, Component::Normal(_)));
        let missing = lexical && no_links(project, rel).is_ok() && std::fs::symlink_metadata(project.join(rel)).is_err();
        (if missing { 404 } else { 403 }, e)
    })
}

/// O vídeo ou áudio aberto pelo caminho absoluto que o usuário escolheu, com as regras de `read_media_at`.
pub fn stream_path_at(path: &Path) -> Result<PathBuf, (u16, String)> {
    if !path.is_absolute() {
        return Err((403, "O caminho do arquivo precisa ser absoluto.".into()));
    }
    stream_ext(path, &path.display().to_string())?;
    Ok(path.to_path_buf())
}

/// O tipo MIME do vídeo ou áudio pela extensão. `.mov` vai como `video/mp4`: o Chromium não toca `video/quicktime`, e o
/// `.mov` H.264 costuma tocar assim.
fn stream_type(path: &Path) -> &'static str {
    match path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).as_deref() {
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wav") => "audio/wav",
        Some("ogg" | "oga") => "audio/ogg",
        Some("flac") => "audio/flac",
        Some("m4a") => "audio/mp4",
        _ => "video/mp4",
    }
}

/// O intervalo inclusivo pedido em `range` num arquivo de `total` bytes, cortado em `MAX_STREAM_CHUNK`. Aceita um intervalo
/// só (`a-b`, `a-`, `-n`); sem cabeçalho, ou com um que não se entende, vale `bytes=0-`. `None`: não cabe no arquivo (416).
fn stream_range(range: Option<&str>, total: u64) -> Option<(u64, u64)> {
    let last = total.checked_sub(1)?;
    let spec = range.and_then(|r| r.trim().strip_prefix("bytes=")).and_then(|spec| {
        let (a, b) = spec.split_once('-')?;
        let num = |s: &str| s.trim().parse::<u64>().ok();
        match (a.trim().is_empty(), b.trim().is_empty()) {
            (false, false) => Some((num(a)?, num(b)?)).filter(|(a, b)| a <= b).map(|(a, b)| (Some(a), Some(b))),
            (false, true) => Some((Some(num(a)?), None)),
            (true, false) => Some((None, Some(num(b)?))),
            (true, true) => None,
        }
    });
    let (start, end) = match spec.unwrap_or((Some(0), None)) {
        (Some(a), b) => (a, b.map_or(last, |b| b.min(last))),
        (None, Some(n)) if n > 0 => (total.saturating_sub(n), last),
        _ => return None,
    };
    (start <= last).then(|| (start, end.min(start + MAX_STREAM_CHUNK - 1)))
}

/// Resposta de erro em texto.
pub fn text_response(status: u16, message: String) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header("Content-Type", "text/plain; charset=utf-8")
        .body(message.into_bytes())
        .expect("resposta de texto válida")
}

/// O pedaço do vídeo ou áudio `file` (já validado) pedido no cabeçalho `range`: 206 com `Content-Range`, 416 se o intervalo não cabe,
/// 200 vazio se o arquivo é vazio, 404 se sumiu, 500 em outro erro de leitura. Lê num handle só, só os bytes do intervalo.
pub fn serve_stream(file: &Path, range: Option<&str>) -> tauri::http::Response<Vec<u8>> {
    let erro = |e: std::io::Error| match e.kind() {
        std::io::ErrorKind::NotFound => text_response(404, format!("Arquivo não encontrado: {}", file.display())),
        _ => text_response(500, format!("Não foi possível ler o arquivo: {e}")),
    };
    let read = || -> std::io::Result<tauri::http::Response<Vec<u8>>> {
        let mut handle = File::open(file)?;
        let total = handle.metadata()?.len();
        let response = tauri::http::Response::builder().header("Accept-Ranges", "bytes").header("Content-Type", stream_type(file));
        if total == 0 {
            return Ok(response.status(200).header("Content-Length", "0").body(Vec::new()).expect("resposta válida"));
        }
        let unsatisfiable = |response: tauri::http::response::Builder| {
            Ok(response.status(416).header("Content-Range", format!("bytes */{total}")).body(Vec::new()).expect("resposta válida"))
        };
        let Some((start, end)) = stream_range(range, total) else {
            return unsatisfiable(response);
        };
        handle.seek(SeekFrom::Start(start))?;
        let mut body = Vec::new();
        handle.take(end - start + 1).read_to_end(&mut body)?;
        // Se o arquivo encolheu depois do `metadata`, o intervalo anunciado é o que de fato foi lido.
        if body.is_empty() {
            return unsatisfiable(response);
        }
        let end = start + body.len() as u64 - 1;
        Ok(response
            .status(206)
            .header("Content-Range", format!("bytes {start}-{end}/{total}"))
            .header("Content-Length", body.len().to_string())
            .body(body)
            .expect("resposta válida"))
    };
    read().unwrap_or_else(erro)
}

/// Grava `text` em `rel` só se o arquivo ainda é `base` (o texto que a tela leu) e ainda tem o mesmo BOM (`base_bom`).
/// Se mudou, sumiu, virou link, mudou de lugar, não é UTF-8 ou é binário, recusa e não grava nada. A decisão é deste
/// backend, não da tela.
///
/// Durante a comparação e a escrita o arquivo fica aberto sem `FILE_SHARE_WRITE` (ver `hold`): outro programa não
/// consegue gravar nele no meio. A janela residual é a de quem já tinha o arquivo aberto para gravar antes desta chamada.
/// O fim de linha de `text` vai como veio, sem conversão (ver `write_held`). Não há gravação atômica: se a escrita falha no
/// meio, o erro avisa que o arquivo pode ter ficado parcial.
pub fn write_checked(project: &Path, rel: &str, text: &str, base: &str, base_bom: bool) -> Result<(), String> {
    write_held(hold(project, rel)?, text, base, base_bom)
}

/// Grava um caminho absoluto escolhido pelo usuário, criando o arquivo se ainda não existe e recusando mudanças externas.
pub fn write_checked_at(path: &Path, text: &str, base: &str, base_bom: bool) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("O caminho do arquivo precisa ser absoluto.".into());
    }
    let file = if std::fs::symlink_metadata(path).is_ok() {
        hold_absolute(path)?
    } else {
        if !base.is_empty() || base_bom {
            return Err("O arquivo não existe mais. Nada foi gravado.".into());
        }
        // Antes de criar: recusa depois do `create_new` deixaria um arquivo vazio.
        check_size(text)?;
        let file = std::fs::OpenOptions::new().read(true).write(true).create_new(true).open(path).map_err(open_err)?;
        return write_held(file, text, base, base_bom);
    };
    write_held(file, text, base, base_bom)
}

fn check_size(text: &str) -> Result<(), String> {
    if text.len() as u64 > MAX_FILE {
        return Err("O arquivo ficaria grande demais para abrir aqui.".into());
    }
    Ok(())
}

/// `text` vai com o fim de linha que o editor escolheu (o do disco, ou o que o formatador definiu): nada é convertido.
fn write_held(mut file: File, text: &str, base: &str, base_bom: bool) -> Result<(), String> {
    check_size(text)?;
    let mut current = Vec::new();
    // O `take` evita carregar tudo se um programa externo inchou o arquivo.
    file.seek(SeekFrom::Start(0)).and_then(|_| (&mut file).take(MAX_FILE + 1).read_to_end(&mut current)).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?;
    if current.len() as u64 > MAX_FILE {
        return Err("O arquivo ficou grande demais para gravar aqui. Nada foi gravado.".into());
    }
    if is_binary(&current) {
        return Err("Não é possível gravar um arquivo binário. Nada foi gravado.".into());
    }
    let content = current.strip_prefix(&BOM).unwrap_or(&current[..]);
    let bom = content.len() < current.len();
    if std::str::from_utf8(content).is_err() {
        return Err("Não é possível gravar um arquivo que não é UTF-8 válido. Nada foi gravado.".into());
    }
    if bom != base_bom || content != base.as_bytes() {
        return Err("O arquivo foi alterado por outro programa desde que foi aberto. Nada foi gravado.".into());
    }
    let mut bytes = Vec::with_capacity(text.len() + BOM.len());
    if bom {
        bytes.extend_from_slice(&BOM);
    }
    bytes.extend_from_slice(text.as_bytes());
    put(&mut file, &bytes).map_err(|e| format!("Não foi possível gravar o arquivo: {e}. Ele pode ter ficado parcialmente gravado; confira antes de tentar de novo."))
}

/// Escreve `bytes` no começo do arquivo já aberto e corta o que sobrar, para o tamanho novo.
fn put(file: &mut File, bytes: &[u8]) -> std::io::Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.write_all(bytes)?;
    file.set_len(bytes.len() as u64)?;
    file.sync_all()
}

/// Abre `rel` para ler e gravar, sem deixar outro programa gravar, renomear ou apagar até o `File` ser solto: o
/// `FILE_SHARE_READ` sozinho recusa quem pede escrita ou exclusão. Antes de abrir, `plain` valida o caminho; depois,
/// confere de novo que o último nível não virou link e que o caminho ainda leva ao mesmo arquivo. Como o arquivo aberto não
/// pode mais ser renomeado por outro programa, isso fecha a troca por link entre a validação e a abertura.
fn hold(project: &Path, rel: &str) -> Result<File, String> {
    let lexical = plain(project, rel, false)?;
    let real = inside(project, rel, false)?;
    let file = platform::abrir_para_gravar(&lexical).map_err(open_err)?;
    let link = std::fs::symlink_metadata(&lexical).is_ok_and(|m| m.file_type().is_symlink());
    if link || lexical.canonicalize().ok().as_deref() != Some(real.as_path()) {
        return Err(format!("{rel} mudou de lugar desde a validação. Nada foi gravado."));
    }
    Ok(file)
}

/// Abre um arquivo escolhido fora do projeto sem permitir gravação, renomeação ou troca do destino durante a checagem.
fn hold_absolute(path: &Path) -> Result<File, String> {
    if !path.is_absolute() {
        return Err("O caminho do arquivo precisa ser absoluto.".into());
    }
    let real = path.canonicalize().map_err(open_err)?;
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return Err(format!("{} não é um arquivo.", path.display()));
    }
    let file = platform::abrir_para_gravar(path).map_err(open_err)?;
    if path.canonicalize().ok().as_deref() != Some(real.as_path()) {
        return Err(format!("{} mudou de lugar desde a validação. Nada foi gravado.", path.display()));
    }
    Ok(file)
}

/// Mensagem para quem não conseguiu abrir o arquivo para gravar. Outro programa com o arquivo aberto para gravar cai aqui.
fn open_err(e: std::io::Error) -> String {
    if platform::em_uso(&e) {
        "Outro programa está gravando este arquivo agora. Nada foi gravado; tente de novo.".into()
    } else if e.kind() == std::io::ErrorKind::NotFound {
        "O arquivo não existe mais. Nada foi gravado.".into()
    } else {
        format!("Não foi possível abrir o arquivo para gravar: {e}")
    }
}

/// Nomes que o Windows reserva, com ou sem extensão (`CON.txt` também é reservado).
const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
    "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Diz se `name` serve de nome de arquivo no Windows: um nome só, sem caractere proibido, sem nome reservado e sem
/// espaço ou ponto nas pontas. A mesma regra está em `fileNameError` (src/file-tab-actions.ts), que mostra o motivo antes do envio.
pub fn check_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Digite um nome para o arquivo.".into());
    }
    if name == "." || name == ".." || name.len() > 255 {
        return Err("Esse nome não é permitido.".into());
    }
    if name.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c)) {
        return Err("O nome tem caractere que o Windows não aceita: < > : \" / \\ | ? *".into());
    }
    if name.starts_with(' ') || name.ends_with(' ') || name.ends_with('.') {
        return Err("O nome não pode começar nem terminar com espaço, nem terminar com ponto.".into());
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Err(format!("“{name}” é um nome reservado pelo Windows."));
    }
    Ok(())
}

/// Recusas que valem para toda ação sobre arquivo: a pasta `.git` nunca entra (em qualquer caixa, e com ponto ou espaço no
/// fim, que o Windows descarta), e o caminho sai do projeto só por erro. Caminho com barra invertida é recusado: o contrato
/// é `/`, e o `Path` do Windows também aceita `\`, o que contornaria as checagens. `:` também: o NTFS lê
/// `.git::$INDEX_ALLOCATION` como a própria pasta `.git`.
pub(crate) fn no_git(rel: &str) -> Result<(), String> {
    if rel.contains(['\\', ':']) {
        return Err(format!("Caminho inválido: {rel}. Use / como separador."));
    }
    if rel.split('/').any(|part| part.trim_end_matches(['.', ' ']).eq_ignore_ascii_case(".git")) {
        return Err("A pasta .git não pode ser alterada aqui.".into());
    }
    Ok(())
}

/// Recusa link simbólico ou junction em qualquer parte de `rel`, inclusive pasta do meio. Seguir um desses faria a ação
/// cair em outro lugar, mesmo que o destino esteja dentro do projeto (um alias para `.git`, por exemplo). A raiz do
/// projeto fica de fora: ela pode ser um link legítimo.
pub(crate) fn no_links(project: &Path, rel: &str) -> Result<(), String> {
    let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty()).collect();
    let mut cur = project.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        cur.push(part);
        if std::fs::symlink_metadata(&cur).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(if i + 1 == parts.len() {
                format!("{rel} é um link. Links não são renomeados nem excluídos por aqui.")
            } else {
                format!("{rel} passa por um link ou junction. Caminhos com link não são usados por aqui.")
            });
        }
    }
    Ok(())
}

/// O caminho de `rel` para ação ou outro programa (Explorer, shell, Lixeira): o da pasta do projeto mais as partes, sem a
/// forma `\\?\` que o `canonicalize` devolve. `dir` diz se é pasta (o cwd do shell) ou arquivo. Recusa `.git`, barra
/// invertida, link em qualquer parte do caminho (a ação não pode seguir o link e mexer no destino dele) e o que sai do projeto.
pub fn plain(project: &Path, rel: &str, dir: bool) -> Result<PathBuf, String> {
    no_git(rel)?;
    no_links(project, rel)?;
    let real = inside(project, rel, dir)?;
    // O caminho real pega o `.git` que o nome escrito esconde, como o nome curto `GIT~1`.
    let base = project.canonicalize().map_err(|e| format!("Não foi possível abrir o projeto: {e}"))?;
    if real.strip_prefix(&base).is_ok_and(|r| r.components().any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))) {
        return Err("A pasta .git não pode ser alterada aqui.".into());
    }
    Ok(rel.split('/').filter(|p| !p.is_empty()).fold(project.to_path_buf(), |path, part| path.join(part)))
}

/// Renomeia o arquivo `rel` para `name`, na mesma pasta. Não sobrescreve: se o nome já existe (inclusive como link quebrado),
/// recusa. Só a troca de maiúsculas e minúsculas do mesmo arquivo passa por cima do próprio nome. Devolve o caminho novo.
pub fn rename(project: &Path, rel: &str, name: &str) -> Result<String, String> {
    check_name(name)?;
    let it = item(project, rel)?;
    if it.name == name {
        return Ok(it.rel);
    }
    let next = match it.rel.rsplit_once('/') {
        Some((parent, _)) => format!("{parent}/{name}"),
        None => name.to_string(),
    };
    let dest = it.path.with_file_name(name);
    let occupied = std::fs::symlink_metadata(&dest).is_ok();
    let link = std::fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink());
    let same_file = occupied && !link && dest.canonicalize().ok() == it.path.canonicalize().ok();
    if occupied && !same_file {
        return Err(format!("Já existe um item chamado “{name}” nesta pasta."));
    }
    if same_file {
        // O Windows vê `a.txt` e `A.txt` como um só: vai primeiro para um nome temporário, sem sobrescrever.
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
        let tmp = it.path.with_file_name(format!(".orq-renomear-{}-{stamp}.tmp", std::process::id()));
        move_no_replace(&it.path, &tmp)?;
        if let Err(e) = move_no_replace(&tmp, &dest) {
            let _ = move_no_replace(&tmp, &it.path);
            return Err(e);
        }
    } else {
        move_no_replace(&it.path, &dest)?;
    }
    Ok(next)
}

/// Muda o nome (ou move) sem sobrescrever: se o destino existe, recusa. Ver `platform::mover_sem_substituir`.
fn move_no_replace(from: &Path, to: &Path) -> Result<(), String> {
    platform::mover_sem_substituir(from, to).map_err(|err| {
        if platform::ja_existe(&err) {
            "Já existe um item com este nome na pasta de destino. Nada foi movido.".into()
        } else {
            format!("Não foi possível concluir a operação: {err}")
        }
    })
}

/// Um item do projeto pronto para uma ação: o caminho para o sistema, se é pasta, o `rel` sem barras sobrando e o nome.
struct Item {
    path: PathBuf,
    dir: bool,
    rel: String,
    name: String,
}

/// Um item que foi de `from` para `to` (caminhos relativos ao projeto). Em pasta, o que está dentro acompanha: quem atualiza
/// as abas troca o prefixo `from/` por `to/`. Ao colar, `from` é o caminho absoluto que veio do clipboard, como o Windows o
/// deu, e `to` é o que foi criado (com o nome que ganhou, se houve conflito).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
}

/// Resultado de uma ação em lote: o que foi feito (`done`) e o erro do que parou (`error`, `null` se tudo deu certo).
/// Vem sempre como resposta, nunca como erro: a tela precisa do `done` também quando a operação falhou no meio.
/// Quando a checagem prévia recusa o lote, `done` vem vazio e nada foi mexido.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Batch<T> {
    pub done: Vec<T>,
    pub error: Option<String>,
}

/// Lote recusado antes de mexer em nada.
fn refused<T>(error: String) -> Batch<T> {
    Batch { done: Vec::new(), error: Some(error) }
}

/// O item `rel` do projeto, pronto para uma ação. Recusa a raiz, `.git`, link em qualquer parte do caminho, o que sai do
/// projeto, e pasta com `.git` ou link dentro (a ação pegaria esse conteúdo junto). Arquivo ou pasta, sem seguir link.
fn item(project: &Path, rel: &str) -> Result<Item, String> {
    let rel = rel.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("/");
    if rel.is_empty() {
        return Err("A raiz do projeto não pode ser alterada aqui.".into());
    }
    let dir = std::fs::metadata(project.join(&rel)).is_ok_and(|m| m.is_dir());
    let path = plain(project, &rel, dir)?;
    let name = rel.rsplit('/').next().unwrap_or(&rel).to_string();
    if dir {
        check_tree(&path, &name)?;
    }
    Ok(Item { path, dir, rel, name })
}

/// Os itens de `rels`, checados antes de qualquer mudança: lista não vazia, sem repetido, sem um item dentro de outro e cada
/// um passando por `item`. Qualquer recusa aborta o lote inteiro.
fn selection(project: &Path, rels: &[String]) -> Result<Vec<Item>, String> {
    if rels.is_empty() {
        return Err("Nenhum item selecionado.".into());
    }
    let items = rels.iter().map(|rel| item(project, rel)).collect::<Result<Vec<Item>, String>>()?;
    let keys: Vec<String> = items.iter().map(|it| it.rel.to_lowercase()).collect();
    for (i, it) in items.iter().enumerate() {
        for (j, key) in keys.iter().enumerate() {
            if i == j {
                continue;
            }
            if i < j && *key == keys[i] {
                return Err(format!("“{}” está repetido na seleção.", it.name));
            }
            if key.starts_with(&format!("{}/", keys[i])) {
                return Err(format!("“{}” está dentro de “{}”, que também está na seleção. Escolha só um dos dois.", items[j].name, it.name));
            }
        }
    }
    Ok(items)
}

/// Roda `step` em cada item do plano, na ordem, e para no primeiro erro. Plano recusado não mexe em nada. Se alguns itens
/// já foram feitos, o erro diz quantos e que não são desfeitos aqui.
fn each<I, T>(plan: Result<Vec<I>, String>, mut step: impl FnMut(I) -> Result<T, String>) -> Batch<T> {
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => return refused(error),
    };
    let total = plan.len();
    let mut done = Vec::with_capacity(total);
    for item in plan {
        match step(item) {
            Ok(t) => done.push(t),
            Err(msg) if done.is_empty() => return Batch { done, error: Some(msg) },
            Err(msg) => return Batch { error: Some(format!("Feitos {} de {total} antes da falha; os primeiros não são desfeitos aqui. {msg}", done.len())), done },
        }
    }
    Batch { done, error: None }
}

/// Confere o plano de mover `rels` para a pasta `dest`, antes de mexer em qualquer coisa. Recusa pasta dentro dela mesma ou
/// de um descendente, item já na pasta de destino, nome que já existe no destino (inclusive dentro do próprio lote).
/// Devolve, por item, o caminho de destino e o `rel` novo.
fn move_plan(project: &Path, rels: &[String], dest: &str) -> Result<Vec<(Item, PathBuf, String)>, String> {
    let items = selection(project, rels)?;
    let dest_path = plain(project, dest, true)?;
    let dest_canon = dest_path.canonicalize().map_err(|e| format!("Não foi possível abrir a pasta de destino: {e}"))?;
    let dest_rel = dest.split('/').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("/");
    let mut taken = HashSet::new();
    let mut plan = Vec::with_capacity(items.len());
    for it in items {
        let src_canon = it.path.canonicalize().map_err(|e| format!("Não foi possível abrir “{}”: {e}", it.name))?;
        if it.dir && dest_canon.starts_with(&src_canon) {
            return Err(format!("Não é possível mover “{}” para dentro dela mesma ou de uma pasta dentro dela. Nada foi movido.", it.name));
        }
        if it.path.parent().and_then(|p| p.canonicalize().ok()).as_deref() == Some(dest_canon.as_path()) {
            return Err(format!("“{}” já está nesta pasta. Nada foi movido.", it.name));
        }
        let clash = !taken.insert(it.name.to_lowercase());
        let to = dest_path.join(&it.name);
        if clash || std::fs::symlink_metadata(&to).is_ok() {
            return Err(format!("Já existe um item chamado “{}” na pasta de destino. Nada foi movido.", it.name));
        }
        let to_rel = if dest_rel.is_empty() { it.name.clone() } else { format!("{dest_rel}/{}", it.name) };
        plan.push((it, to, to_rel));
    }
    Ok(plan)
}

/// Move os itens `rels` para a pasta `dest` do projeto (vazio é a raiz), como o arrastar e soltar. Tudo é checado antes
/// (ver `move_plan`); colisão não ganha nome novo, recusa. Se um movimento falha depois de outros, os que já saíram vêm em
/// `done` e não voltam sozinhos.
pub fn move_items(project: &Path, rels: &[String], dest: &str) -> Batch<Moved> {
    each(move_plan(project, rels, dest), |(it, to, to_rel)| move_no_replace(&it.path, &to).map(|()| Moved { from: it.rel, to: to_rel }))
}

/// Põe os itens `rels` no clipboard como copiados ou recortados, com a mesma checagem de `selection`. Nada é movido ou
/// apagado aqui: o movimento do recorte acontece quando alguém cola.
pub fn copy_items(project: &Path, rels: &[String], cut: bool) -> Result<(), String> {
    let paths: Vec<PathBuf> = selection(project, rels)?.into_iter().map(|it| it.path).collect();
    platform::set_files(&paths, cut)
}

/// Manda os itens `rels` para a Lixeira do Windows (pasta inteira ou arquivo), com a checagem de `selection` antes. Se um
/// falha depois de outros, os que já foram ficam na Lixeira e vêm em `done`.
pub fn recycle_many(project: &Path, rels: &[String]) -> Batch<String> {
    each(selection(project, rels), |it| recycle_path(&it.path).map(|()| it.rel))
}

/// Manda o arquivo ou a pasta `rel` para a Lixeira do Windows, pelo `IFileOperation` com `FOFX_RECYCLEONDELETE`: a operação pede
/// reciclagem e não tem o caminho de apagar de vez. Não dá para mudar isso com `SHFileOperationW`, que aceita a exclusão
/// definitiva por diálogo. Só conta como feito se a operação não foi abortada e o arquivo saiu do lugar; senão é erro.
pub fn recycle(project: &Path, rel: &str) -> Result<(), String> {
    recycle_path(&item(project, rel)?.path)
}

/// Manda `file` (arquivo ou pasta já validada) para a Lixeira e confere que saiu do lugar.
fn recycle_path(file: &Path) -> Result<(), String> {
    platform::mandar_para_lixeira(file).map_err(erro_lixeira)?;
    if std::fs::symlink_metadata(file).is_ok() {
        return Err("A Lixeira não recebeu o arquivo. Nada foi excluído.".into());
    }
    Ok(())
}

/// A mensagem de uma falha da Lixeira.
fn erro_lixeira(erro: ErroShell) -> String {
    match erro {
        ErroShell::SemCom | ErroShell::SemOperacao => "Não foi possível abrir a Lixeira do Windows. Nada foi excluído.".into(),
        ErroShell::ItemAusente => "Não foi possível localizar o arquivo para a Lixeira. Nada foi excluído.".into(),
        ErroShell::Recusada => "A Lixeira não recebeu o arquivo. Nada foi excluído.".into(),
        ErroShell::NaoSuportado => "Ainda não suportado neste sistema. Nada foi excluído.".into(),
    }
}

/// Abre o Explorer do Windows com o arquivo selecionado.
pub fn reveal(project: &Path, rel: &str) -> Result<(), String> {
    let path = plain(project, rel, false)?;
    platform::revelar_no_explorador(&path).map_err(|e| format!("Não foi possível abrir o Explorer: {e}"))
}

/// Cria um arquivo vazio `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo criado.
/// Não sobrescreve: `create_new` recusa nome já existente, inclusive link quebrado.
pub fn create_file(project: &Path, dir: &str, name: &str) -> Result<String, String> {
    create(project, dir, name, false)
}

/// Cria a pasta `name` dentro da pasta `dir` do projeto (vazio é a raiz). Devolve o caminho relativo criado.
/// Não sobrescreve: `create_dir` falha se o nome já existe, seja arquivo, pasta ou link.
pub fn create_dir(project: &Path, dir: &str, name: &str) -> Result<String, String> {
    create(project, dir, name, true)
}

fn create(project: &Path, dir: &str, name: &str, folder: bool) -> Result<String, String> {
    let parent = plain(project, dir, true)?;
    check_new_name(name)?;
    let path = parent.join(name);
    // `symlink_metadata` também vê link quebrado, que o `create_new` recusaria de qualquer jeito; a checagem dá a mensagem certa.
    if std::fs::symlink_metadata(&path).is_ok() {
        return Err(exists(name));
    }
    let made = if folder {
        std::fs::create_dir(&path)
    } else {
        std::fs::OpenOptions::new().write(true).create_new(true).open(&path).map(drop)
    };
    match made {
        Ok(()) => Ok(if dir.trim_end_matches('/').is_empty() { name.to_string() } else { format!("{}/{name}", dir.trim_end_matches('/')) }),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(exists(name)),
        Err(e) => Err(format!("Não foi possível criar {}: {e}", if folder { "a pasta" } else { "o arquivo" })),
    }
}

fn exists(name: &str) -> String {
    format!("Já existe um item chamado “{name}” nesta pasta. Nada foi criado.")
}

/// Nome de item novo ou colado: passa em `check_name` e não é `.git`, em qualquer caixa.
fn check_new_name(name: &str) -> Result<(), String> {
    check_name(name)?;
    if name.eq_ignore_ascii_case(".git") {
        return Err("A pasta .git não pode ser alterada aqui.".into());
    }
    Ok(())
}

/// Abre no Explorer a pasta `rel` do projeto (vazio é a raiz). Recusa `.git`, link e o que sai do projeto.
pub fn reveal_dir(project: &Path, rel: &str) -> Result<(), String> {
    let path = plain(project, rel, true)?;
    platform::abrir_pasta_no_explorador(&path).map_err(|e| format!("Não foi possível abrir o Explorer: {e}"))
}

/// Cola em `dest` (pasta do projeto, vazio é a raiz) os itens de `snap`: copia, ou move se `snap.cut`. `done` traz cada item
/// colado; se parou, `error` traz o motivo (os que já foram antes continuam em `done`, sem desfazer). Tudo é
/// checado antes de começar: se um item é recusado, nada é colado. Nome que já existe vira "nome (2).ext", "nome (3).ext"...;
/// nada existente é sobrescrito. A cópia e o movimento são do IFileOperation do Windows, que copia pasta inteira e move
/// entre volumes. Recusa antes de mexer se um item está dentro de outro da mesma colagem, ou se um aparece duas vezes:
/// a cópia ou o recorte falharia no meio sem motivo.
pub fn paste_detailed(project: &Path, dest: &str, snap: &Snapshot) -> Batch<Moved> {
    let (parent, plan) = match paste_plan(project, dest, snap) {
        Ok(plan) => plan,
        Err(error) => return refused(error),
    };
    let base = dest.trim_end_matches('/');
    let total = plan.len();
    let mut done = Vec::with_capacity(total);
    for (i, (src, name)) in plan.iter().enumerate() {
        if let Err(msg) = transfer(src, &parent, name, snap.cut) {
            let error = if i == 0 { msg } else { format!("Colados {i} de {total}; os primeiros já estão na pasta de destino. {msg}") };
            return Batch { done, error: Some(error) };
        }
        let to = if base.is_empty() { name.clone() } else { format!("{base}/{name}") };
        done.push(Moved { from: src.display().to_string(), to });
    }
    Batch { done, error: None }
}

/// A checagem de `paste_detailed` antes de qualquer cópia: a pasta de destino, e por item a origem e o nome que ganha lá.
fn paste_plan(project: &Path, dest: &str, snap: &Snapshot) -> Result<(PathBuf, Vec<(PathBuf, String)>), String> {
    if snap.paths.is_empty() {
        return Err("Não há arquivo ou pasta copiado para colar.".into());
    }
    check_overlap(&snap.paths)?;
    let parent = plain(project, dest, true)?;
    let parent_canon = parent.canonicalize().map_err(|e| format!("Não foi possível abrir a pasta de destino: {e}"))?;
    let mut taken = HashSet::new();
    let mut plan = Vec::with_capacity(snap.paths.len());
    for src in &snap.paths {
        plan.push(check_source(src, &parent, &parent_canon, snap.cut, &mut taken)?);
    }
    Ok((parent, plan))
}

/// Recusa a colagem em que um item aparece de novo ou está dentro de outro item. Compara o caminho do clipboard sem caixa.
fn check_overlap(paths: &[PathBuf]) -> Result<(), String> {
    let keys: Vec<String> = paths.iter().map(|p| p.to_string_lossy().to_lowercase().replace('/', "\\")).collect();
    for (i, key) in keys.iter().enumerate() {
        let name = paths[i].file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        for (j, other) in keys.iter().enumerate() {
            if i == j {
                continue;
            }
            if i < j && other == key {
                return Err(format!("“{name}” está repetido na colagem. Nada foi colado."));
            }
            if key.starts_with(&format!("{other}\\")) {
                let outer = paths[j].file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                return Err(format!("“{name}” está dentro de “{outer}”, que também está na colagem. Escolha só um dos dois. Nada foi colado."));
            }
        }
    }
    Ok(())
}

/// Checa um item copiado antes de colar: ainda existe, não é link, não tem `.git` no caminho nem dentro, não cai dentro de si
/// mesmo e, se foi recortado, não está já na pasta de destino. Devolve o caminho de origem e o nome que ganha no destino.
fn check_source(src: &Path, dest: &Path, dest_canon: &Path, cut: bool, taken: &mut HashSet<String>) -> Result<(PathBuf, String), String> {
    let name = src.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    let meta = std::fs::symlink_metadata(src).map_err(|_| format!("“{name}” não existe mais. Nada foi colado."))?;
    if meta.file_type().is_symlink() {
        return Err(format!("“{name}” é um link ou junction. Links não são colados por aqui."));
    }
    if src.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git")) {
        return Err(format!("“{name}” está dentro de uma pasta .git. Nada foi colado."));
    }
    check_new_name(&name)?;
    let dir = meta.is_dir();
    if dir {
        check_tree(src, &name)?;
    }
    let src_canon = src.canonicalize().map_err(|e| format!("Não foi possível abrir “{name}”: {e}"))?;
    if dir && dest_canon.starts_with(&src_canon) {
        return Err(format!("Não é possível colar “{name}” dentro dela mesma."));
    }
    if cut && src.parent().and_then(|p| p.canonicalize().ok()).as_deref() == Some(dest_canon) {
        return Err(format!("“{name}” já está nesta pasta. Recortar e colar na mesma pasta não faz nada."));
    }
    Ok((src.to_path_buf(), pick_name(dest, &name, dir, taken)))
}

/// Recusa a pasta copiada se tem `.git` ou link/junction em qualquer nível: a cópia seguiria o link para fora do que foi escolhido.
fn check_tree(dir: &Path, name: &str) -> Result<(), String> {
    let read = std::fs::read_dir(dir).map_err(|e| format!("Não foi possível ler “{name}”: {e}"))?;
    for entry in read {
        let entry = entry.map_err(|e| format!("Não foi possível ler “{name}”: {e}"))?;
        let inner = entry.file_name().to_string_lossy().into_owned();
        if inner.eq_ignore_ascii_case(".git") {
            return Err(format!("“{name}” tem uma pasta .git dentro. Ela não é colada por aqui."));
        }
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| format!("Não foi possível ler “{name}”: {e}"))?;
        if meta.file_type().is_symlink() {
            return Err(format!("“{name}” tem um link ou junction dentro ({inner}). Nada foi colado."));
        }
        if meta.is_dir() {
            check_tree(&path, name)?;
        }
    }
    Ok(())
}

/// Nome livre na pasta `dest`: o próprio, ou "nome (2).ext", "nome (3).ext"... Conta também os nomes já escolhidos nesta
/// colagem (`taken`, em minúsculas, pois o Windows não diferencia caixa).
fn pick_name(dest: &Path, name: &str, dir: bool, taken: &mut HashSet<String>) -> String {
    let (stem, ext) = if dir {
        (name.to_string(), None)
    } else {
        let path = Path::new(name);
        (path.file_stem().map_or_else(|| name.to_string(), |s| s.to_string_lossy().into_owned()), path.extension().map(|e| e.to_string_lossy().into_owned()))
    };
    let mut n = 1;
    loop {
        let candidate = match (n, &ext) {
            (1, _) => name.to_string(),
            (_, Some(ext)) => format!("{stem} ({n}).{ext}"),
            (_, None) => format!("{stem} ({n})"),
        };
        let key = candidate.to_lowercase();
        if !taken.contains(&key) && std::fs::symlink_metadata(dest.join(&candidate)).is_err() {
            taken.insert(key);
            return candidate;
        }
        n += 1;
    }
}

/// Copia ou move `src` para `parent/name` (que ainda não existe). Confere no disco: vale só se o destino existe e, no
/// recorte, a origem saiu do lugar. Se falha, diz se ficou algo no destino.
fn transfer(src: &Path, parent: &Path, name: &str, cut: bool) -> Result<(), String> {
    let result = platform::copiar_ou_mover(src, parent, name, cut).map_err(erro_colagem);
    let target = parent.join(name);
    let made = std::fs::symlink_metadata(&target).is_ok();
    let left = std::fs::symlink_metadata(src).is_ok();
    let partial = if made {
        format!("Pode ter ficado parte de “{name}” na pasta de destino; confira antes de tentar de novo.")
    } else {
        "Nada foi colado para este item.".to_string()
    };
    match result {
        Ok(()) if made && !(cut && left) => Ok(()),
        Ok(()) => Err(format!("O Windows não confirmou a colagem de “{name}”. {partial}")),
        Err(e) => Err(format!("{e} {partial}")),
    }
}

/// A mensagem de uma falha da colagem pelo Shell.
fn erro_colagem(erro: ErroShell) -> String {
    match erro {
        ErroShell::SemCom => "Não foi possível iniciar o COM do Windows. Nada foi colado.".into(),
        ErroShell::SemOperacao => "Não foi possível iniciar a cópia do Windows. Nada foi colado.".into(),
        ErroShell::ItemAusente => "Não foi possível localizar o item para colar. Nada foi colado.".into(),
        ErroShell::Recusada => "O Windows não concluiu a operação.".into(),
        ErroShell::NaoSuportado => "Ainda não suportado neste sistema. Nada foi colado.".into(),
    }
}

/// O caminho real de `rel` dentro do projeto. Recusa o que sai da pasta, o que não existe e o que não é do tipo pedido
/// (pasta se `dir`, senão arquivo).
fn inside(project: &Path, rel: &str, dir: bool) -> Result<PathBuf, String> {
    // Só nomes normais: `..`, barra inicial, unidade (`D:`) e `.` ficam de fora.
    if !Path::new(rel).components().all(|c| matches!(c, Component::Normal(_))) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    let base = project.canonicalize().map_err(|e| format!("Não foi possível abrir o projeto: {e}"))?;
    let real = base.join(rel).canonicalize().map_err(|_| {
        if dir { format!("Pasta não encontrada: {rel}") } else { format!("Arquivo não encontrado: {rel}") }
    })?;
    // Um link simbólico que aponta para fora do projeto também sai da pasta.
    if !real.starts_with(&base) {
        return Err(format!("Caminho fora do projeto: {rel}"));
    }
    if real.is_dir() != dir {
        return Err(if dir { format!("Não é uma pasta: {rel}") } else { format!("Não é um arquivo: {rel}") });
    }
    Ok(real)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{junction, Tmp};

    fn e(name: &str, dir: bool) -> Entry {
        Entry { name: name.into(), dir, ignored: false }
    }

    /// Um projeto de teste: pastas e arquivos com nomes que misturam caixa, mais um `.git` na raiz e outro na subpasta.
    fn fixture(tag: &str) -> Tmp {
        let tmp = Tmp::new("orq-files", tag);
        let root = &tmp.0;
        for dir in ["Zeta", "alfa", "Beta", ".git", "sub/.git", "sub/dentro"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["b.txt", "A.txt", ".env", "sub/x.md"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        tmp
    }

    #[test]
    fn pastas_primeiro_e_alfabetico_sem_diferenciar_caixa() {
        let root = &fixture("ordem").0;
        let got = list(&root, "").unwrap();
        assert_eq!(got, vec![e("alfa", true), e("Beta", true), e("sub", true), e("Zeta", true), e(".env", false), e("A.txt", false), e("b.txt", false)]);
    }

    #[test]
    fn git_fica_fora_da_raiz_e_aparece_na_subpasta() {
        let root = &fixture("git").0;
        assert!(!list(&root, "").unwrap().iter().any(|x| x.name == ".git"));
        assert_eq!(list(&root, "sub").unwrap(), vec![e(".git", true), e("dentro", true), e("x.md", false)]);
    }

    #[test]
    fn marca_o_que_o_gitignore_ignora_na_raiz_e_na_subpasta() {
        let tmp = crate::testutil::Tmp::new("orq-files", "ignorados");
        let root = &tmp.0;
        crate::testutil::git(root, &["init", "-q"]);
        crate::testutil::write(root, ".gitignore", "node_modules/
*.log
src/gerado/
");
        for dir in ["node_modules", "src/gerado", "src/codigo"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["a.log", "b.txt", "src/c.log"] {
            crate::testutil::write(root, file, "");
        }
        let flags = |rel: &str| list(root, rel).unwrap().into_iter().map(|x| (x.name, x.ignored)).collect::<Vec<_>>();
        assert_eq!(flags(""), [("node_modules", true), ("src", false), (".gitignore", false), ("a.log", true), ("b.txt", false)].map(|(n, i)| (n.to_string(), i)));
        assert_eq!(flags("src"), [("codigo", false), ("gerado", true), ("c.log", true)].map(|(n, i)| (n.to_string(), i)));
    }

    #[test]
    fn lista_so_um_nivel_da_subpasta() {
        let root = &fixture("sub").0;
        assert_eq!(list(&root, "sub/dentro").unwrap(), vec![]);
        assert_eq!(list(&root, "Zeta").unwrap(), vec![]);
    }

    #[test]
    fn recusa_caminho_que_sai_da_pasta_ou_e_absoluto() {
        let root = &fixture("fora").0;
        for rel in ["..", "../x", "sub/../../x", "/", "/etc", "C:/", "\\\\servidor\\x", "./sub", "sub/.."] {
            let got = list(&root, rel);
            assert!(got.is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = root.to_string_lossy().into_owned();
        assert_eq!(list(&root, &absoluto).unwrap_err(), format!("Caminho fora do projeto: {absoluto}"));
    }

    #[test]
    fn pasta_inexistente_e_arquivo_sao_recusados_em_portugues() {
        let root = &fixture("inexistente").0;
        assert_eq!(list(&root, "nao-existe").unwrap_err(), "Pasta não encontrada: nao-existe");
        assert_eq!(list(&root, "sub/x.md").unwrap_err(), "Não é uma pasta: sub/x.md");
    }

    #[test]
    fn texto_com_acento_e_crlf_volta_igual_e_sem_bom() {
        let root = &fixture("texto").0;
        std::fs::write(root.join("sub/nota.md"), "\u{feff}Olá\r\nmundo\r\n").unwrap();
        assert_eq!(read(&root, "sub/nota.md").unwrap(), Content { binary: false, too_large: false, read_only: false, bom: true, text: "Olá\r\nmundo\r\n".into() });
    }

    #[test]
    fn arquivo_vazio_devolve_texto_vazio() {
        let root = &fixture("vazio").0;
        assert_eq!(read(&root, "A.txt").unwrap(), Content { binary: false, too_large: false, read_only: false, bom: false, text: String::new() });
    }

    #[test]
    fn arquivo_com_byte_nulo_e_binario() {
        let root = &fixture("binario").0;
        std::fs::write(root.join("img.bin"), [0x89, b'P', 0, b'N', b'G']).unwrap();
        assert_eq!(read(&root, "img.bin").unwrap(), Content { binary: true, too_large: false, read_only: false, bom: false, text: String::new() });
    }

    #[test]
    fn arquivo_com_mais_de_1_mib_volta_too_large() {
        let root = &fixture("grande").0;
        std::fs::write(root.join("grande.txt"), vec![b'a'; 1_048_577]).unwrap();
        assert_eq!(read(&root, "grande.txt").unwrap(), Content { binary: false, too_large: true, read_only: false, bom: false, text: String::new() });
    }

    #[test]
    fn recusa_caminho_de_leitura_que_sai_da_pasta_ou_e_absoluto() {
        let root = &fixture("ler-fora").0;
        for rel in ["..", "../x", "sub/../../x", "/etc", "C:/", "./sub/x.md"] {
            let got = read(&root, rel);
            assert!(got.is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = root.join("sub/x.md").to_string_lossy().into_owned();
        assert_eq!(read(&root, &absoluto).unwrap_err(), format!("Caminho fora do projeto: {absoluto}"));
    }

    #[test]
    fn arquivo_inexistente_e_pasta_sao_recusados_em_portugues() {
        let root = &fixture("ler-inexistente").0;
        assert_eq!(read(&root, "nao-existe.md").unwrap_err(), "Arquivo não encontrado: nao-existe.md");
        assert_eq!(read(&root, "sub").unwrap_err(), "Não é um arquivo: sub");
    }

    #[test]
    fn midia_devolve_os_bytes_de_imagem_sem_olhar_o_conteudo() {
        let root = &fixture("midia-ok").0;
        let png = [0x89, b'P', b'N', b'G', 0, 0, 0, 0];
        std::fs::write(root.join("sub/logo.PNG"), png).unwrap();
        assert_eq!(read_media(&root, "sub/logo.PNG").unwrap(), png);
    }

    #[test]
    fn midia_recusa_extensao_fora_da_lista_e_arquivo_sem_extensao() {
        let root = &fixture("midia-ext").0;
        for name in ["x.svg", "x.html", "x.exe", "x.txt", "x.wav", "x.mp4", "sem-ext", ".png"] {
            std::fs::write(root.join(name), b"x").unwrap();
            let got = read_media(&root, name).unwrap_err();
            assert_eq!(got, format!("Tipo de arquivo não aberto como mídia: {name}"), "{name}");
        }
    }

    #[test]
    fn midia_recusa_caminho_que_sai_do_projeto_entra_no_git_ou_passa_por_link() {
        let root = &fixture("midia-caminho").0;
        let outside = &Tmp::new("orq-files-fora", "midia").0;
        std::fs::write(outside.join("segredo.png"), b"x").unwrap();
        std::fs::write(root.join(".git/x.png"), b"x").unwrap();
        std::fs::write(root.join("sub/.git/y.png"), b"x").unwrap();
        for rel in ["../x.png", "sub/../../x.png", "/etc/x.png", "C:/x.png", ".git/x.png", "sub/.git/y.png", "sub\\x.png", "nao-existe.png", "sub.png"] {
            assert!(read_media(&root, rel).is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = outside.join("segredo.png").to_string_lossy().into_owned();
        assert!(read_media(&root, &absoluto).is_err());
        // Junction não pede privilégio. Se o `mklink` não criar, só esta parte não roda.
        if junction(&root.join("alias"), &outside) && junction(&root.join("git-alias"), &root.join(".git")) {
            assert!(read_media(&root, "alias/segredo.png").is_err());
            assert!(read_media(&root, "git-alias/x.png").is_err());
        } else {
            eprintln!("sem junction: parte do teste pulada");
        }
    }

    #[test]
    fn midia_acima_do_limite_e_recusada_com_mensagem() {
        let root = &fixture("midia-grande").0;
        std::fs::write(root.join("grande.png"), vec![0u8; (MAX_MEDIA + 1) as usize]).unwrap();
        assert_eq!(read_media(&root, "grande.png").unwrap_err(), "Arquivo grande demais para abrir aqui (limite de 32 MiB): grande.png");
    }

    /// Status, `Content-Range`, `Content-Length` e corpo da resposta de vídeo.
    fn video(file: &Path, range: Option<&str>) -> (u16, Option<String>, Option<String>, Vec<u8>) {
        let r = serve_stream(file, range);
        let header = |name: &str| r.headers().get(name).map(|v| v.to_str().unwrap().to_string());
        (r.status().as_u16(), header("Content-Range"), header("Content-Length"), r.body().clone())
    }

    #[test]
    fn video_serve_o_intervalo_pedido_com_206() {
        let root = &fixture("video-range").0;
        std::fs::write(root.join("sub/clipe.mp4"), b"0123456789").unwrap();
        let file = stream_path(root, "sub/clipe.mp4").unwrap();
        let some = |s: &str| Some(s.to_string());
        assert_eq!(video(&file, Some("bytes=2-5")), (206, some("bytes 2-5/10"), some("4"), b"2345".to_vec()));
        assert_eq!(video(&file, Some("bytes=7-")), (206, some("bytes 7-9/10"), some("3"), b"789".to_vec()));
        assert_eq!(video(&file, Some("bytes=-3")), (206, some("bytes 7-9/10"), some("3"), b"789".to_vec()));
        assert_eq!(video(&file, Some("bytes=-30")), (206, some("bytes 0-9/10"), some("10"), b"0123456789".to_vec()));
        assert_eq!(video(&file, Some("bytes=8-100")), (206, some("bytes 8-9/10"), some("2"), b"89".to_vec()));
        assert_eq!(video(&file, None), (206, some("bytes 0-9/10"), some("10"), b"0123456789".to_vec()));
        // Cabeçalho que não se entende (vários intervalos, outra unidade, `b < a`) vale como `bytes=0-`.
        for range in ["bytes=0-1,4-5", "items=0-1", "bytes=5-2", "bytes=x-"] {
            assert_eq!(video(&file, Some(range)).0, 206, "{range}");
            assert_eq!(video(&file, Some(range)).1, some("bytes 0-9/10"), "{range}");
        }
        let r = serve_stream(&file, Some("bytes=0-0"));
        assert_eq!(r.headers()["Accept-Ranges"], "bytes");
        assert_eq!(r.headers()["Content-Type"], "video/mp4");
    }

    #[test]
    fn video_corta_o_intervalo_no_teto() {
        let root = &fixture("video-teto").0;
        let total = MAX_STREAM_CHUNK + 1;
        std::fs::write(root.join("grande.webm"), vec![7u8; total as usize]).unwrap();
        let file = stream_path(root, "grande.webm").unwrap();
        let last = MAX_STREAM_CHUNK - 1;
        for range in [None, Some("bytes=0-"), Some(format!("bytes=0-{total}")).as_deref()] {
            let (status, content_range, len, body) = video(&file, range);
            assert_eq!((status, content_range, len), (206, Some(format!("bytes 0-{last}/{total}")), Some(MAX_STREAM_CHUNK.to_string())), "{range:?}");
            assert_eq!(body.len() as u64, MAX_STREAM_CHUNK);
        }
        assert_eq!(video(&file, Some("bytes=1-")).1, Some(format!("bytes 1-{MAX_STREAM_CHUNK}/{total}")));
        assert_eq!(video(&file, Some(&format!("bytes=-{total}"))).1, Some(format!("bytes 0-{last}/{total}")));
    }

    #[test]
    fn video_fora_do_arquivo_e_416_e_vazio_e_200() {
        let root = &fixture("video-416").0;
        std::fs::write(root.join("clipe.mp4"), b"0123456789").unwrap();
        std::fs::write(root.join("vazio.mp4"), b"").unwrap();
        let file = stream_path(root, "clipe.mp4").unwrap();
        for range in ["bytes=10-", "bytes=10-20", "bytes=-0"] {
            assert_eq!(video(&file, Some(range)), (416, Some("bytes */10".into()), None, Vec::new()), "{range}");
        }
        let vazio = stream_path(root, "vazio.mp4").unwrap();
        for range in [None, Some("bytes=0-"), Some("bytes=5-9")] {
            assert_eq!(video(&vazio, range), (200, None, Some("0".into()), Vec::new()), "{range:?}");
        }
    }

    #[test]
    fn video_tipo_pela_extensao_sem_diferenciar_caixa() {
        let root = &fixture("video-tipo").0;
        for (name, mime) in [("a.mp4", "video/mp4"), ("b.MP4", "video/mp4"), ("c.m4v", "video/mp4"), ("d.MOV", "video/mp4"), ("e.webm", "video/webm"), ("f.WebM", "video/webm")] {
            std::fs::write(root.join(name), b"x").unwrap();
            let r = serve_stream(&stream_path(root, name).unwrap(), None);
            assert_eq!(r.headers()["Content-Type"], mime, "{name}");
        }
    }

    #[test]
    fn audio_serve_pelo_protocolo_com_tipo_e_intervalo_e_sem_o_teto_de_midia() {
        let root = &fixture("audio-stream").0;
        for (name, mime) in [("a.mp3", "audio/mpeg"), ("b.WAV", "audio/wav"), ("c.ogg", "audio/ogg"), ("d.oga", "audio/ogg"), ("e.flac", "audio/flac"), ("f.m4a", "audio/mp4")] {
            std::fs::write(root.join(name), b"0123456789").unwrap();
            let file = stream_path(root, name).unwrap();
            let r = serve_stream(&file, Some("bytes=2-5"));
            assert_eq!((r.status().as_u16(), r.headers()["Content-Type"].to_str().unwrap(), r.body().as_slice()), (206, mime, &b"2345"[..]), "{name}");
            assert_eq!(r.headers()["Accept-Ranges"], "bytes");
        }
        // Acima de `MAX_MEDIA` o protocolo serve (em pedaços), enquanto `read_media` recusa a extensão.
        let total = MAX_MEDIA + 1;
        std::fs::write(root.join("grande.wav"), vec![1u8; total as usize]).unwrap();
        let file = stream_path(root, "grande.wav").unwrap();
        let (status, content_range, len, body) = video(&file, Some(&format!("bytes={}-", total - 3)));
        assert_eq!((status, content_range, len), (206, Some(format!("bytes {}-{}/{total}", total - 3, total - 1)), Some("3".into())));
        assert_eq!(body, vec![1u8; 3]);
        assert!(read_media(root, "grande.wav").is_err());
        let absoluto = stream_path_at(&root.join("grande.wav")).unwrap();
        assert_eq!(video(&absoluto, Some("bytes=0-0")).0, 206);
    }

    #[test]
    fn audio_recusa_caminho_que_sai_do_projeto_ou_entra_no_git() {
        let root = &fixture("audio-caminho").0;
        std::fs::write(root.join(".git/x.wav"), b"x").unwrap();
        for rel in ["../x.wav", "sub/../../x.wav", ".git/x.wav", "sub\\x.wav"] {
            assert_eq!(stream_path(root, rel).map_err(|e| e.0), Err(403), "{rel} deveria ser recusado");
        }
        assert_eq!(stream_path(root, "nao-existe.mp3").unwrap_err().0, 404);
    }

    #[test]
    fn video_recusa_extensao_fora_da_lista() {
        let root = &fixture("video-ext").0;
        for name in ["x.png", "x.txt", "x.exe", "sem-ext", ".mp4", ".wav"] {
            std::fs::write(root.join(name), b"x").unwrap();
            assert_eq!(stream_path(root, name).unwrap_err(), (403, format!("Tipo de arquivo não aberto como vídeo ou áudio: {name}")), "{name}");
            let absoluto = root.join(name);
            assert_eq!(stream_path_at(&absoluto).unwrap_err().0, 403, "{name}");
        }
    }

    #[test]
    fn video_recusa_caminho_que_sai_do_projeto_entra_no_git_ou_passa_por_link() {
        let root = &fixture("video-caminho").0;
        let outside = &Tmp::new("orq-files-fora", "video").0;
        std::fs::write(outside.join("segredo.mp4"), b"x").unwrap();
        std::fs::write(root.join(".git/x.mp4"), b"x").unwrap();
        std::fs::write(root.join("sub/.git/y.mp4"), b"x").unwrap();
        std::fs::create_dir(root.join("pasta.mp4")).unwrap();
        for rel in ["../x.mp4", "sub/../../x.mp4", "/etc/x.mp4", "C:/x.mp4", ".git/x.mp4", "sub/.git/y.mp4", "sub\\x.mp4", "pasta.mp4"] {
            assert_eq!(stream_path(root, rel).map_err(|e| e.0), Err(403), "{rel} deveria ser recusado");
        }
        assert_eq!(stream_path(root, "nao-existe.mp4").unwrap_err(), (404, "Arquivo não encontrado: nao-existe.mp4".into()));
        let absoluto = outside.join("segredo.mp4").to_string_lossy().into_owned();
        assert_eq!(stream_path(root, &absoluto).map_err(|e| e.0), Err(403));
        if junction(&root.join("alias"), outside) && junction(&root.join("git-alias"), &root.join(".git")) {
            assert_eq!(stream_path(root, "alias/segredo.mp4").map_err(|e| e.0), Err(403));
            assert_eq!(stream_path(root, "git-alias/x.mp4").map_err(|e| e.0), Err(403));
        } else {
            eprintln!("sem junction: parte do teste pulada");
        }
    }

    #[test]
    fn video_absoluto_exige_caminho_absoluto_e_404_se_sumiu() {
        let root = &fixture("video-absoluto").0;
        std::fs::write(root.join("clipe.mov"), b"0123").unwrap();
        assert_eq!(stream_path_at(Path::new("clipe.mov")).unwrap_err(), (403, "O caminho do arquivo precisa ser absoluto.".into()));
        let file = stream_path_at(&root.join("clipe.mov")).unwrap();
        assert_eq!(video(&file, Some("bytes=1-2")).3, b"12");
        let sumiu = stream_path_at(&root.join("nao-existe.mp4")).unwrap();
        assert_eq!(serve_stream(&sumiu, None).status().as_u16(), 404);
        assert_eq!(serve_stream(&sumiu, None).headers()["Content-Type"], "text/plain; charset=utf-8");
    }

    #[test]
    fn nome_invalido_no_windows_e_recusado() {
        for name in ["", "   ", ".", "..", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a|b", "a\u{1}b", " x", "x ", "x.", "CON", "nul.txt", "com1.md", "LPT9.log"] {
            assert!(check_name(name).is_err(), "{name:?} deveria ser recusado");
        }
        for name in ["novo.txt", "Arquivo com espaço.md", "conta.con", "com10.txt", ".env", "ação.ts"] {
            assert!(check_name(name).is_ok(), "{name:?} deveria passar");
        }
    }

    #[test]
    fn renomeia_na_mesma_pasta_e_devolve_o_caminho_novo() {
        let root = &fixture("renomeia").0;
        assert_eq!(rename(&root, "sub/x.md", "y.md").unwrap(), "sub/y.md");
        assert!(root.join("sub/y.md").exists());
        assert!(!root.join("sub/x.md").exists());
        assert_eq!(rename(&root, "b.txt", "novo.txt").unwrap(), "novo.txt");
        assert!(root.join("novo.txt").exists());
    }

    #[test]
    fn colisao_nao_sobrescreve_e_deixa_os_dois_arquivos() {
        let root = &fixture("colisao").0;
        std::fs::write(root.join("A.txt"), "primeiro").unwrap();
        std::fs::write(root.join("b.txt"), "segundo").unwrap();
        assert_eq!(rename(&root, "b.txt", "A.txt").unwrap_err(), "Já existe um item chamado “A.txt” nesta pasta.");
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "primeiro");
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "segundo");
    }

    #[test]
    fn troca_so_de_maiusculas_funciona_e_nome_igual_nao_faz_nada() {
        let root = &fixture("caixa").0;
        assert_eq!(rename(&root, "b.txt", "B.txt").unwrap(), "B.txt");
        assert!(std::fs::read_dir(&root).unwrap().flatten().any(|e| e.file_name() == "B.txt"));
        assert_eq!(rename(&root, "B.txt", "B.txt").unwrap(), "B.txt");
    }

    #[cfg(windows)]
    #[test]
    fn link_quebrado_ocupa_o_nome_e_link_nao_e_renomeado() {
        let root = &fixture("links").0;
        // Criar link simbólico exige privilégio no Windows; sem ele, o teste só não roda esta parte.
        if crate::platform::symlink_arquivo(&root.join("sub/x.md"), &root.join("link.md")).is_err() {
            eprintln!("sem privilégio para link simbólico: teste de links pulado");
            return;
        }
        crate::platform::symlink_arquivo(&root.join("nao-existe.md"), &root.join("quebrado.md")).unwrap();
        assert!(rename(&root, "b.txt", "quebrado.md").unwrap_err().starts_with("Já existe"));
        assert!(rename(&root, "link.md", "outro.md").unwrap_err().contains("é um link"));
        assert!(rename(&root, "link.md", "outro.md").is_err());
        assert!(root.join("sub/x.md").exists());
    }

    #[test]
    fn git_pastas_e_caminhos_de_fora_nao_entram_nas_acoes() {
        let root = &fixture("acoes-recusas").0;
        std::fs::write(root.join(".git/config"), "").unwrap();
        for rel in [".git/config", "sub/.git/config"] {
            assert!(rename(&root, rel, "x").is_err(), "{rel}");
            assert!(recycle(&root, rel).is_err(), "{rel}");
            assert!(plain(&root, rel, false).is_err(), "{rel}");
        }
        // Pasta com `.git` dentro não vai para a Lixeira, nem como pasta.
        assert!(recycle(&root, "sub").unwrap_err().contains(".git"));
        assert!(recycle(&root, "../x").is_err());
        assert!(plain(&root, "sub/dentro", true).is_ok());
        assert!(plain(&root, "b.txt", true).is_err());
        assert!(plain(&root, "..", true).is_err());
        assert!(root.join(".git/config").exists());
    }

    #[test]
    fn junction_no_meio_do_caminho_nao_entra_nas_acoes() {
        let root = &fixture("junction").0;
        std::fs::write(root.join(".git/config"), "").unwrap();
        // Junction não pede privilégio. Se o `mklink` não criar, o teste só não roda esta parte.
        if !junction(&root.join("alias"), &root.join("sub")) || !junction(&root.join("git-alias"), &root.join(".git")) {
            eprintln!("sem junction: teste pulado");
        } else {
            assert!(rename(&root, "alias/x.md", "y.md").is_err());
            assert!(recycle(&root, "alias/x.md").is_err());
            assert!(plain(&root, "alias", true).is_err());
            assert!(rename(&root, "git-alias/config", "x").is_err());
            assert!(recycle(&root, "git-alias/config").is_err());
            assert!(plain(&root, "git-alias/config", false).is_err());
            assert!(root.join("sub/x.md").exists());
            assert!(root.join(".git/config").exists());
            assert_eq!(rename(&root, "sub/x.md", "y.md").unwrap(), "sub/y.md");
        }
        // Remove os links sem seguir: `remove_dir` tira a junction, o destino fica.
        let _ = std::fs::remove_dir(root.join("alias"));
        let _ = std::fs::remove_dir(root.join("git-alias"));
    }

    #[test]
    fn dois_pontos_nao_contornam_git() {
        let root = &fixture("dois-pontos").0;
        std::fs::write(root.join(".git/config"), "antes").unwrap();
        for rel in [".git::$INDEX_ALLOCATION/config", "a:b"] {
            assert!(no_git(rel).is_err(), "{rel}");
            assert!(write_checked(&root, rel, "x", "antes", false).is_err(), "{rel}");
            assert!(create_file(&root, rel, "hook").is_err(), "{rel}");
        }
        assert!(create_file(&root, ".git::$INDEX_ALLOCATION", "hook").is_err());
        assert_eq!(std::fs::read_to_string(root.join(".git/config")).unwrap(), "antes");
        assert!(!root.join(".git/hook").exists());
        // Só onde o volume gera nomes 8.3.
        if root.join("GIT~1").is_dir() {
            assert!(write_checked(&root, "GIT~1/config", "x", "antes", false).is_err());
            assert!(create_file(&root, "GIT~1", "hook").is_err());
            assert!(plain(&root, "git~1", true).is_err());
        }
    }

    #[test]
    fn barra_invertida_nao_contorna_git_nem_links() {
        let root = &fixture("barra-invertida").0;
        for rel in [".git\\config", "sub\\arquivo", "alias\\arquivo.txt"] {
            assert!(rename(&root, rel, "x").unwrap_err().starts_with("Caminho inválido"), "{rel}");
            assert!(recycle(&root, rel).unwrap_err().starts_with("Caminho inválido"), "{rel}");
            assert!(plain(&root, rel, false).unwrap_err().starts_with("Caminho inválido"), "{rel}");
        }
    }

    /// Manda para a Lixeira só um arquivo temporário criado aqui: o arquivo do projeto nunca entra.
    #[test]
    fn arquivo_temporario_vai_para_a_lixeira_e_sai_do_lugar() {
        let root = &fixture("lixeira").0;
        let file = root.join("sub/descartavel.txt");
        std::fs::write(&file, "temporário").unwrap();
        assert_eq!(recycle(&root, "sub/descartavel.txt"), Ok(()));
        assert!(std::fs::symlink_metadata(&file).is_err());
    }

    /// Cola como `paste_detailed`, devolvendo só os caminhos criados ou o erro. Só os testes usam esta forma.
    fn paste(project: &Path, dest: &str, snap: &Snapshot) -> Result<Vec<String>, String> {
        let got = paste_detailed(project, dest, snap);
        match got.error {
            Some(error) => Err(error),
            None => Ok(got.done.into_iter().map(|p| p.to).collect()),
        }
    }

    fn snap(paths: &[PathBuf], cut: bool) -> Snapshot {
        Snapshot { paths: paths.to_vec(), cut }
    }

    #[test]
    fn cria_arquivo_vazio_na_raiz_e_na_subpasta() {
        let root = &fixture("cria-arquivo").0;
        assert_eq!(create_file(&root, "", "novo.txt").unwrap(), "novo.txt");
        assert_eq!(create_file(&root, "sub", "dentro.md").unwrap(), "sub/dentro.md");
        assert_eq!(std::fs::metadata(root.join("novo.txt")).unwrap().len(), 0);
        assert_eq!(std::fs::metadata(root.join("sub/dentro.md")).unwrap().len(), 0);
    }

    #[test]
    fn cria_pasta_na_raiz_e_na_subpasta() {
        let root = &fixture("cria-pasta").0;
        assert_eq!(create_dir(&root, "", "nova").unwrap(), "nova");
        assert_eq!(create_dir(&root, "sub/", "nova").unwrap(), "sub/nova");
        assert!(root.join("nova").is_dir());
        assert!(root.join("sub/nova").is_dir());
    }

    #[test]
    fn criacao_nao_sobrescreve_arquivo_nem_pasta_existente() {
        let root = &fixture("cria-colisao").0;
        std::fs::write(root.join("A.txt"), "original").unwrap();
        assert_eq!(create_file(&root, "", "A.txt").unwrap_err(), "Já existe um item chamado “A.txt” nesta pasta. Nada foi criado.");
        assert_eq!(create_file(&root, "", "a.txt").unwrap_err(), "Já existe um item chamado “a.txt” nesta pasta. Nada foi criado.");
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "original");
        assert!(create_file(&root, "", "alfa").is_err());
        assert!(create_dir(&root, "", "Zeta").is_err());
        assert!(create_dir(&root, "", "A.txt").is_err());
        assert!(root.join("Zeta").is_dir());
    }

    #[cfg(windows)]
    #[test]
    fn criacao_nao_sobrescreve_link_quebrado() {
        let root = &fixture("cria-quebrado").0;
        // Criar link simbólico exige privilégio no Windows; sem ele, o teste só não roda esta parte.
        if crate::platform::symlink_arquivo(&root.join("nao-existe.md"), &root.join("quebrado.md")).is_err() {
            eprintln!("sem privilégio para link simbólico: teste de link quebrado pulado");
            return;
        }
        assert!(create_file(&root, "", "quebrado.md").is_err());
        assert!(create_dir(&root, "", "quebrado.md").is_err());
        assert!(std::fs::symlink_metadata(root.join("quebrado.md")).unwrap().file_type().is_symlink());
        assert!(!root.join("nao-existe.md").exists());
    }

    #[test]
    fn criacao_recusa_nome_invalido_git_e_caminho_que_sai() {
        let root = &fixture("cria-recusas").0;
        for name in ["", "a/b", "..", ".", "x:y", "CON.txt", " x", ".git", ".GIT", ".Git"] {
            assert!(create_file(&root, "", name).is_err(), "{name:?} deveria ser recusado");
            assert!(create_dir(&root, "", name).is_err(), "{name:?} deveria ser recusado");
        }
        for dir in ["..", "../x", "sub/../..", "/", "C:/", ".git", "sub/.git", ".GIT/x", "sub\\dentro", "nao-existe", "sub/x.md"] {
            assert!(create_file(&root, dir, "novo.txt").is_err(), "{dir} deveria ser recusado");
            assert!(create_dir(&root, dir, "novo").is_err(), "{dir} deveria ser recusado");
        }
        assert!(!root.join("novo.txt").exists());
        assert!(!root.join("novo").exists());
        assert!(!root.join("sub/novo.txt").exists());
        assert!(!root.join("sub/.git/novo.txt").exists());
    }

    #[test]
    fn criacao_recusa_pasta_que_e_junction_para_fora_ou_para_git() {
        let root = &fixture("cria-junction").0;
        std::fs::write(root.join(".git/config"), "").unwrap();
        if !junction(&root.join("alias"), &root.join("sub")) || !junction(&root.join("git-alias"), &root.join(".git")) {
            eprintln!("sem junction: teste pulado");
        } else {
            assert!(create_file(&root, "alias", "novo.txt").is_err());
            assert!(create_dir(&root, "alias", "nova").is_err());
            assert!(create_file(&root, "git-alias", "x").is_err());
            assert!(!root.join("sub/novo.txt").exists());
            assert!(!root.join("sub/nova").exists());
            assert!(!root.join(".git/x").exists());
        }
        let _ = std::fs::remove_dir(root.join("alias"));
        let _ = std::fs::remove_dir(root.join("git-alias"));
    }

    #[test]
    fn revelar_pasta_aceita_raiz_e_subpasta_e_recusa_git_e_fora() {
        let root = &fixture("revela").0;
        assert_eq!(plain(&root, "", true).unwrap(), *root);
        assert_eq!(plain(&root, "sub/dentro", true).unwrap(), root.join("sub").join("dentro"));
        for rel in [".git", "sub/.git", "..", "../x", "/", "b.txt"] {
            assert!(plain(&root, rel, true).is_err(), "{rel} deveria ser recusado");
        }
    }

    #[test]
    fn copia_arquivo_de_fora_para_subpasta_e_mantem_o_original() {
        let root = &fixture("cola-copia").0;
        let ext = &Tmp::new("orq-files-fora", "cola-copia").0;
        std::fs::write(ext.join("de-fora.txt"), "conteúdo").unwrap();
        assert_eq!(paste(&root, "sub", &snap(&[ext.join("de-fora.txt")], false)).unwrap(), vec!["sub/de-fora.txt"]);
        assert_eq!(std::fs::read_to_string(root.join("sub/de-fora.txt")).unwrap(), "conteúdo");
        assert_eq!(std::fs::read_to_string(ext.join("de-fora.txt")).unwrap(), "conteúdo");
    }

    #[test]
    fn recorte_de_arquivo_move_e_tira_da_origem() {
        let root = &fixture("cola-recorte").0;
        std::fs::write(root.join("b.txt"), "movido").unwrap();
        assert_eq!(paste(&root, "sub", &snap(&[root.join("b.txt")], true)).unwrap(), vec!["sub/b.txt"]);
        assert_eq!(std::fs::read_to_string(root.join("sub/b.txt")).unwrap(), "movido");
        assert!(std::fs::symlink_metadata(root.join("b.txt")).is_err());
    }

    #[test]
    fn copia_pasta_inteira_com_conteudo_e_mantem_a_origem() {
        let root = &fixture("cola-pasta").0;
        let ext = &Tmp::new("orq-files-fora", "cola-pasta").0;
        std::fs::create_dir_all(ext.join("pasta/deep")).unwrap();
        std::fs::write(ext.join("pasta/a.txt"), "a").unwrap();
        std::fs::write(ext.join("pasta/deep/b.txt"), "bê").unwrap();
        assert_eq!(paste(&root, "", &snap(&[ext.join("pasta")], false)).unwrap(), vec!["pasta"]);
        assert_eq!(std::fs::read_to_string(root.join("pasta/a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(root.join("pasta/deep/b.txt")).unwrap(), "bê");
        assert_eq!(std::fs::read_to_string(ext.join("pasta/deep/b.txt")).unwrap(), "bê");
    }

    #[test]
    fn recorte_de_pasta_move_o_conteudo_inteiro() {
        let root = &fixture("cola-recorte-pasta").0;
        std::fs::write(root.join("sub/dentro/f.txt"), "f").unwrap();
        assert_eq!(paste(&root, "Beta", &snap(&[root.join("sub/dentro")], true)).unwrap(), vec!["Beta/dentro"]);
        assert_eq!(std::fs::read_to_string(root.join("Beta/dentro/f.txt")).unwrap(), "f");
        assert!(std::fs::symlink_metadata(root.join("sub/dentro")).is_err());
    }

    #[test]
    fn colisao_ganha_nome_novo_e_o_existente_fica_igual() {
        let root = &fixture("cola-colisao").0;
        let ext = &Tmp::new("orq-files-fora", "cola-colisao").0;
        std::fs::write(root.join("A.txt"), "primeiro").unwrap();
        std::fs::write(ext.join("A.txt"), "segundo").unwrap();
        std::fs::create_dir_all(root.join("alfa")).unwrap();
        std::fs::write(root.join("alfa/keep.txt"), "guardado").unwrap();
        std::fs::create_dir_all(ext.join("alfa")).unwrap();
        std::fs::write(ext.join("alfa/novo.txt"), "de fora").unwrap();
        let got = paste(&root, "", &snap(&[ext.join("A.txt"), ext.join("alfa")], false)).unwrap();
        assert_eq!(got, vec!["A (2).txt", "alfa (2)"]);
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "primeiro");
        assert_eq!(std::fs::read_to_string(root.join("A (2).txt")).unwrap(), "segundo");
        assert_eq!(std::fs::read_to_string(root.join("alfa/keep.txt")).unwrap(), "guardado");
        assert_eq!(std::fs::read_to_string(root.join("alfa (2)/novo.txt")).unwrap(), "de fora");
    }

    #[test]
    fn auto_colagem_e_recorte_na_mesma_pasta_sao_recusados() {
        let root = &fixture("cola-auto").0;
        let err = paste(&root, "", &snap(&[root.join("b.txt")], true)).unwrap_err();
        assert!(err.contains("já está nesta pasta"), "{err}");
        assert!(root.join("b.txt").exists());
        let err = paste(&root, "sub/dentro", &snap(&[root.join("sub/dentro")], true)).unwrap_err();
        assert!(err.contains("dela mesma"), "{err}");
        std::fs::create_dir_all(root.join("pai/filho")).unwrap();
        let err = paste(&root, "pai/filho", &snap(&[root.join("pai")], false)).unwrap_err();
        assert!(err.contains("dela mesma"), "{err}");
        assert!(!root.join("pai/filho/pai").exists());
        assert!(root.join("pai/filho").is_dir());
    }

    #[test]
    fn um_item_recusado_faz_nada_ser_colado() {
        let root = &fixture("cola-previa").0;
        let ext = &Tmp::new("orq-files-fora", "cola-previa").0;
        std::fs::write(root.join(".git/config"), "").unwrap();
        std::fs::write(ext.join("ok.txt"), "ok").unwrap();
        let err = paste(&root, "sub", &snap(&[ext.join("ok.txt"), root.join(".git/config")], false)).unwrap_err();
        assert!(err.contains(".git"), "{err}");
        assert!(!root.join("sub/ok.txt").exists());
        assert!(paste(&root, "sub", &snap(&[], false)).is_err());
    }

    #[test]
    fn colar_em_git_ou_de_git_e_recusado() {
        let root = &fixture("cola-git").0;
        let ext = &Tmp::new("orq-files-fora", "cola-git").0;
        std::fs::write(ext.join("x.txt"), "x").unwrap();
        for dest in [".git", "sub/.git", "../fora", "/"] {
            assert!(paste(&root, dest, &snap(&[ext.join("x.txt")], false)).is_err(), "{dest}");
        }
        std::fs::create_dir_all(ext.join("repo/.git")).unwrap();
        assert!(paste(&root, "", &snap(&[ext.join("repo")], false)).unwrap_err().contains(".git"));
        assert!(paste(&root, "", &snap(&[root.join(".git/config")], false)).is_err());
        assert!(!root.join("x.txt").exists());
        assert!(!root.join("repo").exists());
    }

    #[test]
    fn colar_item_que_some_ou_que_e_junction_e_recusado() {
        let root = &fixture("cola-junction").0;
        let ext = &Tmp::new("orq-files-fora", "cola-junction").0;
        assert!(paste(&root, "", &snap(&[ext.join("sumiu.txt")], false)).unwrap_err().contains("não existe mais"));
        std::fs::create_dir_all(ext.join("pasta")).unwrap();
        let atalho = ext.join("pasta").join("atalho");
        if !junction(&atalho, &root.join("sub")) {
            eprintln!("sem junction: teste pulado");
        } else {
            let err = paste(&root, "", &snap(&[ext.join("pasta")], false)).unwrap_err();
            assert!(err.contains("link ou junction"), "{err}");
            assert!(!root.join("pasta").exists());
            assert!(!root.join("sub/pasta").exists());
        }
        let _ = std::fs::remove_dir(&atalho);
    }

    #[test]
    fn paste_detailed_devolve_origem_absoluta_e_destino_relativo() {
        let root = &fixture("detalhe-ok").0;
        let ext = &Tmp::new("orq-files-fora", "detalhe-ok").0;
        std::fs::write(ext.join("A.txt"), "a").unwrap();
        std::fs::write(root.join("A.txt"), "ja existe").unwrap();
        let sources = [ext.join("A.txt")];
        let done = ok_lote(paste_detailed(&root, "sub", &snap(&sources, false)));
        assert_eq!(done, vec![Moved { from: ext.join("A.txt").display().to_string(), to: "sub/A.txt".into() }]);
        // A API antiga continua dando só os destinos, com o mesmo nome.
        assert_eq!(paste(&root, "", &snap(&sources, false)).unwrap(), vec!["A (2).txt"]);
    }

    #[test]
    fn paste_detailed_recusa_pasta_com_o_proprio_filho_antes_de_mexer() {
        let root = &fixture("detalhe-filho").0;
        let ext = &Tmp::new("orq-files-fora", "detalhe-filho").0;
        std::fs::create_dir_all(ext.join("pasta")).unwrap();
        std::fs::write(ext.join("pasta/f.txt"), "f").unwrap();
        let sources = [ext.join("pasta"), ext.join("pasta/f.txt")];
        let err = paste_detailed(&root, "", &snap(&sources, true));
        assert!(err.done.is_empty());
        assert!(err.error.as_deref().is_some_and(|e| e.contains("dentro de “pasta”")), "{:?}", err.error);
        assert!(ext.join("pasta/f.txt").exists(), "nada foi movido");
        let twice = [ext.join("pasta/f.txt"), ext.join("pasta/f.txt")];
        assert!(erro_lote(paste_detailed(&root, "", &snap(&twice, false))).contains("repetido"));
    }

    /// Uma pasta limpa, sem `.git`, com um arquivo dentro e outro ao lado: `origem/interna/f.txt` e `origem/g.txt`.
    fn pasta(root: &Path) {
        std::fs::create_dir_all(root.join("origem/interna")).unwrap();
        std::fs::write(root.join("origem/interna/f.txt"), "f").unwrap();
        std::fs::write(root.join("origem/g.txt"), "g").unwrap();
    }

    fn rels(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// O `done` de um lote que deu certo. Falha se o lote parou.
    fn ok_lote<T>(lote: Batch<T>) -> Vec<T> {
        assert!(lote.error.is_none(), "{:?}", lote.error);
        lote.done
    }

    /// O erro de um lote que parou. Falha se ele deu certo.
    fn erro_lote<T>(lote: Batch<T>) -> String {
        lote.error.expect("o lote deveria ter parado")
    }

    #[test]
    fn renomeia_pasta_e_os_descendentes_seguem() {
        let root = &fixture("renomeia-pasta").0;
        pasta(&root);
        assert_eq!(rename(&root, "origem", "destino").unwrap(), "destino");
        assert_eq!(std::fs::read_to_string(root.join("destino/interna/f.txt")).unwrap(), "f");
        assert!(!root.join("origem").exists());
        assert_eq!(rename(&root, "destino/interna", "dentro").unwrap(), "destino/dentro");
        assert!(root.join("destino/dentro/f.txt").exists());
        assert!(rename(&root, "", "x").is_err());
        assert!(rename(&root, "Zeta", "alfa").unwrap_err().starts_with("Já existe"));
        assert!(root.join("Zeta").is_dir() && root.join("alfa").is_dir());
    }

    #[test]
    fn move_arquivo_e_pasta_para_outra_pasta_e_devolve_o_mapeamento() {
        let root = &fixture("move-ok").0;
        pasta(&root);
        let got = ok_lote(move_items(&root, &rels(&["origem", "b.txt"]), "Beta"));
        assert_eq!(
            got,
            vec![
                Moved { from: "origem".into(), to: "Beta/origem".into() },
                Moved { from: "b.txt".into(), to: "Beta/b.txt".into() },
            ]
        );
        assert_eq!(std::fs::read_to_string(root.join("Beta/origem/interna/f.txt")).unwrap(), "f");
        assert!(!root.join("origem").exists() && !root.join("b.txt").exists());
        // De volta para a raiz: "" é o destino.
        assert_eq!(ok_lote(move_items(&root, &rels(&["Beta/b.txt"]), ""))[0].to, "b.txt");
        assert!(root.join("b.txt").exists());
    }

    /// Cada caso recusa o lote inteiro, com `done` vazio, e a mensagem diz o motivo. Depois de todos, nada saiu do lugar.
    #[test]
    fn move_recusa_o_lote_inteiro_antes_de_mexer() {
        let root = &fixture("move-recusa").0;
        pasta(&root);
        std::fs::create_dir_all(root.join("x")).unwrap();
        std::fs::write(root.join("x/c.txt"), "x").unwrap();
        std::fs::write(root.join("c.txt"), "c").unwrap();
        std::fs::write(root.join("Beta/b.txt"), "já estava").unwrap();
        let casos: [(&[&str], &str, &str); 14] = [
            (&["origem"], "origem", "dela mesma"),
            (&["origem"], "origem/interna", "dela mesma"),
            (&["b.txt"], "", "já está nesta pasta"),
            (&["b.txt"], "Beta", "Já existe"),
            (&["x/c.txt", "c.txt"], "Beta", "Já existe"),
            (&["origem", "origem/interna"], "Beta", "dentro de"),
            (&["c.txt", "c.txt"], "Beta", "repetido"),
            (&["../x"], "Beta", "fora do projeto"),
            (&["b.txt"], "../fora", "fora do projeto"),
            (&[".git/config"], "Beta", ".git"),
            (&["b.txt"], ".git", ".git"),
            (&[""], "Beta", "raiz"),
            (&[], "Beta", "Nenhum item"),
            (&["b.txt", "nao-existe.txt"], "Beta", "não encontrado"),
        ];
        for (srcs, dest, quer) in casos {
            let err = move_items(&root, &rels(srcs), dest);
            assert!(err.done.is_empty(), "{srcs:?} -> {dest:?} não deveria mexer em nada");
            assert!(err.error.as_deref().is_some_and(|e| e.contains(quer)), "{srcs:?} -> {dest:?}: {:?}", err.error);
        }
        assert!(root.join("origem/interna/f.txt").exists() && root.join("origem/g.txt").exists());
        assert!(root.join("b.txt").exists() && root.join("c.txt").exists() && root.join("x/c.txt").exists());
        assert_eq!(std::fs::read_to_string(root.join("Beta/b.txt")).unwrap(), "já estava");
        assert!(!root.join("Beta/c.txt").exists() && !root.join("Beta/origem").exists());
    }

    #[test]
    fn move_recusa_link_e_junction_no_item_e_no_destino() {
        let root = &fixture("move-junction").0;
        pasta(&root);
        let junction_ok = junction(&root.join("alias"), &root.join("origem"));
        if !junction_ok {
            eprintln!("sem junction: teste pulado");
        } else {
            assert!(erro_lote(move_items(&root, &rels(&["alias"]), "Beta")).contains("link"));
            assert!(erro_lote(move_items(&root, &rels(&["alias/g.txt"]), "Beta")).contains("link"));
            assert!(erro_lote(move_items(&root, &rels(&["b.txt"]), "alias")).contains("link"));
            assert!(root.join("origem/g.txt").exists());
            assert!(!root.join("Beta/g.txt").exists());
            assert!(!root.join("Beta/b.txt").exists());
        }
        let _ = std::fs::remove_dir(root.join("alias"));
    }

    #[test]
    fn lixeira_em_lote_recusa_tudo_antes_de_mexer() {
        let root = &fixture("lote-lixeira").0;
        pasta(&root);
        std::fs::write(root.join(".git/config"), "").unwrap();
        let err = recycle_many(&root, &rels(&["b.txt", ".git/config"]));
        assert!(err.done.is_empty() && err.error.as_deref().is_some_and(|e| e.contains(".git")), "{:?}", err.error);
        assert!(erro_lote(recycle_many(&root, &rels(&["origem", "origem/interna"]))).contains("dentro de"));
        assert!(erro_lote(recycle_many(&root, &rels(&["sub"]))).contains(".git"));
        assert!(recycle_many(&root, &rels(&[""])).error.is_some());
        assert!(recycle_many(&root, &[]).error.is_some());
        assert!(root.join("b.txt").exists() && root.join(".git/config").exists() && root.join("origem/interna/f.txt").exists());
    }

    /// Vai para a Lixeira só o que foi criado dentro do temporário; a pasta e o arquivo somem da origem.
    #[test]
    fn lixeira_leva_pasta_inteira_e_arquivo_em_lote() {
        let root = &fixture("lixeira-pasta").0;
        std::fs::create_dir_all(root.join("lixo/dentro")).unwrap();
        std::fs::write(root.join("lixo/dentro/f.txt"), "f").unwrap();
        std::fs::write(root.join("descartavel.txt"), "d").unwrap();
        assert_eq!(ok_lote(recycle_many(&root, &rels(&["lixo", "descartavel.txt"]))), rels(&["lixo", "descartavel.txt"]));
        assert!(std::fs::symlink_metadata(root.join("lixo")).is_err());
        assert!(std::fs::symlink_metadata(root.join("descartavel.txt")).is_err());
    }

    #[test]
    fn gravacao_verificada_recusa_mudanca_externa_e_nao_grava() {
        let root = &fixture("verifica-externa").0;
        std::fs::write(root.join("A.txt"), "base").unwrap();
        let lido = read(&root, "A.txt").unwrap();
        std::fs::write(root.join("A.txt"), "de outro programa").unwrap();
        let err = write_checked(&root, "A.txt", "minha versão", &lido.text, lido.bom).unwrap_err();
        assert!(err.contains("foi alterado"), "{err}");
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "de outro programa");
    }

    #[test]
    fn gravacao_verificada_preserva_bom_e_fim_de_linha() {
        let root = &fixture("verifica-bom").0;
        std::fs::write(root.join("A.txt"), b"\xEF\xBB\xBFa\r\nb\r\n").unwrap();
        let lido = read(&root, "A.txt").unwrap();
        // O BOM continua e o fim de linha é o do texto mandado: o editor já manda o do disco.
        write_checked(&root, "A.txt", "c\r\nd\r\n", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"\xEF\xBB\xBFc\r\nd\r\n");
        // Depois do Formatar Documento com LF, o LF vai ao disco, e a gravação seguinte parte dele sem acusar alteração.
        let lido = read(&root, "A.txt").unwrap();
        write_checked(&root, "A.txt", "e\nf\n", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"\xEF\xBB\xBFe\nf\n");
        write_checked(&root, "A.txt", "g\n", "e\nf\n", true).unwrap();
        // Arquivo sem BOM e com LF continua assim.
        std::fs::write(root.join("b.txt"), "x\ny\n").unwrap();
        let lido = read(&root, "b.txt").unwrap();
        write_checked(&root, "b.txt", "z\nw\n", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"z\nw\n");
        // Gravar de novo a partir da leitura nova funciona.
        let lido = read(&root, "b.txt").unwrap();
        write_checked(&root, "b.txt", "fim", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"fim");
    }

    #[test]
    fn arquivo_absoluto_le_grava_com_revisao_bom_limite_e_criacao() {
        let root = &fixture("absoluto").0;
        let file = root.join("fora.txt");
        std::fs::write(&file, b"\xEF\xBB\xBFantes\r\nlinha\r\n").unwrap();
        let lido = read_at(&file).unwrap();
        assert!(lido.bom);
        assert_eq!(lido.text, "antes\r\nlinha\r\n");
        write_checked_at(&file, "depois\r\nfim\r\n", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"\xEF\xBB\xBFdepois\r\nfim\r\n");

        let lido = read_at(&file).unwrap();
        std::fs::write(&file, "mudou fora").unwrap();
        assert!(write_checked_at(&file, "minha versão", &lido.text, lido.bom).unwrap_err().contains("foi alterado"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "mudou fora");

        let novo = root.join("novo.txt");
        write_checked_at(&novo, "novo", "", false).unwrap();
        assert_eq!(std::fs::read_to_string(&novo).unwrap(), "novo");

        let grande = root.join("grande.txt");
        std::fs::write(&grande, vec![b'x'; MAX_FILE as usize + 1]).unwrap();
        assert!(read_at(&grande).unwrap().too_large);
        let limite = root.join("limite.txt");
        std::fs::write(&limite, vec![b'x'; MAX_FILE as usize]).unwrap();
        assert!(!read_at(&limite).unwrap().too_large);
    }

    #[test]
    fn gravacao_recusa_arquivo_atual_grande_e_nao_cria_arquivo_novo_com_texto_grande() {
        let root = &fixture("grava-grande").0;
        let grande = root.join("grande.txt");
        std::fs::write(&grande, vec![b'x'; MAX_FILE as usize + 1]).unwrap();
        assert!(write_checked_at(&grande, "x", "", false).unwrap_err().contains("ficou grande demais"));
        assert_eq!(std::fs::metadata(&grande).unwrap().len(), MAX_FILE + 1);

        let novo = root.join("novo-grande.txt");
        let texto = "x".repeat(MAX_FILE as usize + 1);
        assert!(write_checked_at(&novo, &texto, "", false).unwrap_err().contains("ficaria grande demais"));
        assert!(!novo.exists());
    }

    #[test]
    fn gravacao_verificada_recusa_bom_trocado_e_arquivo_apagado() {
        let root = &fixture("verifica-bom-troca").0;
        std::fs::write(root.join("A.txt"), b"\xEF\xBB\xBFantes").unwrap();
        assert!(write_checked(&root, "A.txt", "depois", "antes", false).unwrap_err().contains("alterado"));
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"\xEF\xBB\xBFantes");
        std::fs::remove_file(root.join("A.txt")).unwrap();
        assert!(write_checked(&root, "A.txt", "x", "antes", true).is_err());
        assert!(!root.join("A.txt").exists());
    }

    #[cfg(windows)]
    #[test]
    fn gravacao_verificada_recusa_link_no_lugar_do_arquivo() {
        let root = &fixture("verifica-link").0;
        std::fs::write(root.join("sub/x.md"), "alvo").unwrap();
        // Criar link simbólico exige privilégio no Windows; sem ele, o teste só não roda esta parte.
        if crate::platform::symlink_arquivo(&root.join("sub/x.md"), &root.join("link.md")).is_err() {
            eprintln!("sem privilégio para link simbólico: teste pulado");
            return;
        }
        assert!(write_checked(&root, "link.md", "novo", "alvo", false).unwrap_err().contains("é um link"));
        assert_eq!(std::fs::read_to_string(root.join("sub/x.md")).unwrap(), "alvo");
    }

    #[test]
    fn gravacao_verificada_recusa_utf8_invalido_binario_e_caminhos_ruins() {
        let root = &fixture("verifica-recusas").0;
        std::fs::write(root.join("A.txt"), [b'a', 0xFF, b'b']).unwrap();
        std::fs::write(root.join("bin.dat"), [b'a', 0, b'b']).unwrap();
        assert!(write_checked(&root, "A.txt", "x", "a", false).unwrap_err().contains("UTF-8"));
        assert!(write_checked(&root, "bin.dat", "x", "", false).unwrap_err().contains("binário"));
        for rel in ["../novo.txt", ".git/config", "sub", "nao-existe.txt", "sub\\x.md", ""] {
            assert!(write_checked(&root, rel, "x", "", false).is_err(), "{rel} deveria ser recusado");
        }
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), [b'a', 0xFF, b'b']);
        assert!(!root.join("novo.txt").exists() && !root.join("nao-existe.txt").exists());
    }

    /// Enquanto a gravação segura o arquivo, outro programa que tenta gravar nele leva erro, e a gravação dela própria também.
    #[test]
    fn enquanto_a_gravacao_segura_o_arquivo_outro_gravador_nao_entra() {
        let root = &fixture("verifica-trava").0;
        std::fs::write(root.join("A.txt"), "base").unwrap();
        let held = hold(&root, "A.txt").unwrap();
        assert!(std::fs::OpenOptions::new().write(true).open(root.join("A.txt")).is_err(), "outro gravador entrou com o arquivo travado");
        assert!(write_checked(&root, "A.txt", "novo", "base", false).unwrap_err().contains("gravando"));
        drop(held);
        write_checked(&root, "A.txt", "novo", "base", false).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "novo");
    }
}
