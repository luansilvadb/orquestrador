//! O portão de abertura: no instalado, antes de liberar o uso, o app se atualiza (`crate::atualizacao`) e os dois CLIs
//! são garantidos (`clis::garantir_todos`), com o andamento na tela (porte de update.rs e da tela `Screen` do App.tsx).
//! O build de desenvolvimento libera direto.
use super::ui::{self, Variante};
use crate::tema::{self, cor};
use gpui_kit::TestSupportExt;
use gpui_kit::{
    anchored, canvas, deferred, div, point, prelude::*, px, size, App, Bounds, FontWeight, Hsla, MouseButton, PathBuilder,
    Pixels, Window,
};
use crate::atualizacao::Fase;
use crate::estado::AvisosDispensados;
use orquestrador_core::clis::{Andamento, Aviso, Cli, Passo};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq)]
pub enum Portao {
    Atualizacao(crate::atualizacao::Fase),
    Clis { claude: Andamento, codex: Andamento },
    Pronto,
    Falhou(String),
    /// Os CLIs estão prontos, mas algo não bloqueante falhou; o uso espera o "Continuar".
    Aviso(Vec<Aviso>),
}

impl Portao {
    pub fn inicial() -> Portao {
        if cfg!(debug_assertions) {
            Portao::Pronto
        } else {
            Portao::Atualizacao(Fase::Verificando)
        }
    }
}

fn passo(p: Passo) -> &'static str {
    match p {
        Passo::Checking => "conferindo…",
        Passo::Installing => "instalando…",
        Passo::Updating => "atualizando…",
        Passo::Ready => "pronto",
    }
}

pub fn linha_do_cli(nome: &str, a: &Andamento) -> String {
    match &a.version {
        Some(v) => format!("{nome} {v}: {}", passo(a.step)),
        None => format!("{nome}: {}", passo(a.step)),
    }
}

const MB: f64 = 1024. * 1024.;

/// O texto de status e as linhas dos CLIs.
pub fn textos(p: &Portao) -> (String, Vec<String>) {
    let (status, linhas): (&str, _) = match p {
        Portao::Atualizacao(Fase::Verificando) => ("Verificando se há uma versão nova…", vec![]),
        Portao::Atualizacao(Fase::Baixando { versao, baixado, total }) => {
            let tamanho = match total {
                Some(t) if *t > 0 => format!("{}%", (*baixado as f64 / *t as f64 * 100.).round()),
                _ => format!("{:.1} MB", *baixado as f64 / MB).replace('.', ","),
            };
            return (format!("Baixando a versão {versao}… {tamanho}"), vec![]);
        }
        Portao::Atualizacao(Fase::Instalando { versao }) => {
            return (format!("Instalando a versão {versao}. O app reabre sozinho."), vec![]);
        }
        Portao::Clis { claude, codex } => {
            ("Preparando os CLIs…", vec![linha_do_cli("Claude Code", claude), linha_do_cli("Codex", codex)])
        }
        Portao::Falhou(_) => ("Não foi possível atualizar. O uso fica bloqueado até a atualização dar certo.", vec![]),
        Portao::Aviso(_) => ("Os CLIs estão prontos, com um aviso.", vec![]),
        Portao::Pronto => ("", vec![]),
    };
    (status.to_string(), linhas)
}

/// Fração da barra do download; `None` fora dele, 1 com o total desconhecido (barra cheia).
pub fn progresso(p: &Portao) -> Option<f32> {
    match p {
        Portao::Atualizacao(Fase::Baixando { baixado, total: Some(t), .. }) if *t > 0 => Some((*baixado as f32 / *t as f32).min(1.)),
        Portao::Atualizacao(Fase::Baixando { .. }) => Some(1.),
        _ => None,
    }
}

