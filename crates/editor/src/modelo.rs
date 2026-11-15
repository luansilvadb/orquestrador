//! Modelo puro do espaço de trabalho do editor (porte de `src/editor/workspace-model.ts`): até dois grupos lado a lado,
//! cada um com abas e prévia. Quem chama guarda os buffers e diz quais documentos estão sujos.
//!
//! Contratos: cada aba tem `id` (único no workspace) e `alvo` (a identidade). Aba de arquivo é identificada pelo `doc_id`,
//! que nunca muda; o caminho só diz onde o arquivo está. Dentro de um grupo uma identidade aparece no máximo uma vez,
//! fixadas vêm primeiro, há no máximo uma prévia e fixada nunca é prévia. Toda operação devolve um estado novo; operação
//! inválida devolve o mesmo estado. Fechar é em duas etapas: [`planejar_fechamento`] e [`fechar_abas`].
use crate::caminhos::{dentro_de, eh_absoluto};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

pub const VERSAO_WORKSPACE: u64 = 1;
pub const MAX_GRUPOS: usize = 2;
const MAX_FECHADAS: usize = 20;
/// `Number.MAX_SAFE_INTEGER`: o JSON salvo é o mesmo formato do React.
const MAX_SEGURO: u64 = (1 << 53) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Grupo {
    A,
    B,
}

