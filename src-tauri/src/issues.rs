//! As issues de cada projeto, só neste computador: um JSON por projeto em `%LOCALAPPDATA%\orquestrador\issues`, com
//! as issues (abertas e fechadas), o último número dado, a issue em andamento e a próxima. O arquivo é ligado ao projeto
//! pelo caminho da pasta, sem diferenciar maiúsculas: o nome é o da pasta com um hash do caminho, e o caminho completo
//! vai dentro dele, para quem procura à mão. Pasta renomeada ou movida começa sem issues; o arquivo antigo fica onde
//! estava. Cada mudança relê o arquivo, aplica a mudança e o troca inteiro (grava ao lado e renomeia), sob uma trava
//! do processo; duas janelas do app no mesmo projeto não apagam o que a outra gravou. Arquivo ilegível é erro, e nada
//! é gravado por cima dele.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

/// A versão do formato do arquivo. Um arquivo de versão mais nova não é lido nem regravado.
const VERSAO: u32 = 1;
/// O maior nome de label, em caracteres.
const LABEL_MAX: usize = 50;

/// Uma mudança por vez, no processo todo: a leitura e a gravação de uma não se misturam com as de outra.
static TRAVA: Mutex<()> = Mutex::new(());

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum IssueState {
    Open,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: IssueState,
    /// Os nomes, sem repetir (sem diferenciar maiúsculas).
    pub labels: Vec<String>,
    /// As datas em ISO 8601, UTC, com segundos.
    pub created_at: String,
    pub updated_at: String,
    pub closed_at: Option<String>,
}

/// A issue com o orquestrador: desde quando (o `Date.now()` da tela) e se uma rodada dele já começou com ela.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Active {
    pub number: u64,
    pub since: u64,
    pub turned: bool,
}

/// O arquivo de um projeto, como a tela o recebe. Chave que falta vale o padrão; tipo errado é erro.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Dados {
    pub v: u32,
    /// O caminho do projeto.
    pub project: String,
    /// O último número dado. Só sobe: o número de uma issue excluída não volta.
    pub last_number: u64,
    /// Na ordem de criação.
    pub issues: Vec<Issue>,
    pub active: Option<Active>,
    pub next: Option<u64>,
}

/// A pasta dos arquivos de issues.
pub fn dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .ok_or("LOCALAPPDATA não está definido; não há onde guardar as issues.")?;
    Ok(PathBuf::from(base).join("orquestrador").join("issues"))
}

/// O arquivo das issues de `projeto` em `pasta`: o nome da pasta do projeto (só o que é seguro num nome de arquivo) e o
/// hash FNV-1a do caminho em minúsculas, com `\`, sem o prefixo `\\?\` e sem a barra do fim.
pub fn arquivo(pasta: &Path, projeto: &Path) -> PathBuf {
    let caminho = projeto.to_string_lossy().replace('/', "\\");
    let caminho = caminho.strip_prefix(r"\\?\").unwrap_or(&caminho);
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in caminho.trim_end_matches('\\').to_lowercase().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    let nome: String = projeto
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || "-_.".contains(c) { c } else { '-' })
        .take(40)
        .collect();
    let nome = nome.trim_matches(|c| c == '-' || c == '.');
    pasta.join(format!("{}-{hash:016x}.json", if nome.is_empty() { "projeto" } else { nome }))
}

/// `ms` desde 1970 em ISO 8601, UTC, com segundos (`2026-10-16T21:04:05Z`). A data sai dos dias pelo algoritmo
/// `civil_from_days` de Howard Hinnant.
fn iso(ms: u64) -> String {
    let segundos = ms / 1000;
    let (dias, resto) = ((segundos / 86_400) as i64, segundos % 86_400);
    let z = dias + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let dia = doy - (153 * mp + 2) / 5 + 1;
    let mes = if mp < 10 { mp + 3 } else { mp - 9 };
    let ano = yoe + era * 400 + i64::from(mes <= 2);
    format!("{ano:04}-{mes:02}-{dia:02}T{:02}:{:02}:{:02}Z", resto / 3600, resto % 3600 / 60, resto % 60)
}

