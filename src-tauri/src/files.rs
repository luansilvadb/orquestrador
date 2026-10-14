//! O explorador de arquivos do projeto: lista pastas, lê e grava conteúdo de arquivos, e renomeia, manda à Lixeira
//! e mostra no Explorer do Windows. Toda ação recebe o caminho relativo ao projeto e recusa o que sai dele.
use crate::clipboard::Snapshot;
use crate::proc::quiet;
use serde::Serialize;
use std::collections::HashSet;
use std::ffi::c_void;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use windows_sys::core::{GUID, HRESULT};
use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_SHARING_VIOLATION};
use windows_sys::Win32::Storage::FileSystem::{MoveFileExW, FILE_SHARE_READ};
use windows_sys::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
use windows_sys::Win32::UI::Shell::{
    SHCreateItemFromParsingName, FOFX_RECYCLEONDELETE, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_RENAMEONCOLLISION,
    FOF_SILENT,
};

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

/// Uma entrada da pasta listada: só o nome, o caminho é o da pasta pedida mais esse nome.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
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
            Some(Entry { name, dir })
        })
        .collect();
    entries.sort_by_key(|e| (!e.dir, e.name.to_lowercase()));
    Ok(entries)
}

/// O arquivo `rel` do projeto (com barras "/"). Arquivo grande ou binário volta sem `text`.
pub fn read(project: &Path, rel: &str) -> Result<Content, String> {
    let file = inside(project, rel, false)?;
    let size = std::fs::metadata(&file).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?.len();
    if size > MAX_FILE {
        return Ok(Content { binary: false, too_large: true, read_only: false, bom: false, text: String::new() });
    }
    let bytes = std::fs::read(&file).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?;
    if is_binary(&bytes) {
        return Ok(Content { binary: true, too_large: false, read_only: false, bom: false, text: String::new() });
    }
    let text = bytes.strip_prefix(&BOM).unwrap_or(&bytes[..]);
    Ok(Content { binary: false, too_large: false, read_only: std::str::from_utf8(text).is_err(), bom: text.len() != bytes.len(), text: String::from_utf8_lossy(text).into_owned() })
}

/// Grava `text` em `rel` só se o arquivo ainda é `base` (o texto que a tela leu) e ainda tem o mesmo BOM (`base_bom`).
/// Se mudou, sumiu, virou link, mudou de lugar, não é UTF-8 ou é binário, recusa e não grava nada. A decisão é deste
/// backend, não da tela.
///
/// Durante a comparação e a escrita o arquivo fica aberto sem `FILE_SHARE_WRITE` (ver `hold`): outro programa não
/// consegue gravar nele no meio. A janela residual é a de quem já tinha o arquivo aberto para gravar antes desta chamada.
/// Se `base` usa só CRLF e `text` veio só com LF (o textarea normaliza), `text` volta a ter CRLF. Não há gravação atômica:
/// se a escrita falha no meio, o erro avisa que o arquivo pode ter ficado parcial.
pub fn write_checked(project: &Path, rel: &str, text: &str, base: &str, base_bom: bool) -> Result<(), String> {
    let text = restore_eol(text, base);
    if text.len() as u64 > MAX_FILE {
        return Err("O arquivo ficaria grande demais para abrir aqui.".into());
    }
    let mut file = hold(project, rel)?;
    let mut current = Vec::new();
    file.seek(SeekFrom::Start(0)).and_then(|_| file.read_to_end(&mut current)).map_err(|e| format!("Não foi possível ler o arquivo: {e}"))?;
    if current.len() as u64 > MAX_FILE {
        return Err("O arquivo ficou grande demais para gravar aqui. Nada foi gravado.".into());
    }
    if is_binary(&current) {
        return Err("Não é possível gravar um arquivo binário. Nada foi gravado.".into());
    }
    let (bom, content) = match current.strip_prefix(&BOM) {
        Some(content) => (true, content),
        None => (false, current.as_slice()),
    };
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
    let file = OpenOptions::new().read(true).write(true).share_mode(FILE_SHARE_READ).open(&lexical).map_err(open_err)?;
    let link = std::fs::symlink_metadata(&lexical).is_ok_and(|m| m.file_type().is_symlink());
    if link || lexical.canonicalize().ok().as_deref() != Some(real.as_path()) {
        return Err(format!("{rel} mudou de lugar desde a validação. Nada foi gravado."));
    }
    Ok(file)
}

/// Mensagem para quem não conseguiu abrir o arquivo para gravar. Outro programa com o arquivo aberto para gravar cai aqui.
fn open_err(e: std::io::Error) -> String {
    match e.raw_os_error().map(|c| c as u32) {
        Some(ERROR_SHARING_VIOLATION) => "Outro programa está gravando este arquivo agora. Nada foi gravado; tente de novo.".into(),
        _ if e.kind() == std::io::ErrorKind::NotFound => "O arquivo não existe mais. Nada foi gravado.".into(),
        _ => format!("Não foi possível abrir o arquivo para gravar: {e}"),
    }
}

