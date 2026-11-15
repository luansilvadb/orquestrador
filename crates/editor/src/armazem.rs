//! Buffers, rascunhos, conflitos e recuperação do editor (porte de `src/editor/workspace-store.ts`). Sem tela e sem
//! disco: quem chama lê e grava. Um buffer é do arquivo (`doc_id`), não do caminho. `raw` é o texto como veio do disco
//! (sem BOM); `draft` é o que o editor mostra, com `\n`. Sujo é `draft` diferente do `raw` normalizado.
use crate::caminhos::dentro_de;
use crate::modelo::{caminho_arquivo_seguro, caminho_seguro};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashSet};

pub const VERSAO_RECUPERACAO: u64 = 1;
/// Limites da recuperação: texto de um rascunho e total, em unidades UTF-16 como o `length` do JavaScript.
pub const MAX_RASCUNHO: usize = 2_000_000;
pub const MAX_RECUPERACAO: usize = 4_000_000;
const MAX_BUFFERS: usize = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FimDeLinha {
    #[default]
    #[serde(rename = "\n")]
    Lf,
    #[serde(rename = "\r\n")]
    CrLf,
    #[serde(rename = "\r")]
    Cr,
}

impl FimDeLinha {
    pub fn texto(self) -> &'static str {
        match self {
            FimDeLinha::Lf => "\n",
            FimDeLinha::CrLf => "\r\n",
            FimDeLinha::Cr => "\r",
        }
    }
}

pub fn normalizar_quebras(t: &str) -> String {
    if t.contains('\r') {
        t.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        t.to_string()
    }
}

pub fn quebra_de(t: &str) -> FimDeLinha {
    if t.contains("\r\n") {
        FimDeLinha::CrLf
    } else if t.contains('\r') {
        FimDeLinha::Cr
    } else {
        FimDeLinha::Lf
    }
}

pub fn para_disco(t: &str, q: FimDeLinha) -> String {
    match q {
        FimDeLinha::Lf => t.to_string(),
        q => t.replace('\n', q.texto()),
    }
}

pub fn chave_conflito(caminho: &str) -> String {
    format!("conflict:{}", caminho.to_lowercase())
}

/// Tamanho em unidades UTF-16.
pub fn tamanho_js(t: &str) -> usize {
    if t.is_ascii() {
        t.len()
    } else {
        t.encode_utf16().count()
    }
}

/// Buffer de um documento. `path` vazio é rascunho (sem arquivo ainda).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Buffer {
    pub doc_id: String,
    pub path: String,
    pub raw: String,
    pub bom: bool,
    pub newline: FimDeLinha,
    pub draft: String,
    pub read_only: bool,
}

impl Buffer {
    /// Buffer recém-lido do disco.
    pub fn do_disco(doc_id: &str, path: &str, raw: String, bom: bool, read_only: bool) -> Buffer {
        Buffer {
            doc_id: doc_id.into(),
            path: path.into(),
            newline: quebra_de(&raw),
            draft: normalizar_quebras(&raw),
            raw,
            bom,
            read_only,
        }
    }

    /// Rascunho vazio, sem caminho.
    pub fn vazio(doc_id: &str) -> Buffer {
        Buffer { doc_id: doc_id.into(), path: String::new(), raw: String::new(), bom: false, newline: FimDeLinha::Lf, draft: String::new(), read_only: false }
    }

    /// Compara com o `raw` normalizado sem copiá-lo.
    pub fn sujo(&self) -> bool {
        difere_do_disco(&self.raw, &self.draft)
    }
}

/// `draft` difere de `raw` com as quebras normalizadas.
pub fn difere_do_disco(raw: &str, draft: &str) -> bool {
    if !raw.contains('\r') {
        return draft != raw;
    }
    let (r, d) = (raw.as_bytes(), draft.as_bytes());
    let (mut i, mut j) = (0, 0);
    while i < r.len() {
        let mut c = r[i];
        if c == b'\r' {
            if r.get(i + 1) == Some(&b'\n') {
                i += 1;
            }
            c = b'\n';
        }
        if d.get(j) != Some(&c) {
            return true;
        }
        i += 1;
        j += 1;
    }
    j != d.len()
}

/// Rascunho de resolução de conflito: `origin` é o texto carregado, `revision` a da pasta do Git.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RascunhoConflito {
    pub path: String,
    pub origin: String,
    pub draft: String,
    pub revision: String,
}