/// A rodada numa thread: atualização e depois CLIs; o andamento chega por `avisar`, e o fim traz os caminhos ou o erro.
/// `dispensados` são os avisos já dispensados, que não voltam.
pub fn garantir(avisar: async_channel::Sender<Portao>, exes: orquestrador_core::agents::cli::Exes, dispensados: AvisosDispensados) {
    std::thread::spawn(move || {
        let progresso = avisar.clone();
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::atualizacao::rodada(|f| {
                let _ = progresso.try_send(Portao::Atualizacao(f));
            })?;
            orquestrador_core::clis::garantir_todos(&orquestrador_core::clis::Real, |claude, codex| {
                let _ = progresso.try_send(Portao::Clis { claude, codex });
            })
        }));
        let fim = match r {
            Ok(Ok(g)) => {
                exes.set(g.caminhos);
                fim_com_avisos(g.avisos, &dispensados)
            }
            Ok(Err(msg)) => Portao::Falhou(msg),
            Err(_) => Portao::Falhou("A verificação dos CLIs parou inesperadamente.".into()),
        };
        let _ = avisar.try_send(fim);
    });
}

/// Quanto a notificação do `update` que falhou fica à vista, se não for fechada antes.
pub const NOTIFICACAO_MS: u64 = 8000;

/// A notificação discreta de um aviso de `update`.
pub fn notificacao(texto: &str) -> orquestrador_ui::avisos::Aviso {
    orquestrador_ui::avisos::Aviso::alerta(texto.to_string(), NOTIFICACAO_MS)
}

/// O estado depois do sucesso: saem os avisos com a chave já dispensada para aquela origem; sem nenhum, liberado. Os
/// dispensáveis que sobram esperam o "Continuar" na tela; os de `update` viram notificação quando o uso é liberado.
pub fn fim_com_avisos(avisos: Vec<Aviso>, dispensados: &AvisosDispensados) -> Portao {
    let restantes: Vec<Aviso> = avisos
        .into_iter()
        .filter(|a| a.dispensa().is_none_or(|(origem, chave)| dispensados.chaves.get(origem).map(String::as_str) != Some(chave)))
        .collect();
    if restantes.is_empty() { Portao::Pronto } else { Portao::Aviso(restantes) }
}

/// Os dispensados depois do "Continuar": a chave de cada aviso mostrado substitui a anterior da mesma origem.
pub fn dispensar(mut dispensados: AvisosDispensados, mostrados: &[Aviso]) -> AvisosDispensados {
    for (origem, chave) in mostrados.iter().filter_map(Aviso::dispensa) {
        dispensados.chaves.insert(origem.to_string(), chave.to_string());
    }
    dispensados
}

/// O quadro de texto do portão, com o fundo e a borda no tom de `tom`.
fn quadro(id: &'static str, tom: Hsla, texto: String) -> impl IntoElement {
    div()
        .id(id)
        .text_left()
        .px(px(12.))
        .py(px(8.))
        .rounded(tema::RAIO_SM)
        .bg(tom.opacity(0x1f as f32 / 255.))
        .border_1()
        .border_color(tom.opacity(0x66 as f32 / 255.))
        .text_size(tema::TEXTO_BODY)
        .child(texto)
}

/// Um botão primário centralizado.
fn botao_central(id: &'static str, rotulo: &'static str, acao: impl Fn(&mut Window, &mut App) + 'static) -> impl IntoElement {
    div().flex().justify_center().child(
        ui::botao(id, rotulo, Variante::Primario, false).on_click(move |_, window, cx| acao(window, cx)).test_support(),
    )
}