fn agora() -> String {
    iso(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0))
}

/// Sem arquivo, nenhuma issue.
fn ler(caminho: &Path) -> Result<Dados, String> {
    let ilegivel = |e: String| format!("Não foi possível ler as issues deste projeto em {}: {e}", caminho.display());
    let texto = match std::fs::read_to_string(caminho) {
        Ok(texto) => texto,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Dados::default()),
        Err(e) => return Err(ilegivel(e.to_string())),
    };
    let dados: Dados = serde_json::from_str(texto.trim_start_matches('\u{feff}')).map_err(|e| ilegivel(e.to_string()))?;
    if dados.v > VERSAO {
        return Err(format!("As issues em {} são de uma versão mais nova do app. Atualize o app para abri-las.", caminho.display()));
    }
    Ok(dados)
}

/// Grava ao lado e renomeia por cima: quem lê vê o arquivo de antes ou o de depois, nunca um pedaço.
fn gravar(caminho: &Path, dados: &Dados) -> Result<(), String> {
    let falha = |e: std::io::Error| format!("Não foi possível gravar as issues em {}: {e}", caminho.display());
    std::fs::create_dir_all(caminho.parent().unwrap_or(Path::new("."))).map_err(falha)?;
    let temp = caminho.with_extension(format!("json.{}.tmp", std::process::id()));
    std::fs::write(&temp, serde_json::to_string_pretty(dados).unwrap()).map_err(falha)?;
    std::fs::rename(&temp, caminho).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        falha(e)
    })
}

/// Relê o arquivo, aplica `mudanca` e grava, sob a trava. Se `mudanca` recusa, nada é gravado.
fn mudar<T>(pasta: &Path, projeto: &Path, mudanca: impl FnOnce(&mut Dados) -> Result<T, String>) -> Result<T, String> {
    let _trava = TRAVA.lock().unwrap_or_else(|e| e.into_inner());
    let caminho = arquivo(pasta, projeto);
    let mut dados = ler(&caminho)?;
    let feito = mudanca(&mut dados)?;
    dados.v = VERSAO;
    dados.project = projeto.display().to_string();
    gravar(&caminho, &dados)?;
    Ok(feito)
}

fn titulo(title: &str) -> Result<String, String> {
    let title = title.trim();
    if title.is_empty() {
        return Err("O título da issue está vazio.".to_string());
    }
    Ok(title.to_string())
}

/// As labels como vão para o arquivo: espaços juntados, sem vazias e sem repetir (sem diferenciar maiúsculas). Uma que
/// já está em alguma issue do projeto fica com a grafia de lá. Mais de `LABEL_MAX` caracteres é erro.
fn labels(pedidas: &[String], issues: &[Issue]) -> Result<Vec<String>, String> {
    let mut limpas: Vec<String> = Vec::new();
    for label in pedidas {
        let label = label.split_whitespace().collect::<Vec<_>>().join(" ");
        if label.is_empty() {
            continue;
        }
        if label.chars().count() > LABEL_MAX {
            return Err(format!("A label “{label}” passa de {LABEL_MAX} caracteres."));
        }
        let chave = label.to_lowercase();
        if limpas.iter().any(|l| l.to_lowercase() == chave) {
            continue;
        }
        let ja = issues.iter().flat_map(|i| &i.labels).find(|l| l.to_lowercase() == chave);
        limpas.push(ja.cloned().unwrap_or(label));
    }
    Ok(limpas)
}

fn achar(dados: &mut Dados, number: u64) -> Result<&mut Issue, String> {
    dados.issues.iter_mut().find(|i| i.number == number).ok_or_else(|| format!("A issue #{number} não existe mais."))
}

/// As issues do projeto, a em andamento e a próxima. Sem arquivo, nenhuma.
pub fn load(pasta: &Path, projeto: &Path) -> Result<Dados, String> {
    ler(&arquivo(pasta, projeto))
}