impl RascunhoConflito {
    pub fn sujo(&self) -> bool {
        self.draft != self.origin
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dados {
    pub buffers: BTreeMap<String, Buffer>,
    pub conflicts: BTreeMap<String, RascunhoConflito>,
}

/// Chaves sujas no formato do modelo: `doc_id` para arquivo e `conflict:<caminho>` para conflito.
pub fn chaves_sujas(d: &Dados) -> HashSet<String> {
    let mut k: HashSet<String> = d.buffers.values().filter(|b| b.sujo()).map(|b| b.doc_id.clone()).collect();
    k.extend(d.conflicts.values().filter(|c| c.sujo()).map(|c| chave_conflito(&c.path)));
    k
}

/// Há rascunho neste caminho (ou dentro dele)? Bloqueia descarte no Git. Rascunho sem caminho nunca bloqueia.
pub fn ha_rascunho_em(d: &Dados, caminho: &str) -> bool {
    d.buffers.values().any(|b| !b.path.is_empty() && b.sujo() && dentro_de(&b.path, caminho))
        || d.conflicts.values().any(|c| c.sujo() && dentro_de(&c.path, caminho))
}

/// Renomear ou mover: troca o caminho dos buffers e conflitos dentro de `de`, sem tocar no texto nem no `doc_id`.
pub fn remapear_dados(d: &Dados, de: &str, para: &str) -> Dados {
    if !caminho_arquivo_seguro(de) || !caminho_arquivo_seguro(para) {
        return d.clone();
    }
    let mover = |p: &str| if dentro_de(p, de) { format!("{para}{}", &p[de.len()..]) } else { p.to_string() };
    Dados {
        buffers: d.buffers.iter().map(|(k, b)| (k.clone(), Buffer { path: mover(&b.path), ..b.clone() })).collect(),
        conflicts: d
            .conflicts
            .values()
            .map(|c| {
                let path = mover(&c.path);
                (chave_conflito(&path), RascunhoConflito { path, ..c.clone() })
            })
            .collect(),
    }
}

// ---- Recuperação ----

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rolagem {
    pub top: f64,
    pub left: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sujeira {
    pub raw: String,
    pub bom: bool,
    pub newline: FimDeLinha,
    pub draft: String,
    pub read_only: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BufferRecuperado {
    pub doc_id: String,
    pub path: String,
    pub cursor: Option<crate::caminhos::Cursor>,
    #[serde(default)]
    pub scroll: Option<Rolagem>,
    pub dirty: Option<Sujeira>,
}

/// `split` é a proporção do grupo A (0,15 a 0,85) e `layout` o texto opaco do layout da bancada.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recuperacao {
    pub workspace: Option<String>,
    pub layout: Option<String>,
    pub split: Option<f64>,
    pub buffers: Vec<BufferRecuperado>,
    pub conflicts: Vec<RascunhoConflito>,
}

const PREFIXO: &str = "orquestrador.editor:";
pub const SEM_PROJETO: &str = "__sem-projeto__";

/// Grafia única do caminho do projeto: `D:\Proj`, `d:/proj` e `D:\Proj\.` dão o mesmo texto.
pub fn normalizar_projeto(p: &str) -> String {
    let mut s = p.to_lowercase().replace('/', "\\");
    loop {
        if let Some(t) = s.strip_suffix("\\.") {
            s = t.to_string();
        } else if let Some(t) = s.strip_suffix('\\') {
            s = t.to_string();
        } else {
            break;
        }
    }
    s
}

pub fn chave_recuperacao(projeto: &str) -> String {
    format!("{PREFIXO}{}", if projeto.is_empty() { SEM_PROJETO.to_string() } else { normalizar_projeto(projeto) })
}

/// Armazenamento de chave e texto, como o `localStorage`.
pub trait Chaves {
    fn ler(&self, chave: &str) -> Option<String>;
    fn gravar(&mut self, chave: &str, texto: &str) -> Result<(), String>;
    fn apagar(&mut self, chave: &str);
    fn chaves(&self) -> Vec<String>;
}

/// Lê pela chave normalizada; sem ela, migra uma chave antiga de grafia diferente (a do caminho cru primeiro). Erro
/// na cópia mantém a antiga.
pub fn ler_recuperacao(a: &mut dyn Chaves, projeto: &str) -> Option<String> {
    let chave = chave_recuperacao(projeto);
    let texto = a.ler(&chave);
    if texto.is_some() || projeto.is_empty() {
        return texto;
    }
    let alvo = normalizar_projeto(projeto);
    let mut antigas = vec![format!("{PREFIXO}{projeto}")];
    for k in a.chaves() {
        if let Some(resto) = k.strip_prefix(PREFIXO) {
            if k != chave && normalizar_projeto(resto) == alvo {
                antigas.push(k);
            }
        }
    }
    for k in antigas {
        if k == chave {
            continue;
        }
        let Some(achado) = a.ler(&k) else { continue };
        if a.gravar(&chave, &achado).is_ok() {
            a.apagar(&k);
        }
        return Some(achado);
    }
    None
}

/// Monta o texto salvo: só rascunho sujo leva texto. Passando do limite total, descarta rascunhos do fim e avisa.
pub fn codificar_recuperacao(projeto: &str, r: &Recuperacao) -> (String, Vec<String>) {
    let mut notas = Vec::new();
    let omitidos = r.buffers.iter().skip(MAX_BUFFERS).filter(|b| b.dirty.is_some()).count();
    if omitidos > 0 {
        notas.push(format!("Mais de {MAX_BUFFERS} abas: rascunhos de {omitidos} não foram guardados."));
    }
    let mut buffers: Vec<BufferRecuperado> = r
        .buffers
        .iter()
        .take(MAX_BUFFERS)
        .map(|b| match &b.dirty {
            Some(d) if tamanho_js(&d.draft) > MAX_RASCUNHO => {
                notas.push(format!("Rascunho de {} grande demais para guardar.", b.path));
                BufferRecuperado { dirty: None, ..b.clone() }
            }
            _ => b.clone(),
        })
        .collect();
    let mut conflitos: Vec<RascunhoConflito> = r
        .conflicts
        .iter()
        .filter(|c| {
            let cabe = tamanho_js(&c.draft) <= MAX_RASCUNHO;
            if !cabe {
                notas.push(format!("Rascunho do conflito de {} grande demais para guardar.", c.path));
            }
            cabe
        })
        .cloned()
        .collect();
    let montar = |b: &[BufferRecuperado], c: &[RascunhoConflito]| {
        json!({
            "version": VERSAO_RECUPERACAO, "project": projeto, "workspace": r.workspace, "layout": r.layout, "split": r.split,
            "buffers": b, "conflicts": c,
        })
        .to_string()
    };
    let mut texto = montar(&buffers, &conflitos);
    while tamanho_js(&texto) > MAX_RECUPERACAO && (!conflitos.is_empty() || buffers.iter().any(|b| b.dirty.is_some())) {
        if !conflitos.is_empty() {
            conflitos.pop();
        } else if let Some(b) = buffers.iter_mut().rev().find(|b| b.dirty.is_some()) {
            b.dirty = None;
        }
        notas.push("Espaço de recuperação cheio: alguns rascunhos não foram guardados.".into());
        texto = montar(&buffers, &conflitos);
    }
    if tamanho_js(&texto) > MAX_RECUPERACAO {
        texto = montar(&[], &[]);
    }
    let mut vistas = HashSet::new();
    notas.retain(|n| vistas.insert(n.clone()));
    (texto, notas)
}

/// Lê o texto salvo com desconfiança: versão, projeto, caminhos, tamanhos e tipos.
pub fn decodificar_recuperacao(texto: &str, projeto: &str) -> (Recuperacao, Vec<String>) {
    let vazio = |nota: Option<&str>| (Recuperacao::default(), nota.map(|n| vec![n.to_string()]).unwrap_or_default());
    let Ok(cru) = serde_json::from_str::<Value>(texto) else { return vazio(Some("Recuperação ilegível: começou vazia.")) };
    let Some(o) = cru.as_object().filter(|o| o.get("version").and_then(Value::as_f64) == Some(VERSAO_RECUPERACAO as f64)) else {
        return vazio(Some("Recuperação de outra versão: começou vazia."));
    };
    if o.get("project").and_then(Value::as_str).map(normalizar_projeto) != Some(normalizar_projeto(projeto)) {
        return vazio(None);
    }
    let mut notas = Vec::new();
    let mut buffers = Vec::new();
    let mut ids = HashSet::new();
    for e in o.get("buffers").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]).iter().take(MAX_BUFFERS) {
        let doc = e.get("docId").and_then(Value::as_str).filter(|d| !d.is_empty() && tamanho_js(d) <= 128 && !ids.contains(*d));
        let path = e.get("path").and_then(Value::as_str).filter(|p| p.is_empty() || caminho_arquivo_seguro(p));
        let (Some(doc), Some(path)) = (doc, path) else {
            notas.push("Aba de arquivo inválida descartada.".into());
            continue;
        };
        ids.insert(doc.to_string());
        let cru_sujo = e.get("dirty").filter(|v| !v.is_null());
        let dirty = cru_sujo.and_then(ler_sujeira);
        if cru_sujo.is_some() && dirty.is_none() {
            notas.push(format!("Rascunho de {path} inválido descartado."));
        }
        buffers.push(BufferRecuperado {
            doc_id: doc.into(),
            path: path.into(),
            cursor: e.get("cursor").and_then(ler_cursor),
            scroll: e.get("scroll").and_then(ler_rolagem),
            dirty,
        });
    }
    let mut conflitos = Vec::new();
    let mut caminhos = HashSet::new();
    for e in o.get("conflicts").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]) {
        let s = |k: &str| e.get(k).and_then(Value::as_str);
        match (s("path"), s("origin"), s("draft"), s("revision")) {
            (Some(p), Some(origin), Some(draft), Some(revision))
                if caminho_seguro(p)
                    && !caminhos.contains(&p.to_lowercase())
                    && tamanho_js(draft) <= MAX_RASCUNHO
                    && tamanho_js(origin) <= MAX_RASCUNHO =>
            {
                caminhos.insert(p.to_lowercase());
                conflitos.push(RascunhoConflito { path: p.into(), origin: origin.into(), draft: draft.into(), revision: revision.into() });
            }
            _ => notas.push("Rascunho de conflito inválido descartado.".into()),
        }
    }
    let texto_ate = |k: &str, max: usize| o.get(k).and_then(Value::as_str).filter(|t| tamanho_js(t) <= max).map(str::to_string);
    let r = Recuperacao {
        workspace: texto_ate("workspace", 1_000_000),
        layout: texto_ate("layout", 10_000),
        split: o.get("split").and_then(Value::as_f64).filter(|s| (0.15..=0.85).contains(s)),
        buffers,
        conflicts: conflitos,
    };
    (r, notas)
}

