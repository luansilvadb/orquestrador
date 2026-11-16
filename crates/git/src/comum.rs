//! Tipos e peças comuns às telas do Git: o alvo de uma aba, cores e letras de cada tipo de alteração, datas em pt-BR,
//! caminhos e os botões pequenos do painel.
use crate::fonte::{GitRef, Kind, RefKind};
use chrono::{DateTime, Datelike, Local, Timelike};
use orquestrador_ui::avisos::{self, Aviso};
use gpui_kit::{div, img, prelude::*, px, rgb, App, Div, Hsla, SharedString, Stateful, Window};
use orquestrador_ui::tema::{self, cor};
use orquestrador_ui::widgets::icone;

/// Lado de um diff do índice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Lado {
    Preparadas,
    Alteracoes,
}

impl Lado {
    pub fn staged(self) -> bool {
        self == Lado::Preparadas
    }
}

/// O que uma aba do editor mostra a partir do Git (diff, conflito e commit).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Alvo {
    Diff { lado: Lado, caminho: String },
    Conflito { caminho: String },
    Commit { hash: String, caminho: String },
}

impl Alvo {
    pub fn caminho(&self) -> &str {
        match self {
            Alvo::Diff { caminho, .. } | Alvo::Conflito { caminho } | Alvo::Commit { caminho, .. } => caminho,
        }
    }

    /// Título da aba: `nome · conflito`, `curto · nome` (commit) e `nome · alterações/preparadas`.
    pub fn titulo(&self) -> String {
        let nome = dividir(self.caminho()).0;
        match self {
            Alvo::Diff { lado: Lado::Preparadas, .. } => format!("{nome} · preparadas"),
            Alvo::Diff { lado: Lado::Alteracoes, .. } => format!("{nome} · alterações"),
            Alvo::Conflito { .. } => format!("{nome} · conflito"),
            Alvo::Commit { hash, .. } => format!("{} · {nome}", &hash[..hash.len().min(7)]),
        }
    }
}

/// Título e descrição numa linha cada.
fn com_descricao(titulo: SharedString, descricao: &str) -> SharedString {
    if descricao.is_empty() {
        titulo
    } else {
        format!("{titulo}\n{descricao}").into()
    }
}

/// Erro de ação: a mensagem do backend em aviso por 7 s.
pub fn falha(texto: impl Into<SharedString>, _: &mut Window, cx: &mut App) {
    avisos::mostrar(Aviso::erro(texto, 7000), cx);
}

/// Recusa explicada (executor ativo, operação em andamento), por 4 s.
pub fn aviso(texto: impl Into<SharedString>, _: &mut Window, cx: &mut App) {
    avisos::mostrar(Aviso::alerta(texto, 4000), cx);
}

pub fn aviso_longo(titulo: impl Into<SharedString>, descricao: &str, _: &mut Window, cx: &mut App) {
    avisos::mostrar(Aviso::alerta(com_descricao(titulo.into(), descricao), 7000), cx);
}

/// Sucesso por 3 s, com a descrição quando houver.
pub fn sucesso(titulo: impl Into<SharedString>, descricao: &str, _: &mut Window, cx: &mut App) {
    avisos::mostrar(Aviso::sucesso(com_descricao(titulo.into(), descricao), 3000), cx);
}

pub const TRAVADO: &str = "Há executores trabalhando. Espere o lote terminar.";

pub fn ultima_linha(t: &str) -> String {
    t.trim().lines().last().unwrap_or("").to_string()
}

pub fn commits(n: u32) -> String {
    format!("{n} {}", if n == 1 { "commit" } else { "commits" })
}

/// Nome e pasta de um caminho com "/".
pub fn dividir(caminho: &str) -> (&str, &str) {
    let (pasta, nome) = orquestrador_ui::caminhos::dividir(caminho);
    (nome, pasta)
}

/// Letra, título e cor de cada tipo (KIND do painel e KIND_COLOR das decorações).
pub fn letra(k: Kind) -> &'static str {
    match k {
        Kind::Modified => "M",
        Kind::Added => "A",
        Kind::Deleted => "D",
        Kind::Renamed => "R",
        Kind::Untracked => "U",
        Kind::Conflicted => "C",
    }
}

pub fn palavra(k: Kind) -> &'static str {
    match k {
        Kind::Modified => "modificado",
        Kind::Added => "novo",
        Kind::Deleted => "apagado",
        Kind::Renamed => "renomeado",
        Kind::Untracked => "não rastreado",
        Kind::Conflicted => "em conflito",
    }
}

