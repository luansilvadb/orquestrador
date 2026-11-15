//! Os dois CLIs na abertura: depois de o app estar em dia, cada um é localizado (PATH do registro, depois o local do
//! instalador oficial), conferido com `--version`, instalado pelo script oficial se faltar ou não responder, e
//! atualizado com `update`. Os dois correm em paralelo; qualquer falha mantém o uso bloqueado. A máquina (PATH,
//! arquivos e comandos) entra por `Maquina`, para os testes não tocarem na rede nem na instalação.
use crate::platform;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Prazo do `--version`: medido em 72 ms no Claude Code 2.1.325 e 126 ms no Codex 0.169.0.
const PRAZO_VERSAO: Duration = Duration::from_secs(30);
/// Prazo do script oficial de instalação, que baixa o binário inteiro.
const PRAZO_INSTALACAO: Duration = Duration::from_secs(15 * 60);
/// Prazo do `update`: em dia, medido em 2,0 s no Claude Code 2.1.325 e 2,1 s no Codex 0.169.0; com versão nova, baixa.
const PRAZO_UPDATE: Duration = Duration::from_secs(15 * 60);
/// Quanto do fim da saída de um comando entra na mensagem de falha.
const FIM_DA_SAIDA: usize = 400;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cli {
    Claude,
    Codex,
}

impl Cli {
    fn nome(self) -> &'static str {
        match self {
            Cli::Claude => "Claude Code",
            Cli::Codex => "Codex",
        }
    }

    /// Só o binário nativo: o atalho do npm (`.cmd` ou sem extensão) não abre num PTY.
    pub fn exe(self) -> &'static str {
        match self {
            Cli::Claude => "claude.exe",
            Cli::Codex => "codex.exe",
        }
    }

    fn script(self) -> &'static str {
        match self {
            Cli::Claude => "irm https://claude.ai/install.ps1 | iex",
            Cli::Codex => "irm https://chatgpt.com/codex/install.ps1 | iex",
        }
    }

    fn env(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Cli::Claude => &[],
            Cli::Codex => &[("CODEX_NON_INTERACTIVE", "1")],
        }
    }
}

/// O passo de um CLI, como a tela o mostra.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Passo {
    Checking,
    Installing,
    Updating,
    Ready,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Andamento {
    pub step: Passo,
    /// O número lido de `--version`; `None` enquanto não foi lido.
    pub version: Option<String>,
}

impl Andamento {
    const INICIO: Andamento = Andamento { step: Passo::Checking, version: None };
}

/// Como terminou um comando.
#[derive(Debug)]
pub enum Execucao {
    NaoIniciou(String),
    /// O prazo estourou; traz a saída até ali.
    Estourou(String),
    Saiu { codigo: Option<i32>, saida: String },
}

/// O que a etapa usa da máquina.
pub trait Maquina: Sync {
    /// As pastas do PATH, na ordem de busca.
    fn pastas_do_path(&self) -> Vec<PathBuf>;
    fn local_padrao(&self, cli: Cli) -> Option<PathBuf>;
    fn e_arquivo(&self, path: &Path) -> bool;
    fn powershell(&self) -> Option<PathBuf>;
    /// Roda sem janela, com entrada vazia e prazo; a saída junta stdout e stderr.
    fn rodar(&self, programa: &Path, args: &[&str], env: &[(&str, &str)], prazo: Duration) -> Execucao;
}

/// A máquina de verdade.
pub struct Real;

impl Maquina for Real {
    fn pastas_do_path(&self) -> Vec<PathBuf> {
        platform::pastas_do_path_registrado()
    }

    fn local_padrao(&self, cli: Cli) -> Option<PathBuf> {
        platform::local_padrao_do_cli(cli.exe())
    }