/// Cria a issue aberta, com o número seguinte ao último dado. Título vazio é recusado.
pub fn create(pasta: &Path, projeto: &Path, title: &str, body: &str, pedidas: &[String]) -> Result<Issue, String> {
    let title = titulo(title)?;
    mudar(pasta, projeto, |dados| {
        let number = dados.issues.iter().map(|i| i.number).fold(dados.last_number, u64::max) + 1;
        let agora = agora();
        let issue = Issue {
            number,
            title,
            body: body.to_string(),
            state: IssueState::Open,
            labels: labels(pedidas, &dados.issues)?,
            created_at: agora.clone(),
            updated_at: agora,
            closed_at: None,
        };
        dados.last_number = number;
        dados.issues.push(issue.clone());
        Ok(issue)
    })
}

/// Troca o título, a descrição e as labels.
pub fn update(pasta: &Path, projeto: &Path, number: u64, title: &str, body: &str, pedidas: &[String]) -> Result<Issue, String> {
    let title = titulo(title)?;
    mudar(pasta, projeto, |dados| {
        let labels = labels(pedidas, &dados.issues)?;
        let issue = achar(dados, number)?;
        if issue.title != title || issue.body != body || issue.labels != labels {
            issue.title = title;
            issue.body = body.to_string();
            issue.labels = labels;
            issue.updated_at = agora();
        }
        Ok(issue.clone())
    })
}

/// Fecha (`open` falso) ou reabre. Já no estado pedido, nada muda.
pub fn set_state(pasta: &Path, projeto: &Path, number: u64, open: bool) -> Result<Issue, String> {
    mudar(pasta, projeto, |dados| {
        let issue = achar(dados, number)?;
        let state = if open { IssueState::Open } else { IssueState::Closed };
        if issue.state != state {
            let agora = agora();
            issue.state = state;
            issue.closed_at = (!open).then(|| agora.clone());
            issue.updated_at = agora;
        }
        Ok(issue.clone())
    })
}

/// Apaga de vez. Se era a em andamento ou a próxima, a marca sai junto.
pub fn delete(pasta: &Path, projeto: &Path, number: u64) -> Result<(), String> {
    mudar(pasta, projeto, |dados| {
        achar(dados, number)?;
        dados.issues.retain(|i| i.number != number);
        if dados.active.is_some_and(|a| a.number == number) {
            dados.active = None;
        }
        if dados.next == Some(number) {
            dados.next = None;
        }
        Ok(())
    })
}