pub fn cor_tipo(k: Kind) -> Hsla {
    rgb(match k {
        Kind::Conflicted => 0xe4676b,
        Kind::Deleted => 0xc74e39,
        Kind::Modified => 0xe2c08d,
        Kind::Added => 0x81b88b,
        Kind::Renamed | Kind::Untracked => 0x73c991,
    })
    .into()
}

/// Cores das etiquetas: branch local azul, remota lilás, tag laranja.
pub fn cor_ref(k: RefKind) -> Hsla {
    rgb(match k {
        RefKind::Remote => 0xb180d7,
        RefKind::Tag => 0xea5c00,
        _ => 0x59a4f9,
    })
    .into()
}

/// Paleta das faixas do grafo (scmGraph do VS Code), a primeira a da branch atual.
pub fn cor_faixa(n: usize) -> Hsla {
    const P: [u32; 6] = [0x59a4f9, 0xffb000, 0xdc267f, 0x994f00, 0x40b0a6, 0xb66dff];
    rgb(P[n % P.len()]).into()
}

/// `--ant-color-text-secondary` (#AAAAAA): abas inativas, cabeçalho e segunda linha do histórico, botão desabilitado.
pub fn secundario() -> Hsla {
    cor::text_secondary()
}

/// O `Text type="secondary"` e a cor dos `ActionIcon` do Lobe (#6F6F6F): avisos, contadores, dicas.
pub fn descricao() -> Hsla {
    cor::text_tertiary()
}

pub fn perigo() -> Hsla {
    cor::signal_error()
}

/// Ícone do arquivo (Material Icon Theme), colorido: `img`, não `svg`.
pub fn icone_arquivo(caminho: &str, tamanho: f32) -> impl IntoElement {
    img(orquestrador_ui::icone_arquivo(caminho, false, false)).size(px(tamanho)).flex_none()
}

/// Animação do ícone a cada clique: atualizar dá uma volta; Enviar solta a seta para cima, Receber
/// e Buscar para baixo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Animacao {
    Giro,
    Subir,
    Descer,
}

/// `cubic-bezier(0.22, 1, 0.36, 1)` aproximada pela saída cúbica.
fn suave(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// O ícone no quadro atual da animação começada em `inicio`; pede o próximo quadro enquanto ela dura.
pub fn icone_animado(
    caminho: &'static str,
    tamanho: f32,
    c: Hsla,
    anim: Option<(Animacao, std::time::Instant)>,
    window: &mut Window,
) -> gpui_kit::Svg {
    let svg = icone(caminho, tamanho, c);
    let Some((tipo, inicio)) = anim else { return svg };
    let ms = inicio.elapsed().as_secs_f32() * 1000.;
    match tipo {
        Animacao::Giro if ms < 500. => {
            window.request_animation_frame();
            svg.with_transformation(gpui_kit::Transformation::rotate(gpui_kit::percentage(suave(ms / 500.))))
        }
        Animacao::Subir | Animacao::Descer if ms < 600. => {
            window.request_animation_frame();
            let dir = if tipo == Animacao::Subir { -1. } else { 1. };
            let t = ms / 600.;
            // A seta sai pelo lado da direção (40% do tempo) e volta pelo oposto.
            let (dy, op) = if t < 0.4 { (dir * 8. * (t / 0.4).powi(2), 1. - t / 0.4) } else {
                let u = suave((t - 0.41) / 0.59);
                (-dir * 8. * (1. - u), u)
            };
            icone(caminho, tamanho, c.opacity(op))
                .with_transformation(gpui_kit::Transformation::translate(gpui_kit::point(px(0.), px(dy * tamanho / 24.))))
        }
        _ => svg,
    }
}

/// Ação de cabeçalho de seção: 22 px, ícone de 14 px, raio 4 (SECTION_ACTION).
pub fn acao_secao(id: impl Into<gpui_kit::ElementId>, caminho: &'static str, habilitada: bool) -> Stateful<Div> {
    acao_secao_com(id, icone(caminho, 14., descricao()), habilitada)
}

pub fn acao_secao_com(id: impl Into<gpui_kit::ElementId>, svg: gpui_kit::Svg, habilitada: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(22.))
        .rounded(tema::RAIO_XS)
        .when(habilitada, |d| d.cursor_pointer().hover(|s| s.bg(cor::fill_hover())))
        .when(!habilitada, |d| d.opacity(0.35))
        .child(svg)
}