fn ler_sujeira(v: &Value) -> Option<Sujeira> {
    let o = v.as_object()?;
    let raw = o.get("raw")?.as_str()?;
    let draft = o.get("draft")?.as_str()?;
    let bom = o.get("bom")?.as_bool()?;
    let read_only = o.get("readOnly")?.as_bool()?;
    if tamanho_js(raw) > MAX_RASCUNHO || tamanho_js(draft) > MAX_RASCUNHO {
        return None;
    }
    let newline = match o.get("newline")?.as_str()? {
        "\n" => FimDeLinha::Lf,
        "\r\n" => FimDeLinha::CrLf,
        "\r" => FimDeLinha::Cr,
        _ => return None,
    };
    Some(Sujeira { raw: raw.into(), bom, newline, draft: draft.into(), read_only })
}

fn ler_cursor(v: &Value) -> Option<crate::caminhos::Cursor> {
    let ok = |k: &str| v.get(k)?.as_f64().filter(|n| n.fract() == 0. && *n >= 1. && *n <= 9_007_199_254_740_991.).map(|n| n as u64);
    Some(crate::caminhos::Cursor { line: ok("line")?, col: ok("col")? })
}

fn ler_rolagem(v: &Value) -> Option<Rolagem> {
    let ok = |k: &str| v.get(k)?.as_f64().filter(|n| n.is_finite() && (0.0..=1e8).contains(n));
    Some(Rolagem { top: ok("top")?, left: ok("left")? })
}