/// Guarda a em andamento e a próxima, como a tela as manda.
pub fn set_tracked(pasta: &Path, projeto: &Path, active: Option<Active>, next: Option<u64>) -> Result<(), String> {
    mudar(pasta, projeto, |dados| {
        dados.active = active;
        dados.next = next;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::Tmp;

    fn nomes(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn data_em_iso_utc() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(951_782_400_000), "2000-02-29T00:00:00Z");
        assert_eq!(iso(946_684_799_000), "1999-12-31T23:59:59Z");
        assert_eq!(iso(1_735_689_599_999), "2024-12-31T23:59:59Z");
        assert_eq!(iso(1_792_184_645_000), "2026-10-16T21:04:05Z");
    }

    #[test]
    fn arquivo_pelo_caminho_sem_diferenciar_maiusculas_nem_barras() {
        let pasta = Path::new(r"C:\dados\issues");
        let um = arquivo(pasta, Path::new(r"D:\Projetos\Meu App"));
        assert_eq!(um.parent(), Some(pasta));
        let nome = um.file_name().unwrap().to_string_lossy().into_owned();
        assert!(nome.starts_with("Meu-App-") && nome.ends_with(".json") && nome.len() == "Meu-App-".len() + 16 + 5, "{nome}");
        for igual in [r"d:\projetos\meu app", "D:/Projetos/Meu App", r"D:\Projetos\Meu App\", r"\\?\D:\Projetos\Meu App"] {
            assert_eq!(arquivo(pasta, Path::new(igual)).file_name().unwrap().to_string_lossy().to_lowercase(), nome.to_lowercase(), "{igual}");
        }
        assert_ne!(arquivo(pasta, Path::new(r"D:\Outros\Meu App")), um);
        // O hash não muda entre versões (FNV-1a): o arquivo de hoje é achado amanhã.
        assert_eq!(arquivo(pasta, Path::new(r"D:\orquestrador")).file_name().unwrap(), "orquestrador-93ee4029c0fcac10.json");
        assert!(arquivo(pasta, Path::new(r"D:\")).file_name().unwrap().to_string_lossy().starts_with("projeto-"));
        assert!(arquivo(pasta, Path::new(r"D:\ação")).file_name().unwrap().to_string_lossy().starts_with("a--o-"));
    }

    #[test]
    fn sem_arquivo_nenhuma_issue() {
        let tmp = Tmp::new("orq-issues", "vazio");
        assert_eq!(load(&tmp.0, Path::new(r"D:\nada")).unwrap(), Dados::default());
    }

    #[test]
    fn criar_editar_fechar_reabrir_e_reler() {
        let tmp = Tmp::new("orq-issues", "ciclo");
        let projeto = Path::new(r"D:\proj");
        let um = create(&tmp.0, projeto, "  Primeira  ", "corpo\n", &nomes(&["bug", " ui  nova ", "Bug", ""])).unwrap();
        assert_eq!((um.number, um.title.as_str(), um.body.as_str(), um.state), (1, "Primeira", "corpo\n", IssueState::Open));
        assert_eq!(um.labels, nomes(&["bug", "ui nova"]));
        assert_eq!(um.created_at, um.updated_at);
        assert_eq!(um.closed_at, None);
        // A label que já existe fica com a grafia de lá.
        let dois = create(&tmp.0, projeto, "Segunda", "", &nomes(&["BUG", "Docs"])).unwrap();
        assert_eq!((dois.number, dois.labels.clone()), (2, nomes(&["bug", "Docs"])));

        let editada = update(&tmp.0, projeto, 1, "Primeira, editada", "outro", &nomes(&["docs"])).unwrap();
        assert_eq!((editada.title.as_str(), editada.body.as_str(), editada.labels.clone()), ("Primeira, editada", "outro", nomes(&["Docs"])));
        let fechada = set_state(&tmp.0, projeto, 2, false).unwrap();
        assert_eq!(fechada.state, IssueState::Closed);
        assert_eq!(fechada.closed_at.as_deref(), Some(fechada.updated_at.as_str()));
        assert_eq!(set_state(&tmp.0, projeto, 2, false).unwrap(), fechada, "fechar de novo não muda nada");
        let reaberta = set_state(&tmp.0, projeto, 2, true).unwrap();
        assert_eq!((reaberta.state, reaberta.closed_at.clone()), (IssueState::Open, None));

        let dados = load(&tmp.0, projeto).unwrap();
        assert_eq!(dados.issues, vec![editada, reaberta]);
        assert_eq!((dados.v, dados.project.as_str(), dados.last_number), (1, r"D:\proj", 2));
        // Os nomes que a tela lê.
        let json = serde_json::to_value(&dados).unwrap();
        assert_eq!(json["lastNumber"], 2);
        assert_eq!(json["issues"][0]["createdAt"], dados.issues[0].created_at);
        assert_eq!(json["issues"][0]["closedAt"], serde_json::Value::Null);
        assert_eq!(json["issues"][0]["state"], "open");
        assert_eq!(json["active"], serde_json::Value::Null);
    }

    #[test]
    fn titulo_vazio_label_longa_e_issue_que_nao_existe_sao_recusados_sem_gravar() {
        let tmp = Tmp::new("orq-issues", "recusas");
        let projeto = Path::new(r"D:\proj");
        assert_eq!(create(&tmp.0, projeto, " \n ", "", &[]).unwrap_err(), "O título da issue está vazio.");
        assert!(!arquivo(&tmp.0, projeto).exists());
        let longa = "x".repeat(LABEL_MAX + 1);
        assert_eq!(create(&tmp.0, projeto, "a", "", &[longa.clone()]).unwrap_err(), format!("A label “{longa}” passa de 50 caracteres."));
        assert!(create(&tmp.0, projeto, "a", "", &["y".repeat(LABEL_MAX)]).is_ok());
        assert_eq!(update(&tmp.0, projeto, 7, "t", "", &[]).unwrap_err(), "A issue #7 não existe mais.");
        assert_eq!(set_state(&tmp.0, projeto, 7, false).unwrap_err(), "A issue #7 não existe mais.");
        assert_eq!(delete(&tmp.0, projeto, 7).unwrap_err(), "A issue #7 não existe mais.");
    }

    #[test]
    fn numero_de_excluida_nao_volta_e_a_marca_sai_junto() {
        let tmp = Tmp::new("orq-issues", "excluir");
        let projeto = Path::new(r"D:\proj");
        for title in ["a", "b", "c"] {
            create(&tmp.0, projeto, title, "", &[]).unwrap();
        }
        set_tracked(&tmp.0, projeto, Some(Active { number: 3, since: 1_700_000_000_000, turned: true }), Some(2)).unwrap();
        delete(&tmp.0, projeto, 3).unwrap();
        delete(&tmp.0, projeto, 2).unwrap();
        let dados = load(&tmp.0, projeto).unwrap();
        assert_eq!((dados.active, dados.next), (None, None));
        assert_eq!(create(&tmp.0, projeto, "d", "", &[]).unwrap().number, 4);
        // Excluir uma que não é a marcada mantém as marcas.
        set_tracked(&tmp.0, projeto, Some(Active { number: 4, since: 5, turned: false }), Some(1)).unwrap();
        create(&tmp.0, projeto, "e", "", &[]).unwrap();
        delete(&tmp.0, projeto, 5).unwrap();
        let dados = load(&tmp.0, projeto).unwrap();
        assert_eq!((dados.active, dados.next), (Some(Active { number: 4, since: 5, turned: false }), Some(1)));
    }

    #[test]
    fn arquivo_ilegivel_ou_mais_novo_e_erro_e_nada_e_gravado_por_cima() {
        let tmp = Tmp::new("orq-issues", "ilegivel");
        let projeto = Path::new(r"D:\proj");
        let caminho = arquivo(&tmp.0, projeto);
        std::fs::write(&caminho, "{ isto não é json").unwrap();
        assert!(load(&tmp.0, projeto).unwrap_err().starts_with("Não foi possível ler as issues deste projeto em "));
        assert!(create(&tmp.0, projeto, "a", "", &[]).is_err());
        assert_eq!(std::fs::read_to_string(&caminho).unwrap(), "{ isto não é json");
        std::fs::write(&caminho, r#"{"v": 2, "issues": []}"#).unwrap();
        assert!(load(&tmp.0, projeto).unwrap_err().ends_with("são de uma versão mais nova do app. Atualize o app para abri-las."));
        assert!(set_tracked(&tmp.0, projeto, None, None).is_err());
        // Chave que falta vale o padrão; o BOM do começo é aceito.
        std::fs::write(&caminho, "\u{feff}{\"v\": 1, \"issues\": [{\"number\": 9, \"title\": \"t\", \"body\": \"\", \"state\": \"closed\", \"labels\": [], \"createdAt\": \"x\", \"updatedAt\": \"x\", \"closedAt\": \"x\"}]}").unwrap();
        let dados = load(&tmp.0, projeto).unwrap();
        assert_eq!((dados.issues.len(), dados.last_number, dados.active), (1, 0, None));
        // O número segue o maior já usado, mesmo sem `lastNumber`.
        assert_eq!(create(&tmp.0, projeto, "a", "", &[]).unwrap().number, 10);
    }
}
