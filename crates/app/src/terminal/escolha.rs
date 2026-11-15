//! O formulário de ferramenta, modelo e esforço numa janela (porte de `choice-form.tsx`): o mesmo para o orquestrador e
//! para os executores.
use super::ui::{self, Variante};
use crate::tema::{self, cor};
use gpui_kit::TestSupportExt;
use gpui_kit::{
    anchored, deferred, div, point, prelude::*, px, App, Context, EventEmitter, FocusHandle, Focusable, FontWeight,
    KeyDownEvent, MouseButton, SharedString, Window,
};
use orquestrador_core::config::{Choice, Options, ToolOption};

pub const ROTULO_PADRAO: &str = "Padrão do CLI";

pub fn nome_da_ferramenta(tool: &str) -> &str {
    match tool {
        "claude" => "Claude",
        "codex" => "Codex",
        outro => outro,
    }
}

pub fn rotulo_esforco(e: &str) -> &str {
    match e {
        "low" => "Baixo",
        "medium" => "Médio",
        "high" => "Alto",
        "xhigh" => "Muito alto",
        "max" => "Máximo",
        outro => outro,
    }
}

/// Só os esforços que o modelo aceita; modelo fora da lista (ou padrão) recebe todos os da ferramenta.
pub fn esforcos_para(tool: &ToolOption, modelo: &str) -> Vec<String> {
    let conhecidos = tool.models.iter().find(|m| m.name == modelo).map(|m| &m.efforts);
    tool.efforts.iter().filter(|e| conhecidos.is_none_or(|k| k.contains(e))).cloned().collect()
}

fn esforco_valido(opcoes: &Options, escolha: &Choice) -> String {
    let Some(tool) = opcoes.tools.iter().find(|t| t.name == escolha.tool) else { return String::new() };
    if esforcos_para(tool, &escolha.model).contains(&escolha.effort) {
        escolha.effort.clone()
    } else {
        String::new()
    }
}

/// A escolha inicial: ferramenta desconhecida vira a primeira; modelo que a ferramenta não lista vira o padrão do CLI.
pub fn escolha_inicial(opcoes: &Options, inicial: &Choice) -> Choice {
    let pega = if opcoes.tools.iter().any(|t| t.name == inicial.tool) {
        inicial.clone()
    } else {
        Choice::new(opcoes.tools.first().map_or("claude", |t| t.name.as_str()), "", "")
    };
    let modelos = opcoes.tools.iter().find(|t| t.name == pega.tool).map(|t| &t.models);
    let modelo = if modelos.is_some_and(|m| m.iter().any(|m| m.name == pega.model)) { pega.model.clone() } else { String::new() };
    let mut c = Choice { model: modelo, ..pega };
    c.effort = esforco_valido(opcoes, &c);
    c
}

/// O esforço é corrigido junto com a troca de modelo: o que o novo modelo não aceita vira padrão do CLI.
pub fn trocar_modelo(opcoes: &Options, escolha: &Choice, modelo: &str) -> Choice {
    let mut c = Choice { model: modelo.to_string(), ..escolha.clone() };
    c.effort = esforco_valido(opcoes, &c);
    c
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Campo {
    Modelo,
    Esforco,
}

pub enum EventoEscolha {
    Confirmar(Choice),
    Cancelar,
}

pub struct FormEscolha {
    pub titulo: SharedString,
    pub dicas: Vec<SharedString>,
    pub opcoes: Options,
    pub escolha: Choice,
    pub rotulo_confirmar: SharedString,
    /// Sem cancelar, Esc, clique fora e o X não fecham: só confirmar.
    pub cancelavel: bool,
    pub aberto: Option<Campo>,
    pub ocupado: bool,
    pub erro: Option<String>,
    pub foco: FocusHandle,
}

impl EventEmitter<EventoEscolha> for FormEscolha {}

impl Focusable for FormEscolha {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.foco.clone()
    }
}