    fn e_arquivo(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn powershell(&self) -> Option<PathBuf> {
        platform::powershell_do_sistema()
    }

    fn rodar(&self, programa: &Path, args: &[&str], env: &[(&str, &str)], prazo: Duration) -> Execucao {
        let mut cmd = crate::proc::quiet(&programa.to_string_lossy());
        cmd.args(args).envs(env.iter().copied());
        match crate::proc::saida_com_prazo(cmd, b"", prazo) {
            Err(falha) => Execucao::NaoIniciou(format!("{falha:?}")),
            Ok(s) => {
                let saida = format!("{}{}", String::from_utf8_lossy(&s.out), String::from_utf8_lossy(&s.err));
                if s.estourou {
                    Execucao::Estourou(saida)
                } else {
                    Execucao::Saiu { codigo: s.status.code(), saida }
                }
            }
        }
    }
}

/// Os caminhos resolvidos dos dois CLIs.
#[derive(Clone, Debug, PartialEq)]
pub struct Caminhos {
    pub claude: String,
    pub codex: String,
}

/// `claude.exe`/`codex.exe` na primeira pasta do PATH que o tiver, senão no local do instalador oficial.
pub fn localizar(m: &dyn Maquina, cli: Cli) -> Option<PathBuf> {
    m.pastas_do_path()
        .into_iter()
        .map(|dir| dir.join(cli.exe()))
        .chain(m.local_padrao(cli))
        .find(|p| m.e_arquivo(p))
}

/// Build de desenvolvimento: a troca (`ORQ_*_EXE`) vale primeiro; depois só localiza, sem rodar nada; não achando,
/// fica o nome.
pub fn localizar_sem_rodar(m: &dyn Maquina, troca_claude: Option<String>, troca_codex: Option<String>) -> Caminhos {
    let um = |troca: Option<String>, cli: Cli| {
        troca.filter(|v| !v.trim().is_empty()).unwrap_or_else(|| {
            localizar(m, cli).map(|p| p.to_string_lossy().to_string()).unwrap_or_else(|| cli.exe().into())
        })
    };
    Caminhos { claude: um(troca_claude, Cli::Claude), codex: um(troca_codex, Cli::Codex) }
}

/// O número de versão: a primeira palavra que começa com dígito e tem ponto (`2.1.325 (Claude Code)`,
/// `codex-cli 0.169.0`).
pub fn extrair_versao(texto: &str) -> Option<String> {
    texto
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()) && w.contains('.'))
        .map(String::from)
}

/// O fim da saída, aparado, para a mensagem.
fn fim(saida: &str) -> String {
    let saida = saida.trim();
    let corte = saida.char_indices().rev().nth(FIM_DA_SAIDA - 1).map(|(i, _)| i).unwrap_or(0);
    let fim = &saida[corte..];
    if fim.is_empty() { String::new() } else if corte > 0 { format!(": …{fim}") } else { format!(": {fim}") }
}

fn minutos(prazo: Duration) -> String {
    if prazo.as_secs() >= 60 { format!("{} min", prazo.as_secs() / 60) } else { format!("{} s", prazo.as_secs()) }
}

/// Roda e devolve a saída se terminou com código 0; senão, o motivo.
fn com_sucesso(m: &dyn Maquina, programa: &Path, args: &[&str], env: &[(&str, &str)], prazo: Duration) -> Result<String, String> {
    match m.rodar(programa, args, env, prazo) {
        Execucao::NaoIniciou(e) => Err(format!("não iniciou ({e})")),
        Execucao::Estourou(saida) => Err(format!("não terminou em {}{}", minutos(prazo), fim(&saida))),
        Execucao::Saiu { codigo: Some(0), saida } => Ok(saida),
        Execucao::Saiu { codigo, saida } => {
            let codigo = codigo.map(|c| c.to_string()).unwrap_or_else(|| "desconhecido".into());
            Err(format!("saiu com código {codigo}{}", fim(&saida)))
        }
    }
}

fn versao(m: &dyn Maquina, exe: &Path) -> Result<String, String> {
    let saida = com_sucesso(m, exe, &["--version"], &[], PRAZO_VERSAO)?;
    extrair_versao(&saida).ok_or_else(|| format!("não devolveu uma versão{}", fim(&saida)))
}