// ---- Conflitos: marcadores normais e diff3, em linhas `\n`. ----

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlocoConflito {
    pub ours: Vec<String>,
    pub base: Option<Vec<String>>,
    pub theirs: Vec<String>,
    /// Linhas de marcador como estavam, para devolver um bloco pendente sem perda.
    pub head: String,
    pub base_head: Option<String>,
    pub mid: String,
    pub tail: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Peca {
    Linha(String),
    Bloco(BlocoConflito),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Escolha {
    Ours,
    Theirs,
    Both,
    Pending,
}

/// Exatamente sete caracteres seguidos de espaço (inclui `\r`) ou fim.
fn marcador(linha: &str) -> Option<char> {
    let c = linha.chars().next()?;
    if !"<|=>".contains(c) {
        return None;
    }
    let b = linha.as_bytes();
    if b.len() < 7 || !b[..7].iter().all(|&x| x as char == c) {
        return None;
    }
    match linha[7..].chars().next() {
        None => Some(c),
        Some(x) if x.is_whitespace() => Some(c),
        _ => None,
    }
}

/// Quebra em linhas e blocos; `malformado` quando um bloco não fecha.
pub fn ler_conflito(texto: &str) -> (Vec<Peca>, bool) {
    let linhas: Vec<&str> = texto.split('\n').collect();
    let mut pecas = Vec::new();
    let mut i = 0;
    while i < linhas.len() {
        let linha = linhas[i];
        if marcador(linha) != Some('<') {
            pecas.push(Peca::Linha(linha.into()));
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut ours = Vec::new();
        while j < linhas.len() && !matches!(marcador(linhas[j]), Some('=') | Some('|')) {
            ours.push(linhas[j].to_string());
            j += 1;
        }
        let (mut base_head, mut base) = (None, None);
        if j < linhas.len() && marcador(linhas[j]) == Some('|') {
            base_head = Some(linhas[j].to_string());
            j += 1;
            let mut b = Vec::new();
            while j < linhas.len() && marcador(linhas[j]) != Some('=') {
                b.push(linhas[j].to_string());
                j += 1;
            }
            base = Some(b);
        }
        if j >= linhas.len() {
            return (pecas, true);
        }
        let mid = linhas[j].to_string();
        j += 1;
        let mut theirs = Vec::new();
        while j < linhas.len() && marcador(linhas[j]) != Some('>') {
            theirs.push(linhas[j].to_string());
            j += 1;
        }
        if j >= linhas.len() {
            return (pecas, true);
        }
        pecas.push(Peca::Bloco(BlocoConflito { ours, base, theirs, head: linha.into(), base_head, mid, tail: linhas[j].into() }));
        i = j + 1;
    }
    (pecas, false)
}

/// Texto final: cada bloco vira o lado escolhido; `Pending` devolve o bloco com os marcadores.
pub fn montar_conflito(pecas: &[Peca], escolhas: &[Escolha]) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut n = 0;
    for p in pecas {
        match p {
            Peca::Linha(l) => out.push(l),
            Peca::Bloco(b) => {
                let e = escolhas.get(n).copied().unwrap_or(Escolha::Pending);
                n += 1;
                match e {
                    Escolha::Ours => out.extend(b.ours.iter().map(String::as_str)),
                    Escolha::Theirs => out.extend(b.theirs.iter().map(String::as_str)),
                    Escolha::Both => {
                        out.extend(b.ours.iter().map(String::as_str));
                        out.extend(b.theirs.iter().map(String::as_str));
                    }
                    Escolha::Pending => {
                        out.push(&b.head);
                        out.extend(b.ours.iter().map(String::as_str));
                        if let Some(bh) = &b.base_head {
                            out.push(bh);
                            out.extend(b.base.iter().flatten().map(String::as_str));
                        }
                        out.push(&b.mid);
                        out.extend(b.theirs.iter().map(String::as_str));
                        out.push(&b.tail);
                    }
                }
            }
        }
    }
    out.join("\n")
}

pub fn contar_blocos(pecas: &[Peca]) -> usize {
    pecas.iter().filter(|p| matches!(p, Peca::Bloco(_))).count()
}

/// Ainda há linha de marcador de conflito no texto.
pub fn tem_marcadores(texto: &str) -> bool {
    texto.split('\n').any(|l| marcador(l).is_some())
}

/// Um bloco `@@` de um patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlocoPatch {
    pub header: String,
    pub text: String,
}

pub fn blocos_patch(patch: &str) -> Vec<BlocoPatch> {
    let mut blocos = Vec::new();
    let mut atual: Option<Vec<&str>> = None;
    let fechar = |atual: &mut Option<Vec<&str>>, blocos: &mut Vec<BlocoPatch>| {
        if let Some(c) = atual.take() {
            blocos.push(BlocoPatch { header: c[0].into(), text: c.join("\n") });
        }
    };
    for l in patch.split('\n') {
        if l.starts_with("@@") {
            fechar(&mut atual, &mut blocos);
            atual = Some(vec![l]);
        } else if let Some(c) = atual.as_mut() {
            c.push(l);
        }
    }
    fechar(&mut atual, &mut blocos);
    blocos
}

/// Recuperação num arquivo próprio em `%LOCALAPPDATA%\orquestrador\editor-recuperacao.json`: um mapa chave → texto, com
/// a mesma semântica do `localStorage` (gravação atômica por renomeação).
pub struct ArquivoChaves {
    arquivo: Option<std::path::PathBuf>,
    mapa: BTreeMap<String, String>,
}

impl ArquivoChaves {
    pub fn abrir(arquivo: Option<std::path::PathBuf>) -> ArquivoChaves {
        let mapa = arquivo
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        ArquivoChaves { arquivo, mapa }
    }

    /// O arquivo padrão do app.
    pub fn padrao() -> ArquivoChaves {
        let arquivo = std::env::var_os("LOCALAPPDATA").map(|d| std::path::PathBuf::from(d).join("orquestrador").join("editor-recuperacao.json"));
        ArquivoChaves::abrir(arquivo)
    }

    fn salvar(&self) -> Result<(), String> {
        let Some(arquivo) = &self.arquivo else { return Ok(()) };
        if let Some(p) = arquivo.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        let texto = serde_json::to_string(&self.mapa).map_err(|e| e.to_string())?;
        let tmp = arquivo.with_extension("json.tmp");
        std::fs::write(&tmp, texto).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, arquivo).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            e.to_string()
        })
    }
}