impl FormEscolha {
    pub fn new(
        titulo: impl Into<SharedString>, dicas: Vec<SharedString>, opcoes: Options, inicial: &Choice,
        rotulo_confirmar: impl Into<SharedString>, cancelavel: bool, cx: &mut Context<Self>,
    ) -> Self {
        let escolha = escolha_inicial(&opcoes, inicial);
        FormEscolha {
            titulo: titulo.into(),
            dicas,
            opcoes,
            escolha,
            rotulo_confirmar: rotulo_confirmar.into(),
            cancelavel,
            aberto: None,
            ocupado: false,
            erro: None,
            foco: cx.focus_handle(),
        }
    }

    fn ferramenta(&self) -> Option<&ToolOption> {
        self.opcoes.tools.iter().find(|t| t.name == self.escolha.tool)
    }

    pub fn escolher_ferramenta(&mut self, nome: &str, cx: &mut Context<Self>) {
        self.escolha = Choice::new(nome, "", "");
        cx.notify();
    }

    pub fn escolher_modelo(&mut self, modelo: &str, cx: &mut Context<Self>) {
        self.escolha = trocar_modelo(&self.opcoes, &self.escolha, modelo);
        self.aberto = None;
        cx.notify();
    }

    pub fn escolher_esforco(&mut self, esforco: &str, cx: &mut Context<Self>) {
        self.escolha.effort = esforco.to_string();
        self.aberto = None;
        cx.notify();
    }

    pub fn confirmar(&mut self, cx: &mut Context<Self>) {
        if self.ocupado {
            return;
        }
        self.ocupado = true;
        self.erro = None;
        cx.emit(EventoEscolha::Confirmar(self.escolha.clone()));
        cx.notify();
    }

    /// A confirmação falhou: o erro aparece no formulário, que segue aberto.
    pub fn falhou(&mut self, erro: String, cx: &mut Context<Self>) {
        self.ocupado = false;
        self.erro = Some(erro);
        cx.notify();
    }

    fn cancelar(&mut self, cx: &mut Context<Self>) {
        if self.cancelavel {
            cx.emit(EventoEscolha::Cancelar);
        }
    }