/// O script oficial no Windows PowerShell do sistema, sem perfil e sem esperar digitação; depois, localiza de novo.
fn instalar(m: &dyn Maquina, cli: Cli) -> Result<PathBuf, String> {
    let falhou = |motivo: String| format!("{}: a instalação falhou: {motivo}.", cli.nome());
    let ps = m.powershell().ok_or_else(|| falhou("o Windows PowerShell do sistema não foi encontrado".into()))?;
    com_sucesso(m, &ps, &["-NoProfile", "-NonInteractive", "-Command", cli.script()], cli.env(), PRAZO_INSTALACAO).map_err(falhou)?;
    localizar(m, cli).ok_or_else(|| falhou(format!("{} não apareceu no PATH nem no local padrão", cli.exe())))
}

/// Garante um CLI: localiza (ou instala), confere a versão (reinstala uma vez se ela falhar) e atualiza.
fn garantir(m: &dyn Maquina, cli: Cli, avisar: &dyn Fn(Andamento)) -> Result<PathBuf, String> {
    let passo = |step, version: Option<&String>| avisar(Andamento { step, version: version.cloned() });
    passo(Passo::Checking, None);
    let (mut exe, mut instalado) = (localizar(m, cli), false);
    if exe.is_none() {
        passo(Passo::Installing, None);
        exe = Some(instalar(m, cli)?);
        instalado = true;
        passo(Passo::Checking, None);
    }
    let mut exe = exe.expect("localizado ou instalado");
    let lida = match versao(m, &exe) {
        Ok(v) => v,
        Err(motivo) if instalado => return Err(format!("{}: `--version` falhou depois da instalação: {motivo}.", cli.nome())),
        Err(motivo) => {
            passo(Passo::Installing, None);
            exe = instalar(m, cli)?;
            passo(Passo::Checking, None);
            versao(m, &exe).map_err(|de_novo| {
                format!("{}: `--version` falhou ({motivo}) e continuou falhando depois de reinstalar: {de_novo}.", cli.nome())
            })?
        }
    };
    passo(Passo::Updating, Some(&lida));
    com_sucesso(m, &exe, &["update"], &[], PRAZO_UPDATE).map_err(|motivo| format!("{}: `update` falhou: {motivo}.", cli.nome()))?;
    passo(Passo::Ready, Some(&lida));
    Ok(exe)
}