/// A tela do portão sobre tudo, menos os controles da janela; `tentar` refaz a rodada depois de uma falha e
/// `continuar` libera o uso depois de um aviso.
pub fn tela(
    p: &Portao,
    inicio: Instant,
    window: &mut Window,
    tentar: impl Fn(&mut Window, &mut App) + 'static,
    continuar: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let janela = window.viewport_size();
    let (status, linhas) = textos(p);
    let falha = match p {
        Portao::Falhou(m) => Some(m.clone()),
        _ => None,
    };
    // Só os dispensáveis; os de `update` viram notificação depois do "Continuar".
    let aviso = match p {
        Portao::Aviso(avisos) => {
            Some(avisos.iter().filter(|a| a.dispensa().is_some()).map(Aviso::texto).collect::<Vec<_>>().join("\n\n"))
        }
        _ => None,
    };
    // Esperando o clique (falha ou aviso), a marca para e a tela não redesenha; só a falha apaga o centro.
    let marca = Marca { parada: falha.is_some() || aviso.is_some(), erro: falha.is_some() };
    let barra = progresso(p);
    let marca = canvas(|_, _, _| (), move |b, _, window, _| pintar_marca(b, inicio, marca, window)).size(px(64.)).mx_auto().mb(px(4.));
    let topo = div()
        .id("portao-topo")
        .window_control_area(gpui_kit::WindowControlArea::Drag)
        .flex()
        .items_center()
        .h(tema::TITLEBAR)
        .w(janela.width - tema::BOTAO_JANELA * 3.)
        .px(px(10.))
        .bg(cor::panel_black())
        .border_b_1()
        .border_color(cor::hairline())
        .child(gpui_kit::img(crate::recursos::icone::MARCA).size(px(16.)).flex_none());
    let coluna = div()
        .id("portao")
        .flex()
        .flex_col()
        .gap(px(16.))
        .w_full()
        .max_w(px(448.))
        .text_center()
        .child(marca)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(div().text_size(tema::TEXTO_TITLE).font_weight(FontWeight::SEMIBOLD).line_height(px(16.)).child("Orquestrador"))
                .child(
                    div()
                        .id("portao-status")
                        .flex()
                        .flex_col()
                        .text_size(tema::TEXTO_BODY)
                        .text_color(cor::text_secondary())
                        .child(status)
                        .children(linhas.into_iter().enumerate().map(|(i, l)| div().id(("portao-cli", i)).child(l)))
                        .test_support(),
                ),
        )
        .when_some(barra, |d, f| {
            d.child(
                div()
                    .id("portao-progresso")
                    .h(px(4.))
                    .w_full()
                    .rounded(px(2.))
                    .bg(cor::hairline())
                    .child(div().h_full().w(gpui_kit::relative(f)).rounded(px(2.)).bg(cor::signal_warning())),
            )
        })
        .when_some(falha, |d, m| {
            d.child(quadro("portao-erro", cor::signal_error(), m)).child(botao_central("portao-tentar", "Tentar novamente", tentar))
        })
        .when_some(aviso, |d, m| {
            d.child(quadro("portao-aviso", cor::signal_warning(), m)).child(botao_central("portao-continuar", "Continuar", continuar))
        });
    deferred(
        anchored().position(point(px(0.), px(0.))).child(
            div()
                .id("portao-tela")
                .relative()
                .w(janela.width)
                .h(janela.height)
                .text_color(cor::text_primary())
                .font_family(tema::FONTE)
                // Os controles da janela, que a tela não cobre, seguem funcionando.
                .child(topo.occlude())
                .child(
                    div()
                        .id("portao-corpo")
                        .occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .bg(cor::pitch_black())
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(tema::TITLEBAR)
                        .bottom_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(24.))
                        .child(coluna),
                ),
        ),
    )
    .with_priority(3)
}

// ---------- a marca animada do index.html ----------

/// `cubic-bezier(.2, .8, .2, 1)` aproximada: sai rápido e assenta.
fn suave(t: f32) -> f32 {
    1. - (1. - t.clamp(0., 1.)).powi(3)
}

/// Interpola nos quadros-chave `(fração, valor)`, com a curva entre eles.
fn quadros(t: f32, chaves: &[(f32, f32)]) -> f32 {
    for par in chaves.windows(2) {
        let ((a, va), (b, vb)) = (par[0], par[1]);
        if t <= b {
            let f = if b > a { suave((t - a) / (b - a)) } else { 1. };
            return va + (vb - va) * f;
        }
    }
    chaves.last().map_or(0., |c| c.1)
}

const CICLO: f32 = 2400.;

/// Como a marca aparece: `parada` congela os ramos abertos e não pede outro quadro; `erro` apaga o centro laranja.
#[derive(Clone, Copy)]
struct Marca {
    parada: bool,
    erro: bool,
}