/// Se `base` tem só CRLF (toda quebra é `\r\n`) e `text` não tem nenhum `\r`, cada `\n` de `text` vira `\r\n`. É o caso do
/// textarea, que devolve só LF. Qualquer outro texto volta igual.
fn restore_eol(text: &str, base: &str) -> String {
    let lf = base.matches('\n').count();
    let crlf = base.matches("\r\n").count();
    if crlf > 0 && crlf == lf && !text.contains('\r') {
        text.replace('\n', "\r\n")
    } else {
        text.to_string()
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

/// Recusas que valem para toda ação sobre arquivo: a pasta `.git` nunca entra, e o caminho sai do projeto só por erro.
/// Caminho com barra invertida é recusado: o contrato é `/`, e o `Path` do Windows também aceita `\`, o que contornaria as checagens.
fn no_git(rel: &str) -> Result<(), String> {
    if rel.contains('\\') {
        return Err(format!("Caminho inválido: {rel}. Use / como separador."));
    }
    if rel.split('/').any(|part| part.eq_ignore_ascii_case(".git")) {
        return Err("A pasta .git não pode ser alterada aqui.".into());
    }
    Ok(())
}

/// Recusa link simbólico ou junction em qualquer parte de `rel`, inclusive pasta do meio. Seguir um desses faria a ação
/// cair em outro lugar, mesmo que o destino esteja dentro do projeto (um alias para `.git`, por exemplo). A raiz do
/// projeto fica de fora: ela pode ser um link legítimo.
fn no_links(project: &Path, rel: &str) -> Result<(), String> {
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
    inside(project, rel, dir)?;
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

/// Muda o nome (ou move) sem sobrescrever: `MoveFileExW` sem `MOVEFILE_REPLACE_EXISTING` falha se o destino existir (inclusive
/// um link). O `std::fs::rename` do Windows sobrescreve, por isso não é usado aqui.
fn move_no_replace(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide = |p: &Path| p.as_os_str().encode_wide().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (src, dst) = (wide(from), wide(to));
    if unsafe { MoveFileExW(src.as_ptr(), dst.as_ptr(), 0) } != 0 {
        return Ok(());
    }
    let err = std::io::Error::last_os_error();
    match err.raw_os_error().map(|c| c as u32) {
        Some(ERROR_ALREADY_EXISTS) | Some(ERROR_FILE_EXISTS) => Err("Já existe um item com este nome na pasta de destino. Nada foi movido.".into()),
        _ => Err(format!("Não foi possível concluir a operação: {err}")),
    }
}

/// Um item do projeto pronto para uma ação: o caminho para o sistema, se é pasta, o `rel` sem barras sobrando e o nome.
struct Item {
    path: PathBuf,
    dir: bool,
    rel: String,
    name: String,
}

/// Um item que foi de `from` para `to` (caminhos relativos ao projeto). Em pasta, o que está dentro acompanha: quem atualiza
/// as abas troca o prefixo `from/` por `to/`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Moved {
    pub from: String,
    pub to: String,
}

/// Falha de uma operação em lote. `done` lista o que já foi feito antes da falha (não é desfeito aqui); `error` diz o motivo.
/// Quando a checagem prévia recusa o lote, `done` vem vazio e nada foi mexido.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Partial<T> {
    pub done: Vec<T>,
    pub error: String,
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

/// Texto do erro de um lote: se alguns itens já foram feitos, diz quantos e que não são desfeitos aqui.
fn partial_text(done: usize, total: usize, msg: &str) -> String {
    if done == 0 {
        msg.to_string()
    } else {
        format!("Feitos {done} de {total} antes da falha; os primeiros não são desfeitos aqui. {msg}")
    }
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
pub fn move_items(project: &Path, rels: &[String], dest: &str) -> Result<Vec<Moved>, Partial<Moved>> {
    let plan = move_plan(project, rels, dest).map_err(|error| Partial { done: Vec::new(), error })?;
    let total = plan.len();
    let mut done = Vec::with_capacity(total);
    for (it, to, to_rel) in plan {
        if let Err(msg) = move_no_replace(&it.path, &to) {
            return Err(Partial { error: partial_text(done.len(), total, &msg), done });
        }
        done.push(Moved { from: it.rel, to: to_rel });
    }
    Ok(done)
}

/// Põe os itens `rels` no clipboard como copiados ou recortados, com a mesma checagem de `selection`. Nada é movido ou
/// apagado aqui: o movimento do recorte acontece quando alguém cola.
pub fn copy_items(project: &Path, rels: &[String], cut: bool) -> Result<(), String> {
    let paths: Vec<PathBuf> = selection(project, rels)?.into_iter().map(|it| it.path).collect();
    crate::clipboard::set_files(&paths, cut)
}

/// Manda os itens `rels` para a Lixeira do Windows (pasta inteira ou arquivo), com a checagem de `selection` antes. Se um
/// falha depois de outros, os que já foram ficam na Lixeira e vêm em `done`.
pub fn recycle_many(project: &Path, rels: &[String]) -> Result<Vec<String>, Partial<String>> {
    let items = selection(project, rels).map_err(|error| Partial { done: Vec::new(), error })?;
    let total = items.len();
    let mut done = Vec::with_capacity(total);
    for it in items {
        if let Err(msg) = recycle_path(&it.path) {
            return Err(Partial { error: partial_text(done.len(), total, &msg), done });
        }
        done.push(it.rel);
    }
    Ok(done)
}

/// Manda o arquivo ou a pasta `rel` para a Lixeira do Windows, pelo `IFileOperation` com `FOFX_RECYCLEONDELETE`: a operação pede
/// reciclagem e não tem o caminho de apagar de vez. Não dá para mudar isso com `SHFileOperationW`, que aceita a exclusão
/// definitiva por diálogo. Só conta como feito se a operação não foi abortada e o arquivo saiu do lugar; senão é erro.
pub fn recycle(project: &Path, rel: &str) -> Result<(), String> {
    recycle_path(&item(project, rel)?.path)
}

/// Manda `file` (arquivo ou pasta já validada) para a Lixeira e confere que saiu do lugar.
fn recycle_path(file: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = file.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let started = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
    if started < 0 {
        return Err("Não foi possível abrir a Lixeira do Windows. Nada foi excluído.".into());
    }
    let result = unsafe { send_to_recycle_bin(&wide) };
    // `S_OK` e `S_FALSE` (já estava inicializado) pedem o `CoUninitialize` de volta.
    unsafe { CoUninitialize() };
    result?;
    if std::fs::symlink_metadata(file).is_ok() {
        return Err("A Lixeira não recebeu o arquivo. Nada foi excluído.".into());
    }
    Ok(())
}

const CLSID_FILE_OPERATION: GUID = GUID { data1: 0x3AD05575, data2: 0x8857, data3: 0x4850, data4: [0x92, 0x77, 0x11, 0xB8, 0x5B, 0xDB, 0x8E, 0x09] };
const IID_IFILE_OPERATION: GUID = GUID { data1: 0x947AAB5F, data2: 0x0A5C, data3: 0x4C13, data4: [0xB4, 0xD6, 0x4B, 0xF7, 0x83, 0x6F, 0xC9, 0xF8] };
const IID_ISHELL_ITEM: GUID = GUID { data1: 0x43826D1E, data2: 0xE718, data3: 0x42EE, data4: [0xBC, 0x55, 0xA1, 0xE2, 0x61, 0xC3, 0x7B, 0xFE] };

/// Vtable de IUnknown: o começo de todo objeto COM. `release` serve também ao `IShellItem` e ao `IFileOperation`.
#[repr(C)]
struct Unknown {
    query_interface: usize,
    add_ref: usize,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

/// Vtable de `IFileOperation` (shobjidl_core.h), na ordem da interface. Só `SetOperationFlags`, `MoveItem`, `CopyItem`,
/// `DeleteItem`, `PerformOperations` e `GetAnyOperationsAborted` são chamados; os demais slots são só espaço, com o mesmo tamanho de um ponteiro.
#[repr(C)]
struct FileOperationVtbl {
    base: Unknown,
    advise: usize,
    unadvise: usize,
    set_operation_flags: unsafe extern "system" fn(*mut c_void, u32) -> HRESULT,
    set_progress_message: usize,
    set_progress_dialog: usize,
    set_properties: usize,
    set_owner_window: usize,
    apply_properties_to_item: usize,
    apply_properties_to_items: usize,
    rename_item: usize,
    rename_items: usize,
    move_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const u16, *mut c_void) -> HRESULT,
    move_items: usize,
    copy_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *const u16, *mut c_void) -> HRESULT,
    copy_items: usize,
    delete_item: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> HRESULT,
    delete_items: usize,
    new_item: usize,
    perform_operations: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_any_operations_aborted: unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT,
}

/// Um ponteiro COM que solta a referência ao sair do escopo.
struct Com(*mut c_void);

impl Drop for Com {
    fn drop(&mut self) {
        // SAFETY: o ponteiro veio de uma chamada COM bem-sucedida e esta struct é a dona da referência.
        unsafe { ((**(self.0 as *const *const Unknown)).release)(self.0) };
    }
}

/// Pede ao Windows para reciclar `wide` (caminho com zero final). Erro se o Windows não criar a operação, não aceitar a
/// reciclagem, abortar ou não executar. Não há caminho de exclusão definitiva.
unsafe fn send_to_recycle_bin(wide: &[u16]) -> Result<(), String> {
    let mut op: *mut c_void = std::ptr::null_mut();
    let hr = CoCreateInstance(&CLSID_FILE_OPERATION, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_IFILE_OPERATION, &mut op);
    if hr < 0 || op.is_null() {
        return Err("Não foi possível abrir a Lixeira do Windows. Nada foi excluído.".into());
    }
    let op = Com(op);
    let mut item: *mut c_void = std::ptr::null_mut();
    let hr = SHCreateItemFromParsingName(wide.as_ptr(), std::ptr::null_mut(), &IID_ISHELL_ITEM, &mut item);
    if hr < 0 || item.is_null() {
        return Err("Não foi possível localizar o arquivo para a Lixeira. Nada foi excluído.".into());
    }
    let item = Com(item);
    let vtbl = &**(op.0 as *const *const FileOperationVtbl);
    let flags = FOFX_RECYCLEONDELETE | FOF_ALLOWUNDO | FOF_SILENT | FOF_NOERRORUI;
    let mut hr = (vtbl.set_operation_flags)(op.0, flags);
    if hr >= 0 {
        hr = (vtbl.delete_item)(op.0, item.0, std::ptr::null_mut());
    }
    if hr >= 0 {
        hr = (vtbl.perform_operations)(op.0);
    }
    let mut aborted = 0;
    let aborted_hr = (vtbl.get_any_operations_aborted)(op.0, &mut aborted);
    if hr < 0 || aborted_hr < 0 || aborted != 0 {
        return Err("A Lixeira não recebeu o arquivo. Nada foi excluído.".into());
    }
    Ok(())
}

/// Abre `programa` sem janela com `arg` como linha de comando crua. Se não abrir, `falha` vira o começo da mensagem.
/// O código de saída não é lido: o Explorer sai com 1 mesmo quando funciona.
fn abrir_programa(programa: &str, arg: String, falha: &str) -> Result<(), String> {
    quiet(programa).raw_arg(arg).spawn().map(drop).map_err(|e| format!("{falha}: {e}"))
}

/// Abre o Explorer do Windows com o arquivo selecionado.
pub fn reveal(project: &Path, rel: &str) -> Result<(), String> {
    let path = plain(project, rel, false)?;
    abrir_programa("explorer.exe", format!("/select,\"{}\"", path.display()), "Não foi possível abrir o Explorer")
}

/// Abre o diálogo "Como você quer abrir este arquivo?" do Windows, o mesmo do Abrir com... do Explorer.
pub fn open_with(project: &Path, rel: &str) -> Result<(), String> {
    let path = plain(project, rel, false)?;
    abrir_programa("rundll32.exe", format!("shell32.dll,OpenAs_RunDLL \"{}\"", path.display()), "Não foi possível abrir o diálogo do Windows")
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
    abrir_programa("explorer.exe", format!("\"{}\"", path.display()), "Não foi possível abrir o Explorer")
}

/// Uma colagem feita. `from` é o caminho absoluto que estava no clipboard, como o Windows o deu; `to` é o caminho
/// relativo ao projeto do que foi criado (com o nome que ganhou, se houve conflito).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pasted {
    pub from: String,
    pub to: String,
}

/// Cola em `dest` (pasta do projeto, vazio é a raiz) os itens de `snap`: copia, ou move se `snap.cut`. `Ok` traz cada item
/// colado; `Err` traz os que já foram antes da falha em `done` (não são desfeitos aqui) e a mensagem de `error`. Tudo é
/// checado antes de começar: se um item é recusado, nada é colado. Nome que já existe vira "nome (2).ext", "nome (3).ext"...;
/// nada existente é sobrescrito. A cópia e o movimento são do IFileOperation do Windows, que copia pasta inteira e move
/// entre volumes. Recusa antes de mexer se um item está dentro de outro da mesma colagem, ou se um aparece duas vezes:
/// a cópia ou o recorte falharia no meio sem motivo.
pub fn paste_detailed(project: &Path, dest: &str, snap: &Snapshot) -> Result<Vec<Pasted>, Partial<Pasted>> {
    let fail = |error: String| Partial { done: Vec::new(), error };
    if snap.paths.is_empty() {
        return Err(fail("Não há arquivo ou pasta copiado para colar.".into()));
    }
    check_overlap(&snap.paths).map_err(fail)?;
    let parent = plain(project, dest, true).map_err(fail)?;
    let parent_canon = parent.canonicalize().map_err(|e| fail(format!("Não foi possível abrir a pasta de destino: {e}")))?;
    let mut taken = HashSet::new();
    let mut plan = Vec::with_capacity(snap.paths.len());
    for src in &snap.paths {
        plan.push(check_source(src, &parent, &parent_canon, snap.cut, &mut taken).map_err(fail)?);
    }
    let base = dest.trim_end_matches('/');
    let total = plan.len();
    let mut done = Vec::with_capacity(total);
    for (i, (src, name)) in plan.iter().enumerate() {
        if let Err(msg) = transfer(src, &parent, name, snap.cut) {
            let error = if i == 0 { msg } else { format!("Colados {i} de {total}; os primeiros já estão na pasta de destino. {msg}") };
            return Err(Partial { done, error });
        }
        let to = if base.is_empty() { name.clone() } else { format!("{base}/{name}") };
        done.push(Pasted { from: src.display().to_string(), to });
    }
    Ok(done)
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
    let result = unsafe { run_transfer(src, parent, name, cut) };
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

/// Um `IShellItem` para o caminho `path` do sistema de arquivos. O Shell recusa `/`, então as barras viram `\` aqui.
unsafe fn shell_item(path: &Path) -> Result<Com, String> {
    let mut wide = wide(path.as_os_str());
    for unit in wide.iter_mut().filter(|u| **u == b'/' as u16) {
        *unit = b'\\' as u16;
    }
    let mut item: *mut c_void = std::ptr::null_mut();
    let hr = SHCreateItemFromParsingName(wide.as_ptr(), std::ptr::null_mut(), &IID_ISHELL_ITEM, &mut item);
    if hr < 0 || item.is_null() {
        return Err("Não foi possível localizar o item para colar. Nada foi colado.".into());
    }
    Ok(Com(item))
}

/// Caminho com zero final, para as APIs do Windows.
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

/// Inicializa o COM na thread que chama e desfaz ao sair, inclusive quando a chamada já o tinha inicializado.
struct Apartment;

impl Apartment {
    fn enter() -> Result<Apartment, String> {
        // SAFETY: COM é inicializado na própria thread, e o Drop chama `CoUninitialize` para cada chamada bem-sucedida.
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) };
        if hr < 0 {
            return Err("Não foi possível iniciar o COM do Windows. Nada foi colado.".into());
        }
        Ok(Apartment)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: par do `CoInitializeEx` de `enter`.
        unsafe { CoUninitialize() };
    }
}

