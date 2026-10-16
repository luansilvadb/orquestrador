//! As issues do GitHub do repositório aberto. O GitHub é a única fonte: nada vai para o disco. O token é o do
//! GitHub CLI já logado (`gh auth token`); as chamadas vão direto à API REST, com `ETag`/`If-None-Match` para que a
//! consulta repetida da tela, sem mudança, volte 304. O token nunca sai daqui: nem em erro, nem em log, nem na tela.
use crate::proc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Stdio;
use std::sync::Mutex;
use std::time::Duration;
use ureq::Agent;

const API: &str = "https://api.github.com";
const LIMITE: &str = "O limite de chamadas do GitHub acabou. Tente de novo em alguns minutos.";

/// Por que a tela de issues não pode usar o GitHub.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Reason {
    NoGh,
    NoLogin,
    NoRemote,
    NoAccess,
    IssuesDisabled,
    Offline,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum GhStatus {
    Ready { owner: String, repo: String, login: String },
    Unavailable { reason: Reason, message: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IssueLabel {
    pub name: String,
    /// Hex sem `#`.
    pub color: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub number: u64,
    pub title: String,
    /// Vazio quando o GitHub manda `null`.
    pub body: String,
    pub state: IssueState,
    pub labels: Vec<IssueLabel>,
    /// O `user.login`.
    pub author: String,
    pub comments: u64,
    /// As datas em ISO 8601, como vêm do GitHub.
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
    /// O `html_url`.
    pub url: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Label {
    pub name: String,
    pub color: String,
    /// Vazio quando o GitHub manda `null`.
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IssuesData {
    /// As abertas (mais novas primeiro) e depois as 50 fechadas mais recentes, sem repetir número.
    pub issues: Vec<Issue>,
    pub labels: Vec<Label>,
}

/// `data` vem `None` quando nada mudou desde a `version` que a tela já tem.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IssuesSnapshot {
    pub version: u64,
    pub data: Option<IssuesData>,
}

/// O repositório do GitHub (`dono/repo`) apontado pelo `origin`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Slug {
    pub owner: String,
    pub repo: String,
}

impl std::fmt::Display for Slug {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

/// Por que uma chamada não deu certo. O texto para a tela sai de `texto`, que precisa do repositório.
#[derive(Debug, PartialEq)]
enum Falha {
    Fora(Reason),
    Limite,
    /// Texto já pronto para a tela.
    Github(String),
}

impl Falha {
    fn texto(&self, alvo: &str) -> String {
        match self {
            Falha::Fora(reason) => mensagem(*reason, alvo),
            Falha::Limite => LIMITE.to_string(),
            Falha::Github(texto) => texto.clone(),
        }
    }
}

/// O texto de cada motivo, como a tela mostra. `alvo` é `dono/repo`.
fn mensagem(reason: Reason, alvo: &str) -> String {
    match reason {
        Reason::NoGh => "O GitHub CLI (gh) não foi encontrado neste computador. Instale o gh e rode `gh auth login`.".to_string(),
        Reason::NoLogin => "O GitHub CLI não está logado. Rode `gh auth login` num terminal e tente de novo.".to_string(),
        Reason::NoRemote => "O remoto `origin` deste repositório não está no GitHub.".to_string(),
        Reason::NoAccess => format!("Sua conta do GitHub não tem acesso a {alvo}."),
        Reason::IssuesDisabled => format!("As issues estão desligadas em {alvo}."),
        Reason::Offline => "Sem conexão com o GitHub.".to_string(),
    }
}

/// `dono/repo` de uma URL do `origin`: `https://github.com/...`, `http(s)://usuario@github.com/...`,
/// `git@github.com:...` e `ssh://git@github.com/...`, com ou sem `.git`. Outro host é `None`.
fn slug_do_remoto(url: &str) -> Option<Slug> {
    let url = url.trim();
    let esquema = ["https://", "http://", "ssh://"].iter().find_map(|e| url.strip_prefix(e));
    let (host, caminho) = match esquema {
        Some(resto) => resto.split_once('/')?,
        // O formato do scp, `usuario@host:caminho`.
        None => url.split_once(':')?,
    };
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let caminho = caminho.trim_end_matches('/');
    let caminho = caminho.strip_suffix(".git").unwrap_or(caminho);
    let (owner, repo) = caminho.split_once('/')?;
    let valido = |s: &str| !s.is_empty() && s != "." && s != ".." && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    (valido(owner) && valido(repo)).then(|| Slug { owner: owner.to_string(), repo: repo.to_string() })
}

/// O repositório do GitHub do projeto, pelo `origin`. Sem Git, sem repositório, sem `origin` ou fora do GitHub, `None`.
fn slug_da_pasta(dir: &Path) -> Option<Slug> {
    slug_do_remoto(&proc::git_saida(dir, &["remote", "get-url", "origin"]).ok()?)
}

/// A URL com `rel="next"` de um cabeçalho `Link` do GitHub.
fn proxima(link: &str) -> Option<String> {
    link.split(',').find_map(|parte| {
        let (url, params) = parte.trim().strip_prefix('<')?.split_once('>')?;
        params.split(';').any(|p| p.trim() == "rel=\"next\"").then(|| url.to_string())
    })
}

/// O token do GitHub CLI. Sem o `gh`, `NoGh`; com erro ou saída vazia, `NoLogin`.
fn token_do_gh() -> Result<String, Reason> {
    let out = proc::quiet("gh").args(["auth", "token"]).stdin(Stdio::null()).output().map_err(|_| Reason::NoGh)?;
    let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || token.is_empty() {
        return Err(Reason::NoLogin);
    }
    Ok(token)
}

#[derive(Deserialize)]
struct LabelGh {
    name: String,
    color: Option<String>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct UserGh {
    login: String,
}

#[derive(Deserialize)]
struct IssueGh {
    number: u64,
    title: String,
    body: Option<String>,
    state: IssueState,
    labels: Vec<LabelGh>,
    user: Option<UserGh>,
    comments: u64,
    created_at: String,
    updated_at: String,
    closed_at: Option<String>,
    html_url: String,
}

impl From<IssueGh> for Issue {
    fn from(i: IssueGh) -> Issue {
        Issue {
            number: i.number,
            title: i.title,
            body: i.body.unwrap_or_default(),
            state: i.state,
            labels: i.labels.into_iter().map(|l| IssueLabel { name: l.name, color: l.color.unwrap_or_default() }).collect(),
            author: i.user.map(|u| u.login).unwrap_or_default(),
            comments: i.comments,
            created_at: i.created_at,
            updated_at: i.updated_at,
            closed_at: i.closed_at,
            url: i.html_url,
        }
    }
}

impl<'de> Deserialize<'de> for IssueState {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match String::deserialize(d)?.as_str() {
            "closed" => Ok(IssueState::Closed),
            _ => Ok(IssueState::Open),
        }
    }
}

fn ilegivel(e: serde_json::Error) -> String {
    format!("O GitHub mandou uma resposta que não deu para ler: {e}")
}

/// Uma issue do JSON do GitHub.
fn issue_do_json(corpo: &str) -> Result<Issue, String> {
    serde_json::from_str::<IssueGh>(corpo).map(Issue::from).map_err(ilegivel)
}

/// As issues de uma página da lista. O endpoint também devolve pull requests: os itens com a chave `pull_request` saem.
fn issues_do_json(corpo: &str) -> Result<Vec<Issue>, String> {
    let itens: Vec<Value> = serde_json::from_str(corpo).map_err(ilegivel)?;
    itens.into_iter().filter(|v| v.get("pull_request").is_none()).map(|v| serde_json::from_value::<IssueGh>(v).map(Issue::from).map_err(ilegivel)).collect()
}

fn labels_do_json(corpo: &str) -> Result<Vec<Label>, String> {
    let labels: Vec<LabelGh> = serde_json::from_str(corpo).map_err(ilegivel)?;
    Ok(labels.into_iter().map(|l| Label { name: l.name, color: l.color.unwrap_or_default(), description: l.description.unwrap_or_default() }).collect())
}

/// A mensagem de erro do GitHub: o `message` e, se houver, o detalhe de cada item de `errors`.
fn mensagem_do_github(corpo: &str) -> String {
    let Ok(v) = serde_json::from_str::<Value>(corpo) else { return String::new() };
    let principal = v.get("message").and_then(Value::as_str).unwrap_or_default().to_string();
    let detalhes: Vec<String> = v
        .get("errors")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| match e.get("message").and_then(Value::as_str) {
            Some(m) => Some(m.to_string()),
            None => {
                let campo = e.get("field").and_then(Value::as_str)?;
                Some(format!("{campo}: {}", e.get("code").and_then(Value::as_str).unwrap_or("invalid")))
            }
        })
        .collect();
    if detalhes.is_empty() { principal } else { format!("{principal} ({})", detalhes.join("; ")) }
}

/// O que uma resposta HTTP sem sucesso quer dizer. `sem_cota` é o `x-ratelimit-remaining: 0` (ou um `retry-after`,
/// do limite secundário). O 401 só chega aqui depois de o token ter sido pedido de novo ao `gh`.
fn falha_do_status(status: u16, sem_cota: bool, corpo: &str) -> Falha {
    let do_github = mensagem_do_github(corpo);
    match status {
        401 => Falha::Fora(Reason::NoLogin),
        403 | 429 if sem_cota || status == 429 => Falha::Limite,
        404 => Falha::Fora(Reason::NoAccess),
        410 => Falha::Fora(Reason::IssuesDisabled),
        422 if !do_github.is_empty() => Falha::Github(do_github),
        _ if do_github.is_empty() => Falha::Github(format!("O GitHub respondeu {status}.")),
        _ => Falha::Github(format!("O GitHub respondeu {status}: {do_github}")),
    }
}

/// O que interessa de uma resposta HTTP.
struct Resposta {
    status: u16,
    etag: Option<String>,
    link: Option<String>,
    sem_cota: bool,
    corpo: String,
}

impl Resposta {
    fn falha(&self) -> Falha {
        falha_do_status(self.status, self.sem_cota, &self.corpo)
    }
}

/// Uma página consultada: o ETag, o corpo e a próxima página (o 304 não traz o `Link`).
#[derive(Clone)]
struct Pagina {
    etag: Option<String>,
    corpo: String,
    proxima: Option<String>,
}

#[derive(Default)]
struct Cache {
    /// O repositório das páginas guardadas. Outro repositório zera as páginas, não a versão.
    alvo: Option<Slug>,
    paginas: HashMap<String, Pagina>,
    /// Sobe a cada mudança vista; nunca volta, para a tela não confundir o retrato de um repositório com o de outro.
    version: u64,
}

/// O acesso ao GitHub: a conexão mantida aberta, o token do `gh` e o cache das listas, só em memória.
pub struct Hub {
    agent: Agent,
    token: Mutex<Option<String>>,
    cache: Mutex<Cache>,
}

impl Default for Hub {
    fn default() -> Self {
        Self::new()
    }
}

impl Hub {
    pub fn new() -> Hub {
        let config = Agent::config_builder().timeout_global(Some(Duration::from_secs(10))).http_status_as_error(false).build();
        Hub { agent: Agent::new_with_config(config), token: Mutex::new(None), cache: Mutex::new(Cache::default()) }
    }

    fn token(&self) -> Result<String, Falha> {
        let mut guardado = self.token.lock().unwrap();
        if let Some(token) = guardado.as_ref() {
            return Ok(token.clone());
        }
        let token = token_do_gh().map_err(Falha::Fora)?;
        *guardado = Some(token.clone());
        Ok(token)
    }

    /// Uma requisição com os cabeçalhos do GitHub. `corpo` com `metodo` POST ou PATCH; sem corpo é GET, com o
    /// `If-None-Match` quando há `etag`. Falha de rede é `Offline`; no 401, o token é pedido de novo ao `gh` uma vez.
    fn chamar(&self, metodo: &str, url: &str, etag: Option<&str>, corpo: Option<&str>) -> Result<Resposta, Falha> {
        for _ in 0..2 {
            let token = self.token()?;
            let resposta = self.enviar(&token, metodo, url, etag, corpo).map_err(|_| Falha::Fora(Reason::Offline))?;
            if resposta.status != 401 {
                return Ok(resposta);
            }
            *self.token.lock().unwrap() = None;
        }
        Err(Falha::Fora(Reason::NoLogin))
    }

    fn enviar(&self, token: &str, metodo: &str, url: &str, etag: Option<&str>, corpo: Option<&str>) -> Result<Resposta, ureq::Error> {
        let auth = format!("Bearer {token}");
        let mut resposta = match corpo {
            None => {
                let mut pedido = self.agent.get(url).header("Authorization", &auth).header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28").header("User-Agent", "orquestrador");
                if let Some(etag) = etag {
                    pedido = pedido.header("If-None-Match", etag);
                }
                pedido.call()?
            }
            Some(json) => {
                let pedido = if metodo == "PATCH" { self.agent.patch(url) } else { self.agent.post(url) };
                pedido.header("Authorization", &auth).header("Accept", "application/vnd.github+json").header("X-GitHub-Api-Version", "2022-11-28").header("User-Agent", "orquestrador").header("Content-Type", "application/json").send(json)?
            }
        };
        let cabecalho = |nome: &str| resposta.headers().get(nome).and_then(|v| v.to_str().ok()).map(str::to_string);
        let (etag, link) = (cabecalho("etag"), cabecalho("link"));
        let sem_cota = cabecalho("x-ratelimit-remaining").as_deref() == Some("0") || cabecalho("retry-after").is_some();
        let status = resposta.status().as_u16();
        let corpo = resposta.body_mut().read_to_string()?;
        Ok(Resposta { status, etag, link, sem_cota, corpo })
    }

    /// Um GET que só aceita 200, devolvendo o JSON.
    fn ler(&self, url: &str) -> Result<Value, Falha> {
        let resposta = self.chamar("GET", url, None, None)?;
        if resposta.status != 200 {
            return Err(resposta.falha());
        }
        serde_json::from_str(&resposta.corpo).map_err(|e| Falha::Github(ilegivel(e)))
    }

    /// O estado da integração para a tela. Erro só no que não é um dos motivos (limite de chamadas, resposta estranha).
    pub fn status(&self, dir: &Path) -> Result<GhStatus, String> {
        let Some(slug) = slug_da_pasta(dir) else {
            return Ok(GhStatus::Unavailable { reason: Reason::NoRemote, message: mensagem(Reason::NoRemote, "") });
        };
        let checar = || -> Result<String, Falha> {
            let user = self.ler(&format!("{API}/user"))?;
            let login = user.get("login").and_then(Value::as_str).unwrap_or_default().to_string();
            let repo = self.ler(&format!("{API}/repos/{slug}"))?;
            if repo.get("has_issues").and_then(Value::as_bool) == Some(false) {
                return Err(Falha::Fora(Reason::IssuesDisabled));
            }
            Ok(login)
        };
        match checar() {
            Ok(login) => Ok(GhStatus::Ready { owner: slug.owner, repo: slug.repo, login }),
            Err(Falha::Fora(reason)) => Ok(GhStatus::Unavailable { reason, message: mensagem(reason, &slug.to_string()) }),
            Err(falha) => Err(falha.texto(&slug.to_string())),
        }
    }

    /// Uma página com o `If-None-Match` do que está guardado. Devolve a página e se ela mudou.
    fn pagina(&self, url: &str) -> Result<(Pagina, bool), Falha> {
        let antes = self.cache.lock().unwrap().paginas.get(url).cloned();
        let resposta = self.chamar("GET", url, antes.as_ref().and_then(|p| p.etag.as_deref()), None)?;
        match (resposta.status, antes) {
            (304, Some(antes)) => Ok((antes, false)),
            (200, antes) => {
                // O token só segue para a própria API.
                let proxima = resposta.link.as_deref().and_then(proxima).filter(|u| u.starts_with(&format!("{API}/")));
                let mudou = antes.map_or(true, |a| a.corpo != resposta.corpo);
                Ok((Pagina { etag: resposta.etag, corpo: resposta.corpo, proxima }, mudou))
            }
            _ => Err(resposta.falha()),
        }
    }

    /// As abertas (todas as páginas), as 50 fechadas mais recentes e os labels. Com `since_version` igual à versão
    /// atual, `data` volta `None`.
    pub fn issues(&self, dir: &Path, since_version: u64) -> Result<IssuesSnapshot, String> {
        let slug = slug_da_pasta(dir).ok_or_else(|| mensagem(Reason::NoRemote, ""))?;
        let alvo = slug.to_string();
        {
            let mut cache = self.cache.lock().unwrap();
            if cache.alvo.as_ref() != Some(&slug) {
                cache.paginas.clear();
                cache.alvo = Some(slug.clone());
            }
        }
        let base = format!("{API}/repos/{alvo}");
        let mut abertas = Vec::new();
        let mut url = Some(format!("{base}/issues?state=open&per_page=100&sort=created&direction=desc"));
        while let Some(atual) = url {
            let (pagina, mudou) = self.pagina(&atual).map_err(|f| f.texto(&alvo))?;
            url = pagina.proxima.clone();
            abertas.push((atual, pagina, mudou));
        }
        let mut lidas = abertas;
        for atual in [format!("{base}/issues?state=closed&per_page=50&sort=updated&direction=desc"), format!("{base}/labels?per_page=100")] {
            let (pagina, mudou) = self.pagina(&atual).map_err(|f| f.texto(&alvo))?;
            lidas.push((atual, pagina, mudou));
        }

        let version = {
            let mut cache = self.cache.lock().unwrap();
            if lidas.iter().any(|(_, _, mudou)| *mudou) {
                cache.version += 1;
            }
            // Se o projeto trocou no meio, o que foi lido não entra no cache do outro.
            if cache.alvo.as_ref() == Some(&slug) {
                for (url, pagina, _) in &lidas {
                    cache.paginas.insert(url.clone(), pagina.clone());
                }
            }
            cache.version
        };
        if version == since_version {
            return Ok(IssuesSnapshot { version, data: None });
        }
        let (labels, issues) = lidas.split_last().expect("a lista tem os labels no fim");
        let mut vistas = std::collections::HashSet::new();
        let mut todas = Vec::new();
        for (_, pagina, _) in issues {
            // Uma issue fechada entre uma página e outra pode vir nas duas listas: fica a primeira.
            todas.extend(issues_do_json(&pagina.corpo)?.into_iter().filter(|i| vistas.insert(i.number)));
        }
        Ok(IssuesSnapshot { version, data: Some(IssuesData { issues: todas, labels: labels_do_json(&labels.1.corpo)? }) })
    }

    /// POST ou PATCH que devolve a issue. Depois de dar certo, o cache das listas é esquecido: a próxima consulta
    /// volta com versão nova.
    fn alterar(&self, dir: &Path, metodo: &str, caminho: &str, corpo: Value) -> Result<Issue, String> {
        let slug = slug_da_pasta(dir).ok_or_else(|| mensagem(Reason::NoRemote, ""))?;
        let alvo = slug.to_string();
        let resposta = self.chamar(metodo, &format!("{API}/repos/{alvo}/{caminho}"), None, Some(&corpo.to_string())).map_err(|f| f.texto(&alvo))?;
        if !matches!(resposta.status, 200 | 201) {
            return Err(resposta.falha().texto(&alvo));
        }
        self.cache.lock().unwrap().paginas.clear();
        issue_do_json(&resposta.corpo)
    }

    /// Cria a issue. Título vazio é recusado sem chamar a API.
    pub fn create_issue(&self, dir: &Path, title: &str, body: &str, labels: &[String]) -> Result<Issue, String> {
        let title = title.trim();
        if title.is_empty() {
            return Err("O título da issue está vazio.".to_string());
        }
        self.alterar(dir, "POST", "issues", serde_json::json!({ "title": title, "body": body, "labels": labels }))
    }

    /// Fecha (como concluída) ou reabre a issue.
    pub fn set_issue_state(&self, dir: &Path, number: u64, open: bool) -> Result<Issue, String> {
        let corpo = if open {
            serde_json::json!({ "state": "open", "state_reason": "reopened" })
        } else {
            serde_json::json!({ "state": "closed", "state_reason": "completed" })
        };
        self.alterar(dir, "PATCH", &format!("issues/{number}"), corpo)
    }
}

/// Aceita só link do site do GitHub, sem nada que mude a linha de comando: espaço, aspas, `<>^|&` ou controle.
fn link_do_github(url: &str) -> Result<&str, String> {
    let proibido = |c: char| c.is_whitespace() || c.is_control() || "\"'<>^|&".contains(c);
    if url.starts_with("https://github.com/") && !url.contains(proibido) {
        Ok(url)
    } else {
        Err("Este link não é do GitHub.".to_string())
    }
}

/// Abre o link do GitHub no navegador padrão do Windows, pelo `url.dll` (sem passar pelo `cmd.exe`).
pub fn open(url: &str) -> Result<(), String> {
    let url = link_do_github(url)?;
    proc::quiet("rundll32.exe")
        .raw_arg(format!("url.dll,FileProtocolHandler {url}"))
        .spawn()
        .map(drop)
        .map_err(|_| "Não foi possível abrir o navegador.".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn so_abre_link_do_github() {
        let ok = "https://github.com/luansilvadb/orquestrador/issues/3";
        assert_eq!(link_do_github(ok), Ok(ok));
        for ruim in [
            "http://github.com/luansilvadb/orquestrador",
            "https://github.com.evil.com/luansilvadb/orquestrador",
            "https://gist.github.com/luansilvadb/1",
            "javascript:alert(1)",
            "file:///C:/Windows/System32/calc.exe",
            "https://github.com/luansilvadb/orquestrador issues",
            "https://github.com/luansilvadb/\"orquestrador",
            "https://github.com/luansilvadb/'orquestrador",
            "https://github.com/luansilvadb/orquestrador?a=1&calc",
            "https://github.com/luansilvadb/orquestrador
",
            "https://github.com/a<b>",
            "https://github.com/a^b|c",
            "https://github.com",
            "",
        ] {
            assert_eq!(link_do_github(ruim), Err("Este link não é do GitHub.".to_string()), "{ruim}");
        }
    }

    fn slug(owner: &str, repo: &str) -> Option<Slug> {
        Some(Slug { owner: owner.to_string(), repo: repo.to_string() })
    }

    #[test]
    fn remoto_do_github_nos_quatro_formatos() {
        assert_eq!(slug_do_remoto("https://github.com/dono/repo.git\n"), slug("dono", "repo"));
        assert_eq!(slug_do_remoto("https://github.com/dono/repo"), slug("dono", "repo"));
        assert_eq!(slug_do_remoto("https://github.com/dono/repo/"), slug("dono", "repo"));
        assert_eq!(slug_do_remoto("http://usuario@github.com/dono/meu.repo.git"), slug("dono", "meu.repo"));
        assert_eq!(slug_do_remoto("https://usuario:senha@GitHub.com/Dono-1/repo_x"), slug("Dono-1", "repo_x"));
        assert_eq!(slug_do_remoto("git@github.com:dono/repo.git"), slug("dono", "repo"));
        assert_eq!(slug_do_remoto("git@github.com:dono/repo"), slug("dono", "repo"));
        assert_eq!(slug_do_remoto("ssh://git@github.com/dono/repo.git"), slug("dono", "repo"));
    }

    #[test]
    fn remoto_fora_do_github_ou_estranho_nao_vale() {
        for url in [
            "https://gitlab.com/dono/repo.git",
            "git@gitlab.com:dono/repo.git",
            "ssh://git@bitbucket.org/dono/repo",
            "https://github.com.evil.com/dono/repo",
            "https://github.com/dono",
            "https://github.com/dono/repo/tree/main",
            "C:\\repos\\dono\\repo",
            "../outro",
            "",
        ] {
            assert_eq!(slug_do_remoto(url), None, "{url}");
        }
    }

    #[test]
    fn link_acha_a_proxima_pagina() {
        let link = r#"<https://api.github.com/repositories/1/issues?state=open&page=2>; rel="next", <https://api.github.com/repositories/1/issues?state=open&page=5>; rel="last""#;
        assert_eq!(proxima(link).as_deref(), Some("https://api.github.com/repositories/1/issues?state=open&page=2"));
        let ultima = r#"<https://api.github.com/repositories/1/issues?page=4>; rel="prev", <https://api.github.com/repositories/1/issues?page=1>; rel="first""#;
        assert_eq!(proxima(ultima), None);
        assert_eq!(proxima(""), None);
    }

    #[test]
    fn issue_do_json_com_body_nulo_label_e_sem_pull_request() {
        let corpo = r#"[
            {"number": 7, "title": "Quebra ao abrir", "body": null, "state": "open",
             "labels": [{"id": 1, "name": "bug", "color": "d73a4a", "description": null}],
             "user": {"login": "ana"}, "comments": 2,
             "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-02T10:00:00Z", "closed_at": null,
             "html_url": "https://github.com/dono/repo/issues/7"},
            {"number": 8, "title": "Um PR", "body": "x", "state": "open", "labels": [], "user": {"login": "bia"},
             "comments": 0, "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-01T10:00:00Z", "closed_at": null,
             "html_url": "https://github.com/dono/repo/pull/8", "pull_request": {"url": "x"}},
            {"number": 3, "title": "Feita", "body": "texto", "state": "closed", "labels": [], "user": {"login": "ana"},
             "comments": 0, "created_at": "2026-09-01T10:00:00Z", "updated_at": "2026-09-03T10:00:00Z",
             "closed_at": "2026-09-03T10:00:00Z", "html_url": "https://github.com/dono/repo/issues/3"}
        ]"#;
        let issues = issues_do_json(corpo).unwrap();
        assert_eq!(issues.iter().map(|i| i.number).collect::<Vec<_>>(), [7, 3]);
        assert_eq!(
            issues[0],
            Issue {
                number: 7,
                title: "Quebra ao abrir".to_string(),
                body: String::new(),
                state: IssueState::Open,
                labels: vec![IssueLabel { name: "bug".to_string(), color: "d73a4a".to_string() }],
                author: "ana".to_string(),
                comments: 2,
                created_at: "2026-10-01T10:00:00Z".to_string(),
                updated_at: "2026-10-02T10:00:00Z".to_string(),
                closed_at: None,
                url: "https://github.com/dono/repo/issues/7".to_string(),
            }
        );
        assert_eq!(issues[1].state, IssueState::Closed);
        assert_eq!(issues[1].closed_at.as_deref(), Some("2026-09-03T10:00:00Z"));
        // Os nomes que a tela lê.
        let json = serde_json::to_value(&issues[0]).unwrap();
        assert_eq!(json["createdAt"], "2026-10-01T10:00:00Z");
        assert_eq!(json["closedAt"], Value::Null);
        assert_eq!(json["state"], "open");
    }

    #[test]
    fn labels_com_descricao_nula() {
        let labels = labels_do_json(r#"[{"name": "bug", "color": "d73a4a", "description": null}, {"name": "ui", "color": "00ff00", "description": "Tela"}]"#).unwrap();
        assert_eq!(labels[0], Label { name: "bug".to_string(), color: "d73a4a".to_string(), description: String::new() });
        assert_eq!(labels[1].description, "Tela");
    }

    #[test]
    fn status_http_vira_a_mensagem_certa() {
        let alvo = "dono/repo";
        assert_eq!(falha_do_status(401, false, "").texto(alvo), "O GitHub CLI não está logado. Rode `gh auth login` num terminal e tente de novo.");
        assert_eq!(falha_do_status(404, false, r#"{"message":"Not Found"}"#).texto(alvo), "Sua conta do GitHub não tem acesso a dono/repo.");
        assert_eq!(falha_do_status(410, false, "").texto(alvo), "As issues estão desligadas em dono/repo.");
        assert_eq!(falha_do_status(403, true, "").texto(alvo), LIMITE);
        assert_eq!(falha_do_status(429, false, "").texto(alvo), LIMITE);
        assert_eq!(falha_do_status(403, false, r#"{"message":"Resource not accessible by integration"}"#).texto(alvo), "O GitHub respondeu 403: Resource not accessible by integration");
        let validacao = r#"{"message":"Validation Failed","errors":[{"resource":"Issue","code":"invalid","field":"labels"},{"message":"title is too long"}]}"#;
        assert_eq!(falha_do_status(422, false, validacao).texto(alvo), "Validation Failed (labels: invalid; title is too long)");
        assert_eq!(falha_do_status(502, false, "<html>").texto(alvo), "O GitHub respondeu 502.");
        assert_eq!(Falha::Fora(Reason::Offline).texto(alvo), "Sem conexão com o GitHub.");
    }

    #[test]
    fn status_serializa_como_a_tela_espera() {
        let pronto = GhStatus::Ready { owner: "dono".to_string(), repo: "repo".to_string(), login: "ana".to_string() };
        assert_eq!(serde_json::to_value(pronto).unwrap(), serde_json::json!({ "kind": "ready", "owner": "dono", "repo": "repo", "login": "ana" }));
        let fora = GhStatus::Unavailable { reason: Reason::IssuesDisabled, message: "m".to_string() };
        assert_eq!(serde_json::to_value(fora).unwrap(), serde_json::json!({ "kind": "unavailable", "reason": "issues-disabled", "message": "m" }));
        assert_eq!(serde_json::to_value(Reason::NoGh).unwrap(), "no-gh");
    }

    #[test]
    fn titulo_vazio_nao_chama_a_api() {
        let hub = Hub::new();
        assert_eq!(hub.create_issue(Path::new("."), "  \n", "", &[]).unwrap_err(), "O título da issue está vazio.");
    }

    /// Contra o GitHub de verdade, no próprio repositório: `cargo test -- --ignored --nocapture checagem_real`. Só lê.
    #[test]
    #[ignore = "usa a rede e o gh logado"]
    fn checagem_real_no_proprio_repositorio() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let hub = Hub::new();
        let status = hub.status(dir).unwrap();
        println!("status: {}", serde_json::to_string(&status).unwrap());
        assert!(matches!(status, GhStatus::Ready { .. }));
        let primeira = hub.issues(dir, 0).unwrap();
        let data = primeira.data.as_ref().expect("a primeira chamada traz o retrato");
        println!("primeira: version {}, {} issues, {} labels", primeira.version, data.issues.len(), data.labels.len());
        for issue in data.issues.iter().take(3) {
            println!("  {}", serde_json::to_string(issue).unwrap());
        }
        println!("  labels: {}", serde_json::to_string(&data.labels.iter().map(|l| &l.name).collect::<Vec<_>>()).unwrap());
        let segunda = hub.issues(dir, primeira.version).unwrap();
        println!("segunda: {}", serde_json::to_string(&segunda).unwrap());
        assert_eq!(segunda, IssuesSnapshot { version: primeira.version, data: None });
    }
}
