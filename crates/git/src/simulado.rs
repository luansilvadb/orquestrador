//! Git simulado com os dados de `src/backend/mock.ts` (o mesmo que gerou as capturas de referência): estado só no que o
//! mock modela (blocos, descarte, commit, histórico, grafo e conflitos). Serve à vitrine e aos testes; registra cada
//! chamada em [`Simulado::chamadas`].
use crate::fonte::*;
use chrono::{Duration, Local, TimeZone};
use similar::TextDiff;
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModoSimulado {
    Repo,
    SemRepo,
    SemGit,
    RepoAcima,
}

#[derive(Clone)]
struct Rec {
    commit: Commit,
    pais: Vec<String>,
    arquivos: Vec<CommitFile>,
}

struct Interno {
    modo: ModoSimulado,
    arquivos: Vec<GitFile>,
    textos: BTreeMap<String, String>,
    git: BTreeMap<String, (String, String)>,
    commits: Vec<Rec>,
    refs: Vec<(String, RefKind, String)>,
    conflitos: BTreeMap<String, (Option<String>, Option<String>)>,
    patches_commit: BTreeMap<String, String>,
    chamadas: Vec<String>,
    falhar: BTreeMap<String, String>,
}

pub struct Simulado(Mutex<Interno>);

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Dados {
    editor_head: Vec<String>,
    marcadores_head: String,
    marcadores_index: String,
    marcadores_worktree: String,
}

const DADOS: &str = include_str!("../assets/simulado.json");

fn entrada(p: &str, staged: Option<Kind>, unstaged: Option<Kind>) -> GitFile {
    GitFile { path: p.into(), orig_path: None, staged, unstaged }
}

fn dias_atras(n: i64, hh: u32, mm: u32) -> String {
    let d = (Local::now() - Duration::days(n)).date_naive().and_hms_opt(hh, mm, 0).unwrap_or_default();
    Local.from_local_datetime(&d).single().map(|d| d.to_rfc3339()).unwrap_or_default()
}

fn djb2(t: &str) -> String {
    let mut h: i32 = 5381;
    for c in t.encode_utf16() {
        h = h.wrapping_shl(5).wrapping_add(h).wrapping_add(c as i32);
    }
    format!("{:08x}", h as u32)
}

/// Um bloco do diff: cabeçalho, corpo e os intervalos de cada lado (início 0-based).
struct Bloco {
    cabecalho: String,
    texto: String,
    antigo: usize,
    antigo_n: usize,
    novo: usize,
    novo_n: usize,
    antigas: Vec<String>,
    novas: Vec<String>,
}

fn blocos(a: &str, b: &str, contexto: usize) -> Vec<Bloco> {
    let diff = TextDiff::from_lines(a, b);
    let mut ud = diff.unified_diff();
    ud.context_radius(contexto);
    ud.iter_hunks()
        .map(|h| {
            let ops = h.ops();
            let (o0, o1) = (ops.first().map_or(0, |o| o.old_range().start), ops.last().map_or(0, |o| o.old_range().end));
            let (n0, n1) = (ops.first().map_or(0, |o| o.new_range().start), ops.last().map_or(0, |o| o.new_range().end));
            let linha = |s: &str| s.strip_suffix('\n').unwrap_or(s).to_string();
            let mut antigas = Vec::new();
            let mut novas = Vec::new();
            for c in h.iter_changes() {
                match c.tag() {
                    similar::ChangeTag::Equal => {
                        antigas.push(linha(&c.to_string_lossy()));
                        novas.push(linha(&c.to_string_lossy()));
                    }
                    similar::ChangeTag::Delete => antigas.push(linha(&c.to_string_lossy())),
                    similar::ChangeTag::Insert => novas.push(linha(&c.to_string_lossy())),
                }
            }
            Bloco {
                cabecalho: h.header().to_string(),
                texto: h.to_string(),
                antigo: o0,
                antigo_n: o1 - o0,
                novo: n0,
                novo_n: n1 - n0,
                antigas,
                novas,
            }
        })
        .collect()
}

fn diff_de(caminho: &str, a: &str, b: &str) -> (String, String, Vec<Bloco>) {
    if a == b {
        return (String::new(), String::new(), Vec::new());
    }
    let bs = blocos(a, b, 3);
    if bs.is_empty() {
        return (String::new(), String::new(), bs);
    }
    let cab = format!("diff --git a/{caminho} b/{caminho}\n--- a/{caminho}\n+++ b/{caminho}\n");
    let patch = cab.clone() + &bs.iter().map(|b| b.texto.as_str()).collect::<String>();
    let completo = cab + &blocos(a, b, 1_000_000).iter().map(|b| b.texto.as_str()).collect::<String>();
    (patch, completo, bs)
}