impl Grupo {
    pub fn outro(self) -> Grupo {
        match self {
            Grupo::A => Grupo::B,
            Grupo::B => Grupo::A,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lado {
    Staged,
    Unstaged,
}

/// A identidade da aba.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Alvo {
    File {
        #[serde(rename = "docId")]
        doc_id: String,
        path: String,
    },
    Diff {
        side: Lado,
        path: String,
    },
    Commit {
        hash: String,
        path: String,
    },
    Conflict {
        path: String,
    },
    /// Sem caminho: "Sem título-<num>".
    Draft {
        #[serde(rename = "docId")]
        doc_id: String,
        num: u64,
    },
}

impl Alvo {
    pub fn arquivo(doc_id: impl Into<String>, path: impl Into<String>) -> Alvo {
        Alvo::File { doc_id: doc_id.into(), path: path.into() }
    }

    /// Caminho, quando há (rascunho não tem).
    pub fn caminho(&self) -> Option<&str> {
        match self {
            Alvo::File { path, .. } | Alvo::Diff { path, .. } | Alvo::Commit { path, .. } | Alvo::Conflict { path } => Some(path),
            Alvo::Draft { .. } => None,
        }
    }

    /// `doc_id` do buffer (arquivo e rascunho).
    pub fn doc_id(&self) -> Option<&str> {
        match self {
            Alvo::File { doc_id, .. } | Alvo::Draft { doc_id, .. } => Some(doc_id),
            _ => None,
        }
    }

    /// Chave estável da identidade, sem diferença de caixa nos caminhos.
    pub fn chave(&self) -> String {
        match self {
            Alvo::File { doc_id, .. } => format!("file:{doc_id}"),
            Alvo::Diff { side, path } => {
                format!("diff:{}:{}", if *side == Lado::Staged { "staged" } else { "unstaged" }, path.to_lowercase())
            }
            Alvo::Commit { hash, path } => format!("commit:{}:{}", hash.to_lowercase(), path.to_lowercase()),
            Alvo::Conflict { path } => format!("conflict:{}", path.to_lowercase()),
            Alvo::Draft { doc_id, .. } => format!("draft:{doc_id}"),
        }
    }
}

/// Nome do rascunho na aba, nos avisos e no diálogo de salvar.
pub fn nome_rascunho(num: u64) -> String {
    format!("Sem título-{num}")
}

/// Modo de exibição: `Preview` é o Markdown renderizado.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Vista {
    Preview,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Aba {
    pub id: String,
    #[serde(rename = "target")]
    pub alvo: Alvo,
    pub pinned: bool,
    pub preview: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Vista>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrupoAbas {
    pub id: Grupo,
    pub tabs: Vec<Aba>,
    pub active: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fechada {
    pub group: Grupo,
    #[serde(rename = "target")]
    pub alvo: Alvo,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<Vista>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workspace {
    pub version: u64,
    /// Esquerda para a direita.
    pub groups: Vec<GrupoAbas>,
    pub focus: Grupo,
    /// A última fechada é a última da lista.
    pub closed: Vec<Fechada>,
    pub next_id: u64,
}

impl Default for Workspace {
    fn default() -> Self {
        Workspace {
            version: VERSAO_WORKSPACE,
            groups: vec![GrupoAbas { id: Grupo::A, tabs: vec![], active: None }],
            focus: Grupo::A,
            closed: vec![],
            next_id: 1,
        }
    }
}

impl Workspace {
    pub fn grupo(&self, id: Grupo) -> Option<&GrupoAbas> {
        self.groups.iter().find(|g| g.id == id)
    }

    pub fn abas(&self) -> impl Iterator<Item = &Aba> {
        self.groups.iter().flat_map(|g| g.tabs.iter())
    }

    /// Grupo e aba pelo id.
    pub fn localizar(&self, id: &str) -> Option<(&GrupoAbas, &Aba)> {
        self.groups.iter().find_map(|g| g.tabs.iter().find(|t| t.id == id).map(|t| (g, t)))
    }

    /// Aba ativa do grupo com o foco.
    pub fn aba_focada(&self) -> Option<&Aba> {
        let g = self.grupo(self.focus)?;
        g.tabs.iter().find(|t| Some(&t.id) == g.active.as_ref())
    }
}

/// O que um fechamento alcança.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Escopo {
    /// X, Fechar, Ctrl+F4: alcança a fixada.
    Uma(String),
    /// Lote: nunca alcança fixada.
    Outras(String),
    Direita(String),
    Salvas(Grupo),
    Todas(Grupo),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanoFechamento {
    pub abas: Vec<String>,
    /// Ids das abas sujas, para o integrador confirmar.
    pub nao_salvas: Vec<String>,
}

/// Sujo: buffer de arquivo ou rascunho (`doc_id`) ou rascunho de conflito (`conflict:<caminho>` em minúsculas).
pub fn esta_sujo(alvo: &Alvo, sujos: &HashSet<String>) -> bool {
    match alvo {
        Alvo::File { doc_id, .. } | Alvo::Draft { doc_id, .. } => sujos.contains(doc_id),
        Alvo::Conflict { path } => sujos.contains(&format!("conflict:{}", path.to_lowercase())),
        _ => false,
    }
}

/// Abre um rascunho novo, permanente e ativo, com o menor número livre.
pub fn abrir_rascunho(s: &Workspace, grupo: Grupo, doc_id: &str) -> Workspace {
    let usados: HashSet<u64> = s.abas().filter_map(|t| if let Alvo::Draft { num, .. } = t.alvo { Some(num) } else { None }).collect();
    let mut num = 1;
    while usados.contains(&num) {
        num += 1;
    }
    abrir_permanente(s, grupo, Alvo::Draft { doc_id: doc_id.into(), num })
}

/// Salvar como: o rascunho vira aba de arquivo em `caminho`, com o mesmo `doc_id`.
pub fn promover_rascunho(s: &Workspace, id: &str, caminho: &str) -> Workspace {
    let Some((_, aba)) = s.localizar(id) else { return s.clone() };
    let Alvo::Draft { doc_id, .. } = &aba.alvo else { return s.clone() };
    if !caminho_arquivo_seguro(caminho) {
        return s.clone();
    }
    let doc_id = doc_id.clone();
    mudar_aba(s, id, |t| Aba { alvo: Alvo::arquivo(doc_id.clone(), caminho), ..t.clone() })
}

/// Salvar como em arquivo aberto: só as abas de arquivo deste `doc_id` passam para `caminho`.
pub fn redirecionar_doc(s: &Workspace, doc: &str, caminho: &str) -> Workspace {
    if !caminho_arquivo_seguro(caminho) {
        return s.clone();
    }
    let mut n = s.clone();
    for g in &mut n.groups {
        for t in &mut g.tabs {
            if let Alvo::File { doc_id, path } = &mut t.alvo {
                if doc_id == doc {
                    *path = caminho.to_string();
                }
            }
        }
    }
    arrumar(n)
}

/// `doc_id` das abas que têm buffer.
pub fn docs_vivos(s: &Workspace) -> HashSet<String> {
    s.abas().filter_map(|t| t.alvo.doc_id().map(str::to_string)).collect()
}

/// Abre (ou ativa) uma prévia. Prévia limpa é substituída; suja vira permanente e a nova entra depois.
pub fn abrir_previa(s: &Workspace, grupo: Grupo, alvo: Alvo, sujos: &HashSet<String>) -> Workspace {
    abrir_em(s, grupo, alvo, true, sujos, None)
}

/// Abre de vez; se a identidade já é prévia no grupo, ela vira permanente.
pub fn abrir_permanente(s: &Workspace, grupo: Grupo, alvo: Alvo) -> Workspace {
    abrir_em(s, grupo, alvo, false, &HashSet::new(), None)
}

/// Ativa a aba no grupo dela e leva o foco para lá.
pub fn ativar(s: &Workspace, id: &str) -> Workspace {
    let Some((g, _)) = s.localizar(id) else { return s.clone() };
    let gid = g.id;
    let mut n = s.clone();
    if let Some(g) = n.groups.iter_mut().find(|g| g.id == gid) {
        g.active = Some(id.to_string());
    }
    n.focus = gid;
    arrumar(n)
}

/// Editar torna a aba permanente.
pub fn marcar_editada(s: &Workspace, id: &str) -> Workspace {
    mudar_aba(s, id, |t| Aba { preview: false, ..t.clone() })
}

/// Troca o modo de exibição (`None` volta ao texto).
pub fn definir_vista(s: &Workspace, id: &str, vista: Option<Vista>) -> Workspace {
    mudar_aba(s, id, |t| Aba { view: vista, ..t.clone() })
}

/// Fixa ou desafixa; fixar torna permanente e leva ao começo.
pub fn alternar_fixa(s: &Workspace, id: &str) -> Workspace {
    mudar_aba(s, id, |t| Aba { pinned: !t.pinned, preview: false, ..t.clone() })
}

/// O que o fechamento tiraria, sem fechar nada.
pub fn planejar_fechamento(s: &Workspace, escopo: &Escopo, sujos: &HashSet<String>) -> PlanoFechamento {
    let (grupo, ref_id) = match escopo {
        Escopo::Uma(id) | Escopo::Outras(id) | Escopo::Direita(id) => (s.localizar(id).map(|(g, _)| g), Some(id.as_str())),
        Escopo::Salvas(g) | Escopo::Todas(g) => (s.grupo(*g), None),
    };
    let Some(grupo) = grupo else { return PlanoFechamento::default() };
    let at = ref_id.and_then(|id| grupo.tabs.iter().position(|t| t.id == id));
    let escolhidas: Vec<&Aba> = grupo
        .tabs
        .iter()
        .enumerate()
        .filter(|&(i, t)| match escopo {
            Escopo::Uma(id) => &t.id == id,
            Escopo::Outras(_) => Some(i) != at && !t.pinned,
            Escopo::Direita(_) => at.is_some_and(|a| i > a) && !t.pinned,
            Escopo::Salvas(_) => !t.pinned && !esta_sujo(&t.alvo, sujos),
            Escopo::Todas(_) => !t.pinned,
        })
        .map(|(_, t)| t)
        .collect();
    PlanoFechamento {
        abas: escolhidas.iter().map(|t| t.id.clone()).collect(),
        nao_salvas: escolhidas.iter().filter(|t| esta_sujo(&t.alvo, sujos)).map(|t| t.id.clone()).collect(),
    }
}

/// Fecha as abas; cada uma vai para a pilha de fechadas (rascunho não) e a vizinha vira ativa.
pub fn fechar_abas(s: &Workspace, ids: &[String]) -> Workspace {
    let fechando: HashSet<&str> = ids.iter().map(String::as_str).collect();
    let mut n = s.clone();
    let mut fechadas = Vec::new();
    for g in &mut n.groups {
        for t in &g.tabs {
            if fechando.contains(t.id.as_str()) && !matches!(t.alvo, Alvo::Draft { .. }) {
                fechadas.push(Fechada { group: g.id, alvo: t.alvo.clone(), view: t.view });
            }
        }
        *g = tirar_do_grupo(g, &fechando);
    }
    n.closed.extend(fechadas);
    arrumar(n)
}

/// Reabre a última fechada no grupo de origem (ou no primeiro), permanente e ativa. `resolver` atualiza o alvo.
pub fn reabrir_fechada(s: &Workspace, resolver: impl Fn(&Alvo) -> Alvo) -> Workspace {
    let Some(ultima) = s.closed.last().cloned() else { return s.clone() };
    let mut resto = s.clone();
    resto.closed.pop();
    let grupo = resto.grupo(ultima.group).map(|g| g.id).unwrap_or(resto.groups[0].id);
    abrir_em(&resto, grupo, resolver(&ultima.alvo), false, &HashSet::new(), ultima.view)
}

/// Anterior (-1) ou próxima (1) aba do grupo focado, dando a volta.
pub fn navegar(s: &Workspace, passo: i32) -> Workspace {
    let Some(g) = s.grupo(s.focus) else { return s.clone() };
    if g.tabs.is_empty() {
        return s.clone();
    }
    let n = g.tabs.len() as i64;
    let at = g.tabs.iter().position(|t| Some(&t.id) == g.active.as_ref());
    let de = match at {
        Some(a) => a as i64,
        None if passo > 0 => -1,
        None => n,
    };
    let i = (de + passo as i64 + n).rem_euclid(n) as usize;
    ativar(s, &g.tabs[i].id.clone())
}

/// Move a aba para outro grupo (criando-o se couber) e a ativa lá. Recusa se a identidade já está no destino.
pub fn mover_aba(s: &Workspace, id: &str, para: Grupo, indice: Option<usize>) -> Workspace {
    let Some((origem, aba)) = s.localizar(id) else { return s.clone() };
    if origem.id == para {
        return match indice {
            Some(i) => reordenar_aba(s, id, i),
            None => s.clone(),
        };
    }
    let (origem_id, aba) = (origem.id, aba.clone());
    let mut base = s.clone();
    if base.grupo(para).is_none() {
        if base.groups.len() >= MAX_GRUPOS {
            return s.clone();
        }
        base.groups.push(GrupoAbas { id: para, tabs: vec![], active: None });
    }
    let chave = aba.alvo.chave();
    if base.grupo(para).is_some_and(|d| d.tabs.iter().any(|t| t.alvo.chave() == chave)) {
        return s.clone();
    }
    let um: HashSet<&str> = [id].into_iter().collect();
    for g in &mut base.groups {
        if g.id == origem_id {
            *g = tirar_do_grupo(g, &um);
        } else if g.id == para {
            let i = indice.unwrap_or(g.tabs.len()).min(g.tabs.len());
            g.tabs.insert(i, aba.clone());
            g.active = Some(id.to_string());
        }
    }
    arrumar(ativar(&base, id))
}

/// Muda a aba de posição dentro do grupo; as fixadas continuam no começo.
pub fn reordenar_aba(s: &Workspace, id: &str, indice: usize) -> Workspace {
    let Some((g, aba)) = s.localizar(id) else { return s.clone() };
    let (gid, aba) = (g.id, aba.clone());
    let mut n = s.clone();
    if let Some(g) = n.groups.iter_mut().find(|g| g.id == gid) {
        g.tabs.retain(|t| t.id != id);
        let i = indice.min(g.tabs.len());
        g.tabs.insert(i, aba);
    }
    arrumar(n)
}

/// Abre uma cópia da aba no outro grupo (criando-o), mesma identidade e id novo.
pub fn dividir_aba(s: &Workspace, id: &str) -> Workspace {
    let Some((g, aba)) = s.localizar(id) else { return s.clone() };
    let outro = g.id.outro();
    let (alvo, vista) = (aba.alvo.clone(), aba.view);
    let mut base = s.clone();
    if base.grupo(outro).is_none() {
        base.groups.push(GrupoAbas { id: outro, tabs: vec![], active: None });
    }
    abrir_em(&base, outro, alvo, false, &HashSet::new(), vista)
}

/// Une o segundo grupo ao primeiro; identidade repetida fica uma vez (fixada ou permanente vence).
pub fn unir_grupos(s: &Workspace) -> Workspace {
    if s.groups.len() < 2 {
        return s.clone();
    }
    let (primeiro, segundo) = (&s.groups[0], &s.groups[1]);
    let mut tabs = primeiro.tabs.clone();
    for t in &segundo.tabs {
        match tabs.iter().position(|x| x.alvo.chave() == t.alvo.chave()) {
            None => tabs.push(t.clone()),
            Some(i) => {
                tabs[i].pinned |= t.pinned;
                tabs[i].preview &= t.preview;
            }
        }
    }
    let mut n = s.clone();
    n.groups = vec![GrupoAbas { id: primeiro.id, tabs, active: primeiro.active.clone() }];
    n.focus = primeiro.id;
    arrumar(n)
}

/// Remapeia um prefixo de caminho depois de renomear ou mover (arquivo, diff, conflito; abertas ou fechadas). Histórico
/// e rascunho ficam como estão.
pub fn remapear(s: &Workspace, de: &str, para: &str) -> Workspace {
    if !caminho_arquivo_seguro(de) || !caminho_arquivo_seguro(para) {
        return s.clone();
    }
    let mover = |alvo: &mut Alvo| {
        let path = match alvo {
            Alvo::File { path, .. } | Alvo::Diff { path, .. } | Alvo::Conflict { path } => path,
            _ => return,
        };
        if dentro_de(path, de) {
            *path = format!("{para}{}", &path[de.len()..]);
        }
    };
    let mut n = s.clone();
    for g in &mut n.groups {
        for t in &mut g.tabs {
            mover(&mut t.alvo);
        }
    }
    for f in &mut n.closed {
        mover(&mut f.alvo);
    }
    arrumar(n)
}

/// Estado lido e o que foi descartado.
pub struct Carregado {
    pub estado: Workspace,
    pub notas: Vec<String>,
}

/// Lê o JSON salvo e devolve um estado seguro; versão desconhecida ou JSON quebrado começam vazios.
pub fn carregar(texto: &str, aceitar_rascunhos: bool) -> Carregado {
    let novo = |nota: &str| Carregado { estado: Workspace::default(), notas: vec![nota.to_string()] };
    let Ok(cru) = serde_json::from_str::<Value>(texto) else {
        return novo("JSON inválido: o espaço de trabalho começou vazio.");
    };
    let Some(cru) = cru.as_object().filter(|o| o.get("version").and_then(Value::as_f64) == Some(VERSAO_WORKSPACE as f64)) else {
        return novo("Versão desconhecida: o espaço de trabalho começou vazio.");
    };
    let mut notas = Vec::new();
    let mut grupos: Vec<GrupoAbas> = Vec::new();
    let mut ids_aba = HashSet::new();
    for g in como_lista(cru.get("groups")) {
        let id = g.get("id").and_then(grupo_de);
        let Some(id) = id.filter(|id| g.is_object() && !grupos.iter().any(|x| x.id == *id) && grupos.len() < MAX_GRUPOS) else {
            notas.push("Grupo inválido ou excedente descartado.".into());
            continue;
        };
        let mut tabs = Vec::new();
        for t in como_lista(g.get("tabs")) {
            let tid = t.get("id").and_then(Value::as_str).filter(|i| !i.is_empty() && i.encode_utf16().count() <= 64);
            let alvo = t.get("target").and_then(|a| alvo_seguro(a, aceitar_rascunhos));
            match (tid, alvo) {
                (Some(tid), Some(alvo)) if t.is_object() && !ids_aba.contains(tid) => {
                    ids_aba.insert(tid.to_string());
                    tabs.push(Aba {
                        id: tid.to_string(),
                        alvo,
                        pinned: t.get("pinned") == Some(&Value::Bool(true)),
                        preview: t.get("preview") == Some(&Value::Bool(true)),
                        view: vista_de(t),
                    });
                }
                _ => notas.push("Aba inválida descartada.".into()),
            }
        }
        grupos.push(GrupoAbas { id, tabs, active: g.get("active").and_then(Value::as_str).map(str::to_string) });
    }
    let mut fechadas = Vec::new();
    for f in como_lista(cru.get("closed")) {
        let grupo = f.get("group").and_then(grupo_de);
        let alvo = f.get("target").and_then(|a| alvo_seguro(a, aceitar_rascunhos));
        match (grupo, alvo) {
            (Some(group), Some(alvo)) if f.is_object() => fechadas.push(Fechada { group, alvo, view: vista_de(f) }),
            _ => notas.push("Aba fechada inválida descartada.".into()),
        }
    }
    let next_id = cru
        .get("nextId")
        .and_then(Value::as_f64)
        .filter(|n| n.fract() == 0. && *n >= 1. && *n < MAX_SEGURO as f64)
        .map_or(1, |n| n as u64);
    let focus = cru.get("focus").and_then(grupo_de).unwrap_or(Grupo::A);
    let estado = arrumar(Workspace { version: VERSAO_WORKSPACE, groups: grupos, focus, closed: fechadas, next_id });
    Carregado { estado, notas }
}

fn como_lista(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn grupo_de(v: &Value) -> Option<Grupo> {
    match v.as_str()? {
        "A" => Some(Grupo::A),
        "B" => Some(Grupo::B),
        _ => None,
    }
}

fn vista_de(v: &Value) -> Option<Vista> {
    (v.get("view").and_then(Value::as_str) == Some("preview")).then_some(Vista::Preview)
}

fn alvo_seguro(v: &Value, aceitar_rascunhos: bool) -> Option<Alvo> {
    let o = v.as_object()?;
    let s = |k: &str| o.get(k).and_then(Value::as_str);
    let doc_ok = |d: Option<&str>| d.filter(|d| !d.is_empty() && d.encode_utf16().count() <= 128).map(str::to_string);
    match s("kind")? {
        "file" => {
            let path = s("path").filter(|p| caminho_arquivo_seguro(p))?;
            Some(Alvo::File { doc_id: doc_ok(s("docId"))?, path: path.into() })
        }
        "diff" => {
            let path = s("path").filter(|p| caminho_seguro(p))?;
            let side = match s("side")? {
                "staged" => Lado::Staged,
                "unstaged" => Lado::Unstaged,
                _ => return None,
            };
            Some(Alvo::Diff { side, path: path.into() })
        }
        "commit" => {
            let path = s("path").filter(|p| caminho_seguro(p))?;
            let hash = s("hash").filter(|h| (4..=64).contains(&h.len()) && h.chars().all(|c| c.is_ascii_hexdigit()))?;
            Some(Alvo::Commit { hash: hash.into(), path: path.into() })
        }
        "conflict" => Some(Alvo::Conflict { path: s("path").filter(|p| caminho_seguro(p))?.into() }),
        "draft" if aceitar_rascunhos => {
            let num = o.get("num").and_then(Value::as_f64).filter(|n| n.fract() == 0. && *n > 0. && *n <= MAX_SEGURO as f64)?;
            Some(Alvo::Draft { doc_id: doc_ok(s("docId"))?, num: num as u64 })
        }
        _ => None,
    }
}

fn abrir_em(s: &Workspace, gid: Grupo, alvo: Alvo, previa: bool, sujos: &HashSet<String>, vista: Option<Vista>) -> Workspace {
    let Some(grupo) = s.grupo(gid) else { return s.clone() };
    let chave = alvo.chave();
    if let Some(mesma) = grupo.tabs.iter().find(|t| t.alvo.chave() == chave) {
        let mantido = if !previa && mesma.preview { marcar_editada(s, &mesma.id) } else { s.clone() };
        return ativar(&mantido, &mesma.id.clone());
    }
    let (mut n, id) = cunhar(s);
    let aba = Aba { id: id.clone(), alvo, pinned: false, preview: previa, view: vista };
    let g = n.groups.iter_mut().find(|g| g.id == gid).expect("grupo existe");
    let previa_atual = g.tabs.iter().position(|t| t.preview);
    match previa_atual {
        Some(i) if previa && !esta_sujo(&g.tabs[i].alvo, sujos) => g.tabs[i] = aba,
        _ => {
            // Prévia suja (ou abertura permanente): a prévia atual, se suja, fica permanente e a nova vem depois.
            if previa {
                if let Some(i) = previa_atual {
                    g.tabs[i].preview = false;
                }
            }
            g.tabs.push(aba);
        }
    }
    ativar(&n, &id)
}

fn cunhar(s: &Workspace) -> (Workspace, String) {
    let usados: HashSet<&str> = s.abas().map(|t| t.id.as_str()).collect();
    let proximo = |n: u64| if n >= MAX_SEGURO - 1 { 1 } else { n + 1 };
    let mut n = proximo(s.next_id);
    while usados.contains(format!("tab-{n}").as_str()) {
        n = proximo(n);
    }
    let mut novo = s.clone();
    novo.next_id = proximo(n);
    (novo, format!("tab-{n}"))
}

fn mudar_aba(s: &Workspace, id: &str, f: impl Fn(&Aba) -> Aba) -> Workspace {
    if s.localizar(id).is_none() {
        return s.clone();
    }
    let mut n = s.clone();
    for g in &mut n.groups {
        for t in &mut g.tabs {
            if t.id == id {
                *t = f(t);
            }
        }
    }
    arrumar(n)
}

/// Tira as abas do grupo; se a ativa sai, a vizinha da direita (depois a da esquerda) vira ativa.
fn tirar_do_grupo(g: &GrupoAbas, fechando: &HashSet<&str>) -> GrupoAbas {
    let ativa = match &g.active {
        Some(a) if fechando.contains(a.as_str()) => {
            let at = g.tabs.iter().position(|t| &t.id == a).unwrap_or(0);
            g.tabs[at + 1..]
                .iter()
                .find(|t| !fechando.contains(t.id.as_str()))
                .or_else(|| g.tabs[..at].iter().rev().find(|t| !fechando.contains(t.id.as_str())))
                .map(|t| t.id.clone())
        }
        a => a.clone(),
    };
    GrupoAbas { id: g.id, tabs: g.tabs.iter().filter(|t| !fechando.contains(t.id.as_str())).cloned().collect(), active: ativa }
}

/// Conserta o estado: até dois grupos únicos, uma identidade e uma prévia por grupo, fixadas no começo, ativa válida.
fn arrumar(s: Workspace) -> Workspace {
    let mut grupos: Vec<GrupoAbas> = Vec::new();
    for g in s.groups {
        if grupos.iter().any(|x| x.id == g.id) || grupos.len() == MAX_GRUPOS {
            continue;
        }
        grupos.push(arrumar_grupo(g));
    }
    // Grupo vazio some quando o outro tem abas; os dois vazios ficam um só.
    if grupos.len() == 2 {
        let cheios = grupos.iter().filter(|g| !g.tabs.is_empty()).count();
        if cheios == 0 {
            grupos.truncate(1);
        } else if cheios == 1 {
            grupos.retain(|g| !g.tabs.is_empty());
        }
    }
    if grupos.is_empty() {
        grupos.push(GrupoAbas { id: Grupo::A, tabs: vec![], active: None });
    }
    let focus = if grupos.iter().any(|g| g.id == s.focus) { s.focus } else { grupos[0].id };
    let pular = s.closed.len().saturating_sub(MAX_FECHADAS);
    Workspace {
        version: VERSAO_WORKSPACE,
        groups: grupos,
        focus,
        closed: s.closed.into_iter().skip(pular).collect(),
        next_id: s.next_id.max(1),
    }
}

fn arrumar_grupo(g: GrupoAbas) -> GrupoAbas {
    let mut chaves = HashSet::new();
    let mut tabs = Vec::new();
    let mut previa_vista = false;
    for mut t in g.tabs {
        if !chaves.insert(t.alvo.chave()) {
            continue;
        }
        t.preview = t.preview && !t.pinned && !previa_vista;
        previa_vista |= t.preview;
        tabs.push(t);
    }
    let (mut fixadas, demais): (Vec<_>, Vec<_>) = tabs.into_iter().partition(|t| t.pinned);
    fixadas.extend(demais);
    let active = if fixadas.iter().any(|t| Some(&t.id) == g.active.as_ref()) {
        g.active
    } else {
        fixadas.first().map(|t| t.id.clone())
    };
    GrupoAbas { id: g.id, tabs: fixadas, active }
}

/// Caminho relativo seguro: não vazio, sem barra inicial, `\`, `:`, `.`/`..`, controle nem `.git`.
pub fn caminho_seguro(p: &str) -> bool {
    if p.is_empty() || p.encode_utf16().count() > 1024 || p.starts_with('/') {
        return false;
    }
    if p.chars().any(|c| (c as u32) < 0x20 || c == '\\' || c == ':') {
        return false;
    }
    p.split('/').all(|seg| {
        let git = seg.len() >= 4 && seg[..4].eq_ignore_ascii_case(".git") && seg[4..].chars().all(|c| c == '.' || c == ' ');
        !seg.is_empty() && seg != "." && seg != ".." && !git
    })
}

/// Caminho do projeto ou absoluto escolhido pelo usuário.
pub fn caminho_arquivo_seguro(p: &str) -> bool {
    if caminho_seguro(p) {
        return true;
    }
    if p.is_empty() || p.encode_utf16().count() > 32_768 || !eh_absoluto(p) {
        return false;
    }
    !p.chars().any(|c| (c as u32) < 0x20) && !p.split(['\\', '/']).any(|s| s == "." || s == "..")
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-workspace.mjs`.
    use super::*;
    use serde_json::json;

    fn nenhum() -> HashSet<String> {
        HashSet::new()
    }
    fn sujos(v: &[&str]) -> HashSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn file(d: &str, p: &str) -> Alvo {
        Alvo::arquivo(d, p)
    }
    fn diff(staged: bool, p: &str) -> Alvo {
        Alvo::Diff { side: if staged { Lado::Staged } else { Lado::Unstaged }, path: p.into() }
    }
    fn commit(h: &str, p: &str) -> Alvo {
        Alvo::Commit { hash: h.into(), path: p.into() }
    }
    fn conflito(p: &str) -> Alvo {
        Alvo::Conflict { path: p.into() }
    }

    fn consistente(s: &Workspace) {
        assert_eq!(s.version, 1);
        assert!((1..=2).contains(&s.groups.len()));
        assert!(s.groups.iter().any(|g| g.id == s.focus));
        let ids: Vec<_> = s.abas().map(|t| t.id.clone()).collect();
        assert_eq!(ids.iter().collect::<HashSet<_>>().len(), ids.len());
        for g in &s.groups {
            let chaves: HashSet<_> = g.tabs.iter().map(|t| t.alvo.chave()).collect();
            assert_eq!(chaves.len(), g.tabs.len());
            if let Some(p) = g.tabs.iter().position(|t| !t.pinned) {
                assert!(g.tabs[p..].iter().all(|t| !t.pinned));
            }
            assert!(g.tabs.iter().filter(|t| t.preview).count() <= 1);
            assert!(g.tabs.iter().all(|t| !(t.pinned && t.preview)));
            if g.tabs.is_empty() {
                assert_eq!(g.active, None);
            } else {
                assert!(g.tabs.iter().any(|t| Some(&t.id) == g.active.as_ref()));
            }
        }
        assert!(s.closed.len() <= 20);
    }

    fn rotulo(t: &Aba) -> String {
        match &t.alvo {
            Alvo::File { path, .. } => path.clone(),
            Alvo::Diff { side, path } => format!("{}:{path}", if *side == Lado::Staged { "staged" } else { "unstaged" }),
            Alvo::Commit { hash, path } => format!("{hash}:{path}"),
            Alvo::Draft { num, .. } => nome_rascunho(*num),
            Alvo::Conflict { path } => format!("conflito:{path}"),
        }
    }
    fn tabs(s: &Workspace, g: Grupo) -> &Vec<Aba> {
        &s.grupo(g).unwrap().tabs
    }
    fn nomes(s: &Workspace, g: Grupo) -> Vec<String> {
        tabs(s, g).iter().map(rotulo).collect()
    }
    fn abrir_todos(s: Workspace, g: Grupo, v: &[(&str, &str)]) -> Workspace {
        v.iter().fold(s, |s, (d, p)| abrir_permanente(&s, g, file(d, p)))
    }
    fn id(s: &Workspace, g: Grupo, r: &str) -> String {
        tabs(s, g).iter().find(|t| rotulo(t) == r).unwrap().id.clone()
    }
    fn ativa(s: &Workspace, g: Grupo) -> Option<String> {
        s.grupo(g).unwrap().active.clone()
    }
    use Grupo::{A, B};

    #[test]
    fn previa_suja_preservada() {
        let s = abrir_previa(&Workspace::default(), A, file("d1", "a.ts"), &nenhum());
        let s = abrir_previa(&s, A, file("d2", "b.ts"), &sujos(&["d1"]));
        assert_eq!(nomes(&s, A), ["a.ts", "b.ts"]);
        assert!(!tabs(&s, A)[0].preview);
        assert!(tabs(&s, A)[1].preview);
        consistente(&s);
    }

    #[test]
    fn previa_limpa_substituida() {
        let s = abrir_permanente(&Workspace::default(), A, file("d0", "z.ts"));
        let s = abrir_previa(&s, A, file("d1", "a.ts"), &nenhum());
        let s = abrir_previa(&s, A, file("d2", "b.ts"), &nenhum());
        assert_eq!(nomes(&s, A), ["z.ts", "b.ts"]);
        assert!(tabs(&s, A)[1].preview);
        consistente(&s);
    }

    #[test]
    fn previa_mesma_identidade_editar_fixar() {
        let s = abrir_previa(&Workspace::default(), A, file("d1", "a.ts"), &nenhum());
        let s = abrir_previa(&s, A, file("d1", "a.ts"), &nenhum());
        assert_eq!(tabs(&s, A).len(), 1);
        let s = marcar_editada(&s, &tabs(&s, A)[0].id.clone());
        assert!(!tabs(&s, A)[0].preview);
        let s = abrir_previa(&s, A, file("d2", "b.ts"), &nenhum());
        let s = abrir_permanente(&s, A, file("d2", "b.ts"));
        assert!(!tabs(&s, A)[1].preview);
        let s = abrir_previa(&s, A, file("d3", "c.ts"), &nenhum());
        let s = alternar_fixa(&s, &id(&s, A, "c.ts"));
        assert!(!tabs(&s, A)[0].preview && tabs(&s, A)[0].pinned);
        consistente(&s);
    }

    #[test]
    fn fixar_e_desafixar() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "a.ts"), ("d2", "b.ts"), ("d3", "c.ts")]);
        let s = alternar_fixa(&s, &id(&s, A, "c.ts"));
        assert_eq!(nomes(&s, A), ["c.ts", "a.ts", "b.ts"]);
        let s = alternar_fixa(&s, &id(&s, A, "c.ts"));
        assert_eq!(tabs(&s, A).len(), 3);
        assert!(tabs(&s, A).iter().all(|t| !t.pinned));
        consistente(&s);
    }

    #[test]
    fn mesma_doc_em_dois_grupos() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "a.ts"), ("d2", "b.ts")]);
        let original = id(&s, A, "a.ts");
        let s = dividir_aba(&s, &original);
        assert_eq!(nomes(&s, A), ["a.ts", "b.ts"]);
        assert_eq!(nomes(&s, B), ["a.ts"]);
        let copia = &tabs(&s, B)[0];
        assert_ne!(copia.id, original);
        assert_eq!(copia.alvo.doc_id(), Some("d1"));
        assert_eq!(s.focus, B);
        consistente(&s);
        let de_novo = abrir_permanente(&s, A, file("d1", "a.ts"));
        assert_eq!(tabs(&de_novo, A).len(), 2);
        assert_eq!(ativa(&de_novo, A), Some(original));
        assert_eq!(de_novo.focus, A);
        consistente(&de_novo);
    }

    #[test]
    fn mover_entre_grupos() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts")]);
        let b = id(&s, A, "b.ts");
        let s = ativar(&s, &b);
        let s = mover_aba(&s, &b, B, Some(0));
        assert_eq!(nomes(&s, A), ["a.ts", "c.ts"]);
        assert_eq!(nomes(&s, B), ["b.ts"]);
        assert_eq!(tabs(&s, B)[0].id, b);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "c.ts")));
        assert_eq!(ativa(&s, B), Some(b));
        assert_eq!(s.focus, B);
        consistente(&s);
    }

    #[test]
    fn mover_recusado_com_mesma_identidade() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "a.ts")]);
        let s = dividir_aba(&s, &id(&s, A, "a.ts"));
        assert_eq!(mover_aba(&s, &id(&s, A, "a.ts"), B, None), s);
    }

    #[test]
    fn reordenar() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts")]);
        let s = alternar_fixa(&s, &id(&s, A, "a.ts"));
        let s = reordenar_aba(&s, &id(&s, A, "c.ts"), 0);
        assert_eq!(nomes(&s, A), ["a.ts", "c.ts", "b.ts"]);
        let s = reordenar_aba(&s, &id(&s, A, "b.ts"), 1);
        assert_eq!(nomes(&s, A), ["a.ts", "b.ts", "c.ts"]);
        consistente(&s);
    }

    #[test]
    fn planejar_nao_muda() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts")]);
        let antes = s.clone();
        let p = planejar_fechamento(&s, &Escopo::Todas(A), &sujos(&["db"]));
        assert_eq!(p.abas, [id(&s, A, "a.ts"), id(&s, A, "b.ts"), id(&s, A, "c.ts")]);
        assert_eq!(p.nao_salvas, [id(&s, A, "b.ts")]);
        assert_eq!(s, antes);
    }

    #[test]
    fn fechar_todas_e_salvas() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts")]);
        let s = alternar_fixa(&s, &id(&s, A, "a.ts"));
        let todas = planejar_fechamento(&s, &Escopo::Todas(A), &sujos(&["dc"]));
        assert_eq!(todas.abas, [id(&s, A, "b.ts"), id(&s, A, "c.ts")]);
        let salvas = planejar_fechamento(&s, &Escopo::Salvas(A), &sujos(&["dc"]));
        assert_eq!(salvas.abas, [id(&s, A, "b.ts")]);
        let f = fechar_abas(&s, &todas.abas);
        assert_eq!(nomes(&f, A), ["a.ts"]);
        consistente(&f);
    }

    #[test]
    fn fechar_outras_e_direita() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts"), ("dd", "d.ts")]);
        let s = alternar_fixa(&s, &id(&s, A, "d.ts"));
        let b = id(&s, A, "b.ts");
        assert_eq!(planejar_fechamento(&s, &Escopo::Outras(b.clone()), &nenhum()).abas, [id(&s, A, "a.ts"), id(&s, A, "c.ts")]);
        assert_eq!(planejar_fechamento(&s, &Escopo::Direita(b), &nenhum()).abas, [id(&s, A, "c.ts")]);
        let d = id(&s, A, "d.ts");
        assert_eq!(planejar_fechamento(&s, &Escopo::Uma(d.clone()), &nenhum()).abas, [d]);
    }

    #[test]
    fn fechar_ativa_escolhe_vizinha() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts")]);
        let s = ativar(&s, &id(&s, A, "b.ts"));
        let s = fechar_abas(&s, &[id(&s, A, "b.ts")]);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "c.ts")));
        let s = fechar_abas(&s, &[id(&s, A, "c.ts")]);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "a.ts")));
        consistente(&s);
    }

    #[test]
    fn reabrir() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts")]);
        let s = fechar_abas(&s, &[id(&s, A, "b.ts")]);
        let s = fechar_abas(&s, &[id(&s, A, "a.ts")]);
        let s = reabrir_fechada(&s, Clone::clone);
        assert_eq!(nomes(&s, A), ["a.ts"]);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "a.ts")));
        let s = reabrir_fechada(&s, Clone::clone);
        assert_eq!(nomes(&s, A), ["a.ts", "b.ts"]);
        assert!(s.closed.is_empty());
        assert_eq!(reabrir_fechada(&s, Clone::clone), s);
        consistente(&s);
    }

    #[test]
    fn reabrir_com_resolver() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts")]);
        let s = fechar_abas(&s, &[id(&s, A, "a.ts")]);
        let s = abrir_permanente(&s, A, file("da2", "a.ts"));
        let s = ativar(&s, &id(&s, A, "b.ts"));
        let s = reabrir_fechada(&s, |t| match t {
            Alvo::File { path, .. } if path == "a.ts" => file("da2", "a.ts"),
            t => t.clone(),
        });
        assert_eq!(nomes(&s, A), ["b.ts", "a.ts"]);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "a.ts")));
        assert!(s.closed.is_empty());
        consistente(&s);
    }

    #[test]
    fn navegar_da_a_volta() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts"), ("dc", "c.ts")]);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "c.ts")));
        let s = navegar(&s, 1);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "a.ts")));
        let s = navegar(&s, -1);
        assert_eq!(ativa(&s, A), Some(id(&s, A, "c.ts")));
        consistente(&s);
    }

    #[test]
    fn grupo_vazio_some() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts")]);
        let s = dividir_aba(&s, &id(&s, A, "a.ts"));
        let copia = tabs(&s, B)[0].id.clone();
        let s = fechar_abas(&s, &[copia]);
        assert_eq!(s.groups.len(), 1);
        assert_eq!(s.focus, A);
        assert_eq!(nomes(&s, A), ["a.ts", "b.ts"]);
        consistente(&s);
    }

    #[test]
    fn unir() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts")]);
        let s = dividir_aba(&s, &id(&s, A, "b.ts"));
        let s = abrir_permanente(&s, B, file("dc", "c.ts"));
        let s = unir_grupos(&s);
        assert_eq!(s.groups.len(), 1);
        assert_eq!(nomes(&s, A), ["a.ts", "b.ts", "c.ts"]);
        consistente(&s);
    }

    #[test]
    fn renomear_pasta() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "src/App.tsx"), ("d2", "src2/a.ts")]);
        let s = abrir_permanente(&s, A, diff(true, "src/x.ts"));
        let s = abrir_permanente(&s, A, diff(false, "src/x.ts"));
        let s = abrir_permanente(&s, A, commit("abc1234", "src/y.ts"));
        let s = abrir_permanente(&s, A, conflito("SRC/z.ts"));
        let s = fechar_abas(&s, &[id(&s, A, "unstaged:src/x.ts")]);
        let s = remapear(&s, "src", "lib/core");
        assert_eq!(
            nomes(&s, A),
            ["lib/core/App.tsx", "src2/a.ts", "staged:lib/core/x.ts", "abc1234:src/y.ts", "conflito:lib/core/z.ts"]
        );
        assert_eq!(s.closed[0].alvo.caminho(), Some("lib/core/x.ts"));
        assert_eq!(tabs(&s, A)[0].alvo.doc_id(), Some("d1"));
        consistente(&s);
    }

    #[test]
    fn renomear_arquivo() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "src/App.tsx")]);
        let antes = tabs(&s, A)[0].clone();
        let s = remapear(&s, "src/App.tsx", "src/Main.tsx");
        let depois = &tabs(&s, A)[0];
        assert_eq!(depois.id, antes.id);
        assert_eq!(depois.alvo, file("d1", "src/Main.tsx"));
    }

    #[test]
    fn remapear_prefixo() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "src2/a.ts"), ("d2", "SRC/b.ts"), ("d3", "srcx/c.ts")]);
        let s = remapear(&s, "src", "novo");
        assert_eq!(nomes(&s, A), ["src2/a.ts", "novo/b.ts", "srcx/c.ts"]);
    }

    #[test]
    fn identidade_historica() {
        assert_ne!(commit("abc1234", "a.ts").chave(), commit("def5678", "a.ts").chave());
        assert_ne!(diff(true, "a.ts").chave(), diff(false, "a.ts").chave());
        assert_eq!(diff(true, "SRC/a.ts").chave(), diff(true, "src/a.ts").chave());
    }

    #[test]
    fn json_ida_e_volta() {
        let s = abrir_todos(Workspace::default(), A, &[("da", "a.ts"), ("db", "b.ts")]);
        let s = alternar_fixa(&s, &id(&s, A, "a.ts"));
        let s = dividir_aba(&s, &id(&s, A, "b.ts"));
        let s = abrir_previa(&s, B, diff(true, "c.ts"), &nenhum());
        let s = fechar_abas(&s, &[id(&s, A, "b.ts")]);
        let c = carregar(&serde_json::to_string(&s).unwrap(), false);
        assert!(c.notas.is_empty());
        assert_eq!(c.estado, s);
    }

    #[test]
    fn json_quebrado_comeca_vazio() {
        for t in ["{nope", &json!({"version": 2, "groups": []}).to_string(), "null", "[]", "\"A\""] {
            let c = carregar(t, false);
            assert_eq!(c.estado, Workspace::default(), "{t}");
            assert_eq!(c.notas.len(), 1, "{t}");
        }
    }

    fn aba_json(id: &str, alvo: Value, pinned: bool, preview: bool) -> Value {
        json!({"id": id, "target": alvo, "pinned": pinned, "preview": preview})
    }
    fn fj(d: &str, p: &str) -> Value {
        json!({"kind": "file", "docId": d, "path": p})
    }

    #[test]
    fn estado_adulterado() {
        let t = json!({
            "version": 1,
            "groups": [
                {"id": "A", "active": "t9", "tabs": [
                    aba_json("t1", fj("x", "../../secret.env"), false, false),
                    aba_json("t2", fj("y", "C:\\Windows\\win.ini"), false, false),
                    aba_json("t3", fj("z", "/etc/passwd"), false, false),
                    aba_json("t4", fj("w", "src\\a.ts"), false, false),
                    aba_json("t5", fj("v", "src//a.ts"), false, false),
                    aba_json("t6", fj("u", "src/a.ts"), true, true),
                    aba_json("t7", fj("s", "src/b.ts"), false, true),
                    aba_json("t8", fj("r", "src/c.ts"), false, true),
                    aba_json("t6", fj("dup", "x.ts"), false, false),
                    aba_json("t10", json!({"kind": "commit", "hash": "zz", "path": "a.ts"}), false, false),
                ]},
                {"id": "C", "tabs": []}, {"id": "A", "tabs": []}, {"id": "B", "tabs": []}
            ],
            "focus": "Z", "recent": ["t99", "t7"], "closed": [{"group": "Q", "target": fj("k", "../x")}], "nextId": -4
        });
        let c = carregar(&t.to_string(), false);
        consistente(&c.estado);
        assert!(!c.notas.is_empty());
        let s = &c.estado;
        assert_eq!(nomes(s, A), ["src/a.ts", "C:\\Windows\\win.ini", "/etc/passwd", "src/b.ts", "src/c.ts"]);
        assert_eq!(
            tabs(s, A).iter().map(|t| (t.pinned, t.preview)).collect::<Vec<_>>(),
            [(true, false), (false, false), (false, false), (false, true), (false, false)]
        );
        assert_eq!(tabs(s, A)[0].id, "t6");
        assert_eq!(ativa(s, A).as_deref(), Some("t6"));
        assert_eq!(s.groups.len(), 1);
        assert_eq!(s.focus, A);
        assert!(s.closed.is_empty());
        assert_eq!(s.next_id, 1);
    }

    #[test]
    fn rascunho_no_json() {
        let s = abrir_rascunho(&Workspace::default(), A, "draft-1");
        let c = carregar(&serde_json::to_string(&s).unwrap(), true);
        assert!(c.notas.is_empty());
        assert_eq!(c.estado, s);
        assert_eq!(c.estado.groups[0].tabs[0].alvo.doc_id(), Some("draft-1"));
    }

    #[test]
    fn tres_grupos_ou_repetido() {
        let t = json!({
            "version": 1, "focus": "B",
            "groups": [
                {"id": "A", "active": null, "tabs": [aba_json("a1", fj("d1", "a.ts"), false, false)]},
                {"id": "A", "active": "a1", "tabs": [aba_json("a2", fj("d2", "b.ts"), false, false)]},
                {"id": "B", "active": null, "tabs": []},
                {"id": "B", "active": null, "tabs": []}
            ],
            "recent": [], "closed": [], "nextId": 3
        });
        let c = carregar(&t.to_string(), false);
        consistente(&c.estado);
        assert_eq!(c.estado.groups.iter().map(|g| g.id).collect::<Vec<_>>(), [A]);
        assert_eq!(ativa(&c.estado, A).as_deref(), Some("a1"));
        assert_eq!(c.estado.focus, A);
    }

    #[test]
    fn historico_nao_muda_de_caminho() {
        let s = abrir_permanente(&Workspace::default(), A, commit("abcdef1", "src/a.ts"));
        let s = abrir_permanente(&s, A, file("d1", "src/a.ts"));
        let s = abrir_permanente(&s, A, conflito("src/a.ts"));
        let s = remapear(&s, "src", "novo");
        let alvos: Vec<_> = tabs(&s, A).iter().map(|t| t.alvo.clone()).collect();
        assert!(alvos.contains(&commit("abcdef1", "src/a.ts")));
        assert!(alvos.contains(&file("d1", "novo/a.ts")));
        assert!(alvos.contains(&conflito("novo/a.ts")));
        consistente(&s);
    }

    #[test]
    fn conflito_sujo_no_fechamento() {
        let s = abrir_permanente(&Workspace::default(), A, conflito("src/a.ts"));
        assert_eq!(planejar_fechamento(&s, &Escopo::Todas(A), &sujos(&["conflict:src/a.ts"])).nao_salvas.len(), 1);
        assert_eq!(planejar_fechamento(&s, &Escopo::Todas(A), &nenhum()).nao_salvas.len(), 0);
    }

    #[test]
    fn git_recusado_ao_carregar() {
        for ruim in [".git/config", ".GIT/HEAD", "x/.Git./y", "x/.git "] {
            let t = json!({"version": 1, "focus": "A", "recent": [], "closed": [], "nextId": 2,
                "groups": [{"id": "A", "active": "a", "tabs": [aba_json("a", fj("d", ruim), false, false)]}]});
            let c = carregar(&t.to_string(), false);
            assert!(tabs(&c.estado, A).is_empty(), "aceitou {ruim}");
            assert!(!c.notas.is_empty());
        }
    }

    #[test]
    fn id_no_limite() {
        let s = Workspace { next_id: MAX_SEGURO - 1, ..Workspace::default() };
        let s = abrir_permanente(&s, A, file("d1", "a.ts"));
        let s = abrir_permanente(&s, A, file("d2", "b.ts"));
        consistente(&s);
        let mut v = serde_json::to_value(&s).unwrap();
        v["nextId"] = json!(MAX_SEGURO);
        let c = carregar(&v.to_string(), false);
        assert_eq!(c.estado.next_id, 1);
    }

    #[test]
    fn rascunho_numeracao() {
        let s = abrir_rascunho(&Workspace::default(), A, "x1");
        let s = abrir_rascunho(&s, A, "x2");
        assert_eq!(nomes(&s, A), ["Sem título-1", "Sem título-2"]);
        assert!(tabs(&s, A).iter().all(|t| !t.preview));
        assert_eq!(ativa(&s, A), Some(tabs(&s, A)[1].id.clone()));
        let s = fechar_abas(&s, &[tabs(&s, A)[0].id.clone()]);
        let s = abrir_rascunho(&s, A, "x3");
        assert_eq!(nomes(&s, A), ["Sem título-2", "Sem título-1"]);
        assert_eq!(nome_rascunho(7), "Sem título-7");
        consistente(&s);
    }

    #[test]
    fn docs_vivos_e_rascunho() {
        let s = abrir_permanente(&Workspace::default(), A, file("d1", "a.ts"));
        let s = abrir_permanente(&s, A, diff(true, "a.ts"));
        let s = abrir_rascunho(&s, A, "x1");
        let s = abrir_permanente(&s, A, conflito("b.ts"));
        assert_eq!(docs_vivos(&s), sujos(&["d1", "x1"]));
        let f = fechar_abas(&s, &[tabs(&s, A)[0].id.clone()]);
        assert_eq!(docs_vivos(&f), sujos(&["x1"]));
        consistente(&f);
    }

    #[test]
    fn rascunho_em_outro_grupo() {
        let s = abrir_permanente(&Workspace::default(), A, file("d1", "a.ts"));
        let s = dividir_aba(&s, &tabs(&s, A)[0].id.clone());
        let s = abrir_rascunho(&s, B, "x1");
        assert!(matches!(tabs(&s, B).last().unwrap().alvo, Alvo::Draft { num: 1, .. }));
        assert_eq!(s.focus, B);
        consistente(&s);
    }

    #[test]
    fn rascunho_sujo() {
        let s = abrir_rascunho(&Workspace::default(), A, "x1");
        let t = tabs(&s, A)[0].clone();
        assert!(esta_sujo(&t.alvo, &sujos(&["x1"])));
        assert!(!esta_sujo(&t.alvo, &nenhum()));
        assert_eq!(planejar_fechamento(&s, &Escopo::Uma(t.id.clone()), &sujos(&["x1"])).nao_salvas, std::slice::from_ref(&t.id));
        assert!(planejar_fechamento(&s, &Escopo::Uma(t.id), &nenhum()).nao_salvas.is_empty());
    }

    #[test]
    fn rascunho_nao_entra_nas_fechadas() {
        let s = abrir_rascunho(&Workspace::default(), A, "x1");
        let s = abrir_permanente(&s, A, file("d1", "a.ts"));
        let ids: Vec<_> = tabs(&s, A).iter().map(|t| t.id.clone()).collect();
        let s = fechar_abas(&s, &ids);
        assert_eq!(s.closed, [Fechada { group: A, alvo: file("d1", "a.ts"), view: None }]);
        consistente(&s);
    }

    #[test]
    fn promover_rascunho_teste() {
        let s = abrir_rascunho(&Workspace::default(), A, "x1");
        let t = tabs(&s, A)[0].clone();
        let s = promover_rascunho(&s, &t.id, "pasta/novo.ts");
        assert_eq!(tabs(&s, A)[0], Aba { id: t.id.clone(), alvo: file("x1", "pasta/novo.ts"), pinned: false, preview: false, view: None });
        consistente(&s);
        assert_eq!(promover_rascunho(&s, &t.id, "outro.ts"), s);
        let o = abrir_rascunho(&Workspace::default(), A, "x2");
        assert_eq!(promover_rascunho(&o, &tabs(&o, A)[0].id.clone(), "../fora.ts"), o);
    }

    #[test]
    fn redirecionar() {
        let s = abrir_todos(Workspace::default(), A, &[("d1", "a.ts")]);
        let s = dividir_aba(&s, &id(&s, A, "a.ts"));
        let s = abrir_permanente(&s, A, diff(false, "a.ts"));
        let s = abrir_permanente(&s, A, conflito("a.ts"));
        let s = abrir_permanente(&s, A, file("d2", "c.ts"));
        let s = fechar_abas(&s, &[id(&s, A, "c.ts")]);
        let m = redirecionar_doc(&s, "d1", "b.ts");
        assert_eq!(nomes(&m, A), ["b.ts", "unstaged:a.ts", "conflito:a.ts"]);
        assert_eq!(nomes(&m, B), ["b.ts"]);
        assert_eq!(m.closed, s.closed);
        assert_eq!(tabs(&m, A)[0].id, tabs(&s, A)[0].id);
        consistente(&m);
        assert_eq!(redirecionar_doc(&s, "d1", "../fora.ts"), s);
        assert_eq!(nomes(&redirecionar_doc(&s, "nenhum", "x.ts"), A), nomes(&s, A));
    }

    #[test]
    fn remapear_nao_mexe_no_rascunho() {
        let s = abrir_rascunho(&Workspace::default(), A, "x1");
        let s = abrir_permanente(&s, A, file("d1", "src/a.ts"));
        let s = remapear(&s, "src", "lib");
        assert_eq!(nomes(&s, A), ["Sem título-1", "lib/a.ts"]);
        consistente(&s);
    }
}