    fn tecla(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match e.keystroke.key.as_str() {
            "escape" if self.aberto.is_some() => {
                self.aberto = None;
                cx.notify();
            }
            "escape" => self.cancelar(cx),
            "enter" if self.aberto.is_none() => self.confirmar(cx),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn rotulo(texto: &'static str) -> impl IntoElement {
        div().text_size(tema::TEXTO_BODY).font_weight(FontWeight::MEDIUM).child(texto)
    }

    fn segmentado(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut faixa = div()
            .id("escolha-ferramenta")
            .flex()
            .w_full()
            .p(px(3.))
            .gap(px(4.))
            .rounded(tema::RAIO_SM)
            .bg(cor::pitch_black())
            .border_1()
            .border_color(cor::hairline());
        for t in &self.opcoes.tools {
            let ativo = t.name == self.escolha.tool;
            let nome = t.name.clone();
            faixa = faixa.child(
                div()
                    .id(SharedString::from(format!("ferramenta-{}", t.name)))
                    .flex_1()
                    .flex()
                    .justify_center()
                    .items_center()
                    .h(px(32.))
                    .rounded(px(5.))
                    .text_size(tema::TEXTO_MENU)
                    .when(ativo, |d| d.bg(cor::raised_black()).font_weight(FontWeight::SEMIBOLD).text_color(cor::text_primary()))
                    .when(!ativo, |d| d.text_color(cor::text_secondary()).hover(|s| s.text_color(cor::text_primary())))
                    .on_click(cx.listener(move |this, _, _, cx| this.escolher_ferramenta(&nome, cx)))
                    .child(nome_da_ferramenta(&t.name).to_string())
                    .test_support(),
            );
        }
        faixa
    }

    fn select(&self, campo: Campo, cx: &mut Context<Self>) -> impl IntoElement {
        let (id, valor, mono) = match campo {
            Campo::Modelo => ("escolha-modelo", self.escolha.model.clone(), !self.escolha.model.is_empty()),
            Campo::Esforco => ("escolha-esforco", rotulo_esforco(&self.escolha.effort).to_string(), false),
        };
        let texto = if valor.is_empty() { ROTULO_PADRAO.to_string() } else { valor };
        let aberto = self.aberto == Some(campo);
        let itens: Vec<(String, String, bool)> = {
            let mut v = vec![(String::new(), ROTULO_PADRAO.to_string(), false)];
            if let Some(t) = self.ferramenta() {
                match campo {
                    Campo::Modelo => v.extend(t.models.iter().map(|m| (m.name.clone(), m.name.clone(), true))),
                    Campo::Esforco => {
                        v.extend(esforcos_para(t, &self.escolha.model).into_iter().map(|e| (e.clone(), rotulo_esforco(&e).to_string(), false)))
                    }
                }
            }
            v
        };
        let atual = match campo {
            Campo::Modelo => self.escolha.model.clone(),
            Campo::Esforco => self.escolha.effort.clone(),
        };
        div()
            .relative()
            .w_full()
            .child(
                div()
                    .id(id)
                    .flex()
                    .items_center()
                    .w_full()
                    .h(tema::CAMPO)
                    .px(px(12.))
                    .rounded(tema::RAIO_SM)
                    .bg(cor::fill_hover())
                    .hover(|s| s.bg(gpui_kit::rgba(0xffffff24)))
                    .text_size(tema::TEXTO_BODY)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.aberto = if this.aberto == Some(campo) { None } else { Some(campo) };
                        cx.notify();
                    }))
                    .child(div().flex_1().truncate().when(mono, |d| d.font_family(super::vista::FONTE)).child(texto))
                    .child(ui::icone(if aberto { "icons/chevron-up.svg" } else { "icons/chevron-down.svg" }, 14., cor::text_secondary())),
            )
            .when(aberto, |d| {
                let mut lista = div()
                    .id(SharedString::from(format!("{id}-lista")))
                    .occlude()
                    .flex()
                    .flex_col()
                    .w(px(428.))
                    .mt(px(6.))
                    .p(px(4.))
                    .rounded(tema::RAIO_MD)
                    .bg(cor::raised_black())
                    .border_1()
                    .border_color(gpui_kit::rgba(0xffffff26))
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.aberto = None;
                        cx.notify();
                    }));
                for (valor, rotulo, mono) in itens {
                    let marcado = valor == atual;
                    let v = valor.clone();
                    lista = lista.child(
                        div()
                            .id(SharedString::from(format!("{id}-{valor}")))
                            .flex()
                            .items_center()
                            .h(px(32.))
                            .px(px(12.))
                            .rounded(tema::RAIO_SM)
                            .text_size(tema::TEXTO_BODY)
                            .when(marcado, |d| d.bg(cor::fill_hover()))
                            .hover(|s| s.bg(cor::fill_muted()))
                            .on_click(cx.listener(move |this, _, _, cx| match campo {
                                Campo::Modelo => this.escolher_modelo(&v, cx),
                                Campo::Esforco => this.escolher_esforco(&v, cx),
                            }))
                            .child(div().flex_1().when(mono, |d| d.font_family(super::vista::FONTE)).child(rotulo))
                            .when(marcado, |d| d.child(ui::icone("icons/check.svg", 14., cor::text_primary()))),
                    );
                }
                d.child(deferred(anchored().snap_to_window().child(lista)).with_priority(2))
            })
    }
}