fn linhas_de(t: &str) -> Vec<String> {
    if t.is_empty() {
        return Vec::new();
    }
    t.strip_suffix('\n').unwrap_or(t).split('\n').map(str::to_string).collect()
}

fn trocar_linhas(t: &str, em: usize, n: usize, novas: &[String]) -> String {
    let mut ls = linhas_de(t);
    ls.splice(em..(em + n).min(ls.len()), novas.iter().cloned());
    ls.iter().map(|l| format!("{l}\n")).collect()
}

const ATUAL: &str = "main";

impl Default for Simulado {
    fn default() -> Self {
        Self::new(ModoSimulado::Repo)
    }
}

impl Simulado {
    pub fn new(modo: ModoSimulado) -> Simulado {
        let d: Dados = serde_json::from_str(DADOS).expect("simulado.json");
        let editor_head = d.editor_head.join("\n") + "\n";
        let editor_index =
            editor_head.replace("  remove(paths: string[]): void;", "  remove(paths: string[], keep?: boolean): void;");
        let editor_work = editor_index
            .replace("useLayoutEffect, useRef, useState,", "useLayoutEffect, useMemo, useRef, useState,")
            .replace(
                "import { Columns2, Ellipsis, Pin, X } from 'lucide-react';",
                "import { ChevronDown, Columns2, Ellipsis, Pin, X } from 'lucide-react';",
            )
            .replace(
                "type DiskVersion = { text: string; bom: boolean; readOnly: boolean };",
                "type DiskVersion = { text: string; bom: boolean; readOnly: boolean; size: number };",
            );
        let sem_quebra_head: Vec<String> = (0..150).map(|i| format!("const v{i} = {i};")).collect();
        let mut sem_quebra_work: Vec<String> = sem_quebra_head[..2].to_vec();
        sem_quebra_work.extend((1..=4).map(|i| format!("const novo{i} = {i};")));
        sem_quebra_work.extend_from_slice(&sem_quebra_head[2..139]);
        sem_quebra_work.extend(["const troca1 = 1;".to_string(), "const troca2 = 2;".to_string()]);
        sem_quebra_work.extend_from_slice(&sem_quebra_head[140..149]);
        sem_quebra_work.push("const v149 = 1490;".into());
        let comparador_head = [
            "# Guia do comparador", "",
            "Este parágrafo descreve o comportamento anterior do comparador lado a lado e tem uma linha comprida para testar a quebra automática dentro da coluna sem criar rolagem horizontal.",
            "A calha mantém os números de linha e os sinais de cada lado.", "",
            "## Trecho com mais linhas no novo",
            "A versão antiga tinha somente esta explicação curta.",
            "", "## Trecho com menos linhas no novo",
            "Esta primeira linha explica em detalhes a configuração anterior do painel Git e como as alterações eram apresentadas ao usuário.",
            "Esta segunda linha continua a explicação anterior com outro texto longo para que a quebra apareça nas duas colunas.",
            "Esta terceira linha fecha o conteúdo antigo que será reduzido na nova versão.", "",
            "O restante do documento permanece igual depois das mudanças.",
        ];
        let comparador_work = [
            "# Guia do comparador", "",
            "Este parágrafo descreve o comportamento atual do comparador lado a lado e tem uma linha comprida que deve quebrar dentro da coluna mantendo a rolagem vertical e os números de linha visíveis.",
            "A calha mantém os números de linha e os sinais de cada lado.", "",
            "## Trecho com mais linhas no novo",
            "A versão nova mostra o primeiro detalhe.",
            "A versão nova mostra o segundo detalhe.",
            "A versão nova mostra o terceiro detalhe.",
            "", "## Trecho com menos linhas no novo",
            "O texto atualizado resume a configuração do painel Git numa única linha mais comprida, que deve quebrar automaticamente dentro da coluna sem afetar a calha.", "",
            "O restante do documento permanece igual depois das mudanças.",
        ];
        let app = "import { useState } from 'react';\n\nexport function App() {\n  const [route, setRoute] = useState('arquivos');\n  return <main>Orquestrador {route}</main>;\n}\n".to_string();
        let design = "# Design system\n\nA interface usa o Lobe UI, com Tailwind CSS v4. Tema escuro fixo.\nMonoespaçado só em terminal e valores técnicos.\n".to_string();
        let readme_head = "# Orquestrador\n\nUm app para Windows que põe CLIs de IA para trabalhar em papéis separados.\n".to_string();
        let mut textos = BTreeMap::new();
        let mut t = |k: &str, v: String| {
            textos.insert(k.to_string(), v);
        };
        t("README.md", readme_head.clone() + "O orquestrador lê o projeto e delega; os executores trabalham em paralelo.\n");
        t("docs/design-system.md", design.clone());
        t("src/App.tsx", app.clone());
        t("src/main.tsx", "import { createRoot } from 'react-dom/client';\nimport { App } from './App';\n\ncreateRoot(document.getElementById('root')!).render(<App />);\n".into());
        t("src/editor-workspace.tsx", editor_work);
        t("src/lib/utils.ts", "// Caminho relativo com \"/\": a raiz é vazia.\n// 🚀 join também usa \"/\" no Windows.\nexport const join = (dir: string, name: string) => (dir ? `${dir}/${name}` : name);\n".into());
        t("src/conflito.ts", "<<<<<<< ours\nexport const v = 2;\n=======\nexport const v = 3;\n>>>>>>> theirs\n".into());
        t("docs/apagado.md", "# Rascunho\n\nAjuste.\n".into());
        t("src/sem-quebra.ts", sem_quebra_work.join("\n"));
        t("docs/visual-comparador.md", comparador_work.join("\n") + "\n");
        t("src/marcadores.ts", d.marcadores_worktree.clone());
        t("docs/rascunho-painel.md", "# Rascunho do painel\n\nIdeias soltas.\n".into());
        let mut git = BTreeMap::new();
        git.insert("README.md".to_string(), (readme_head.clone(), readme_head));
        git.insert(
            "docs/design-system.md".to_string(),
            (design.replace("terminal e valores técnicos.", "terminal."), design),
        );
        git.insert(
            "src/App.tsx".to_string(),
            (
                app.replace("useState('arquivos')", "useState('painel')").replace("Orquestrador {route}", "Orquestrador · {route}"),
                app.replace("Orquestrador {route}", "Orquestrador · {route}"),
            ),
        );
        git.insert("src/editor-workspace.tsx".to_string(), (editor_head, editor_index));
        let sq = sem_quebra_head.join("\n") + "\n";
        git.insert("src/sem-quebra.ts".to_string(), (sq.clone(), sq));
        let cp = comparador_head.join("\n") + "\n";
        git.insert("docs/visual-comparador.md".to_string(), (cp.clone(), cp));
        git.insert("src/marcadores.ts".to_string(), (d.marcadores_head, d.marcadores_index));
        let m = Some(Kind::Modified);
        let arquivos = vec![
            entrada("README.md", None, m),
            entrada("docs/visual-comparador.md", None, m),
            entrada("docs/design-system.md", m, None),
            entrada("docs/rascunho-painel.md", None, Some(Kind::Untracked)),
            entrada("src/App.tsx", m, m),
            entrada("src/editor-workspace.tsx", m, m),
            entrada("src/sem-quebra.ts", None, m),
            entrada("src/marcadores.ts", m, m),
            entrada("dados/exportacao.json", None, m),
            entrada("assets/logo.png", None, m),
            entrada("src/conflito.ts", None, Some(Kind::Conflicted)),
            entrada("docs/apagado.md", None, Some(Kind::Conflicted)),
        ];
        let arq = |p: &str, k: Kind, o: Option<&str>| CommitFile { path: p.into(), orig_path: o.map(Into::into), kind: k };
        let rec = |hash: &str, autor: &str, data: String, assunto: &str, pais: &[&str], arquivos: Vec<CommitFile>| Rec {
            commit: Commit { hash: hash.into(), short: hash[..7].into(), author: autor.into(), date: data, subject: assunto.into() },
            pais: pais.iter().map(|p| p.to_string()).collect(),
            arquivos,
        };
        let (h1, h2, h3, h4) = (
            "9f3a1c2b7d4e5f60718293a4b5c6d7e8f9a0b1c2",
            "4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11",
            "d7e1a9c3b5f7081a2c4e6f8a0b2d4f6e8a0c2e4b",
            "a2c4e6f8b0d2e4f6a8c0b2d4e6f8a0c2e4b6d8f0",
        );
        let commits = vec![
            rec(h1, "Luan", dias_atras(4, 18, 42), "Prepara v0.1.7 com notas completas e publicação validada", &[h2],
                vec![arq("README.md", Kind::Modified, None), arq("docs/releases/v0.1.7.md", Kind::Added, None)]),
            rec(h2, "Luan", dias_atras(6, 11, 5), "Abre e edita arquivos no meio da tela", &[h4, h3], vec![
                arq("src/App.tsx", Kind::Modified, None), arq("src/lib/utils.ts", Kind::Renamed, Some("src/utils.ts")),
                arq("src/components/ui/card.tsx", Kind::Modified, None), arq("docs/design-system.md", Kind::Modified, None),
            ]),
            rec(h3, "Luan", dias_atras(16, 9, 30), "Corrige a ordem das pastas na árvore", &[h4],
                vec![arq("src/components/ui/card.tsx", Kind::Modified, None), arq("docs/design-system.md", Kind::Modified, None)]),
            rec(h4, "Luan", dias_atras(20, 16, 0), "Versão inicial do explorador", &[], vec![
                arq("README.md", Kind::Added, None), arq("src/App.tsx", Kind::Added, None), arq("src/main.tsx", Kind::Added, None),
                arq("src/utils.ts", Kind::Added, None), arq("src/components/ui/card.tsx", Kind::Added, None),
                arq("docs/design-system.md", Kind::Added, None),
            ]),
        ];
        let refs = vec![
            ("main".to_string(), RefKind::Branch, h1.to_string()),
            ("feat/painel-git".to_string(), RefKind::Branch, h3.to_string()),
            ("origin/main".to_string(), RefKind::Remote, h2.to_string()),
            ("v0.1.7".to_string(), RefKind::Tag, h1.to_string()),
        ];
        let mut conflitos = BTreeMap::new();
        conflitos.insert("src/conflito.ts".to_string(), (Some("export const v = 2;\n".to_string()), Some("export const v = 3;\n".to_string())));
        conflitos.insert("docs/apagado.md".to_string(), (Some("# Rascunho\n\nAjuste.\n".to_string()), None));
        Simulado(Mutex::new(Interno {
            modo,
            arquivos,
            textos,
            git,
            commits,
            refs,
            conflitos,
            patches_commit: BTreeMap::new(),
            chamadas: Vec::new(),
            falhar: BTreeMap::new(),
        }))
    }