fn pintar_marca(b: Bounds<Pixels>, inicio: Instant, Marca { parada, erro }: Marca, window: &mut Window) {
    let ms = if parada { 0. } else { inicio.elapsed().as_secs_f32() * 1000. };
    let escala = f32::from(b.size.width) / 200.;
    let p = |x: f32, y: f32| point(b.origin.x + px((x - 28.) * escala), b.origin.y + px((y - 28.) * escala));
    let tinta: Hsla = gpui_kit::rgb(0xeceae4).into();
    let hub = (73.9, 182.1);
    let raizes = [((73.9, 59.1), 200.), ((156.3, 91.1), 360.), ((196.3, 169.2), 520.)];
    let circulo = |window: &mut Window, (x, y): (f32, f32), r: f32, cor: Hsla| {
        let c = p(x, y);
        let r = px(r * escala);
        window.paint_quad(gpui_kit::quad(
            Bounds::new(point(c.x - r, c.y - r), size(r * 2., r * 2.)),
            r,
            cor,
            px(0.),
            gpui_kit::transparent_black(),
            Default::default(),
        ));
    };
    for ((x, y), atraso) in raizes {
        let t = if parada { 0.5 } else { (((ms - atraso).max(0.)) % CICLO) / CICLO };
        let antes = !parada && ms < atraso;
        let visivel = if antes { 0. } else { quadros(t, &[(0., 0.), (0.22, 1.), (0.70, 1.), (0.90, 0.), (1., 0.)]) };
        if visivel > 0.01 {
            let fim = (hub.0 + (x - hub.0) * visivel, hub.1 + (y - hub.1) * visivel);
            let mut caminho = PathBuilder::stroke(px(19. * escala));
            caminho.move_to(p(hub.0, hub.1));
            caminho.line_to(p(fim.0, fim.1));
            if let Ok(c) = caminho.build() {
                window.paint_path(c, tinta);
            }
            circulo(window, fim, 9.5, tinta);
        }
        let (op, esc) = if antes {
            (0., 0.3)
        } else {
            (
                quadros(t, &[(0., 0.), (0.16, 0.), (0.26, 1.), (0.60, 1.), (0.70, 0.), (1., 0.)]),
                quadros(t, &[(0., 0.3), (0.16, 0.3), (0.26, 1.15), (0.32, 1.), (0.60, 1.), (0.70, 0.3), (1., 0.3)]),
            )
        };
        if op > 0.01 {
            circulo(window, (x, y), 23. * esc, tinta.opacity(op));
        }
    }
    let pop = if erro { 0. } else if parada { 1. } else { (ms / 360.).min(1.) };
    let batida = if parada || ms < 360. { 1. } else { quadros(((ms - 360.) % CICLO) / CICLO, &[(0., 1.), (0.08, 1.1), (0.20, 1.), (1., 1.)]) };
    let laranja: Hsla = gpui_kit::rgb(0xf5a524).into();
    circulo(window, hub, 36.5 * (0.4 + 0.6 * suave(pop)) * batida, laranja.opacity(suave(pop)));
    if !parada {
        window.request_animation_frame();
    }
}

/// O portão como vista: segue o andamento da rodada e desenha a tela enquanto o uso não foi liberado.
pub struct Abertura {
    pub estado: Portao,
    inicio: Instant,
    exes: orquestrador_core::agents::cli::Exes,
    _tarefa: Option<gpui_kit::Task<()>>,
}

impl Abertura {
    pub fn new(exes: orquestrador_core::agents::cli::Exes, cx: &mut gpui_kit::Context<Self>) -> Self {
        let mut a = Abertura { estado: Portao::inicial(), inicio: Instant::now(), exes, _tarefa: None };
        // Só no debug: as notificações de `update` que falhou, para conferir sem rodar os CLIs.
        if cfg!(debug_assertions) && std::env::var("ORQ_PORTAO").as_deref() == Ok("notificacao") {
            let avisos = vec![
                Aviso::update(Cli::Claude, "saiu com código 1", "2.1.325"),
                Aviso::update(Cli::Codex, "não terminou em 15 min", "0.169.0"),
            ];
            a.chegou(fim_com_avisos(avisos, &AvisosDispensados::default()), cx);
            return a;
        }
        // Só no debug, como o `?update=clis` do app simulado: a fase dos CLIs parada, para conferir a tela.
        if cfg!(debug_assertions) && std::env::var("ORQ_PORTAO").as_deref() == Ok("clis") {
            a.estado = Portao::Clis {
                claude: Andamento { step: Passo::Updating, version: Some("2.1.325".into()) },
                codex: Andamento { step: Passo::Installing, version: None },
            };
            return a;
        }
        if a.estado != Portao::Pronto {
            a.rodar(cx);
        }
        a
    }

