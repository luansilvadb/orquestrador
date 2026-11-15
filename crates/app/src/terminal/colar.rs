//! O que vai ao terminal quando o clipboard traz arquivo ou imagem: a mesma regra no orquestrador e nos shells (porte de
//! `src/terminal/paste.ts`).

/// O CLI que roda no terminal; `None` é um shell comum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cli {
    Claude,
    Codex,
}

impl Cli {
    pub fn de(nome: &str) -> Option<Cli> {
        match nome {
            "claude" => Some(Cli::Claude),
            "codex" => Some(Cli::Codex),
            _ => None,
        }
    }
}

/// `Escrever` vai direto ao PTY; `Colar` segue pela colagem do terminal (com bracketed paste se o modo estiver ligado);
/// `Erro` é o que não dá para colar, dito ao usuário.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Colagem {
    Escrever(String),
    Colar(String),
    Erro(String),
}

const BS: &str = "\x1b[200~";
const BE: &str = "\x1b[201~";
/// F12: o Codex não usa, então não mexe no rascunho; mas é uma tecla, e fecha o paste anterior antes do próximo.
const F12: &str = "\x1b[24~";

fn entre_colchetes(texto: &str) -> String {
    format!("{BS}{texto}{BE}")
}

/// Medido no Claude Code 2.1.302 e no Codex 0.163.0 (ver `paste.ts`): o Claude anexa vários caminhos num paste só, um por
/// linha; o Codex só anexa um caminho por paste, separados por uma tecla. Imagem solta usa o gesto de colar imagem do CLI.
pub fn colagem(cli: Option<Cli>, caminhos: &[String], imagem: bool, texto: &str, bracketed: bool) -> Colagem {
    if !caminhos.is_empty() {
        let citados: Vec<String> =
            caminhos.iter().map(|p| if p.contains(' ') { format!("\"{p}\"") } else { p.clone() }).collect();
        return match cli {
            Some(_) if !bracketed => Colagem::Colar(citados.join(" ")),
            None => Colagem::Colar(citados.join(" ")),
            Some(Cli::Claude) => {
                Colagem::Escrever(entre_colchetes(&citados.iter().map(|p| format!("{p} ")).collect::<Vec<_>>().join("\n")))
            }
            Some(Cli::Codex) => {
                Colagem::Escrever(citados.iter().map(|p| entre_colchetes(&format!("{p} "))).collect::<Vec<_>>().join(F12))
            }
        };
    }
    if imagem {
        if let Some(cli) = cli {
            return Colagem::Escrever(if cli == Cli::Codex { "\x16".into() } else { "\x1bv".into() });
        }
    }
    if !texto.is_empty() {
        return Colagem::Colar(texto.to_string());
    }
    Colagem::Erro(if imagem {
        "A imagem não foi colada: este terminal só recebe texto. Abra o Claude Code ou o Codex nele para anexar imagens.".into()
    } else {
        "Não há nada para colar no terminal.".into()
    })
}

/// Texto colado como o `paste` do xterm: quebras viram `\r` e, com o modo ligado, vai entre os marcadores.
pub fn texto_colado(texto: &str, bracketed: bool) -> String {
    let normal = texto.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        entre_colchetes(&normal)
    } else {
        normal
    }
}

#[cfg(test)]
mod testes {
    // Porte de tests/check-paste.mjs.
    use super::*;

    const PNG: &str = "C:\\proj\\a.png";
    const SPACED: &str = "C:\\com espaço\\b.png";
    const TXT: &str = "C:\\proj\\nota.txt";
    const DIR: &str = "C:\\proj\\pasta";

    fn v(itens: &[&str]) -> Vec<String> {
        itens.iter().map(|s| s.to_string()).collect()
    }

    fn escrito(c: Colagem) -> String {
        match c {
            Colagem::Escrever(s) => s,
            outro => panic!("esperava escrita, veio {outro:?}"),
        }
    }

    #[test]
    fn imagem_solta_usa_o_gesto_do_cli_sem_enter_nem_texto() {
        assert_eq!(colagem(Some(Cli::Claude), &[], true, "alt", true), Colagem::Escrever("\x1bv".into()));
        assert_eq!(colagem(Some(Cli::Codex), &[], true, "alt", true), Colagem::Escrever("\x16".into()));
    }

    #[test]
    fn arquivo_de_imagem_vai_como_caminho() {
        for cli in [Cli::Claude, Cli::Codex] {
            assert_eq!(colagem(Some(cli), &v(&[PNG]), true, "", true), Colagem::Escrever(format!("{BS}{PNG} {BE}")));
        }
    }

    #[test]
    fn claude_um_paste_um_por_linha_na_ordem_com_aspas() {
        let out = escrito(colagem(Some(Cli::Claude), &v(&[TXT, PNG, DIR, SPACED]), true, "", true));
        assert_eq!(out, format!("{BS}{TXT} \n{PNG} \n{DIR} \n\"{SPACED}\" {BE}"));
        assert!(!out.contains('\r'), "sem Enter");
    }

    #[test]
    fn codex_um_paste_por_arquivo_separados_por_f12() {
        let out = escrito(colagem(Some(Cli::Codex), &v(&[TXT, PNG, DIR, SPACED]), true, "", true));
        let partes: Vec<&str> = out.split(F12).collect();
        assert_eq!(
            partes,
            vec![
                format!("{BS}{TXT} {BE}"),
                format!("{BS}{PNG} {BE}"),
                format!("{BS}{DIR} {BE}"),
                format!("{BS}\"{SPACED}\" {BE}")
            ]
        );
        assert!(!out.contains(['\r', '\n']));
        let sem = out.replace(BS, "").replace(BE, "").replace(F12, "");
        assert!(!sem.contains('\x1b'), "nenhuma outra tecla");
    }

    #[test]
    fn mesmo_terminal_depois_que_o_cli_sai_vira_texto() {
        assert!(matches!(colagem(Some(Cli::Codex), &v(&[PNG, TXT]), false, "", true), Colagem::Escrever(_)));
        assert_eq!(colagem(None, &v(&[PNG, TXT, SPACED]), false, "", true), Colagem::Colar(format!("{PNG} {TXT} \"{SPACED}\"")));
        assert_eq!(colagem(None, &v(&[PNG]), false, "", false), Colagem::Colar(PNG.into()));
    }

    #[test]
    fn cada_terminal_cola_pelo_proprio_cli() {
        let outs: std::collections::HashSet<String> = [Some(Cli::Claude), Some(Cli::Codex), None]
            .into_iter()
            .map(|cli| format!("{:?}", colagem(cli, &v(&[PNG, TXT]), true, "", true)))
            .collect();
        assert_eq!(outs.len(), 3);
    }

    #[test]
    fn cli_sem_bracketed_cola_texto() {
        assert_eq!(colagem(Some(Cli::Claude), &v(&[PNG, TXT]), false, "", false), Colagem::Colar(format!("{PNG} {TXT}")));
    }

    #[test]
    fn o_que_nao_da_para_colar_vira_erro() {
        assert!(matches!(colagem(None, &[], true, "", true), Colagem::Erro(e) if e.contains("imagem não foi colada")));
        assert!(matches!(colagem(Some(Cli::Codex), &[], false, "", true), Colagem::Erro(e) if e.contains("nada para colar")));
        assert_eq!(colagem(None, &[], true, "texto junto", true), Colagem::Colar("texto junto".into()));
    }

    #[test]
    fn texto_colado_como_o_xterm() {
        assert_eq!(texto_colado("a\r\nb\nc", false), "a\rb\rc");
        assert_eq!(texto_colado("x", true), format!("{BS}x{BE}"));
    }
}