    fn com<T>(&self, cmd: &str, f: impl FnOnce(&mut Interno) -> Result<T, String>) -> Result<T, String> {
        let mut i = self.0.lock().unwrap_or_else(|e| e.into_inner());
        i.chamadas.push(cmd.to_string());
        if let Some(erro) = i.falhar.get(cmd).cloned() {
            return Err(erro);
        }
        f(&mut i)
    }

    /// Comandos recebidos, em ordem (`git_status`, `git_stage`...).
    pub fn chamadas(&self) -> Vec<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).chamadas.clone()
    }

    pub fn contar(&self, cmd: &str) -> usize {
        self.chamadas().iter().filter(|c| *c == cmd).count()
    }

    pub fn trocar_modo(&self, modo: ModoSimulado) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).modo = modo;
    }

    /// Faz o comando falhar com a mensagem até `None`.
    pub fn falhar(&self, cmd: &str, erro: Option<&str>) {
        let mut i = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match erro {
            Some(e) => i.falhar.insert(cmd.to_string(), e.to_string()),
            None => i.falhar.remove(cmd),
        };
    }

    /// Conflito novo, como o `__mock.addConflict`.
    pub fn conflito(&self, caminho: &str, nossa: Option<&str>, deles: Option<&str>, texto: Option<&str>) {
        let mut i = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(t) = texto {
            i.textos.insert(caminho.into(), t.into());
        }
        i.conflitos.insert(caminho.into(), (nossa.map(Into::into), deles.map(Into::into)));
        i.arquivos.push(entrada(caminho, None, Some(Kind::Conflicted)));
    }

    /// Patch de commit fixo para um caminho, como `__mock.commitPatches`.
    pub fn patch_de_commit(&self, caminho: &str, patch: &str) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).patches_commit.insert(caminho.into(), patch.into());
    }

    /// Texto atual de um arquivo na pasta.
    pub fn texto(&self, caminho: &str) -> Option<String> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).textos.get(caminho).cloned()
    }

    /// Escreve na pasta por fora (como uma edição externa).
    pub fn escrever(&self, caminho: &str, texto: &str) {
        let mut i = self.0.lock().unwrap_or_else(|e| e.into_inner());
        i.textos.insert(caminho.into(), texto.into());
        i.sincronizar(caminho);
    }
}