impl Render for FormEscolha {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let janela = window.viewport_size();
        let cancelavel = self.cancelavel;
        let caixa = div()
            .id("escolha")
            .track_focus(&self.foco)
            .key_context("Escolha")
            .on_key_down(cx.listener(Self::tecla))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .flex_col()
            .w(px(460.))
            .max_w(janela.width - px(32.))
            .px(px(16.))
            .pt(px(16.))
            .rounded(tema::RAIO_LG)
            .bg(gpui_kit::rgb(0x141414))
            .border_1()
            .border_color(gpui_kit::rgba(0xffffff1f))
            .text_color(cor::text_primary())
            .font_family(tema::FONTE)
            .child(
                div()
                    .flex()
                    .items_center()
                    .mb(px(12.))
                    .child(div().flex_1().text_size(tema::TEXTO_BODY).font_weight(FontWeight::SEMIBOLD).child(self.titulo.clone()))
                    .when(cancelavel, |d| {
                        d.child(
                            div()
                                .id("escolha-fechar")
                                .flex()
                                .items_center()
                                .justify_center()
                                .size(px(24.))
                                .rounded(tema::RAIO_SM)
                                .hover(|s| s.bg(cor::fill_hover()))
                                .on_click(cx.listener(|this, _, _, cx| this.cancelar(cx)))
                                .child(ui::icone("icons/x.svg", 16., cor::text_secondary())),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(div().flex().flex_col().gap(px(8.)).children(
                        self.dicas.iter().map(|d| div().text_size(tema::TEXTO_BODY).text_color(cor::text_secondary()).child(d.clone())),
                    ))
                    .child(div().flex().flex_col().gap(px(8.)).child(Self::rotulo("Ferramenta")).child(self.segmentado(cx)))
                    .child(div().flex().flex_col().gap(px(8.)).child(Self::rotulo("Modelo")).child(self.select(Campo::Modelo, cx)))
                    .child(div().flex().flex_col().gap(px(8.)).child(Self::rotulo("Esforço")).child(self.select(Campo::Esforco, cx)))
                    .when_some(self.erro.clone(), |d, e| {
                        d.child(
                            div()
                                .id("escolha-erro")
                                .px(px(12.))
                                .py(px(8.))
                                .rounded(tema::RAIO_SM)
                                .bg(gpui_kit::rgba(0xf4416c1f))
                                .border_1()
                                .border_color(gpui_kit::rgba(0xf4416c66))
                                .text_size(tema::TEXTO_BODY)
                                .child(e),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .py(px(16.))
                            .border_t_1()
                            .border_color(cor::hairline())
                            .when(cancelavel, |d| {
                                d.child(
                                    ui::botao("escolha-cancelar", "Cancelar", Variante::Padrao, false)
                                        .on_click(cx.listener(|this, _, _, cx| this.cancelar(cx))),
                                )
                            })
                            .child(
                                ui::botao("escolha-confirmar", self.rotulo_confirmar.clone(), Variante::Primario, self.ocupado)
                                    .on_click(cx.listener(|this, _, _, cx| this.confirmar(cx)))
                                    .test_support(),
                            ),
                    ),
            );
        // Janela modal sobre tudo: a máscara escurece e, com cancelar, o clique fora fecha.
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id("escolha-mascara")
                    .occlude()
                    .w(janela.width)
                    .h(janela.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::rgba(0x000000a6))
                    .on_mouse_down(MouseButton::Left, cx.listener(|this, _, _, cx| this.cancelar(cx)))
                    .child(caixa),
            ),
        )
        .with_priority(1)
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use orquestrador_core::config::{Config, ModelOption};

    fn opcoes() -> Options {
        let m = |n: &str, e: &[&str]| ModelOption { name: n.into(), efforts: e.iter().map(|s| s.to_string()).collect() };
        Options {
            tools: vec![
                ToolOption {
                    name: "claude".into(),
                    efforts: vec!["low".into(), "medium".into(), "high".into(), "max".into()],
                    models: vec![m("opus", &["low", "high", "max"]), m("haiku", &["low"])],
                },
                ToolOption { name: "codex".into(), efforts: vec!["low".into(), "high".into()], models: vec![] },
            ],
            last: Config::default(),
        }
    }

    #[test]
    fn inicial_corrige_ferramenta_modelo_e_esforco() {
        let o = opcoes();
        assert_eq!(escolha_inicial(&o, &Choice::new("x", "y", "z")), Choice::new("claude", "", ""));
        assert_eq!(escolha_inicial(&o, &Choice::new("claude", "sumiu", "high")), Choice::new("claude", "", "high"));
        assert_eq!(escolha_inicial(&o, &Choice::new("claude", "haiku", "high")), Choice::new("claude", "haiku", ""));
    }

    #[test]
    fn trocar_modelo_zera_esforco_que_ele_nao_aceita_e_nao_devolve_o_antigo() {
        let o = opcoes();
        let c = Choice::new("claude", "opus", "high");
        let c = trocar_modelo(&o, &c, "haiku");
        assert_eq!(c.effort, "");
        assert_eq!(trocar_modelo(&o, &c, "opus").effort, "");
        assert_eq!(esforcos_para(&o.tools[0], ""), vec!["low", "medium", "high", "max"]);
        assert_eq!(esforcos_para(&o.tools[0], "opus"), vec!["low", "high", "max"]);
    }
}