    pub fn pronto(&self) -> bool {
        self.estado == Portao::Pronto
    }

    fn rodar(&mut self, cx: &mut gpui_kit::Context<Self>) {
        let (tx, rx) = async_channel::unbounded();
        garantir(tx, self.exes.clone(), orquestrador_ui::estado::ler::<AvisosDispensados>(cx));
        self._tarefa = Some(cx.spawn(async move |this: gpui_kit::WeakEntity<Self>, cx| {
            while let Ok(chegada) = rx.recv().await {
                if this.update(cx, |a, cx| a.chegou(chegada, cx))
                .is_err()
                {
                    break;
                }
            }
        }));
    }

    /// Um estado novo da rodada; um aviso sem nada para dispensar libera o uso na hora.
    pub(crate) fn chegou(&mut self, estado: Portao, cx: &mut gpui_kit::Context<Self>) {
        self.estado = estado;
        if matches!(&self.estado, Portao::Aviso(avisos) if avisos.iter().all(|a| a.dispensa().is_none())) {
            self.liberar(cx);
        }
        cx.notify();
    }

    /// Passa a `Pronto`; os avisos de `update` do estado viram notificação.
    fn liberar(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if let Portao::Aviso(avisos) = std::mem::replace(&mut self.estado, Portao::Pronto) {
            for a in avisos.iter().filter(|a| a.dispensa().is_none()) {
                orquestrador_ui::avisos::mostrar(notificacao(a.texto()), cx);
            }
        }
    }

    /// "Tentar novamente": só depois de uma falha.
    pub fn tentar(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if matches!(self.estado, Portao::Falhou(_)) {
            self.estado = Portao::Atualizacao(Fase::Verificando);
            self.rodar(cx);
            cx.notify();
        }
    }

    /// "Continuar": só depois de um aviso; guarda as chaves dos mostrados e libera o uso.
    pub fn continuar(&mut self, cx: &mut gpui_kit::Context<Self>) {
        if let Portao::Aviso(mostrados) = &self.estado {
            let antes = orquestrador_ui::estado::ler::<AvisosDispensados>(cx);
            orquestrador_ui::estado::gravar(&dispensar(antes, mostrados), cx);
            self.liberar(cx);
            cx.notify();
        }
    }
}