impl Interno {
    fn sincronizar(&mut self, caminho: &str) {
        let Some(i) = self.arquivos.iter().position(|f| f.path == caminho) else { return };
        let Some((head, indice)) = self.git.get(caminho) else { return };
        let staged = (head != indice).then_some(Kind::Modified);
        let unstaged = (Some(indice) != self.textos.get(caminho)).then_some(Kind::Modified);
        if staged.is_none() && unstaged.is_none() {
            self.arquivos.remove(i);
        } else {
            self.arquivos[i].staged = staged;
            self.arquivos[i].unstaged = unstaged;
        }
    }

    fn lado(&self, caminho: &str, staged: bool) -> (String, String, Vec<Bloco>) {
        let Some((head, indice)) = self.git.get(caminho) else { return (String::new(), String::new(), Vec::new()) };
        if staged {
            diff_de(caminho, head, indice)
        } else {
            diff_de(caminho, indice, self.textos.get(caminho).map(String::as_str).unwrap_or(""))
        }
    }

    fn revisao(&self, caminho: &str) -> String {
        match self.textos.get(caminho) {
            Some(t) => djb2(t),
            None => "ausente".into(),
        }
    }

    fn ponta(&self, nome: &str) -> String {
        self.refs.iter().find(|r| r.0 == nome).map(|r| r.2.clone()).unwrap_or_default()
    }

