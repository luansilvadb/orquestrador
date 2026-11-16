//! Comparação antes de gravar sobre o disco ou de usar o disco: as linhas do disco e as da
//! edição, lado a lado, com as removidas em vermelho e as inseridas em verde.
use gpui::{div, prelude::*, px, AnyElement, Hsla, SharedString};
use orquestrador_ui::tema::{self, cor};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Linha {
    Igual,
    Removida,
    Inserida,
}

/// Diferença de linhas por LCS (os textos de um arquivo aberto no editor são pequenos o bastante).
pub fn diferenca<'a>(a: &'a str, b: &'a str) -> Vec<(Linha, &'a str)> {
    let x: Vec<&str> = a.split('\n').collect();
    let y: Vec<&str> = b.split('\n').collect();
    let (n, m) = (x.len(), y.len());
    if n * m > 4_000_000 {
        let mut v: Vec<(Linha, &str)> = x.iter().map(|l| (Linha::Removida, *l)).collect();
        v.extend(y.iter().map(|l| (Linha::Inserida, *l)));
        return v;
    }
    let mut t = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[i][j] = if x[i] == y[j] { t[i + 1][j + 1] + 1 } else { t[i + 1][j].max(t[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut out = Vec::new();
    while i < n || j < m {
        if i < n && j < m && x[i] == y[j] {
            out.push((Linha::Igual, x[i]));
            i += 1;
            j += 1;
        } else if j < m && (i == n || t[i][j + 1] >= t[i + 1][j]) {
            out.push((Linha::Inserida, y[j]));
            j += 1;
        } else {
            out.push((Linha::Removida, x[i]));
            i += 1;
        }
    }
    out
}

fn fundo(l: Linha) -> Option<Hsla> {
    match l {
        Linha::Igual => None,
        Linha::Removida => Some(gpui::rgba(0xf4416c26).into()),
        Linha::Inserida => Some(gpui::rgba(0xc4f04226).into()),
    }
}

pub fn vista(caminho: &str, disco: &str, rascunho: &str, usar: bool) -> AnyElement {
    let disco = crate::armazem::normalizar_quebras(disco);
    let linhas = diferenca(&disco, rascunho);
    let rotulo: SharedString = if usar { "Disco (fica) ← sua edição (sai)".into() } else { "Disco (sai) → sua edição (fica)".into() };
    let mut corpo = div().id("comparacao-linhas").flex().flex_col().max_h(px(480.)).overflow_y_scroll().font_family(crate::documento::FONTE_EDITOR).text_size(tema::TEXTO_MONO);
    for (i, (tipo, texto)) in linhas.iter().enumerate() {
        let sinal = match tipo {
            Linha::Igual => " ",
            Linha::Removida => "-",
            Linha::Inserida => "+",
        };
        corpo = corpo.child(
            div()
                .id(("cmp", i))
                .flex()
                .px(px(8.))
                .when_some(fundo(*tipo), |d, c| d.bg(c))
                .child(div().w(px(16.)).flex_none().text_color(cor::text_tertiary()).child(sinal))
                .child(div().whitespace_nowrap().child(texto.to_string())),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(div().text_size(tema::TEXTO_LABEL).text_color(cor::text_secondary()).child(format!("{caminho} · {rotulo}")))
        .child(div().rounded(tema::RAIO_SM).border_1().border_color(cor::hairline()).bg(crate::documento::cor::fundo()).child(corpo))
        .into_any_element()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn diferenca_de_linhas() {
        let d = diferenca("a\nb\nc", "a\nx\nc");
        assert_eq!(d, [(Linha::Igual, "a"), (Linha::Inserida, "x"), (Linha::Removida, "b"), (Linha::Igual, "c")]);
    }
}
