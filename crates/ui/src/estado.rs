//! Estado de interface que o React guarda no localStorage, num único `interface.json` em `%LOCALAPPDATA%\orquestrador\`.
//! Cada seção tem nome e versão próprios: uma seção de versão desconhecida, ilegível ou ausente volta ao padrão sem
//! derrubar as outras, e seções que este binário não conhece (editor, pesquisa de outras fases) são preservadas.
//! Cada crate de tela declara a própria seção (`impl Secao`) e a lê e grava por [`ler`] e [`gravar`].
use gpui_kit::{App, Global};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

/// Uma seção do arquivo. `NOME` é único entre os crates; mudar o formato exige subir `VERSAO`. `validar` corrige valores
/// fora de faixa depois de ler.
pub trait Secao: Default + Serialize + DeserializeOwned {
    const NOME: &'static str;
    const VERSAO: u32;
    fn validar(&mut self) {}
}

/// O arquivo em memória: seções cruas, decodificadas sob demanda.
pub struct Estado {
    arquivo: Option<PathBuf>,
    secoes: Map<String, Value>,
}

impl Estado {
    /// Sem arquivo (`None`), só guarda em memória. Arquivo ausente ou corrompido começa vazio.
    pub fn abrir(arquivo: Option<PathBuf>) -> Estado {
        let secoes = arquivo
            .as_deref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_json::from_str::<Value>(t.trim_start_matches('\u{feff}')).ok())
            .and_then(|v| match v {
                Value::Object(m) => Some(m),
                _ => None,
            })
            .unwrap_or_default();
        Estado { arquivo, secoes }
    }

    pub fn ler<S: Secao>(&self) -> S {
        let Some(Value::Object(sec)) = self.secoes.get(S::NOME) else { return S::default() };
        if sec.get("versao").and_then(Value::as_u64) != Some(u64::from(S::VERSAO)) {
            return S::default();
        }
        let mut valor = sec.get("dados").cloned().and_then(|d| serde_json::from_value::<S>(d).ok()).unwrap_or_default();
        valor.validar();
        valor
    }

    /// Troca a seção e grava o arquivo. Falha de gravação só perde a preferência; o app segue.
    pub fn gravar<S: Secao>(&mut self, valor: &S) {
        let Ok(dados) = serde_json::to_value(valor) else { return };
        let novo = serde_json::json!({ "versao": S::VERSAO, "dados": dados });
        if self.secoes.get(S::NOME) == Some(&novo) {
            return;
        }
        self.secoes.insert(S::NOME.to_string(), novo);
        if let Err(e) = self.gravar_arquivo() {
            eprintln!("interface.json não gravou: {e}");
        }
    }

    /// Grava num temporário ao lado e renomeia por cima: uma queda no meio não deixa o arquivo pela metade.
    fn gravar_arquivo(&self) -> std::io::Result<()> {
        let Some(arquivo) = &self.arquivo else { return Ok(()) };
        if let Some(pasta) = arquivo.parent() {
            std::fs::create_dir_all(pasta)?;
        }
        let texto = serde_json::to_string_pretty(&self.secoes).map_err(std::io::Error::other)?;
        let tmp = arquivo.with_extension("json.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(texto.as_bytes())?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, arquivo).inspect_err(|_| {
            let _ = std::fs::remove_file(&tmp);
        })
    }
}

/// O estado do app inteiro, instalado pela bancada; os crates de tela o acessam por [`ler`] e [`gravar`].
pub type Compartilhado = Rc<RefCell<Estado>>;

struct Global_(Compartilhado);
impl Global for Global_ {}

/// Instala o estado do app. Chamado uma vez pela bancada, antes de montar as telas.
pub fn instalar(estado: Compartilhado, cx: &mut App) {
    cx.set_global(Global_(estado));
}

/// Lê a seção; sem estado instalado (testes de uma tela isolada), devolve o padrão.
pub fn ler<S: Secao>(cx: &App) -> S {
    cx.try_global::<Global_>().map(|g| g.0.borrow().ler()).unwrap_or_default()
}

/// Grava a seção; sem estado instalado, não faz nada.
pub fn gravar<S: Secao>(valor: &S, cx: &App) {
    if let Some(g) = cx.try_global::<Global_>() {
        g.0.borrow_mut().gravar(valor);
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[derive(Default, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
    #[serde(default)]
    struct Teste {
        n: u32,
    }

    impl Secao for Teste {
        const NOME: &'static str = "teste";
        const VERSAO: u32 = 2;
        fn validar(&mut self) {
            self.n = self.n.min(10);
        }
    }

    #[test]
    fn grava_le_valida_e_preserva_secoes_alheias() {
        let pasta = std::env::temp_dir().join(format!("orq-ui-estado-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        let arquivo = pasta.join("interface.json");
        std::fs::create_dir_all(&pasta).unwrap();
        std::fs::write(&arquivo, r#"{"outra":{"versao":1,"dados":{"x":1}},"teste":{"versao":1,"dados":{"n":3}}}"#).unwrap();
        let mut e = Estado::abrir(Some(arquivo.clone()));
        assert_eq!(e.ler::<Teste>(), Teste::default(), "versão antiga volta ao padrão");
        e.gravar(&Teste { n: 50 });
        let e = Estado::abrir(Some(arquivo.clone()));
        assert_eq!(e.ler::<Teste>(), Teste { n: 10 }, "validado ao ler");
        let texto = std::fs::read_to_string(&arquivo).unwrap();
        assert!(texto.contains("\"outra\""), "seção desconhecida preservada");
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