    fn ancestrais(&self, de: &[String]) -> Vec<String> {
        let mut vistos: Vec<String> = Vec::new();
        let mut pilha = de.to_vec();
        while let Some(h) = pilha.pop() {
            if vistos.contains(&h) {
                continue;
            }
            vistos.push(h.clone());
            if let Some(c) = self.commits.iter().find(|c| c.commit.hash == h) {
                pilha.extend(c.pais.iter().cloned());
            }
        }
        vistos
    }

    fn refs_em(&self, hash: &str) -> Vec<GitRef> {
        let mut v: Vec<GitRef> = self
            .refs
            .iter()
            .filter(|r| r.2 == hash)
            .map(|r| GitRef { name: r.0.clone(), kind: r.1, current: r.1 == RefKind::Branch && r.0 == ATUAL })
            .collect();
        v.sort_by(|a, b| b.current.cmp(&a.current).then(a.kind.cmp(&b.kind)));
        v
    }

    fn pagina(&self, lista: Vec<&Rec>, limite: usize, pular: usize) -> Vec<GraphCommit> {
        lista
            .into_iter()
            .skip(pular)
            .take(limite.clamp(1, 200))
            .map(|c| GraphCommit { commit: c.commit.clone(), parents: c.pais.clone(), refs: self.refs_em(&c.commit.hash) })
            .collect()
    }

    fn por_id(&self, id: &str) -> Result<&Rec, String> {
        if id.len() < 4 || id.len() > 64 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("Commit inválido: {id}"));
        }
        let id = id.to_lowercase();
        self.commits.iter().find(|c| c.commit.hash.starts_with(&id)).ok_or(format!("Commit não encontrado: {id}"))
    }

    fn bloco(&self, r: &HunkRef, staged: bool) -> Result<Bloco, String> {
        let (patch, _, mut bs) = self.lado(&r.path, staged);
        if r.revision != djb2(&patch) {
            return Err("O arquivo mudou desde que o diff foi lido. Nada foi feito; atualize a tela.".into());
        }
        if r.index >= bs.len() || bs[r.index].cabecalho != r.header {
            return Err("O bloco não existe mais. Nada foi feito; atualize a tela.".into());
        }
        Ok(bs.swap_remove(r.index))
    }

}

