//! A lógica da árvore sem tela: linhas à vista, seleção, decorações do Git, nomes, destino do arraste e os textos dos
//! avisos. Porte de `explorer-tree.tsx` (`treeRows`, seleção), `explorer-decorations.ts` e das funções puras de
//! `explorer.tsx`. Caminhos relativos ao projeto com "/"; a raiz é "".
use orquestrador_core::ide::files::{Entry, Moved};
use orquestrador_core::ide::git::{GitFile, Kind};
use std::collections::{HashMap, HashSet};

pub fn juntar(dir: &str, nome: &str) -> String {
    if dir.is_empty() {
        nome.to_string()
    } else {
        format!("{dir}/{nome}")
    }
}

/// ("a/b", "c.txt") de "a/b/c.txt"; ("", "c.txt") na raiz.
pub use orquestrador_ui::caminhos::dividir;

pub fn nome_de(caminho: &str) -> &str {
    dividir(caminho).1
}

pub fn dentro(dir: &str, caminho: &str) -> bool {
    caminho.len() > dir.len() + 1 && caminho.starts_with(dir) && caminho.as_bytes()[dir.len()] == b'/'
}

pub fn sob(base: &str, caminho: &str) -> bool {
    caminho == base || dentro(base, caminho)
}

/// Chave de linha provisória ("Lendo a pasta…" e o nome novo): termina em "/", o que nenhum caminho real faz.
pub fn provisoria(chave: &str) -> bool {
    chave.ends_with('/')
}

pub fn onde(dest: &str) -> String {
    if dest.is_empty() {
        "a raiz do projeto".into()
    } else {
        format!("“{dest}”")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoLinha {
    Arquivo,
    Pasta,
    /// Pasta aberta ainda sem lista.
    Espera,
    /// O campo do nome do item novo.
    Nova,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Linha {
    pub chave: String,
    pub nivel: usize,
    /// Pasta de cima ("" na raiz).
    pub pai: String,
    pub tipo: TipoLinha,
    pub entrada: Option<Entry>,
    /// Tem seta: pasta ainda sem lista ou com algo dentro. Pasta vazia já lida fica sem seta.
    pub expansivel: bool,
}

impl Linha {
    pub fn real(&self) -> bool {
        matches!(self.tipo, TipoLinha::Arquivo | TipoLinha::Pasta)
    }
}

/// O que está em criação: a pasta e se é pasta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Criando<'a> {
    pub dir: &'a str,
    pub pasta: bool,
}

/// As listas lidas e as pastas abertas.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Arvore {
    pub filhos: HashMap<String, Vec<Entry>>,
    pub abertas: Vec<String>,
}

impl Arvore {
    /// Tira uma pasta e tudo o que está dentro dela, aberto ou com lista guardada.
    pub fn tirar(&mut self, caminho: &str) {
        self.abertas.retain(|p| !sob(caminho, p));
        self.filhos.retain(|p, _| !sob(caminho, p));
    }

    pub fn aberta(&self, caminho: &str) -> bool {
        self.abertas.iter().any(|p| p == caminho)
    }

    /// Arquivo de verdade na lista atual. Pasta, nó provisório e caminho que sumiu ficam de fora.
    pub fn eh_arquivo(&self, caminho: &str) -> bool {
        let (dir, nome) = dividir(caminho);
        self.filhos.get(dir).is_some_and(|l| l.iter().any(|e| !e.dir && e.name == nome))
    }

    /// Fecha a pasta e as de dentro; a lista guardada fica para a próxima abertura.
    pub fn fechar(&mut self, caminho: &str) {
        self.abertas.retain(|p| !sob(caminho, p));
    }

    /// Põe a lista da pasta e a abre (a raiz não entra em `abertas`).
    pub fn abrir_com(&mut self, caminho: &str, mut lista: Vec<Entry>) {
        ordenar(&mut lista);
        self.filhos.insert(caminho.to_string(), lista);
        if !caminho.is_empty() && !self.aberta(caminho) {
            self.abertas.push(caminho.to_string());
        }
    }

    /// Aplica as respostas de uma consulta (uma por caminho, na ordem pedida) às pastas que continuam abertas: a que
    /// fechou durante a consulta não se aplica, e a que não abre mais sai da árvore. Devolve se algo mudou.
    pub fn aplicar(&mut self, caminhos: &[String], resultados: &[Result<Vec<Entry>, String>]) -> bool {
        let abertas_antes = self.abertas.clone();
        let mut mudou = false;
        for (caminho, r) in caminhos.iter().zip(resultados) {
            let r = &r.clone().map(|mut l| {
                ordenar(&mut l);
                l
            });
            if caminho.is_empty() {
                if let Ok(lista) = r {
                    if self.filhos.get("") != Some(lista) {
                        self.filhos.insert(String::new(), lista.clone());
                        mudou = true;
                    }
                }
            } else if abertas_antes.contains(caminho) {
                match r {
                    Err(_) => {
                        self.tirar(caminho);
                        mudou = true;
                    }
                    Ok(lista) => {
                        if self.filhos.get(caminho) != Some(lista) {
                            self.filhos.insert(caminho.clone(), lista.clone());
                            mudou = true;
                        }
                    }
                }
            }
        }
        mudou
    }
}

/// A ordem do React: pastas antes, depois `a.name.localeCompare(b.name, 'pt-BR')`.
pub fn ordenar(lista: &mut [Entry]) {
    lista.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| orquestrador_ui::colagem::comparar(&a.name, &b.name)));
}