impl Render for Abertura {
    fn render(&mut self, window: &mut Window, cx: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        if self.pronto() {
            return div().into_any_element();
        }
        let eu = cx.entity().downgrade();
        let eu2 = eu.clone();
        tela(
            &self.estado,
            self.inicio,
            window,
            move |_, cx| {
                let _ = eu.update(cx, |a, cx| a.tentar(cx));
            },
            move |_, cx| {
                let _ = eu2.update(cx, |a, cx| a.continuar(cx));
            },
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod testes {
    // Porte de tests/check-update-clis-ui.mjs (o texto) e da forma da fase `clis`.
    use super::*;

    #[test]
    fn fase_clis_uma_linha_por_cli_com_versao_e_passo() {
        let p = Portao::Clis {
            claude: Andamento { step: Passo::Updating, version: Some("2.1.325".into()) },
            codex: Andamento { step: Passo::Installing, version: None },
        };
        let (status, linhas) = textos(&p);
        assert_eq!(status, "Preparando os CLIs…");
        assert_eq!(linhas, vec!["Claude Code 2.1.325: atualizando…", "Codex: instalando…"]);
    }

    #[test]
    fn fases_da_atualizacao() {
        let t = |f| textos(&Portao::Atualizacao(f)).0;
        assert_eq!(t(Fase::Verificando), "Verificando se há uma versão nova…");
        assert_eq!(t(Fase::Baixando { versao: "0.2.0".into(), baixado: 50, total: Some(200) }), "Baixando a versão 0.2.0… 25%");
        assert_eq!(t(Fase::Baixando { versao: "0.2.0".into(), baixado: 1572864, total: None }), "Baixando a versão 0.2.0… 1,5 MB");
        assert_eq!(t(Fase::Instalando { versao: "0.2.0".into() }), "Instalando a versão 0.2.0. O app reabre sozinho.");
        assert_eq!(progresso(&Portao::Atualizacao(Fase::Baixando { versao: "x".into(), baixado: 1, total: Some(4) })), Some(0.25));
        assert_eq!(progresso(&Portao::Atualizacao(Fase::Baixando { versao: "x".into(), baixado: 1, total: None })), Some(1.));
        assert_eq!(progresso(&Portao::Atualizacao(Fase::Verificando)), None);
    }

    #[test]
    fn falha_bloqueia_com_o_motivo_e_desenvolvimento_libera() {
        assert!(textos(&Portao::Falhou("x".into())).0.starts_with("Não foi possível atualizar"));
        assert_eq!(Portao::inicial(), Portao::Pronto, "o teste roda no build de desenvolvimento");
    }

    fn path(cli: Cli, chave: &str, texto: &str) -> Aviso {
        Aviso::Path { cli, chave: chave.into(), texto: texto.into() }
    }

    const CHAVE: &str = r"c:\users\u\.local\bin|5";

    #[test]
    fn sem_aviso_libera_e_com_path_o_update_vai_junto() {
        let nada = AvisosDispensados::default();
        assert_eq!(fim_com_avisos(vec![], &nada), Portao::Pronto);
        let (a, u) = (path(Cli::Claude, CHAVE, "a"), Aviso::Update("b".into()));
        let p = fim_com_avisos(vec![a.clone(), u.clone()], &nada);
        assert_eq!(p, Portao::Aviso(vec![a, u.clone()]));
        assert_eq!(textos(&p), ("Os CLIs estão prontos, com um aviso.".to_string(), vec![]));
        assert_eq!(progresso(&p), None, "sem barra");
        assert_eq!(fim_com_avisos(vec![u.clone()], &nada), Portao::Aviso(vec![u]), "o `update` nunca é filtrado");
    }

    fn dispensado(a: &Aviso) -> AvisosDispensados {
        dispensar(AvisosDispensados::default(), std::slice::from_ref(a))
    }

    #[test]
    fn mesma_chave_com_outro_texto_e_filtrada() {
        let d = dispensado(&path(Cli::Claude, CHAVE, "Acesso negado"));
        assert_eq!(fim_com_avisos(vec![path(Cli::Claude, CHAVE, "Access is denied")], &d), Portao::Pronto);
    }

    #[test]
    fn outra_chave_ou_outro_cli_aparece() {
        let d = dispensado(&path(Cli::Claude, CHAVE, "x"));
        for novo in [
            path(Cli::Claude, r"c:\users\u\.local\bin|1314", "x"),
            path(Cli::Claude, r"c:\users\u\.local\bin|sem-codigo", "x"),
            path(Cli::Claude, r"c:\outra|5", "x"),
            path(Cli::Codex, CHAVE, "x"),
        ] {
            assert_eq!(fim_com_avisos(vec![novo.clone()], &d), Portao::Aviso(vec![novo]));
        }
    }

    #[test]
    fn um_registro_por_cli_e_dispensar_de_novo_substitui() {
        let primeiro = path(Cli::Claude, CHAVE, "x");
        let segundo = path(Cli::Claude, r"c:\outra|5", "x");
        let codex = path(Cli::Codex, CHAVE, "x");
        let d = dispensar(dispensado(&primeiro), &[segundo.clone(), Aviso::Update("u".into()), codex]);
        assert_eq!(d.chaves.len(), 2);
        assert_eq!((d.chaves["claude.exe"].as_str(), d.chaves["codex.exe"].as_str()), (r"c:\outra|5", CHAVE));
        assert_eq!(fim_com_avisos(vec![primeiro.clone()], &d), Portao::Aviso(vec![primeiro]), "o anterior saiu");
        assert_eq!(fim_com_avisos(vec![segundo], &d), Portao::Pronto);
    }

    #[test]
    fn quadros_chave_interpolam_e_seguram() {
        let k = [(0., 0.), (0.22, 1.), (0.70, 1.), (0.90, 0.), (1., 0.)];
        assert_eq!(quadros(0., &k), 0.);
        assert_eq!(quadros(0.5, &k), 1.);
        assert_eq!(quadros(0.95, &k), 0.);
    }
}