impl Chaves for ArquivoChaves {
    fn ler(&self, chave: &str) -> Option<String> {
        self.mapa.get(chave).cloned()
    }
    fn gravar(&mut self, chave: &str, texto: &str) -> Result<(), String> {
        let antes = self.mapa.insert(chave.into(), texto.into());
        self.salvar().inspect_err(|_| match antes {
            Some(a) => {
                self.mapa.insert(chave.into(), a);
            }
            None => {
                self.mapa.remove(chave);
            }
        })
    }
    fn apagar(&mut self, chave: &str) {
        if self.mapa.remove(chave).is_some() {
            let _ = self.salvar();
        }
    }
    fn chaves(&self) -> Vec<String> {
        self.mapa.keys().cloned().collect()
    }
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-editor-store.mjs`.
    use super::*;

    fn buf(raw: &str, draft: &str) -> Buffer {
        Buffer { doc_id: "doc-1".into(), path: "src/a.ts".into(), raw: raw.into(), bom: false, newline: FimDeLinha::Lf, draft: draft.into(), read_only: false }
    }
    fn com(doc: &str, path: &str, draft: &str) -> Buffer {
        Buffer { doc_id: doc.into(), path: path.into(), ..buf("a\nb", draft) }
    }

    #[test]
    fn sujeira() {
        assert!(!buf("a\nb", "a\nb").sujo());
        assert!(buf("a\nb", "a\nb!").sujo());
        assert!(!buf("a\r\nb", "a\nb").sujo());
        let raws = ["", "a\nb", "a\r\nb", "a\rb", "a\r\nb\nc\rd", "\r\n", "\r", "a\r", "a\r\n\r\n", "\r\r\n"];
        let drafts = ["", "a", "a\nb", "a\nb\nc\nd", "a\nb\nc\nd!", "\n", "\n\n", "a\n", "a\n\n", "a\n\n\n", "a\r\nb", "a\rb", "ab"];
        for raw in raws {
            let limpo = normalizar_quebras(raw);
            assert!(!buf(raw, &limpo).sujo(), "{raw:?}");
            for d in drafts {
                assert_eq!(buf(raw, d).sujo(), d != limpo, "{raw:?} {d:?}");
            }
        }
    }

    #[test]
    fn quebras() {
        assert_eq!(quebra_de("a\rb"), FimDeLinha::Cr);
        assert_eq!(quebra_de("a\r\nb"), FimDeLinha::CrLf);
        assert_eq!(para_disco("a\nb", FimDeLinha::Cr), "a\rb");
    }

    #[test]
    fn salvamento_atrasado() {
        let b = buf("a\nb", "a\nb\nc");
        let depois = Buffer { raw: para_disco("a\nb", b.newline), ..b };
        assert!(depois.sujo());
        assert!(!Buffer { draft: "a\nb".into(), raw: "a\nb".into(), ..depois }.sujo());
    }

    fn dados(buffers: Vec<Buffer>, conflitos: Vec<RascunhoConflito>) -> Dados {
        Dados {
            buffers: buffers.into_iter().map(|b| (b.doc_id.clone(), b)).collect(),
            conflicts: conflitos.into_iter().map(|c| (chave_conflito(&c.path), c)).collect(),
        }
    }
    fn conf(p: &str, o: &str, d: &str) -> RascunhoConflito {
        RascunhoConflito { path: p.into(), origin: o.into(), draft: d.into(), revision: "r".into() }
    }

    #[test]
    fn chaves_sujas_teste() {
        let d = dados(vec![com("doc-1", "src/a.ts", "x"), com("doc-2", "b.ts", "a\nb")], vec![conf("src/c.ts", "o", "p")]);
        let mut k: Vec<_> = chaves_sujas(&d).into_iter().collect();
        k.sort();
        assert_eq!(k, ["conflict:src/c.ts", "doc-1"]);
        assert!(chaves_sujas(&dados(vec![], vec![conf("x.ts", "o", "o")])).is_empty());
    }

    #[test]
    fn remapeamento() {
        let d = dados(vec![com("doc-1", "src/a.ts", "x")], vec![]);
        let m = remapear_dados(&d, "src", "lib");
        assert_eq!(m.buffers["doc-1"].path, "lib/a.ts");
        assert_eq!(m.buffers["doc-1"].draft, "x");
        let d = dados(vec![com("d1", "src2/x.ts", "a\nb"), com("d2", "SRC/y.ts", "a\nb")], vec![]);
        let m = remapear_dados(&d, "src", "novo");
        assert_eq!(m.buffers["d1"].path, "src2/x.ts");
        assert_eq!(m.buffers["d2"].path, "novo/y.ts");
        let d = dados(vec![com("d1", "src/a.ts", "a\nb")], vec![]);
        assert_eq!(remapear_dados(&d, "src", "../fora").buffers["d1"].path, "src/a.ts");
        let d = dados(vec![com("d1", "C:/arquivos/a.ts", "a\nb")], vec![]);
        assert_eq!(remapear_dados(&d, "C:/arquivos/a.ts", "C:/arquivos/b.ts").buffers["d1"].path, "C:/arquivos/b.ts");
        let d = dados(vec![], vec![conf("src/c.ts", "o", "p")]);
        let m = remapear_dados(&d, "src", "lib");
        assert_eq!(m.conflicts.keys().cloned().collect::<Vec<_>>(), [chave_conflito("lib/c.ts")]);
        assert_eq!(m.conflicts[&chave_conflito("lib/c.ts")].draft, "p");
        assert_eq!(remapear_dados(&d, "outra", "nova").conflicts.keys().cloned().collect::<Vec<_>>(), [chave_conflito("src/c.ts")]);
    }

    #[test]
    fn rascunho_em() {
        let d = dados(vec![com("d1", "src/a.ts", "mudou")], vec![]);
        assert!(ha_rascunho_em(&d, "src"));
        assert!(ha_rascunho_em(&d, "src/a.ts"));
        assert!(!ha_rascunho_em(&d, "src2"));
        assert!(!ha_rascunho_em(&dados(vec![com("d1", "src/a.ts", "a\nb")], vec![]), "src"));
        assert!(chaves_sujas(&dados(vec![com("d1", "src/a.ts", "nao salvo")], vec![])).contains("d1"));
    }

    fn sujeira_de(raw: &str, draft: &str) -> Option<Sujeira> {
        Some(Sujeira { raw: raw.into(), bom: false, newline: FimDeLinha::Lf, draft: draft.into(), read_only: false })
    }
    fn br(doc: &str, path: &str, dirty: Option<Sujeira>) -> BufferRecuperado {
        BufferRecuperado { doc_id: doc.into(), path: path.into(), cursor: None, scroll: None, dirty }
    }
    fn exemplo() -> Recuperacao {
        Recuperacao {
            workspace: Some("{\"version\":1}".into()),
            layout: Some("layout-x".into()),
            split: Some(0.4),
            buffers: vec![
                BufferRecuperado { cursor: Some(crate::caminhos::Cursor { line: 3, col: 2 }), ..br("d1", "src/a.ts", sujeira_de("a", "a!")) },
                br("d2", "src/b.ts", None),
            ],
            conflicts: vec![RascunhoConflito { path: "src/c.ts".into(), origin: "<<<<<<< x".into(), draft: "final".into(), revision: "rev-1".into() }],
        }
    }

    #[test]
    fn recuperacao_ida_e_volta() {
        let (t, notas) = codificar_recuperacao("C:/p", &exemplo());
        assert!(notas.is_empty());
        let (r, _) = decodificar_recuperacao(&t, "C:/p");
        assert_eq!(r.layout.as_deref(), Some("layout-x"));
        assert_eq!(r.split, Some(0.4));
        assert_eq!(r.buffers[0].dirty.as_ref().unwrap().draft, "a!");
        assert_eq!(r.buffers[0].cursor, Some(crate::caminhos::Cursor { line: 3, col: 2 }));
        assert_eq!(r.buffers[1].dirty, None);
        assert_eq!(r.conflicts[0].revision, "rev-1");
    }

    #[test]
    fn recuperacao_absoluto_e_rascunho() {
        let r = Recuperacao {
            workspace: Some("{\"version\":1}".into()),
            buffers: vec![br("loose", "C:/arquivos/a.ts", sujeira_de("a", "b")), br("draft", "", sujeira_de("", "novo"))],
            ..Default::default()
        };
        let (t, _) = codificar_recuperacao("C:/p", &r);
        assert_eq!(decodificar_recuperacao(&t, "C:/p").0.buffers.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(), ["C:/arquivos/a.ts", ""]);
    }

    #[test]
    fn recuperacao_de_outro_projeto_ou_quebrada() {
        let (t, _) = codificar_recuperacao("C:/p", &exemplo());
        let (r, _) = decodificar_recuperacao(&t, "D:/outro");
        assert!(r.buffers.is_empty() && r.conflicts.is_empty());
        assert!(decodificar_recuperacao("{não é json", "C:/p").0.buffers.is_empty());
        assert_eq!(decodificar_recuperacao("{\"version\":99,\"project\":\"C:/p\"}", "C:/p").1.len(), 1);
    }

    #[test]
    fn recuperacao_descarta_invalidos() {
        let cru = json!({"version": 1, "project": "C:/p", "workspace": null, "layout": null, "split": null, "conflicts": [],
            "buffers": [
                {"docId": "d1", "path": "../fora.ts", "cursor": null, "dirty": null},
                {"docId": "d2", "path": ".git/config", "cursor": null, "dirty": null},
                {"docId": "d3", "path": "ok.ts", "cursor": null, "dirty": null},
                {"docId": "d3", "path": "ok2.ts", "cursor": null, "dirty": null}]});
        let (r, notas) = decodificar_recuperacao(&cru.to_string(), "C:/p");
        assert_eq!(r.buffers.iter().map(|b| b.path.as_str()).collect::<Vec<_>>(), ["ok.ts"]);
        assert!(notas.len() >= 3);
        let cru = json!({"version": 1, "project": "P", "workspace": null, "layout": null, "split": 2, "conflicts": [],
            "buffers": [{"docId": "d1", "path": "a.ts", "cursor": {"line": 0, "col": 1}, "dirty": null}]});
        let (r, _) = decodificar_recuperacao(&cru.to_string(), "P");
        assert_eq!(r.split, None);
        assert_eq!(r.buffers[0].cursor, None);
    }

    #[test]
    fn limites() {
        let grande = "x".repeat(MAX_RASCUNHO + 1);
        let r = Recuperacao { buffers: vec![br("d1", "a.ts", sujeira_de("a", &grande))], conflicts: vec![], ..exemplo() };
        let (t, notas) = codificar_recuperacao("C:/p", &r);
        assert!(!notas.is_empty());
        assert_eq!(decodificar_recuperacao(&t, "C:/p").0.buffers[0].dirty, None);
        let pedaco = "y".repeat(MAX_RASCUNHO);
        let buffers = (0..3).map(|i| br(&format!("d{i}"), &format!("f{i}.ts"), sujeira_de("a", &pedaco))).collect();
        let (t, notas) = codificar_recuperacao("C:/p", &Recuperacao { buffers, conflicts: vec![], ..exemplo() });
        assert!(tamanho_js(&t) <= MAX_RECUPERACAO);
        assert!(!notas.is_empty());
    }

    #[test]
    fn mais_de_50_abas() {
        let buffers = (0..52).map(|i| br(&format!("d{i}"), &format!("f{i}.ts"), sujeira_de("a", "b"))).collect();
        let (t, notas) = codificar_recuperacao("/p", &Recuperacao { buffers, ..Default::default() });
        assert!(notas.iter().any(|n| n.contains("Mais de 50 abas") && n.contains("2 não foram guardados")), "{notas:?}");
        assert_eq!(serde_json::from_str::<Value>(&t).unwrap()["buffers"].as_array().unwrap().len(), 50);
        let buffers = (0..51).map(|i| br(&format!("d{i}"), &format!("f{i}.ts"), None)).collect();
        assert!(codificar_recuperacao("/p", &Recuperacao { buffers, ..Default::default() }).1.is_empty());
    }

    #[derive(Default)]
    struct Memoria {
        mapa: BTreeMap<String, String>,
        cheio: bool,
    }
    impl Chaves for Memoria {
        fn ler(&self, c: &str) -> Option<String> {
            self.mapa.get(c).cloned()
        }
        fn gravar(&mut self, c: &str, t: &str) -> Result<(), String> {
            if self.cheio {
                return Err("cheio".into());
            }
            self.mapa.insert(c.into(), t.into());
            Ok(())
        }
        fn apagar(&mut self, c: &str) {
            self.mapa.remove(c);
        }
        fn chaves(&self) -> Vec<String> {
            self.mapa.keys().cloned().collect()
        }
    }
    fn memoria(v: &[(&str, &str)]) -> Memoria {
        Memoria { mapa: v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(), cheio: false }
    }

    #[test]
    fn grafias_e_migracao() {
        let chaves: HashSet<_> = ["D:\\Proj", "d:\\proj", "D:\\Proj\\.", "D:/Proj/"].iter().map(|p| chave_recuperacao(p)).collect();
        assert_eq!(chaves, ["orquestrador.editor:d:\\proj".to_string()].into_iter().collect());
        assert_eq!(normalizar_projeto("D:\\Proj\\.\\"), "d:\\proj");
        assert_eq!(chave_recuperacao(""), "orquestrador.editor:__sem-projeto__");
        let (t, _) = codificar_recuperacao("D:\\Proj\\.", &exemplo());
        assert_eq!(decodificar_recuperacao(&t, "d:\\proj").0.buffers.len(), 2);

        let mut m = memoria(&[("orquestrador.editor:D:\\Proj\\.", "antigo"), ("orquestrador.editor:E:\\outro", "x")]);
        assert_eq!(ler_recuperacao(&mut m, "D:\\Proj").as_deref(), Some("antigo"));
        assert_eq!(m.chaves(), ["orquestrador.editor:E:\\outro", "orquestrador.editor:d:\\proj"]);
        let mut ambos = memoria(&[("orquestrador.editor:d:\\proj", "novo"), ("orquestrador.editor:D:\\Proj", "velho")]);
        assert_eq!(ler_recuperacao(&mut ambos, "D:\\Proj").as_deref(), Some("novo"));
        assert_eq!(ambos.mapa.len(), 2);
        assert_eq!(ler_recuperacao(&mut Memoria::default(), "D:\\Proj"), None);
        assert_eq!(ler_recuperacao(&mut memoria(&[("orquestrador.editor:__sem-projeto__", "s")]), "").as_deref(), Some("s"));
        let mut cheio = memoria(&[("orquestrador.editor:D:\\Proj", "antigo")]);
        cheio.cheio = true;
        assert_eq!(ler_recuperacao(&mut cheio, "D:\\Proj").as_deref(), Some("antigo"));
        assert_eq!(cheio.chaves(), ["orquestrador.editor:D:\\Proj"]);
    }

    #[test]
    fn proporcao_e_layout_sem_abas() {
        let (t, _) = codificar_recuperacao("/p", &Recuperacao { split: Some(0.3), ..Default::default() });
        assert_eq!(decodificar_recuperacao(&t, "/p").0.split, Some(0.3));
        let (t, _) = codificar_recuperacao("/p", &Recuperacao { layout: Some("{\"x\":1}".into()), ..Default::default() });
        assert_eq!(decodificar_recuperacao(&t, "/p").0.layout.as_deref(), Some("{\"x\":1}"));
    }

    #[test]
    fn arquivo_chaves_grava_e_rele() {
        let pasta = std::env::temp_dir().join(format!("orq-editor-chaves-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        let arq = pasta.join("rec.json");
        let mut a = ArquivoChaves::abrir(Some(arq.clone()));
        a.gravar("k", "v").unwrap();
        assert_eq!(ArquivoChaves::abrir(Some(arq.clone())).ler("k").as_deref(), Some("v"));
        a.apagar("k");
        assert_eq!(ArquivoChaves::abrir(Some(arq)).ler("k"), None);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    const PLAIN: &str = "antes\n<<<<<<< HEAD\nminha 1\nminha 2\n=======\ndeles 1\n>>>>>>> feature\ndepois";
    const DIFF3: &str = "<<<<<<< HEAD\natual\n||||||| base\noriginal\n=======\nrecebida\n>>>>>>> topic";

    fn bloco(p: &[Peca]) -> &BlocoConflito {
        p.iter().find_map(|x| if let Peca::Bloco(b) = x { Some(b) } else { None }).unwrap()
    }

    #[test]
    fn conflitos() {
        use Escolha::*;
        let (p, mal) = ler_conflito(PLAIN);
        assert!(!mal);
        assert_eq!(contar_blocos(&p), 1);
        assert_eq!(bloco(&p).ours, ["minha 1", "minha 2"]);
        assert_eq!(bloco(&p).theirs, ["deles 1"]);
        assert_eq!(bloco(&p).base, None);
        assert_eq!(montar_conflito(&p, &[Ours]), "antes\nminha 1\nminha 2\ndepois");
        assert_eq!(montar_conflito(&p, &[Theirs]), "antes\ndeles 1\ndepois");
        assert_eq!(montar_conflito(&p, &[Both]), "antes\nminha 1\nminha 2\ndeles 1\ndepois");
        assert_eq!(montar_conflito(&p, &[Pending]), PLAIN);
        assert_eq!(montar_conflito(&ler_conflito(DIFF3).0, &[Pending]), DIFF3);
        let (p3, _) = ler_conflito(DIFF3);
        assert_eq!(bloco(&p3).base.as_deref(), Some(&["original".to_string()][..]));
        assert_eq!(montar_conflito(&p3, &[Ours]), "atual");
        let dois = format!("{PLAIN}\nmeio\n{DIFF3}");
        let (p, _) = ler_conflito(&dois);
        assert_eq!(contar_blocos(&p), 2);
        assert_eq!(montar_conflito(&p, &[Theirs, Ours]), "antes\ndeles 1\ndepois\nmeio\natual");
        assert!(ler_conflito("<<<<<<< x\nalgo\n=======\nmeio").1);
        assert!(ler_conflito("<<<<<<< x\nalgo").1);
        let (p, _) = ler_conflito("<<<<<<< HEAD\n=======\nnovo\n>>>>>>> t");
        assert_eq!(montar_conflito(&p, &[Ours]), "");
        assert_eq!(montar_conflito(&p, &[Theirs]), "novo");
    }

    #[test]
    fn marcadores_de_sete() {
        let t = ["<<<<<<< HEAD", "Título", "=========", "<<<<<<<< nosso", "=======", "Outro", "=========", "<<<<<<<<", ">>>>>>> t"].join("\n");
        let (p, mal) = ler_conflito(&t);
        assert!(!mal);
        assert_eq!(contar_blocos(&p), 1);
        assert_eq!(bloco(&p).ours, ["Título", "=========", "<<<<<<<< nosso"]);
        assert_eq!(bloco(&p).theirs, ["Outro", "=========", "<<<<<<<<"]);
        assert_eq!(montar_conflito(&p, &[Escolha::Pending]), t);
        let (p, _) = ler_conflito("<<<<<<< HEAD\r\nminha\r\n=======\r\ndeles\r\n>>>>>>> t\r\n");
        assert_eq!(bloco(&p).ours, ["minha\r"]);
        assert_eq!(bloco(&p).theirs, ["deles\r"]);
        assert!(tem_marcadores(PLAIN));
        assert!(tem_marcadores("=======\n"));
        assert!(!tem_marcadores("========\n"));
        assert!(!tem_marcadores("a=======b"));
        assert!(!tem_marcadores("texto limpo"));
        assert_eq!(chave_conflito("Src/A.ts"), chave_conflito("src/a.ts"));
    }

    #[test]
    fn conflito_parcial() {
        let parcial = ["antes", "<<<<<<< ours", "A", "=======", "B", ">>>>>>> theirs", "meio", "<<<<<<< ours", "C", "=======", "D", ">>>>>>> theirs", "fim"].join("\n");
        let (p, mal) = ler_conflito(&parcial);
        assert!(!mal);
        assert_eq!(contar_blocos(&p), 2);
        let prox = montar_conflito(&p, &[Escolha::Ours, Escolha::Pending]);
        assert_eq!(contar_blocos(&ler_conflito(&prox).0), 1);
        assert!(prox.contains('A') && !prox.contains(">>>>>>> theirs\nmeio") && prox.contains("<<<<<<< ours\nC"));
        assert_eq!(montar_conflito(&p, &[Escolha::Pending, Escolha::Pending]), parcial);
        assert_eq!(contar_blocos(&ler_conflito("só texto\nfinal").0), 0);
        assert!(!tem_marcadores("só texto\nfinal"));
    }

    #[test]
    fn patch() {
        let patch = ["diff --git a/src/a.ts b/src/a.ts", "--- a/src/a.ts", "+++ b/src/a.ts", "@@ -1,2 +1,2 @@", "-old", "+new", " keep", "@@ -10,1 +10,2 @@", " x", "+y", ""].join("\n");
        let h = blocos_patch(&patch);
        assert_eq!(h.len(), 2);
        assert_eq!(h[0].header, "@@ -1,2 +1,2 @@");
        assert_eq!(h[1].header, "@@ -10,1 +10,2 @@");
        assert!(!h[0].text.contains("diff --git"));
        assert!(h[0].text.contains("+new"));
        assert!(blocos_patch("diff --git a/x b/x\n").is_empty());
    }
}