/// Chama o IFileOperation para copiar ou mover `src` para dentro de `parent` com o nome `name`. Sem confirmação e com
/// renomeação em conflito: nada existente é sobrescrito; um conflito que apareça na hora ganha outro nome (o `transfer` confere).
unsafe fn run_transfer(src: &Path, parent: &Path, name: &str, cut: bool) -> Result<(), String> {
    let _apartment = Apartment::enter()?;
    let mut op: *mut c_void = std::ptr::null_mut();
    let hr = CoCreateInstance(&CLSID_FILE_OPERATION, std::ptr::null_mut(), CLSCTX_INPROC_SERVER, &IID_IFILE_OPERATION, &mut op);
    if hr < 0 || op.is_null() {
        return Err("Não foi possível iniciar a cópia do Windows. Nada foi colado.".into());
    }
    let op = Com(op);
    let item = shell_item(src)?;
    let folder = shell_item(parent)?;
    let name_w = wide(std::ffi::OsStr::new(name));
    let vtbl = &**(op.0 as *const *const FileOperationVtbl);
    let flags = FOF_RENAMEONCOLLISION | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI;
    let mut hr = (vtbl.set_operation_flags)(op.0, flags);
    if hr >= 0 {
        hr = if cut {
            (vtbl.move_item)(op.0, item.0, folder.0, name_w.as_ptr(), std::ptr::null_mut())
        } else {
            (vtbl.copy_item)(op.0, item.0, folder.0, name_w.as_ptr(), std::ptr::null_mut())
        };
    }
    if hr >= 0 {
        hr = (vtbl.perform_operations)(op.0);
    }
    let mut aborted = 0;
    let aborted_hr = (vtbl.get_any_operations_aborted)(op.0, &mut aborted);
    if hr < 0 || aborted_hr < 0 || aborted != 0 {
        return Err("O Windows não concluiu a operação.".into());
    }
    Ok(())
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

    fn e(name: &str, dir: bool) -> Entry {
        Entry { name: name.into(), dir }
    }

    /// Um projeto de teste: pastas e arquivos com nomes que misturam caixa, mais um `.git` na raiz e outro na subpasta.
    fn fixture(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("orq-files-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["Zeta", "alfa", "Beta", ".git", "sub/.git", "sub/dentro"] {
            std::fs::create_dir_all(root.join(dir)).unwrap();
        }
        for file in ["b.txt", "A.txt", ".env", "sub/x.md"] {
            std::fs::write(root.join(file), "").unwrap();
        }
        root
    }

    #[test]
    fn pastas_primeiro_e_alfabetico_sem_diferenciar_caixa() {
        let root = fixture("ordem");
        let got = list(&root, "").unwrap();
        assert_eq!(got, vec![e("alfa", true), e("Beta", true), e("sub", true), e("Zeta", true), e(".env", false), e("A.txt", false), e("b.txt", false)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn git_fica_fora_da_raiz_e_aparece_na_subpasta() {
        let root = fixture("git");
        assert!(!list(&root, "").unwrap().iter().any(|x| x.name == ".git"));
        assert_eq!(list(&root, "sub").unwrap(), vec![e(".git", true), e("dentro", true), e("x.md", false)]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lista_so_um_nivel_da_subpasta() {
        let root = fixture("sub");
        assert_eq!(list(&root, "sub/dentro").unwrap(), vec![]);
        assert_eq!(list(&root, "Zeta").unwrap(), vec![]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recusa_caminho_que_sai_da_pasta_ou_e_absoluto() {
        let root = fixture("fora");
        for rel in ["..", "../x", "sub/../../x", "/", "/etc", "C:/", "\\\\servidor\\x", "./sub", "sub/.."] {
            let got = list(&root, rel);
            assert!(got.is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = root.to_string_lossy().into_owned();
        assert_eq!(list(&root, &absoluto).unwrap_err(), format!("Caminho fora do projeto: {absoluto}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn pasta_inexistente_e_arquivo_sao_recusados_em_portugues() {
        let root = fixture("inexistente");
        assert_eq!(list(&root, "nao-existe").unwrap_err(), "Pasta não encontrada: nao-existe");
        assert_eq!(list(&root, "sub/x.md").unwrap_err(), "Não é uma pasta: sub/x.md");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn texto_com_acento_e_crlf_volta_igual_e_sem_bom() {
        let root = fixture("texto");
        std::fs::write(root.join("sub/nota.md"), "\u{feff}Olá\r\nmundo\r\n").unwrap();
        assert_eq!(read(&root, "sub/nota.md").unwrap(), Content { binary: false, too_large: false, read_only: false, bom: true, text: "Olá\r\nmundo\r\n".into() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_vazio_devolve_texto_vazio() {
        let root = fixture("vazio");
        assert_eq!(read(&root, "A.txt").unwrap(), Content { binary: false, too_large: false, read_only: false, bom: false, text: String::new() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_com_byte_nulo_e_binario() {
        let root = fixture("binario");
        std::fs::write(root.join("img.bin"), [0x89, b'P', 0, b'N', b'G']).unwrap();
        assert_eq!(read(&root, "img.bin").unwrap(), Content { binary: true, too_large: false, read_only: false, bom: false, text: String::new() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_com_mais_de_1_mib_volta_too_large() {
        let root = fixture("grande");
        std::fs::write(root.join("grande.txt"), vec![b'a'; 1_048_577]).unwrap();
        assert_eq!(read(&root, "grande.txt").unwrap(), Content { binary: false, too_large: true, read_only: false, bom: false, text: String::new() });
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn recusa_caminho_de_leitura_que_sai_da_pasta_ou_e_absoluto() {
        let root = fixture("ler-fora");
        for rel in ["..", "../x", "sub/../../x", "/etc", "C:/", "./sub/x.md"] {
            let got = read(&root, rel);
            assert!(got.is_err(), "{rel} deveria ser recusado");
        }
        let absoluto = root.join("sub/x.md").to_string_lossy().into_owned();
        assert_eq!(read(&root, &absoluto).unwrap_err(), format!("Caminho fora do projeto: {absoluto}"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn arquivo_inexistente_e_pasta_sao_recusados_em_portugues() {
        let root = fixture("ler-inexistente");
        assert_eq!(read(&root, "nao-existe.md").unwrap_err(), "Arquivo não encontrado: nao-existe.md");
        assert_eq!(read(&root, "sub").unwrap_err(), "Não é um arquivo: sub");
        let _ = std::fs::remove_dir_all(&root);
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
        let root = fixture("renomeia");
        assert_eq!(rename(&root, "sub/x.md", "y.md").unwrap(), "sub/y.md");
        assert!(root.join("sub/y.md").exists());
        assert!(!root.join("sub/x.md").exists());
        assert_eq!(rename(&root, "b.txt", "novo.txt").unwrap(), "novo.txt");
        assert!(root.join("novo.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn colisao_nao_sobrescreve_e_deixa_os_dois_arquivos() {
        let root = fixture("colisao");
        std::fs::write(root.join("A.txt"), "primeiro").unwrap();
        std::fs::write(root.join("b.txt"), "segundo").unwrap();
        assert_eq!(rename(&root, "b.txt", "A.txt").unwrap_err(), "Já existe um item chamado “A.txt” nesta pasta.");
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "primeiro");
        assert_eq!(std::fs::read_to_string(root.join("b.txt")).unwrap(), "segundo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn troca_so_de_maiusculas_funciona_e_nome_igual_nao_faz_nada() {
        let root = fixture("caixa");
        assert_eq!(rename(&root, "b.txt", "B.txt").unwrap(), "B.txt");
        assert!(std::fs::read_dir(&root).unwrap().flatten().any(|e| e.file_name() == "B.txt"));
        assert_eq!(rename(&root, "B.txt", "B.txt").unwrap(), "B.txt");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn link_quebrado_ocupa_o_nome_e_link_nao_e_renomeado() {
        let root = fixture("links");
        // Criar link simbólico exige privilégio no Windows; sem ele, o teste só não roda esta parte.
        if std::os::windows::fs::symlink_file(root.join("sub/x.md"), root.join("link.md")).is_err() {
            eprintln!("sem privilégio para link simbólico: teste de links pulado");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        std::os::windows::fs::symlink_file(root.join("nao-existe.md"), root.join("quebrado.md")).unwrap();
        assert!(rename(&root, "b.txt", "quebrado.md").unwrap_err().starts_with("Já existe"));
        assert!(rename(&root, "link.md", "outro.md").unwrap_err().contains("é um link"));
        assert!(rename(&root, "link.md", "outro.md").is_err());
        assert!(root.join("sub/x.md").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn git_pastas_e_caminhos_de_fora_nao_entram_nas_acoes() {
        let root = fixture("acoes-recusas");
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
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn junction_no_meio_do_caminho_nao_entra_nas_acoes() {
        let root = fixture("junction");
        std::fs::write(root.join(".git/config"), "").unwrap();
        // Junction não pede privilégio. Se o `mklink` não criar, o teste só não roda esta parte.
        let junction = |link: &Path, target: &Path| {
            std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output().is_ok_and(|o| o.status.success())
        };
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
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn barra_invertida_nao_contorna_git_nem_links() {
        let root = fixture("barra-invertida");
        for rel in [".git\\config", "sub\\arquivo", "alias\\arquivo.txt"] {
            assert!(rename(&root, rel, "x").unwrap_err().starts_with("Caminho inválido"), "{rel}");
            assert!(recycle(&root, rel).unwrap_err().starts_with("Caminho inválido"), "{rel}");
            assert!(plain(&root, rel, false).unwrap_err().starts_with("Caminho inválido"), "{rel}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Manda para a Lixeira só um arquivo temporário criado aqui: o arquivo do projeto nunca entra.
    #[test]
    fn arquivo_temporario_vai_para_a_lixeira_e_sai_do_lugar() {
        let root = fixture("lixeira");
        let file = root.join("sub/descartavel.txt");
        std::fs::write(&file, "temporário").unwrap();
        assert_eq!(recycle(&root, "sub/descartavel.txt"), Ok(()));
        assert!(std::fs::symlink_metadata(&file).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Uma pasta fora do projeto, de onde vêm os itens copiados. Só existe dentro do temporário.
    fn outside(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orq-files-fora-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Junction de teste (não pede privilégio). Diz se foi criada; sem ela, o teste pula a parte dos links.
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(link).arg(target).output().is_ok_and(|o| o.status.success())
    }

    /// Cola como `paste_detailed`, devolvendo só os caminhos criados ou o erro. Só os testes usam esta forma.
    fn paste(project: &Path, dest: &str, snap: &Snapshot) -> Result<Vec<String>, String> {
        paste_detailed(project, dest, snap).map(|pasted| pasted.into_iter().map(|p| p.to).collect()).map_err(|p| p.error)
    }

    fn snap(paths: &[PathBuf], cut: bool) -> crate::clipboard::Snapshot {
        crate::clipboard::Snapshot { paths: paths.to_vec(), cut }
    }

    #[test]
    fn cria_arquivo_vazio_na_raiz_e_na_subpasta() {
        let root = fixture("cria-arquivo");
        assert_eq!(create_file(&root, "", "novo.txt").unwrap(), "novo.txt");
        assert_eq!(create_file(&root, "sub", "dentro.md").unwrap(), "sub/dentro.md");
        assert_eq!(std::fs::metadata(root.join("novo.txt")).unwrap().len(), 0);
        assert_eq!(std::fs::metadata(root.join("sub/dentro.md")).unwrap().len(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cria_pasta_na_raiz_e_na_subpasta() {
        let root = fixture("cria-pasta");
        assert_eq!(create_dir(&root, "", "nova").unwrap(), "nova");
        assert_eq!(create_dir(&root, "sub/", "nova").unwrap(), "sub/nova");
        assert!(root.join("nova").is_dir());
        assert!(root.join("sub/nova").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn criacao_nao_sobrescreve_arquivo_nem_pasta_existente() {
        let root = fixture("cria-colisao");
        std::fs::write(root.join("A.txt"), "original").unwrap();
        assert_eq!(create_file(&root, "", "A.txt").unwrap_err(), "Já existe um item chamado “A.txt” nesta pasta. Nada foi criado.");
        assert_eq!(create_file(&root, "", "a.txt").unwrap_err(), "Já existe um item chamado “a.txt” nesta pasta. Nada foi criado.");
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "original");
        assert!(create_file(&root, "", "alfa").is_err());
        assert!(create_dir(&root, "", "Zeta").is_err());
        assert!(create_dir(&root, "", "A.txt").is_err());
        assert!(root.join("Zeta").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn criacao_nao_sobrescreve_link_quebrado() {
        let root = fixture("cria-quebrado");
        // Criar link simbólico exige privilégio no Windows; sem ele, o teste só não roda esta parte.
        if std::os::windows::fs::symlink_file(root.join("nao-existe.md"), root.join("quebrado.md")).is_err() {
            eprintln!("sem privilégio para link simbólico: teste de link quebrado pulado");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        assert!(create_file(&root, "", "quebrado.md").is_err());
        assert!(create_dir(&root, "", "quebrado.md").is_err());
        assert!(std::fs::symlink_metadata(root.join("quebrado.md")).unwrap().file_type().is_symlink());
        assert!(!root.join("nao-existe.md").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn criacao_recusa_nome_invalido_git_e_caminho_que_sai() {
        let root = fixture("cria-recusas");
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
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn criacao_recusa_pasta_que_e_junction_para_fora_ou_para_git() {
        let root = fixture("cria-junction");
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
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn revelar_pasta_aceita_raiz_e_subpasta_e_recusa_git_e_fora() {
        let root = fixture("revela");
        assert_eq!(plain(&root, "", true).unwrap(), root);
        assert_eq!(plain(&root, "sub/dentro", true).unwrap(), root.join("sub").join("dentro"));
        for rel in [".git", "sub/.git", "..", "../x", "/", "b.txt"] {
            assert!(plain(&root, rel, true).is_err(), "{rel} deveria ser recusado");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copia_arquivo_de_fora_para_subpasta_e_mantem_o_original() {
        let root = fixture("cola-copia");
        let ext = outside("cola-copia");
        std::fs::write(ext.join("de-fora.txt"), "conteúdo").unwrap();
        assert_eq!(paste(&root, "sub", &snap(&[ext.join("de-fora.txt")], false)).unwrap(), vec!["sub/de-fora.txt"]);
        assert_eq!(std::fs::read_to_string(root.join("sub/de-fora.txt")).unwrap(), "conteúdo");
        assert_eq!(std::fs::read_to_string(ext.join("de-fora.txt")).unwrap(), "conteúdo");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn recorte_de_arquivo_move_e_tira_da_origem() {
        let root = fixture("cola-recorte");
        std::fs::write(root.join("b.txt"), "movido").unwrap();
        assert_eq!(paste(&root, "sub", &snap(&[root.join("b.txt")], true)).unwrap(), vec!["sub/b.txt"]);
        assert_eq!(std::fs::read_to_string(root.join("sub/b.txt")).unwrap(), "movido");
        assert!(std::fs::symlink_metadata(root.join("b.txt")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn copia_pasta_inteira_com_conteudo_e_mantem_a_origem() {
        let root = fixture("cola-pasta");
        let ext = outside("cola-pasta");
        std::fs::create_dir_all(ext.join("pasta/deep")).unwrap();
        std::fs::write(ext.join("pasta/a.txt"), "a").unwrap();
        std::fs::write(ext.join("pasta/deep/b.txt"), "bê").unwrap();
        assert_eq!(paste(&root, "", &snap(&[ext.join("pasta")], false)).unwrap(), vec!["pasta"]);
        assert_eq!(std::fs::read_to_string(root.join("pasta/a.txt")).unwrap(), "a");
        assert_eq!(std::fs::read_to_string(root.join("pasta/deep/b.txt")).unwrap(), "bê");
        assert_eq!(std::fs::read_to_string(ext.join("pasta/deep/b.txt")).unwrap(), "bê");
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn recorte_de_pasta_move_o_conteudo_inteiro() {
        let root = fixture("cola-recorte-pasta");
        std::fs::write(root.join("sub/dentro/f.txt"), "f").unwrap();
        assert_eq!(paste(&root, "Beta", &snap(&[root.join("sub/dentro")], true)).unwrap(), vec!["Beta/dentro"]);
        assert_eq!(std::fs::read_to_string(root.join("Beta/dentro/f.txt")).unwrap(), "f");
        assert!(std::fs::symlink_metadata(root.join("sub/dentro")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn colisao_ganha_nome_novo_e_o_existente_fica_igual() {
        let root = fixture("cola-colisao");
        let ext = outside("cola-colisao");
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
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn auto_colagem_e_recorte_na_mesma_pasta_sao_recusados() {
        let root = fixture("cola-auto");
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
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn um_item_recusado_faz_nada_ser_colado() {
        let root = fixture("cola-previa");
        let ext = outside("cola-previa");
        std::fs::write(root.join(".git/config"), "").unwrap();
        std::fs::write(ext.join("ok.txt"), "ok").unwrap();
        let err = paste(&root, "sub", &snap(&[ext.join("ok.txt"), root.join(".git/config")], false)).unwrap_err();
        assert!(err.contains(".git"), "{err}");
        assert!(!root.join("sub/ok.txt").exists());
        assert!(paste(&root, "sub", &snap(&[], false)).is_err());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn colar_em_git_ou_de_git_e_recusado() {
        let root = fixture("cola-git");
        let ext = outside("cola-git");
        std::fs::write(ext.join("x.txt"), "x").unwrap();
        for dest in [".git", "sub/.git", "../fora", "/"] {
            assert!(paste(&root, dest, &snap(&[ext.join("x.txt")], false)).is_err(), "{dest}");
        }
        std::fs::create_dir_all(ext.join("repo/.git")).unwrap();
        assert!(paste(&root, "", &snap(&[ext.join("repo")], false)).unwrap_err().contains(".git"));
        assert!(paste(&root, "", &snap(&[root.join(".git/config")], false)).is_err());
        assert!(!root.join("x.txt").exists());
        assert!(!root.join("repo").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn colar_item_que_some_ou_que_e_junction_e_recusado() {
        let root = fixture("cola-junction");
        let ext = outside("cola-junction");
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
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn paste_detailed_devolve_origem_absoluta_e_destino_relativo() {
        let root = fixture("detalhe-ok");
        let ext = outside("detalhe-ok");
        std::fs::write(ext.join("A.txt"), "a").unwrap();
        std::fs::write(root.join("A.txt"), "ja existe").unwrap();
        let sources = [ext.join("A.txt")];
        let done = paste_detailed(&root, "sub", &snap(&sources, false)).unwrap();
        assert_eq!(done, vec![Pasted { from: ext.join("A.txt").display().to_string(), to: "sub/A.txt".into() }]);
        // A API antiga continua dando só os destinos, com o mesmo nome.
        assert_eq!(paste(&root, "", &snap(&sources, false)).unwrap(), vec!["A (2).txt"]);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
    }

    #[test]
    fn paste_detailed_recusa_pasta_com_o_proprio_filho_antes_de_mexer() {
        let root = fixture("detalhe-filho");
        let ext = outside("detalhe-filho");
        std::fs::create_dir_all(ext.join("pasta")).unwrap();
        std::fs::write(ext.join("pasta/f.txt"), "f").unwrap();
        let sources = [ext.join("pasta"), ext.join("pasta/f.txt")];
        let err = paste_detailed(&root, "", &snap(&sources, true)).unwrap_err();
        assert!(err.done.is_empty());
        assert!(err.error.contains("dentro de “pasta”"), "{}", err.error);
        assert!(ext.join("pasta/f.txt").exists(), "nada foi movido");
        let twice = [ext.join("pasta/f.txt"), ext.join("pasta/f.txt")];
        assert!(paste_detailed(&root, "", &snap(&twice, false)).unwrap_err().error.contains("repetido"));
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&ext);
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

    #[test]
    fn renomeia_pasta_e_os_descendentes_seguem() {
        let root = fixture("renomeia-pasta");
        pasta(&root);
        assert_eq!(rename(&root, "origem", "destino").unwrap(), "destino");
        assert_eq!(std::fs::read_to_string(root.join("destino/interna/f.txt")).unwrap(), "f");
        assert!(!root.join("origem").exists());
        assert_eq!(rename(&root, "destino/interna", "dentro").unwrap(), "destino/dentro");
        assert!(root.join("destino/dentro/f.txt").exists());
        assert!(rename(&root, "", "x").is_err());
        assert!(rename(&root, "Zeta", "alfa").unwrap_err().starts_with("Já existe"));
        assert!(root.join("Zeta").is_dir() && root.join("alfa").is_dir());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn move_arquivo_e_pasta_para_outra_pasta_e_devolve_o_mapeamento() {
        let root = fixture("move-ok");
        pasta(&root);
        let got = move_items(&root, &rels(&["origem", "b.txt"]), "Beta").unwrap();
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
        assert_eq!(move_items(&root, &rels(&["Beta/b.txt"]), "").unwrap()[0].to, "b.txt");
        assert!(root.join("b.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Cada caso recusa o lote inteiro, com `done` vazio, e a mensagem diz o motivo. Depois de todos, nada saiu do lugar.
    #[test]
    fn move_recusa_o_lote_inteiro_antes_de_mexer() {
        let root = fixture("move-recusa");
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
            let err = move_items(&root, &rels(srcs), dest).unwrap_err();
            assert!(err.done.is_empty(), "{srcs:?} -> {dest:?} não deveria mexer em nada");
            assert!(err.error.contains(quer), "{srcs:?} -> {dest:?}: {}", err.error);
        }
        assert!(root.join("origem/interna/f.txt").exists() && root.join("origem/g.txt").exists());
        assert!(root.join("b.txt").exists() && root.join("c.txt").exists() && root.join("x/c.txt").exists());
        assert_eq!(std::fs::read_to_string(root.join("Beta/b.txt")).unwrap(), "já estava");
        assert!(!root.join("Beta/c.txt").exists() && !root.join("Beta/origem").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn move_recusa_link_e_junction_no_item_e_no_destino() {
        let root = fixture("move-junction");
        pasta(&root);
        let junction_ok = junction(&root.join("alias"), &root.join("origem"));
        if !junction_ok {
            eprintln!("sem junction: teste pulado");
        } else {
            assert!(move_items(&root, &rels(&["alias"]), "Beta").unwrap_err().error.contains("link"));
            assert!(move_items(&root, &rels(&["alias/g.txt"]), "Beta").unwrap_err().error.contains("link"));
            assert!(move_items(&root, &rels(&["b.txt"]), "alias").unwrap_err().error.contains("link"));
            assert!(root.join("origem/g.txt").exists());
            assert!(!root.join("Beta/g.txt").exists());
            assert!(!root.join("Beta/b.txt").exists());
        }
        let _ = std::fs::remove_dir(root.join("alias"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn lixeira_em_lote_recusa_tudo_antes_de_mexer() {
        let root = fixture("lote-lixeira");
        pasta(&root);
        std::fs::write(root.join(".git/config"), "").unwrap();
        let err = recycle_many(&root, &rels(&["b.txt", ".git/config"])).unwrap_err();
        assert!(err.done.is_empty() && err.error.contains(".git"), "{}", err.error);
        assert!(recycle_many(&root, &rels(&["origem", "origem/interna"])).unwrap_err().error.contains("dentro de"));
        assert!(recycle_many(&root, &rels(&["sub"])).unwrap_err().error.contains(".git"));
        assert!(recycle_many(&root, &rels(&[""])).is_err());
        assert!(recycle_many(&root, &[]).is_err());
        assert!(root.join("b.txt").exists() && root.join(".git/config").exists() && root.join("origem/interna/f.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Vai para a Lixeira só o que foi criado dentro do temporário; a pasta e o arquivo somem da origem.
    #[test]
    fn lixeira_leva_pasta_inteira_e_arquivo_em_lote() {
        let root = fixture("lixeira-pasta");
        std::fs::create_dir_all(root.join("lixo/dentro")).unwrap();
        std::fs::write(root.join("lixo/dentro/f.txt"), "f").unwrap();
        std::fs::write(root.join("descartavel.txt"), "d").unwrap();
        assert_eq!(recycle_many(&root, &rels(&["lixo", "descartavel.txt"])).unwrap(), rels(&["lixo", "descartavel.txt"]));
        assert!(std::fs::symlink_metadata(root.join("lixo")).is_err());
        assert!(std::fs::symlink_metadata(root.join("descartavel.txt")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gravacao_verificada_recusa_mudanca_externa_e_nao_grava() {
        let root = fixture("verifica-externa");
        std::fs::write(root.join("A.txt"), "base").unwrap();
        let lido = read(&root, "A.txt").unwrap();
        std::fs::write(root.join("A.txt"), "de outro programa").unwrap();
        let err = write_checked(&root, "A.txt", "minha versão", &lido.text, lido.bom).unwrap_err();
        assert!(err.contains("foi alterado"), "{err}");
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "de outro programa");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gravacao_verificada_preserva_bom_e_fim_de_linha() {
        let root = fixture("verifica-bom");
        std::fs::write(root.join("A.txt"), b"\xEF\xBB\xBFa\r\nb\r\n").unwrap();
        let lido = read(&root, "A.txt").unwrap();
        // O textarea devolve só LF: o CRLF do arquivo volta, e o BOM continua.
        write_checked(&root, "A.txt", "c\nd\n", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"\xEF\xBB\xBFc\r\nd\r\n");
        // Arquivo sem BOM e com LF continua assim.
        std::fs::write(root.join("b.txt"), "x\ny\n").unwrap();
        let lido = read(&root, "b.txt").unwrap();
        write_checked(&root, "b.txt", "z\nw\n", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"z\nw\n");
        // Gravar de novo a partir da leitura nova funciona.
        let lido = read(&root, "b.txt").unwrap();
        write_checked(&root, "b.txt", "fim", &lido.text, lido.bom).unwrap();
        assert_eq!(std::fs::read(root.join("b.txt")).unwrap(), b"fim");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gravacao_verificada_recusa_bom_trocado_e_arquivo_apagado() {
        let root = fixture("verifica-bom-troca");
        std::fs::write(root.join("A.txt"), b"\xEF\xBB\xBFantes").unwrap();
        assert!(write_checked(&root, "A.txt", "depois", "antes", false).unwrap_err().contains("alterado"));
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), b"\xEF\xBB\xBFantes");
        std::fs::remove_file(root.join("A.txt")).unwrap();
        assert!(write_checked(&root, "A.txt", "x", "antes", true).is_err());
        assert!(!root.join("A.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gravacao_verificada_recusa_link_no_lugar_do_arquivo() {
        let root = fixture("verifica-link");
        std::fs::write(root.join("sub/x.md"), "alvo").unwrap();
        // Criar link simbólico exige privilégio no Windows; sem ele, o teste só não roda esta parte.
        if std::os::windows::fs::symlink_file(root.join("sub/x.md"), root.join("link.md")).is_err() {
            eprintln!("sem privilégio para link simbólico: teste pulado");
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        assert!(write_checked(&root, "link.md", "novo", "alvo", false).unwrap_err().contains("é um link"));
        assert_eq!(std::fs::read_to_string(root.join("sub/x.md")).unwrap(), "alvo");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn gravacao_verificada_recusa_utf8_invalido_binario_e_caminhos_ruins() {
        let root = fixture("verifica-recusas");
        std::fs::write(root.join("A.txt"), [b'a', 0xFF, b'b']).unwrap();
        std::fs::write(root.join("bin.dat"), [b'a', 0, b'b']).unwrap();
        assert!(write_checked(&root, "A.txt", "x", "a", false).unwrap_err().contains("UTF-8"));
        assert!(write_checked(&root, "bin.dat", "x", "", false).unwrap_err().contains("binário"));
        for rel in ["../novo.txt", ".git/config", "sub", "nao-existe.txt", "sub\\x.md", ""] {
            assert!(write_checked(&root, rel, "x", "", false).is_err(), "{rel} deveria ser recusado");
        }
        assert_eq!(std::fs::read(root.join("A.txt")).unwrap(), [b'a', 0xFF, b'b']);
        assert!(!root.join("novo.txt").exists() && !root.join("nao-existe.txt").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Enquanto a gravação segura o arquivo, outro programa que tenta gravar nele leva erro, e a gravação dela própria também.
    #[test]
    fn enquanto_a_gravacao_segura_o_arquivo_outro_gravador_nao_entra() {
        let root = fixture("verifica-trava");
        std::fs::write(root.join("A.txt"), "base").unwrap();
        let held = hold(&root, "A.txt").unwrap();
        assert!(OpenOptions::new().write(true).open(root.join("A.txt")).is_err(), "outro gravador entrou com o arquivo travado");
        assert!(write_checked(&root, "A.txt", "novo", "base", false).unwrap_err().contains("gravando"));
        drop(held);
        write_checked(&root, "A.txt", "novo", "base", false).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("A.txt")).unwrap(), "novo");
        let _ = std::fs::remove_dir_all(&root);
    }
}