impl FonteGit for Simulado {
    fn estado(&self) -> Result<Estado, String> {
        self.com("git_status", |i| {
            Ok(match i.modo {
                ModoSimulado::Repo => {
                    let mine = i.ancestrais(&[i.ponta(ATUAL)]);
                    let theirs = i.ancestrais(&[i.ponta("origin/main")]);
                    let mut files = i.arquivos.clone();
                    files.sort_by(|a, b| crate::colagem::comparar(&a.path, &b.path));
                    Estado::Repo {
                        status: Status {
                            branch: ATUAL.into(),
                            head: i.ponta(ATUAL),
                            ahead: mine.iter().filter(|h| !theirs.contains(h)).count() as u32,
                            behind: theirs.iter().filter(|h| !mine.contains(h)).count() as u32,
                            files,
                        },
                    }
                }
                ModoSimulado::SemRepo => Estado::NoRepo,
                ModoSimulado::SemGit => Estado::NoGit,
                ModoSimulado::RepoAcima => Estado::RepoAbove { root: "C:\\projetos".into() },
            })
        })
    }
    fn init(&self) -> Result<(), String> {
        self.com("git_init", |i| match i.modo {
            ModoSimulado::SemRepo => {
                i.modo = ModoSimulado::Repo;
                Ok(())
            }
            ModoSimulado::Repo => Err("A pasta já é um repositório Git.".into()),
            ModoSimulado::SemGit => Err("O Git não foi encontrado neste computador.".into()),
            ModoSimulado::RepoAcima => Err("A pasta está dentro de outro repositório.".into()),
        })
    }
    fn stage(&self, caminhos: &[String]) -> Result<(), String> {
        self.com("git_stage", |i| {
            let todos: Vec<String> =
                if caminhos.is_empty() { i.arquivos.iter().map(|f| f.path.clone()).collect() } else { caminhos.to_vec() };
            for p in todos {
                let Some(e) = i.arquivos.iter_mut().find(|f| f.path == p) else { continue };
                if e.unstaged == Some(Kind::Conflicted) {
                    continue;
                }
                if e.unstaged == Some(Kind::Untracked) {
                    e.staged = Some(Kind::Added);
                    e.unstaged = None;
                } else if i.git.contains_key(&p) {
                    let t = i.textos.get(&p).cloned().unwrap_or_default();
                    if let Some(g) = i.git.get_mut(&p) {
                        g.1 = t;
                    }
                    i.sincronizar(&p);
                }
            }
            Ok(())
        })
    }
    fn unstage(&self, caminhos: &[String]) -> Result<(), String> {
        self.com("git_unstage", |i| {
            let todos: Vec<String> =
                if caminhos.is_empty() { i.arquivos.iter().map(|f| f.path.clone()).collect() } else { caminhos.to_vec() };
            for p in todos {
                let Some(e) = i.arquivos.iter_mut().find(|f| f.path == p) else { continue };
                if e.staged == Some(Kind::Added) {
                    e.staged = None;
                    e.unstaged = Some(Kind::Untracked);
                } else if let Some(g) = i.git.get_mut(&p) {
                    g.1 = g.0.clone();
                    i.sincronizar(&p);
                }
            }
            Ok(())
        })
    }
    fn commit(&self, mensagem: &str) -> Result<(), String> {
        self.com("git_commit", |i| {
            if mensagem.trim().is_empty() {
                return Err("Escreva a mensagem do commit.".into());
            }
            let preparados: Vec<GitFile> = i.arquivos.iter().filter(|f| f.staged.is_some()).cloned().collect();
            if preparados.is_empty() {
                return Err("Nada preparado para o commit.".into());
            }
            let id = djb2(&format!("{mensagem}{}", Local::now().timestamp_nanos_opt().unwrap_or(0))).repeat(5);
            let pai = i.ponta(ATUAL);
            let novo = Rec {
                commit: Commit {
                    hash: id.clone(),
                    short: id[..7].into(),
                    author: "Luan".into(),
                    date: Local::now().to_rfc3339(),
                    subject: mensagem.lines().next().unwrap_or("").into(),
                },
                pais: vec![pai],
                arquivos: preparados
                    .iter()
                    .map(|f| CommitFile { path: f.path.clone(), orig_path: f.orig_path.clone(), kind: f.staged.unwrap_or(Kind::Modified) })
                    .collect(),
            };
            i.commits.insert(0, novo);
            if let Some(r) = i.refs.iter_mut().find(|r| r.0 == ATUAL) {
                r.2 = id;
            }
            for f in preparados {
                if let Some(g) = i.git.get_mut(&f.path) {
                    g.0 = g.1.clone();
                    i.sincronizar(&f.path);
                } else if f.unstaged.is_none() {
                    i.arquivos.retain(|x| x.path != f.path);
                } else if let Some(x) = i.arquivos.iter_mut().find(|x| x.path == f.path) {
                    x.staged = None;
                }
            }
            Ok(())
        })
    }
    fn fetch(&self) -> Result<String, String> {
        self.com("git_fetch", |_| Ok("From github.com:exemplo/orquestrador\n   1a2b3c4..5d6e7f8  main -> origin/main\n".into()))
    }
    fn pull(&self) -> Result<String, String> {
        self.com("git_pull", |_| Ok("Already up to date.\n".into()))
    }
    fn push(&self) -> Result<String, String> {
        self.com("git_push", |_| Ok("Enviando 1 commit(s) para origin/main\n   1a2b3c4..5d6e7f8  main -> main\n".into()))
    }
    fn branches(&self) -> Result<Branches, String> {
        self.com("git_branches", |_| Ok(Branches { current: "main".into(), local: vec!["main".into(), "feat/painel-git".into()] }))
    }
    fn switch(&self, _nome: &str) -> Result<(), String> {
        self.com("git_switch", |_| Ok(()))
    }
    fn create_branch(&self, _nome: &str) -> Result<(), String> {
        self.com("git_create_branch", |_| Ok(()))
    }
    fn review_diff(&self, caminho: &str, staged: bool) -> Result<ReviewDiff, String> {
        self.com("git_review_diff", |i| {
            let vazio = Diff { binary: false, too_large: false, patch: String::new() };
            if caminho.ends_with(".png") {
                let d = Diff { binary: true, ..vazio };
                return Ok(ReviewDiff { diff: d.clone(), display: d, revision: djb2(""), untracked: false });
            }
            if caminho == "dados/exportacao.json" {
                let d = Diff { too_large: true, ..vazio };
                return Ok(ReviewDiff { diff: d.clone(), display: d, revision: djb2(""), untracked: false });
            }
            let e = i.arquivos.iter().find(|f| f.path == caminho);
            let novo = if staged { e.and_then(|e| e.staged) == Some(Kind::Added) } else { e.and_then(|e| e.unstaged) == Some(Kind::Untracked) };
            if novo {
                let texto = i.textos.get(caminho).cloned().unwrap_or_default();
                let mut ls: Vec<&str> = texto.split('\n').collect();
                if ls.last() == Some(&"") {
                    ls.pop();
                }
                let patch = format!(
                    "diff --git a/{caminho} b/{caminho}\nnew file mode 100644\n--- /dev/null\n+++ b/{caminho}\n@@ -0,0 +1,{} @@\n{}",
                    ls.len(),
                    ls.iter().map(|l| format!("+{l}\n")).collect::<String>()
                );
                let d = Diff { binary: false, too_large: false, patch: patch.clone() };
                return Ok(ReviewDiff { diff: d.clone(), display: d, revision: djb2(&patch), untracked: !staged });
            }
            let (patch, completo, _) = i.lado(caminho, staged);
            Ok(ReviewDiff {
                diff: Diff { binary: false, too_large: false, patch: patch.clone() },
                display: Diff { binary: false, too_large: false, patch: completo },
                revision: djb2(&patch),
                untracked: false,
            })
        })
    }
    fn stage_hunk(&self, r: &HunkRef) -> Result<(), String> {
        self.com("git_stage_hunk", |i| {
            let b = i.bloco(r, false)?;
            if let Some(g) = i.git.get_mut(&r.path) {
                g.1 = trocar_linhas(&g.1, b.antigo, b.antigo_n, &b.novas);
            }
            i.sincronizar(&r.path);
            Ok(())
        })
    }
    fn unstage_hunk(&self, r: &HunkRef) -> Result<(), String> {
        self.com("git_unstage_hunk", |i| {
            let b = i.bloco(r, true)?;
            if let Some(g) = i.git.get_mut(&r.path) {
                g.1 = trocar_linhas(&g.1, b.novo, b.novo_n, &b.antigas);
            }
            i.sincronizar(&r.path);
            Ok(())
        })
    }
    fn discard_hunk(&self, r: &HunkRef) -> Result<(), String> {
        self.com("git_discard_hunk", |i| {
            let b = i.bloco(r, false)?;
            let t = i.textos.get(&r.path).cloned().unwrap_or_default();
            i.textos.insert(r.path.clone(), trocar_linhas(&t, b.novo, b.novo_n, &b.antigas));
            i.sincronizar(&r.path);
            Ok(())
        })
    }
    fn discard_file(&self, caminho: &str, revisao: &str) -> Result<(), String> {
        self.com("git_discard_file", |i| {
            let e = i.arquivos.iter().find(|f| f.path == caminho);
            if e.is_none_or(|e| e.unstaged == Some(Kind::Untracked)) {
                return Err(format!("{caminho} não está rastreado: o Git não o descarta. Mande-o para a Lixeira pela tela."));
            }
            if revisao != djb2(&i.lado(caminho, false).0) {
                return Err("O arquivo mudou desde que o diff foi lido. Nada foi descartado; atualize a tela.".into());
            }
            if let Some(g) = i.git.get(caminho).cloned() {
                i.textos.insert(caminho.into(), g.1);
            }
            i.sincronizar(caminho);
            Ok(())
        })
    }
    fn history(&self, limite: usize, pular: usize, caminho: Option<&str>) -> Result<Vec<GraphCommit>, String> {
        self.com("git_history", |i| {
            let alc = i.ancestrais(&[i.ponta(ATUAL)]);
            let lista: Vec<&Rec> = i
                .commits
                .iter()
                .filter(|c| alc.contains(&c.commit.hash))
                .filter(|c| caminho.is_none_or(|p| c.arquivos.iter().any(|f| f.path == p || f.orig_path.as_deref() == Some(p))))
                .collect();
            Ok(i.pagina(lista, limite, pular))
        })
    }
    fn graph(&self, limite: usize, pular: usize) -> Result<Vec<GraphCommit>, String> {
        self.com("git_graph", |i| {
            let alc = i.ancestrais(&[i.ponta(ATUAL), i.ponta("origin/main")]);
            let lista: Vec<&Rec> = i.commits.iter().filter(|c| alc.contains(&c.commit.hash)).collect();
            Ok(i.pagina(lista, limite, pular))
        })
    }
    fn commit_detail(&self, id: &str) -> Result<CommitDetail, String> {
        self.com("git_commit_detail", |i| {
            let c = i.por_id(id)?;
            Ok(CommitDetail { commit: c.commit.clone(), files: c.arquivos.clone() })
        })
    }
    fn commit_hover(&self, hash: &str) -> Result<CommitHoverDetail, String> {
        self.com("git_commit_hover", |i| {
            let c = i.por_id(hash)?;
            let fixo = c.commit.short == "9f3a1c2";
            Ok(CommitHoverDetail {
                author: c.commit.author.clone(),
                date: c.commit.date.clone(),
                message: if fixo {
                    "Prepara v0.1.7 com notas completas e publicação validada\n\nConsolida as notas e valida o instalador.\n\nCo-Authored-By: Ana Exemplo <ana@example.com>".into()
                } else {
                    c.commit.subject.clone()
                },
                files_changed: if fixo { 2 } else { c.arquivos.len() },
                insertions: if fixo { 14 } else { c.arquivos.len() * 3 },
                deletions: if fixo { 3 } else { c.arquivos.iter().filter(|f| f.kind != Kind::Added).count() },
                github_url: Some(format!("https://github.com/exemplo/orquestrador/commit/{}", c.commit.hash)),
            })
        })
    }
    fn commit_diff(&self, id: &str, caminho: &str) -> Result<Diff, String> {
        self.com("git_commit_diff", |i| {
            let c = i.por_id(id)?;
            let patch = c
                .arquivos
                .iter()
                .filter(|f| f.path == caminho || f.orig_path.as_deref() == Some(caminho))
                .map(|f| {
                    i.patches_commit.get(&f.path).cloned().unwrap_or_else(|| {
                        let p = &f.path;
                        if f.kind == Kind::Added {
                            format!("diff --git a/{p} b/{p}\nnew file mode 100644\n--- /dev/null\n+++ b/{p}\n@@ -0,0 +1 @@\n+depois\n")
                        } else {
                            format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-antes\n+depois\n")
                        }
                    })
                })
                .collect();
            Ok(Diff { binary: false, too_large: false, patch })
        })
    }
    fn conflict(&self, caminho: &str) -> Result<Conflict, String> {
        self.com("git_conflict", |i| {
            let (ours, theirs) = i.conflitos.get(caminho).cloned().ok_or(format!("{caminho} não está em conflito."))?;
            Ok(Conflict { ours, theirs, worktree: i.textos.get(caminho).cloned(), binary: false, revision: i.revisao(caminho) })
        })
    }
    fn write_resolution(&self, caminho: &str, texto: &str, revisao: &str, resolvido: bool) -> Result<(), String> {
        self.com("git_write_resolution", |i| {
            if !i.conflitos.contains_key(caminho) {
                return Err(format!("{caminho} não está em conflito."));
            }
            if revisao != i.revisao(caminho) {
                return Err("O arquivo mudou desde que foi lido. Nada foi gravado; atualize a tela.".into());
            }
            if resolvido && texto.lines().any(|l| l.starts_with("<<<<<<<") || l.starts_with("=======") || l.starts_with(">>>>>>>")) {
                return Err("Ainda há marcadores de conflito no texto (<<<<<<<, =======, >>>>>>>). Remova-os antes de marcar como resolvido.".into());
            }
            i.textos.insert(caminho.into(), texto.into());
            if resolvido {
                i.conflitos.remove(caminho);
                if let Some(f) = i.arquivos.iter_mut().find(|f| f.path == caminho) {
                    *f = entrada(caminho, Some(Kind::Modified), None);
                }
            }
            Ok(())
        })
    }
    fn accept_deletion(&self, caminho: &str, revisao: &str) -> Result<(), String> {
        self.com("git_accept_deletion", |i| {
            let (o, t) = i.conflitos.get(caminho).cloned().ok_or(format!("{caminho} não está em conflito."))?;
            if o.is_some() && t.is_some() {
                return Err("Os dois lados têm conteúdo: resolva o texto em vez de aceitar a remoção.".into());
            }
            if revisao != i.revisao(caminho) {
                return Err("O arquivo mudou desde que foi lido. Nada foi removido; atualize a tela.".into());
            }
            i.textos.remove(caminho);
            i.conflitos.remove(caminho);
            if let Some(f) = i.arquivos.iter_mut().find(|f| f.path == caminho) {
                *f = entrada(caminho, Some(Kind::Deleted), None);
            }
            Ok(())
        })
    }
    fn abrir_url(&self, _url: &str) -> Result<(), String> {
        self.com("open_url", |_| Ok(()))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn preparar_bloco_pela_revisao_vista() {
        let s = Simulado::default();
        let r = s.review_diff("src/editor-workspace.tsx", false).unwrap();
        let blocos = crate::comparar::blocos_patch(&r.diff.patch);
        assert_eq!(blocos.len(), 2, "a pasta muda as linhas 28 e 32 (um bloco) e a 96 (outro)");
        let h = HunkRef { path: "src/editor-workspace.tsx".into(), index: 0, header: blocos[0].cabecalho.clone(), revision: r.revision.clone() };
        s.stage_hunk(&h).unwrap();
        assert!(s.stage_hunk(&h).is_err(), "revisão velha é recusada");
        let r2 = s.review_diff("src/editor-workspace.tsx", false).unwrap();
        assert_eq!(crate::comparar::blocos_patch(&r2.diff.patch).len(), 1);
    }
}