/// Linhas à vista, na ordem da tela. `criando` põe a linha do nome novo na pasta dela (já lida): pasta nova no topo,
/// arquivo novo depois das pastas (a ordem do VS Code).
pub fn linhas(arvore: &Arvore, criando: Option<Criando>) -> Vec<Linha> {
    let abertas: HashSet<&str> = arvore.abertas.iter().map(String::as_str).collect();
    let mut out = Vec::new();
    andar(arvore, &abertas, criando, "", 0, &mut out);
    out
}

fn andar(arvore: &Arvore, abertas: &HashSet<&str>, criando: Option<Criando>, dir: &str, nivel: usize, out: &mut Vec<Linha>) {
    let vazia = Vec::new();
    let lista = arvore.filhos.get(dir).unwrap_or(&vazia);
    let em = match criando {
        Some(c) if c.dir == dir => Some(if c.pasta { 0 } else { lista.iter().filter(|e| e.dir).count() }),
        _ => None,
    };
    for i in 0..=lista.len() {
        if em == Some(i) {
            out.push(Linha {
                chave: format!("{}/", juntar(dir, "\0novo")),
                nivel,
                pai: dir.to_string(),
                tipo: TipoLinha::Nova,
                entrada: None,
                expansivel: false,
            });
        }
        let Some(entrada) = lista.get(i) else { break };
        let chave = juntar(dir, &entrada.name);
        if !entrada.dir {
            out.push(Linha { chave, nivel, pai: dir.to_string(), tipo: TipoLinha::Arquivo, entrada: Some(entrada.clone()), expansivel: false });
            continue;
        }
        let dentro_lista = arvore.filhos.get(&chave);
        let expansivel = dentro_lista.is_none_or(|l| !l.is_empty()) || criando.is_some_and(|c| c.dir == chave);
        let aberta = abertas.contains(chave.as_str());
        out.push(Linha {
            chave: chave.clone(),
            nivel,
            pai: dir.to_string(),
            tipo: TipoLinha::Pasta,
            entrada: Some(entrada.clone()),
            expansivel,
        });
        if !aberta {
            continue;
        }
        if dentro_lista.is_some() {
            andar(arvore, abertas, criando, &chave, nivel + 1, out);
        } else {
            out.push(Linha {
                chave: format!("{chave}/"),
                nivel: nivel + 1,
                pai: chave,
                tipo: TipoLinha::Espera,
                entrada: None,
                expansivel: false,
            });
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
}

/// Seleção nova do clique em `linha`: Ctrl alterna, Shift pega o intervalo desde a âncora, sem modificador fica só ela.
/// Devolve a seleção e a âncora nova.
pub fn selecionar(
    linhas: &[Linha], selecao: &[String], ancora: Option<&str>, linha: &Linha, mods: Mods,
) -> (Vec<String>, Option<String>) {
    let indice = |k: &str| linhas.iter().position(|l| l.chave == k);
    let de = ancora.and_then(indice);
    if mods.shift {
        if let (Some(de), Some(ate)) = (de, indice(&linha.chave)) {
            let (a, b) = (de.min(ate), de.max(ate));
            let nova = linhas[a..=b].iter().filter(|l| l.real()).map(|l| l.chave.clone()).collect();
            return (nova, ancora.map(str::to_string));
        }
    }
    if mods.ctrl {
        let nova = if selecao.contains(&linha.chave) {
            selecao.iter().filter(|k| **k != linha.chave).cloned().collect()
        } else {
            let mut n = selecao.to_vec();
            n.push(linha.chave.clone());
            n
        };
        return (nova, Some(linha.chave.clone()));
    }
    (vec![linha.chave.clone()], Some(linha.chave.clone()))
}

/// Leva um caminho para onde o item (ou a pasta dele) foi parar. Caminho que não foi movido fica como está.
pub fn remapear(caminho: &str, movidos: &[Moved]) -> String {
    match movidos.iter().find(|m| sob(&m.from, caminho)) {
        Some(m) => format!("{}{}", m.to, &caminho[m.from.len()..]),
        None => caminho.to_string(),
    }
}

/// Motivo para soltar `caminhos` em `dest`, ou None se pode. Pasta não vai para si nem para dentro dela; item já na
/// pasta não tem o que mover.
pub fn problema_soltar(caminhos: &[String], dest: &str) -> Option<String> {
    if let Some(p) = caminhos.iter().find(|p| sob(p, dest)) {
        return Some(format!("Não dá para mover “{}” para dentro dele mesmo.", nome_de(p)));
    }
    if caminhos.iter().all(|p| dividir(p).0 == dest) {
        return Some(if dest.is_empty() { "Os itens já estão na raiz do projeto." } else { "Os itens já estão nesta pasta." }.into());
    }
    None
}

const RESERVADOS: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Motivo para um nome não servir no Windows (`fileNameError`). O core confere de novo.
pub fn erro_nome(nome: &str) -> Option<String> {
    if nome.trim().is_empty() {
        return Some("Digite um nome para o arquivo.".into());
    }
    if nome == "." || nome == ".." || nome.len() > 255 {
        return Some("Esse nome não é permitido.".into());
    }
    if nome.chars().any(|c| c < ' ' || "<>:\"/\\|?*".contains(c)) {
        return Some("O nome tem caractere que o Windows não aceita: < > : \" / \\ | ? *".into());
    }
    if nome.starts_with(' ') || nome.ends_with(' ') || nome.ends_with('.') {
        return Some("O nome não pode começar nem terminar com espaço, nem terminar com ponto.".into());
    }
    if nome.to_lowercase() == ".git" {
        return Some("A pasta .git não pode ser alterada aqui.".into());
    }
    if RESERVADOS.contains(&nome.split('.').next().unwrap_or("").to_uppercase().as_str()) {
        return Some(format!("“{nome}” é um nome reservado pelo Windows."));
    }
    None
}

/// Motivo de recusa do nome de Renomear, com as mensagens do Antigravity: vazio, inválido (sem trim) ou já existente na
/// pasta (sem contar o próprio item, sem diferença de caixa).
pub fn problema_renomear(arvore: &Arvore, caminho: &str, nome: &str) -> Option<String> {
    if nome.trim().is_empty() {
        return Some("É necessário fornecer um nome de arquivo ou pasta.".into());
    }
    if let Some(e) = erro_nome(nome) {
        return Some(e);
    }
    let (dir, atual) = dividir(caminho);
    let colide = arvore
        .filhos
        .get(dir)
        .is_some_and(|l| l.iter().any(|e| e.name != atual && e.name.to_lowercase() == nome.to_lowercase()));
    colide.then(|| format!("Um arquivo ou pasta {nome} já existe nesta localização. Escolha um nome diferente."))
}

/// Fim da seleção inicial do campo de Renomear: o nome sem a última extensão no arquivo (".gitignore" fica inteiro), o
/// nome todo na pasta. Em bytes UTF-8.
pub fn fim_do_nome_base(nome: &str, arquivo: bool) -> usize {
    match nome.rfind('.') {
        Some(i) if arquivo && i > 0 => i,
        _ => nome.len(),
    }
}

// ---------- decorações do Git ----------

#[derive(Clone, Debug, PartialEq)]
pub struct Decoracao {
    pub cor: u32,
    pub letra: String,
    pub titulo: String,
}

// Da mais forte para a mais fraca: a pasta fica com a cor da alteração mais forte de dentro dela.
const ORDEM: [Kind; 6] = [Kind::Conflicted, Kind::Deleted, Kind::Modified, Kind::Added, Kind::Renamed, Kind::Untracked];

/// Cor (as do tema escuro do VS Code, `gitDecoration.*`), letra e título de cada tipo.
pub fn estilo(k: Kind) -> (u32, &'static str, &'static str) {
    match k {
        Kind::Conflicted => (0xe4676b, "!", "Em conflito"),
        Kind::Deleted => (0xc74e39, "D", "Apagado"),
        Kind::Modified => (0xe2c08d, "M", "Modificado"),
        Kind::Added => (0x81b88b, "A", "Novo no índice"),
        Kind::Renamed => (0x73c991, "R", "Renomeado"),
        Kind::Untracked => (0x73c991, "U", "Não rastreado"),
    }
}

pub const IGNORADO: u32 = 0x8c8c8c;
pub const TITULO_IGNORADO: &str = "Ignorado pelo .gitignore";

fn forca(k: Kind) -> usize {
    ORDEM.iter().position(|o| *o == k).unwrap_or(ORDEM.len())
}

fn mais_forte(a: Option<Kind>, b: Option<Kind>) -> Option<Kind> {
    match (a, b) {
        (a, None) => a,
        (None, b) => b,
        (Some(a), Some(b)) => Some(if forca(b) < forca(a) { b } else { a }),
    }
}

/// Caminho (arquivo ou pasta) -> decoração. Pasta leva "•" no lugar da letra e não herda exclusão.
pub fn decoracoes(arquivos: &[GitFile]) -> HashMap<String, Decoracao> {
    let mut tipos: HashMap<&str, Kind> = HashMap::new();
    let mut pastas: HashMap<String, Kind> = HashMap::new();
    for f in arquivos {
        let Some(tipo) = mais_forte(f.staged, f.unstaged) else { continue };
        tipos.insert(&f.path, tipo);
        let sem_exclusao = |k: Option<Kind>| k.filter(|k| *k != Kind::Deleted);
        let Some(herdado) = mais_forte(sem_exclusao(f.staged), sem_exclusao(f.unstaged)) else { continue };
        let partes: Vec<&str> = f.path.split('/').collect();
        for i in 1..partes.len() {
            let dir = partes[..i].join("/");
            let atual = pastas.get(&dir).copied();
            pastas.insert(dir, mais_forte(atual, Some(herdado)).unwrap_or(herdado));
        }
    }
    let mut out = HashMap::new();
    for (dir, k) in pastas {
        let (cor, _, titulo) = estilo(k);
        out.insert(dir, Decoracao { cor, letra: "•".into(), titulo: format!("Contém itens: {}", titulo.to_lowercase()) });
    }
    for (caminho, k) in tipos {
        let (cor, letra, titulo) = estilo(k);
        out.insert(caminho.to_string(), Decoracao { cor, letra: letra.into(), titulo: titulo.into() });
    }
    out
}

// ---------- textos ----------

/// Pergunta e detalhe do diálogo de Excluir, com os textos do VS Code.
pub fn textos_excluir(caminhos: &[String], tem_pasta: bool) -> (String, String) {
    if let [um] = caminhos {
        let nome = nome_de(um);
        let pergunta = format!("Tem certeza de que deseja excluir '{nome}'{}?", if tem_pasta { " e o conteúdo dela" } else { "" });
        let detalhe = format!("Você pode restaurar {} da Lixeira.", if tem_pasta { "essa pasta" } else { "esse arquivo" });
        return (pergunta, detalhe);
    }
    let pergunta = format!(
        "Tem certeza de que deseja excluir os {} {} a seguir{}?",
        caminhos.len(),
        if tem_pasta { "arquivos/pastas" } else { "arquivos" },
        if tem_pasta { " e o conteúdo deles" } else { "" }
    );
    (pergunta, "Você pode restaurar esses itens da Lixeira.".into())
}

/// Aviso das alterações não salvas no diálogo de Excluir.
pub fn aviso_rascunhos(sujos: &[String]) -> Option<String> {
    if sujos.is_empty() {
        return None;
    }
    let quantos = if sujos.len() == 1 { "um item".to_string() } else { format!("{} itens", sujos.len()) };
    let lista = sujos.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
    let outros = if sujos.len() > 3 { " e outros" } else { "" };
    Some(format!(
        "Há alterações não salvas em {quantos}: {lista}{outros}. Elas se perdem ao excluir, e as abas desses arquivos fecham."
    ))
}

/// Dos caminhos dados, os que têm alteração não salva (`sujos` são arquivos; pasta conta se tem um dentro).
pub fn sujos_de(caminhos: &[String], sujos: &[String]) -> Vec<String> {
    caminhos.iter().filter(|c| sujos.iter().any(|s| sob(c, s))).cloned().collect()
}

pub fn texto_movidos(feitos: usize, dest: &str) -> String {
    let n = format!("{feitos} {}", if feitos == 1 { "item" } else { "itens" });
    format!("{n} movido{} para {}", if feitos == 1 { "" } else { "s" }, onde(dest))
}

pub fn texto_falha_mover(erro: &str, feitos: usize) -> String {
    if feitos == 0 {
        return erro.to_string();
    }
    let n = format!("{feitos} {}", if feitos == 1 { "item" } else { "itens" });
    format!("{erro} Antes disso, {n} foi movido.")
}

pub fn texto_colados(movidos: usize, dir: &str) -> String {
    let lugar = if dir.is_empty() { "na raiz do projeto".to_string() } else { format!("em “{dir}”") };
    if movidos > 0 {
        format!("Itens movidos {lugar}")
    } else {
        format!("Itens colados {lugar}")
    }
}

pub fn texto_copiados(n: usize, recorte: bool) -> String {
    if recorte {
        format!("{n} itens recortados. Cole no Explorer para mover.")
    } else {
        format!("{n} itens copiados. Cole no Explorer.")
    }
}

pub fn texto_excluidos(n: usize) -> String {
    if n == 1 {
        "Enviado para a Lixeira".into()
    } else {
        format!("{n} itens enviados para a Lixeira")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TipoCaminho {
    Relativo,
    Absoluto,
}

/// O texto copiado de um caminho do projeto (`pathText`): relativo com o separador do projeto ("." na raiz) ou absoluto
/// (a pasta do projeto sem barra final na raiz; `D:\` mantém a barra).
pub fn texto_caminho(tipo: TipoCaminho, caminho: &str, projeto: &str) -> String {
    let sep = if projeto.contains('\\') { "\\" } else { "/" };
    if caminho.is_empty() {
        return match tipo {
            TipoCaminho::Relativo => ".".into(),
            TipoCaminho::Absoluto => {
                let b = projeto.as_bytes();
                let raiz_unidade = b.len() >= 3
                    && b[0].is_ascii_alphabetic()
                    && b[1] == b':'
                    && projeto[2..].chars().all(|c| c == '\\' || c == '/');
                if raiz_unidade {
                    projeto.into()
                } else {
                    let t = projeto.trim_end_matches(['\\', '/']);
                    if t.is_empty() { projeto.into() } else { t.into() }
                }
            }
        };
    }
    let relativo = caminho.split('/').collect::<Vec<_>>().join(sep);
    match tipo {
        TipoCaminho::Relativo => relativo,
        TipoCaminho::Absoluto => format!("{}{sep}{relativo}", projeto.trim_end_matches(['\\', '/'])),
    }
}

/// Caminho relativo ("/") de um absoluto dentro do projeto, sem diferença de caixa nem de barra; None se está fora.
pub fn relativo_a(projeto: &str, absoluto: &str) -> Option<String> {
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let raiz = norm(projeto);
    let abs = absoluto.replace('\\', "/");
    let comparavel = abs.to_lowercase();
    if raiz.is_empty() || !comparavel.starts_with(&format!("{raiz}/")) {
        return None;
    }
    let resto = abs[raiz.len() + 1..].trim_matches('/');
    (!resto.is_empty()).then(|| resto.to_string())
}

#[cfg(test)]
mod testes {
    //! Porte de `tests/check-explorer.mjs` (decorações) e das partes puras de `tests/check-files-search-ui.mjs`.
    use super::*;

    fn f(path: &str, staged: Option<Kind>, unstaged: Option<Kind>) -> GitFile {
        GitFile { path: path.into(), orig_path: None, staged, unstaged }
    }
    use Kind::*;

    fn letra_cor(d: &HashMap<String, Decoracao>, p: &str) -> (String, u32) {
        let x = &d[p];
        (x.letra.clone(), x.cor)
    }

    #[test]
    fn decoracao_de_arquivo_cor_e_letra_da_mais_forte() {
        let d = decoracoes(&[
            f("a.ts", None, Some(Modified)),
            f("b.ts", None, Some(Untracked)),
            f("c.ts", Some(Added), Some(Modified)),
            f("d.ts", None, Some(Conflicted)),
        ]);
        assert_eq!(letra_cor(&d, "a.ts"), ("M".into(), 0xe2c08d));
        assert_eq!(letra_cor(&d, "b.ts"), ("U".into(), 0x73c991));
        assert_eq!(d["c.ts"].letra, "M", "modificado ganha de novo no índice");
        assert_eq!(d["d.ts"].letra, "!");
        assert!(!d.contains_key("e.ts"));
    }

    #[test]
    fn pasta_herda_a_mais_forte_de_dentro_com_ponto() {
        let d = decoracoes(&[f("src/x/a.ts", None, Some(Untracked)), f("src/b.ts", None, Some(Modified)), f("docs/c.md", None, Some(Deleted))]);
        assert_eq!(letra_cor(&d, "src"), ("•".into(), 0xe2c08d));
        assert_eq!(d["src/x"].cor, 0x73c991);
        assert!(!d.contains_key("docs"), "apagado não colore a pasta");
        assert!(!d.contains_key(""), "a raiz não é decorada");
    }

    #[test]
    fn exclusoes_ficam_no_arquivo() {
        for (s, u) in [(None, Some(Deleted)), (Some(Deleted), None)] {
            let d = decoracoes(&[f("a/b/c/removido.ts", s, u)]);
            assert_eq!(letra_cor(&d, "a/b/c/removido.ts"), ("D".into(), 0xc74e39));
            for dir in ["a", "a/b", "a/b/c"] {
                assert!(!d.contains_key(dir));
            }
        }
    }

    #[test]
    fn pastas_agregam_somente_estados_propagaveis() {
        for (outro, cor) in [(Modified, 0xe2c08d), (Untracked, 0x73c991)] {
            let d = decoracoes(&[f("a/b/excluido.ts", None, Some(Deleted)), f("a/b/outro.ts", None, Some(outro))]);
            for dir in ["a", "a/b"] {
                assert_eq!(letra_cor(&d, dir), ("•".into(), cor));
            }
        }
        for (s, u) in [(Some(Modified), Some(Deleted)), (Some(Deleted), Some(Modified))] {
            let d = decoracoes(&[f("a/b/arquivo.ts", s, u)]);
            assert_eq!(letra_cor(&d, "a/b/arquivo.ts"), ("D".into(), 0xc74e39));
            for dir in ["a", "a/b"] {
                assert_eq!(letra_cor(&d, dir), ("•".into(), 0xe2c08d));
            }
        }
        let c = decoracoes(&[f("a/b/conflito.ts", None, Some(Conflicted)), f("a/b/excluido.ts", Some(Deleted), None)]);
        for dir in ["a", "a/b"] {
            assert_eq!(letra_cor(&c, dir), ("•".into(), 0xe4676b));
        }
    }

    #[test]
    fn pasta_so_com_exclusao_neutra_e_mista_herda_modificacao() {
        let d = decoracoes(&[
            f("somente-exclusao/apagado.md", None, Some(Deleted)),
            f("mista/apagado.md", None, Some(Deleted)),
            f("mista/editado.md", None, Some(Modified)),
        ]);
        assert!(!d.contains_key("somente-exclusao"));
        assert_eq!(d["somente-exclusao/apagado.md"], Decoracao { cor: 0xc74e39, letra: "D".into(), titulo: "Apagado".into() });
        assert_eq!(d["mista"], Decoracao { cor: 0xe2c08d, letra: "•".into(), titulo: "Contém itens: modificado".into() });
    }

    fn e(nome: &str, dir: bool) -> Entry {
        Entry { name: nome.into(), dir, ignored: false }
    }

    #[test]
    fn pastas_antes_e_nomes_como_o_locale_compare() {
        let mut v = vec![e("b.ts", false), e("Zeta", true), e("_x", false), e("á.ts", false), e("A.ts", false), e("alfa", true), e("a10", false), e("a2", false)];
        ordenar(&mut v);
        let nomes: Vec<_> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(nomes, ["alfa", "Zeta", "_x", "A.ts", "á.ts", "a10", "a2", "b.ts"]);
        // A lista da consulta também sai ordenada.
        let mut a = Arvore::default();
        assert!(a.aplicar(&["".into()], &[Ok(vec![e("b", false), e("a", true)])]));
        assert_eq!(a.filhos[""].iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    }

    fn exemplo() -> Arvore {
        let mut a = Arvore::default();
        a.filhos.insert("".into(), vec![e("docs", true), e("src", true), e("vazia", true), e("README.md", false)]);
        a.filhos.insert("docs".into(), vec![e("a.md", false), e("b.md", false), e("c.md", false)]);
        a.filhos.insert("vazia".into(), vec![]);
        a
    }

    fn chaves(l: &[Linha]) -> Vec<&str> {
        l.iter().map(|l| l.chave.as_str()).collect()
    }

    #[test]
    fn linhas_achatam_as_abertas_com_espera_e_sem_seta_na_vazia() {
        let mut a = exemplo();
        a.abertas = vec!["docs".into(), "src".into()];
        let l = linhas(&a, None);
        assert_eq!(chaves(&l), ["docs", "docs/a.md", "docs/b.md", "docs/c.md", "src", "src/", "vazia", "README.md"]);
        assert_eq!(l[5].tipo, TipoLinha::Espera);
        assert_eq!(l[5].nivel, 1);
        assert!(l[4].expansivel, "pasta sem lista tem seta");
        assert!(!l[6].expansivel, "pasta vazia lida fica sem seta");
    }

    #[test]
    fn linha_do_item_novo_pasta_no_topo_arquivo_depois_das_pastas() {
        let a = exemplo();
        let l = linhas(&a, Some(Criando { dir: "", pasta: true }));
        assert_eq!(l[0].tipo, TipoLinha::Nova);
        let l = linhas(&a, Some(Criando { dir: "", pasta: false }));
        assert_eq!(l[3].tipo, TipoLinha::Nova);
        assert!(provisoria(&l[3].chave));
        let l = linhas(&a, Some(Criando { dir: "vazia", pasta: false }));
        assert!(l.iter().find(|x| x.chave == "vazia").unwrap().expansivel, "a pasta que recebe o nome novo ganha seta");
    }

    #[test]
    fn selecao_shift_e_ctrl() {
        let mut a = exemplo();
        a.abertas = vec!["docs".into()];
        let l = linhas(&a, None);
        let linha = |k: &str| l.iter().find(|x| x.chave == k).unwrap().clone();
        let (s, anc) = selecionar(&l, &[], None, &linha("docs/a.md"), Mods::default());
        assert_eq!(s, ["docs/a.md"]);
        let (s, anc) = selecionar(&l, &s, anc.as_deref(), &linha("docs/c.md"), Mods { shift: true, ctrl: false });
        assert_eq!(s, ["docs/a.md", "docs/b.md", "docs/c.md"], "Shift pega o intervalo");
        let (s, _) = selecionar(&l, &s, anc.as_deref(), &linha("docs/b.md"), Mods { ctrl: true, shift: false });
        assert_eq!(s, ["docs/a.md", "docs/c.md"], "Ctrl alterna");
        let (s, _) = selecionar(&l, &s, Some("docs/b.md"), &linha("README.md"), Mods { ctrl: true, shift: false });
        assert_eq!(s, ["docs/a.md", "docs/c.md", "README.md"]);
    }

    #[test]
    fn aplicar_ignora_fechada_e_tira_a_que_falhou() {
        let mut a = exemplo();
        a.abertas = vec!["docs".into()];
        let caminhos = vec!["".to_string(), "docs".into(), "src".into()];
        let mudou = a.aplicar(&caminhos, &[Ok(vec![e("docs", true)]), Err("sumiu".into()), Ok(vec![e("x", false)])]);
        assert!(mudou);
        assert!(!a.aberta("docs") && !a.filhos.contains_key("docs"), "a que não abre sai");
        assert!(!a.filhos.contains_key("src"), "a fechada não recebe lista");
        assert!(!a.aplicar(&["".into()], &[Ok(vec![e("docs", true)])]), "mesma lista não muda");
    }

    #[test]
    fn soltar_remapear_e_nomes() {
        assert_eq!(problema_soltar(&["docs".into()], "docs/sub").unwrap(), "Não dá para mover “docs” para dentro dele mesmo.");
        assert_eq!(problema_soltar(&["docs/a.md".into()], "docs").unwrap(), "Os itens já estão nesta pasta.");
        assert_eq!(problema_soltar(&["a.md".into()], "").unwrap(), "Os itens já estão na raiz do projeto.");
        assert_eq!(problema_soltar(&["docs/a.md".into(), "b.md".into()], ""), None);
        assert_eq!(problema_soltar(&["src2".into()], "src"), None, "prefixo sem barra não é dentro");
        let m = [Moved { from: "docs".into(), to: "dados/docs".into() }];
        assert_eq!(remapear("docs/a.md", &m), "dados/docs/a.md");
        assert_eq!(remapear("docs2/a.md", &m), "docs2/a.md");
        assert!(erro_nome("a:b").unwrap().contains("caractere que o Windows não aceita"));
        assert!(erro_nome(" x ").unwrap().contains("espaço, nem terminar com ponto"));
        assert!(erro_nome("con.txt").unwrap().contains("reservado"));
        assert!(erro_nome(".GIT").unwrap().contains(".git"));
        assert_eq!(erro_nome("projeto"), None);
    }

    #[test]
    fn renomear_colisao_e_nome_base() {
        let a = exemplo();
        assert_eq!(
            problema_renomear(&a, "docs/a.md", "B.md").unwrap(),
            "Um arquivo ou pasta B.md já existe nesta localização. Escolha um nome diferente."
        );
        assert_eq!(problema_renomear(&a, "docs/a.md", "A.md"), None, "o próprio item não colide");
        assert_eq!(problema_renomear(&a, "docs/a.md", " ").unwrap(), "É necessário fornecer um nome de arquivo ou pasta.");
        assert_eq!(fim_do_nome_base("ren-a.md", true), 5);
        assert_eq!(fim_do_nome_base(".ren-oculto", true), 11);
        assert_eq!(fim_do_nome_base("rn.pasta", false), 8);
    }

    #[test]
    fn textos() {
        assert_eq!(textos_excluir(&["docs/projeto-novo".into()], true).0, "Tem certeza de que deseja excluir 'projeto-novo' e o conteúdo dela?");
        assert_eq!(textos_excluir(&["apagar-a.md".into()], false).0, "Tem certeza de que deseja excluir 'apagar-a.md'?");
        assert_eq!(textos_excluir(&["a".into(), "b".into()], false).0, "Tem certeza de que deseja excluir os 2 arquivos a seguir?");
        assert_eq!(texto_movidos(1, "dados"), "1 item movido para “dados”");
        assert_eq!(texto_movidos(2, ""), "2 itens movidos para a raiz do projeto");
        assert_eq!(texto_colados(0, ""), "Itens colados na raiz do projeto");
        assert_eq!(sujos_de(&["docs".into(), "src".into()], &["docs/a.md".into()]), ["docs"]);
        assert_eq!(texto_caminho(TipoCaminho::Relativo, "a/b.md", "C:\\p"), "a\\b.md");
        assert_eq!(texto_caminho(TipoCaminho::Absoluto, "a/b.md", "C:\\p\\"), "C:\\p\\a\\b.md");
        assert_eq!(texto_caminho(TipoCaminho::Absoluto, "", "D:\\"), "D:\\");
        assert_eq!(texto_caminho(TipoCaminho::Relativo, "", "D:\\p"), ".");
        assert_eq!(relativo_a("C:\\P", "c:\\p\\docs\\a.md").as_deref(), Some("docs/a.md"));
        assert_eq!(relativo_a("C:\\P", "C:\\Q\\a.md"), None);
    }
}