/// Ação pequena sem borda (`ActionIcon size="small"`): 24 px, ícone 14.
pub fn acao(id: impl Into<gpui_kit::ElementId>, caminho: &'static str, habilitada: bool) -> Stateful<Div> {
    acao_cor(id, caminho, habilitada, descricao())
}

pub fn acao_cor(id: impl Into<gpui_kit::ElementId>, caminho: &'static str, habilitada: bool, c: Hsla) -> Stateful<Div> {
    acao_com(id, icone(caminho, 14., c), habilitada)
}

pub fn acao_com(id: impl Into<gpui_kit::ElementId>, svg: gpui_kit::Svg, habilitada: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .flex_none()
        .size(px(24.))
        .rounded(tema::RAIO_SM)
        .when(habilitada, |d| d.cursor_pointer().hover(|s| s.bg(cor::fill_hover())))
        .when(!habilitada, |d| d.opacity(0.35))
        .child(svg)
}

/// Linha de status no lugar de uma lista (lendo, vazio, erro).
pub fn nota(texto: impl Into<SharedString>, perigo_: bool) -> Div {
    div()
        .px(px(12.))
        .py(px(6.))
        .text_size(tema::TEXTO_LABEL)
        .text_color(if perigo_ { perigo() } else { descricao() })
        .child(texto.into())
}

/// Botão pequeno do Lobe (`Button size="small"`), padrão ou texto.
pub fn botao(id: impl Into<gpui_kit::ElementId>, rotulo: impl Into<SharedString>, habilitado: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(4.))
        .flex_none()
        .h(px(24.))
        .px(px(9.))
        .rounded(tema::RAIO_MENU)
        .bg(cor::panel_black())
        .border_1()
        .border_color(cor::hairline())
        .text_size(tema::TEXTO_LABEL)
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .when(habilitado, |d| {
            d.text_color(cor::text_primary()).cursor_pointer().hover(|s| s.border_color(gpui_kit::rgba(0xffffff40)))
        })
        .when(!habilitado, |d| d.text_color(gpui_kit::rgb(0x555555)))
        .child(rotulo.into())
}

/// Etiqueta de uma ref: pílula de 18 px na cor do tipo, texto escuro de 12 px; a branch atual tem o alvo e sempre o nome.
pub fn etiqueta(r: &GitRef, mostrar_nome: bool) -> impl IntoElement {
    let caminho = if r.current {
        "icons/target.svg"
    } else {
        match r.kind {
            RefKind::Remote => "icons/cloud.svg",
            RefKind::Tag => "icons/tag.svg",
            _ => "icons/git-branch.svg",
        }
    };
    let escuro: Hsla = rgb(0x181818).into();
    div()
        .flex()

        .min_w_0()
        .max_w(px(144.))
        .h(px(18.))
        .items_center()
        .rounded(px(10.))
        .bg(cor_ref(r.kind))
        .text_color(escuro)
        .child(div().p(px(1.)).flex_none().child(icone(caminho, 14., escuro)))
        .when(r.current || mostrar_nome, |d| {
            d.child(div().min_w_0().truncate().pr(px(4.)).text_size(tema::TEXTO_LABEL).line_height(px(18.)).child(r.name.clone()))
        })
}

fn data(iso: &str) -> Option<DateTime<Local>> {
    DateTime::parse_from_rfc3339(iso).ok().map(|d| d.with_timezone(&Local))
}

const MESES: [&str; 12] =
    ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];

/// `dateStyle: medium, timeStyle: short` em pt-BR: "9 de nov. de 2026, 11:05".
pub fn quando_medio(iso: &str) -> String {
    let Some(d) = data(iso) else { return iso.to_string() };
    let mes = MESES[d.month0() as usize];
    let curto = if mes.len() > 4 { format!("{}.", mes.chars().take(3).collect::<String>()) } else { mes.to_string() };
    format!("{} de {} de {}, {:02}:{:02}", d.day(), curto, d.year(), d.hour(), d.minute())
}

/// `dateStyle: long, timeStyle: short` em pt-BR: "9 de novembro de 2026 às 11:05".
pub fn quando_longo(iso: &str) -> String {
    let Some(d) = data(iso) else { return iso.to_string() };
    format!("{} de {} de {} às {:02}:{:02}", d.day(), MESES[d.month0() as usize], d.year(), d.hour(), d.minute())
}