/// Os dois CLIs em paralelo. `avisar` recebe o andamento dos dois (Claude, Codex) a cada mudança de passo. Se os
/// dois falharem, a mensagem traz os dois motivos.
pub fn garantir_todos(m: &dyn Maquina, avisar: impl Fn(Andamento, Andamento) + Sync) -> Result<Caminhos, String> {
    let estado = Mutex::new([Andamento::INICIO, Andamento::INICIO]);
    let um = |i: usize, cli: Cli| {
        garantir(m, cli, &|a| {
            // Sob a trava, para dois avisos seguidos não chegarem fora de ordem.
            let mut e = estado.lock().unwrap();
            e[i] = a;
            avisar(e[0].clone(), e[1].clone());
        })
    };
    let (claude, codex) = std::thread::scope(|s| {
        let codex = s.spawn(|| um(1, Cli::Codex));
        let claude = um(0, Cli::Claude);
        (claude, codex.join().unwrap_or_else(|_| Err("Codex: a verificação parou inesperadamente.".into())))
    });
    match (claude, codex) {
        (Ok(claude), Ok(codex)) => {
            Ok(Caminhos { claude: claude.to_string_lossy().to_string(), codex: codex.to_string_lossy().to_string() })
        }
        (Err(a), Err(b)) => Err(format!("{a} {b}")),
        (Err(e), _) | (_, Err(e)) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};

    /// Uma máquina falsa: arquivos que existem, respostas por comando e o registro do que rodou. O script de instalação,
    /// quando dá certo, faz o binário aparecer no local padrão.
    #[derive(Default)]
    struct Falsa {
        path: Vec<PathBuf>,
        arquivos: Mutex<HashSet<PathBuf>>,
        /// Respostas por `programa args`; cada chamada consome a primeira da fila, e a última se repete.
        respostas: Mutex<HashMap<String, Vec<Resposta>>>,
        rodou: Mutex<Vec<String>>,
    }

    #[derive(Clone)]
    enum Resposta {
        Ok(&'static str),
        Codigo(i32, &'static str),
        Estoura,
        NaoInicia,
    }

    const PS: &str = r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe";
    const PADRAO_CLAUDE: &str = r"C:\Users\u\.local\bin\claude.exe";
    const PADRAO_CODEX: &str = r"C:\Users\u\AppData\Local\Programs\OpenAI\Codex\bin\codex.exe";

    impl Falsa {
        /// Os dois CLIs saudáveis e em dia, sem nada instalado.
        fn nova() -> Falsa {
            let f = Falsa { path: vec![PathBuf::from(r"C:\Windows"), PathBuf::from(r"C:\npm")], ..Falsa::default() };
            for exe in [PADRAO_CLAUDE, PADRAO_CODEX] {
                f.responde(&format!("{exe} --version"), &[if exe.ends_with("claude.exe") {
                    Resposta::Ok("2.1.325 (Claude Code)\n")
                } else {
                    Resposta::Ok("codex-cli 0.169.0\n")
                }]);
                f.responde(&format!("{exe} update"), &[Resposta::Ok("up to date")]);
            }
            f.responde(&format!("{PS} {}", Cli::Claude.script()), &[Resposta::Ok("instalado")]);
            f.responde(&format!("{PS} {}", Cli::Codex.script()), &[Resposta::Ok("instalado")]);
            f
        }

        fn com(self, arquivo: &str) -> Falsa {
            self.arquivos.lock().unwrap().insert(arquivo.into());
            self
        }

        fn responde(&self, chave: &str, fila: &[Resposta]) {
            self.respostas.lock().unwrap().insert(chave.into(), fila.to_vec());
        }

        fn rodou(&self) -> Vec<String> {
            self.rodou.lock().unwrap().clone()
        }
    }

    impl Maquina for Falsa {
        fn pastas_do_path(&self) -> Vec<PathBuf> {
            self.path.clone()
        }
        fn local_padrao(&self, cli: Cli) -> Option<PathBuf> {
            Some(PathBuf::from(if cli == Cli::Claude { PADRAO_CLAUDE } else { PADRAO_CODEX }))
        }
        fn e_arquivo(&self, path: &Path) -> bool {
            self.arquivos.lock().unwrap().contains(path)
        }
        fn powershell(&self) -> Option<PathBuf> {
            Some(PS.into())
        }
        fn rodar(&self, programa: &Path, args: &[&str], env: &[(&str, &str)], _prazo: Duration) -> Execucao {
            let chave = format!("{} {}", programa.display(), args.last().unwrap());
            if programa == Path::new(PS) {
                assert_eq!(&args[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
                let codex = args[3] == Cli::Codex.script();
                assert_eq!(env, if codex { Cli::Codex.env() } else { &[] });
            }
            self.rodou.lock().unwrap().push(chave.clone());
            let resposta = {
                let mut respostas = self.respostas.lock().unwrap();
                let fila = respostas.get_mut(&chave).unwrap_or_else(|| panic!("comando inesperado: {chave}"));
                if fila.len() > 1 { fila.remove(0) } else { fila[0].clone() }
            };
            let ok = |saida: &str| Execucao::Saiu { codigo: Some(0), saida: saida.into() };
            match resposta {
                Resposta::Ok(saida) => {
                    if programa == Path::new(PS) {
                        let destino = if args[3] == Cli::Codex.script() { PADRAO_CODEX } else { PADRAO_CLAUDE };
                        self.arquivos.lock().unwrap().insert(destino.into());
                    }
                    ok(saida)
                }
                Resposta::Codigo(codigo, saida) => Execucao::Saiu { codigo: Some(codigo), saida: saida.into() },
                Resposta::Estoura => Execucao::Estourou("baixando...".into()),
                Resposta::NaoInicia => Execucao::NaoIniciou("arquivo não encontrado".into()),
            }
        }
    }

    /// Roda a etapa e devolve o resultado e os avisos.
    fn rodar(m: &Falsa) -> (Result<Caminhos, String>, Vec<(Andamento, Andamento)>) {
        let avisos = Mutex::new(Vec::new());
        let r = garantir_todos(m, |a, b| avisos.lock().unwrap().push((a, b)));
        (r, avisos.into_inner().unwrap())
    }

    #[test]
    fn achado_no_path_do_registro_vale_antes_do_local_padrao() {
        let m = Falsa::nova().com(r"C:\npm\claude.exe").com(PADRAO_CLAUDE).com(r"C:\npm\codex").com(r"C:\npm\codex.cmd").com(PADRAO_CODEX);
        m.responde(r"C:\npm\claude.exe --version", &[Resposta::Ok("2.1.325 (Claude Code)")]);
        m.responde(r"C:\npm\claude.exe update", &[Resposta::Ok("")]);
        let (r, avisos) = rodar(&m);
        assert_eq!(r.unwrap(), Caminhos { claude: r"C:\npm\claude.exe".into(), codex: PADRAO_CODEX.into() }, "o atalho do npm não conta");
        assert!(!m.rodou().iter().any(|c| c.starts_with(PS)), "nada instalado");
        let fim = avisos.last().unwrap();
        assert_eq!(fim.0, Andamento { step: Passo::Ready, version: Some("2.1.325".into()) });
        assert_eq!(fim.1, Andamento { step: Passo::Ready, version: Some("0.169.0".into()) });
    }

    #[test]
    fn achado_so_no_local_padrao_confere_e_atualiza() {
        let m = Falsa::nova().com(PADRAO_CLAUDE).com(PADRAO_CODEX);
        let (r, avisos) = rodar(&m);
        assert_eq!(r.unwrap(), Caminhos { claude: PADRAO_CLAUDE.into(), codex: PADRAO_CODEX.into() });
        let mut claude = m.rodou();
        claude.retain(|c| c.contains("claude"));
        assert_eq!(claude, [format!("{PADRAO_CLAUDE} --version"), format!("{PADRAO_CLAUDE} update")]);
        let passos: Vec<Passo> = avisos.iter().map(|a| a.0.step).collect();
        assert!(passos.contains(&Passo::Updating) && !passos.contains(&Passo::Installing));
    }

    #[test]
    fn ausente_instala_e_acha() {
        let m = Falsa::nova().com(PADRAO_CODEX);
        let (r, avisos) = rodar(&m);
        assert_eq!(r.unwrap().claude, PADRAO_CLAUDE);
        assert!(m.rodou().contains(&format!("{PS} {}", Cli::Claude.script())));
        let mut passos: Vec<Passo> = avisos.iter().map(|a| a.0.step).collect();
        passos.dedup();
        assert_eq!(passos, [Passo::Checking, Passo::Installing, Passo::Checking, Passo::Updating, Passo::Ready]);
    }

    #[test]
    fn codex_instala_sem_interacao() {
        let m = Falsa::nova().com(PADRAO_CLAUDE);
        assert_eq!(rodar(&m).0.unwrap().codex, PADRAO_CODEX);
        assert!(m.rodou().contains(&format!("{PS} {}", Cli::Codex.script())), "o env é conferido pela falsa");
    }

    #[test]
    fn ausente_e_instalacao_falha() {
        let m = Falsa::nova().com(PADRAO_CODEX);
        m.responde(&format!("{PS} {}", Cli::Claude.script()), &[Resposta::Codigo(1, "irm : sem rede")]);
        let erro = rodar(&m).0.unwrap_err();
        assert_eq!(erro, "Claude Code: a instalação falhou: saiu com código 1: irm : sem rede.");
    }

    #[test]
    fn versao_falha_reinstala_e_segue() {
        let m = Falsa::nova().com(PADRAO_CLAUDE).com(PADRAO_CODEX);
        m.responde(&format!("{PADRAO_CODEX} --version"), &[Resposta::NaoInicia, Resposta::Ok("codex-cli 0.169.0")]);
        let (r, avisos) = rodar(&m);
        r.unwrap();
        assert!(m.rodou().contains(&format!("{PS} {}", Cli::Codex.script())));
        assert!(avisos.iter().any(|a| a.1.step == Passo::Installing));
        assert_eq!(avisos.last().unwrap().1.version.as_deref(), Some("0.169.0"));
    }

    #[test]
    fn versao_falha_de_novo_depois_de_reinstalar() {
        let m = Falsa::nova().com(PADRAO_CLAUDE).com(PADRAO_CODEX);
        m.responde(&format!("{PADRAO_CODEX} --version"), &[Resposta::Ok("lixo"), Resposta::Codigo(3, "")]);
        let erro = rodar(&m).0.unwrap_err();
        assert_eq!(erro, "Codex: `--version` falhou (não devolveu uma versão: lixo) e continuou falhando depois de reinstalar: saiu com código 3.");
        assert!(!m.rodou().iter().any(|c| c.ends_with("codex.exe update")));
    }

    #[test]
    fn update_com_codigo_diferente_de_zero_falha() {
        let m = Falsa::nova().com(PADRAO_CLAUDE).com(PADRAO_CODEX);
        m.responde(&format!("{PADRAO_CLAUDE} update"), &[Resposta::Codigo(1, "Failed to check for updates")]);
        assert_eq!(rodar(&m).0.unwrap_err(), "Claude Code: `update` falhou: saiu com código 1: Failed to check for updates.");
    }

    #[test]
    fn update_que_estoura_o_prazo_falha() {
        let m = Falsa::nova().com(PADRAO_CLAUDE).com(PADRAO_CODEX);
        m.responde(&format!("{PADRAO_CODEX} update"), &[Resposta::Estoura]);
        assert_eq!(rodar(&m).0.unwrap_err(), "Codex: `update` falhou: não terminou em 15 min: baixando....");
    }

    #[test]
    fn os_dois_falham_e_a_mensagem_traz_os_dois() {
        let m = Falsa::nova().com(PADRAO_CLAUDE).com(PADRAO_CODEX);
        m.responde(&format!("{PADRAO_CLAUDE} update"), &[Resposta::Codigo(2, "")]);
        m.responde(&format!("{PADRAO_CODEX} update"), &[Resposta::Codigo(4, "")]);
        assert_eq!(rodar(&m).0.unwrap_err(), "Claude Code: `update` falhou: saiu com código 2. Codex: `update` falhou: saiu com código 4.");
    }

    #[test]
    fn versao_das_saidas_reais() {
        // Medidas no Claude Code 2.1.325 e no Codex 0.169.0.
        assert_eq!(extrair_versao("2.1.325 (Claude Code)\r\n").as_deref(), Some("2.1.325"));
        assert_eq!(extrair_versao("codex-cli 0.169.0\n").as_deref(), Some("0.169.0"));
        assert_eq!(extrair_versao("Claude Code\n"), None);
    }

    #[test]
    fn fim_da_saida_longa_fica_com_o_final() {
        let longa = format!("{}FIM", "a".repeat(1000));
        let f = fim(&longa);
        assert!(f.starts_with(": …") && f.ends_with("FIM") && f.chars().count() == FIM_DA_SAIDA + 3);
    }

    #[test]
    fn debug_so_localiza_e_respeita_a_troca() {
        // A falsa entra em pânico em qualquer comando sem resposta: nada roda.
        let m = Falsa { respostas: Mutex::new(HashMap::new()), ..Falsa::nova() }.com(PADRAO_CLAUDE).com(r"C:\npm\codex.exe");
        assert_eq!(
            localizar_sem_rodar(&m, None, None),
            Caminhos { claude: PADRAO_CLAUDE.into(), codex: r"C:\npm\codex.exe".into() }
        );
        assert_eq!(
            localizar_sem_rodar(&m, Some(r"D:\falso.exe".into()), Some(" ".into())),
            Caminhos { claude: r"D:\falso.exe".into(), codex: r"C:\npm\codex.exe".into() }
        );
        let vazia = Falsa { respostas: Mutex::new(HashMap::new()), ..Falsa::nova() };
        assert_eq!(localizar_sem_rodar(&vazia, None, None), Caminhos { claude: "claude.exe".into(), codex: "codex.exe".into() });
        assert!(m.rodou().is_empty() && vazia.rodou().is_empty());
    }
}
