//! Vitrine da tela Git com o simulado (os dados de `src/backend/mock.ts`), na posição da bancada: titlebar de 36 px,
//! navegação de 44, lateral de 280 e o editor no meio. Uma cena por referência de `docs/referencia-visual/`:
//! `cargo run -p orquestrador-git --example vitrine-git -- <cena>`, com cena = alteracoes | historico | commit-diff |
//! lado-a-lado | unificado | sem-repositorio | ausente | conflito.
use gpui_kit::{div, prelude::*, px, size, AnyView, App, Bounds, Context, Entity, Window, WindowBounds, WindowOptions};
use orquestrador_git::simulado::{ModoSimulado, Simulado};
use orquestrador_git::{Alvo, Fonte, GuardaGit, Lado, TelaGit, VisaoConflito, VisaoDiff};
use orquestrador_ui::tema::{self, cor};
use std::sync::Arc;
use std::time::Duration;

struct Vitrine {
    tela: Entity<TelaGit>,
    centro: Option<AnyView>,
}

impl Render for Vitrine {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let faixa = || div().h(tema::FAIXA).flex_none().border_b_1().border_color(cor::hairline());
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(cor::pitch_black())
            .font_family(tema::FONTE)
            .text_color(cor::text_primary())
            .child(div().h(tema::TITLEBAR).flex_none().bg(cor::panel_black()))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().w(px(44.)).flex_none().bg(cor::panel_black()).border_r_1().border_color(cor::hairline_soft()))
                    .child(div().w(px(280.)).flex_none().border_r_1().border_color(cor::hairline_soft()).child(self.tela.clone()))
                    .child(
                        div()
                            .w(px(504.))
                            .flex_none()
                            .flex()
                            .flex_col()
                            .child(faixa())
                            .child(div().flex_1().min_h_0().children(self.centro.clone())),
                    )
                    .child(div().flex_1().bg(cor::painel_arrastavel())),
            )
            .child(div().h(tema::STATUS_BAR).flex_none().bg(cor::panel_black()))
            .child(orquestrador_ui::avisos::camada(window, cx))
    }
}

fn main() {
    let cena = std::env::args().nth(1).unwrap_or_else(|| "alteracoes".into());
    gpui_kit::application().with_assets(orquestrador_ui::recursos::Recursos).run(move |cx: &mut App| {
        gpui_kit::init(cx);
        tema::aplicar(cx);
        orquestrador_git::init(cx);
        let modo = match cena.as_str() {
            "sem-repositorio" => ModoSimulado::SemRepo,
            "ausente" => ModoSimulado::SemGit,
            _ => ModoSimulado::Repo,
        };
        let simulado = Arc::new(Simulado::new(modo));
        if cena == "conflito" {
            simulado.conflito(
                "src/dois.ts",
                Some("atual-1\natual-2\n"),
                Some("recebida-1\nrecebida-2\n"),
                Some("topo\n<<<<<<< ours\natual-1\n=======\nrecebida-1\n>>>>>>> theirs\nmeio\n<<<<<<< ours\natual-2\n=======\nrecebida-2\n>>>>>>> theirs\nfim\n"),
            );
        }
        let fonte: Fonte = simulado;
        let guarda = GuardaGit::new();
        let bounds = Bounds::centered(None, size(px(tema::JANELA_LARGURA), px(tema::JANELA_ALTURA)), cx);
        let opcoes = WindowOptions { window_bounds: Some(WindowBounds::Windowed(bounds)), ..Default::default() };
        let cena2 = cena.clone();
        gpui_kit::open_window(opcoes, cx, move |window, cx| {
            let tela = cx.new(|cx| TelaGit::new(fonte.clone(), guarda.clone(), window, cx));
            let alvo = match cena2.as_str() {
                "lado-a-lado" | "unificado" => Some(Alvo::Diff { lado: Lado::Alteracoes, caminho: "src/editor-workspace.tsx".into() }),
                "commit-diff" => Some(Alvo::Commit { hash: "4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11".into(), caminho: "src/lib/utils.ts".into() }),
                "conflito" => Some(Alvo::Conflito { caminho: "src/dois.ts".into() }),
                _ => None,
            };
            tela.update(cx, |t, cx| {
                t.definir_selecionado(alvo.clone(), cx);
                if cena2 == "historico" || cena2 == "commit-diff" {
                    t.pedir_historico(None, cx);
                }
            });
            if cena2 == "commit-diff" {
                let tela2 = tela.clone();
                cx.spawn(async move |cx| {
                    cx.background_executor().timer(Duration::from_millis(400)).await;
                    tela2.update(cx, |t, cx| t.abrir_commit_do_historico("4b2e8d0a1c3f5e7b9d2a4c6e8f0b1d3a5c7e9f11", cx));
                })
                .detach();
            }
            let centro: Option<AnyView> = match alvo {
                Some(a @ Alvo::Conflito { .. }) => {
                    let c = cx.new(|cx| VisaoConflito::new(fonte.clone(), guarda.clone(), a.caminho().into(), None, window, cx));
                    Some(c.into())
                }
                Some(a) => {
                    let v = cx.new(|cx| VisaoDiff::new(fonte.clone(), guarda.clone(), a, window, cx));
                    if cena2 == "unificado" {
                        v.update(cx, |v, cx| v.alternar_modo(cx));
                    }
                    Some(v.into())
                }
                None => None,
            };
            cx.new(|_| Vitrine { tela, centro })
        })
        .expect("janela da vitrine");
    });
}