/// `Intl.RelativeTimeFormat('pt-BR', { numeric: 'auto' })`.
pub fn data_relativa(iso: &str) -> String {
    let Some(d) = data(iso) else { return iso.to_string() };
    relativa((d - Local::now()).num_milliseconds() as f64 / 1000.)
}

pub fn relativa(segundos: f64) -> String {
    const UNIDADES: [(&str, &str, f64); 7] = [
        ("ano", "anos", 31_536_000.),
        ("mês", "meses", 2_592_000.),
        ("semana", "semanas", 604_800.),
        ("dia", "dias", 86_400.),
        ("hora", "horas", 3_600.),
        ("minuto", "minutos", 60.),
        ("segundo", "segundos", 1.),
    ];
    let (um, varios, tam) = UNIDADES.iter().copied().find(|(_, _, t)| segundos.abs() >= *t).unwrap_or(("segundo", "segundos", 1.));
    let n = (segundos / tam).round() as i64;
    // `numeric: auto`: os casos com nome próprio.
    let especial = match (um, n) {
        ("segundo", 0) => Some("agora"),
        ("dia", -1) => Some("ontem"),
        ("dia", 1) => Some("amanhã"),
        ("dia", -2) => Some("anteontem"),
        ("dia", 2) => Some("depois de amanhã"),
        ("semana", -1) => Some("semana passada"),
        ("semana", 1) => Some("próxima semana"),
        ("mês", -1) => Some("mês passado"),
        ("mês", 1) => Some("próximo mês"),
        ("ano", -1) => Some("ano passado"),
        ("ano", 1) => Some("próximo ano"),
        _ => None,
    };
    if let Some(e) = especial {
        return e.to_string();
    }
    let palavra = if n.abs() == 1 { um } else { varios };
    if n < 0 {
        format!("há {} {palavra}", -n)
    } else {
        format!("em {n} {palavra}")
    }
}

/// Coautores dos trailers `Co-Authored-By` no fim da mensagem.
pub fn coautores(mensagem: &str) -> Vec<String> {
    let linhas: Vec<&str> = mensagem.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    let mut fim = linhas.len();
    while fim > 0 && linhas[fim - 1].trim().is_empty() {
        fim -= 1;
    }
    let trailer = |l: &str| {
        let Some((k, v)) = l.split_once(':') else { return false };
        !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !v.trim().is_empty()
    };
    let mut inicio = fim;
    while inicio > 0 && trailer(linhas[inicio - 1]) {
        inicio -= 1;
    }
    if inicio == fim || inicio == 0 || !linhas[inicio - 1].trim().is_empty() {
        return Vec::new();
    }
    linhas[inicio..fim]
        .iter()
        .filter_map(|l| {
            let (k, v) = l.split_once(':')?;
            if !k.eq_ignore_ascii_case("co-authored-by") {
                return None;
            }
            let v = v.trim();
            let nome = match v.rfind('<') {
                Some(i) if v.ends_with('>') => v[..i].trim(),
                _ => v,
            };
            (!nome.is_empty()).then(|| nome.to_string())
        })
        .collect()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn datas_relativas_como_o_intl() {
        assert_eq!(relativa(-4. * 86_400.), "há 4 dias");
        assert_eq!(relativa(-16. * 86_400.), "há 2 semanas");
        assert_eq!(relativa(-20. * 86_400.), "há 3 semanas");
        assert_eq!(relativa(-86_400.), "ontem");
        assert_eq!(relativa(-0.2), "agora");
        assert_eq!(relativa(-3_600.), "há 1 hora");
    }

    #[test]
    fn coautores_dos_trailers() {
        let m = "Prepara v0.1.7\n\nConsolida.\n\nCo-Authored-By: Ana Exemplo <ana@example.com>";
        assert_eq!(coautores(m), ["Ana Exemplo"]);
        assert!(coautores("Só o assunto\nCo-Authored-By: X").is_empty());
    }

    #[test]
    fn titulos_e_caminhos() {
        assert_eq!(dividir("src/lib/utils.ts"), ("utils.ts", "src/lib"));
        assert_eq!(Alvo::Commit { hash: "4b2e8d0a1c".into(), caminho: "src/lib/utils.ts".into() }.titulo(), "4b2e8d0 · utils.ts");
        assert_eq!(Alvo::Conflito { caminho: "src/dois.ts".into() }.titulo(), "dois.ts · conflito");
    }
}
