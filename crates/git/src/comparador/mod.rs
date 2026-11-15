//! Comparador próprio do editor central, no lugar do `Diff` do GPUI Kit: reproduz o `FileDiff` do `@pierre/diffs` 1.2.12
//! como `src/editor/diff-view.tsx` o configura (Consolas 14 px em linhas de 19 px, quebra de linha, calha com o número e o
//! sinal −/+, vão de 40 px no lado a lado, células vazias hachuradas, realce por palavra `word-alt`). A geometria é
//! calculada, não medida: cada fileira tem a altura das suas linhas quebradas, e só as fileiras visíveis são moldadas.
pub mod modelo;
pub mod palavras;

use crate::realce;
use gpui_kit::{
    fill, font, point, px, size, App, Bounds, ClipboardItem, ContentMask, CursorStyle, DispatchPhase, Element, ElementId, EntityId,
    Font, GlobalElementId, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId, IntoElement,
    LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Point, ScrollEvent,
    SharedString, Style, TextAlign, TextRun, Window,
};
use modelo::{Celula, Modelo, Tipo};
use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

/// Medidas do `unsafeCSS` do app e do CSS do pacote.
pub const LINHA: f32 = 19.;
pub const TAMANHO: f32 = 14.;
pub const FONTE: &str = "Consolas";
/// Vão entre as colunas do lado a lado.
pub const VAO: f32 = 40.;
/// `padding-block` da `pre` no lado a lado com quebra.
pub const MARGEM_LADO: f32 = 8.;
/// Divisor: cada lado com pelo menos 15%.
pub const MIN_PCT: f32 = 15.;
pub const MAX_PCT: f32 = 85.;
/// Pixels por linha de roda do mouse (o Chromium rola 100 px por entalhe de 3 linhas).
const PIXELS_POR_LINHA: f32 = 100. / 3.;

pub fn limitar_pct(p: f32) -> f32 {
    p.clamp(MIN_PCT, MAX_PCT)
}

/// Cores do `unsafeCSS` do app e as que o pacote calcula para o tema escuro (conferidas nas capturas do React).
pub mod cores {
    use gpui_kit::{rgb, rgba, Hsla};
    pub fn fundo() -> Hsla {
        rgb(0x1f1f1f).into()
    }
    /// `#ff000033` e `#9bb95533` sobre o fundo.
    pub fn linha_removida() -> Hsla {
        rgb(0x4c1919).into()
    }
    pub fn linha_incluida() -> Hsla {
        rgb(0x383e2a).into()
    }
    /// Segunda camada do trecho alterado: `#ff000033` e `#9ccc2c33`.
    pub fn enfase_removida() -> Hsla {
        rgba(0xff000033).into()
    }
    pub fn enfase_incluida() -> Hsla {
        rgba(0x9ccc2c33).into()
    }
    /// Calha das linhas mudadas: o fundo misturado com a cor base e o número nela.
    pub fn calha_removida() -> Hsla {
        rgb(0x322422).into()
    }
    pub fn calha_incluida() -> Hsla {
        rgb(0x232c24).into()
    }
    pub fn numero_removido() -> Hsla {
        rgb(0xff2e3f).into()
    }
    pub fn numero_incluido() -> Hsla {
        rgb(0x0dbe4e).into()
    }
    pub fn numero() -> Hsla {
        rgb(0x6e7681).into()
    }
    pub fn texto() -> Hsla {
        rgb(0xd4d4d4).into()
    }
    pub fn borda() -> Hsla {
        rgb(0x3c3c3c).into()
    }
    pub fn hachura() -> Hsla {
        rgba(0xcccccc33).into()
    }
    pub fn atual() -> Hsla {
        rgb(0x007fd4).into()
    }
    pub fn divisor() -> Hsla {
        rgb(0x6e6e6e).into()
    }
    pub fn selecao() -> Hsla {
        rgb(0x264f78).into()
    }
}

/// Faixas coloridas de cada linha do patch, já no texto exibido (com as tabulações expandidas).
pub type Realce = Vec<Vec<(Range<usize>, HighlightStyle)>>;

/// Texto exibido de uma linha: tabulação com `tab-size: 2`, e o mapa de bytes do original para o exibido.
#[derive(Clone, Debug, Default)]
struct Exibido {
    texto: String,
    /// `None` quando igual ao original.
    mapa: Option<Vec<usize>>,
}

impl Exibido {
    fn de(original: &str) -> Exibido {
        if !original.contains('\t') {
            return Exibido { texto: original.to_string(), mapa: None };
        }
        let mut texto = String::with_capacity(original.len() + 8);
        let mut mapa = Vec::with_capacity(original.len() + 1);
        let mut coluna = 0;
        for c in original.chars() {
            for _ in 0..c.len_utf8() {
                mapa.push(texto.len());
            }
            if c == '\t' {
                let n = 2 - coluna % 2;
                texto.extend(std::iter::repeat_n(' ', n));
                coluna += n;
            } else {
                texto.push(c);
                coluna += 1;
            }
        }
        mapa.push(texto.len());
        Exibido { texto, mapa: Some(mapa) }
    }

    fn para_exibido(&self, i: usize) -> usize {
        self.mapa.as_ref().map_or(i, |m| m[i.min(m.len() - 1)])
    }

    /// Byte do original de uma posição exibida (dentro de uma tabulação, o início dela).
    fn para_original(&self, d: usize) -> usize {
        match &self.mapa {
            None => d,
            Some(m) => m.partition_point(|&x| x <= d).saturating_sub(1),
        }
    }
}

/// Uma unidade de navegação: as fileiras (inclusive) que ela ocupa em cada modo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FaixaUnidade {
    pub lado: Option<(usize, usize)>,
    pub unificado: Option<(usize, usize)>,
}

/// Colunas da grade (x em relação ao palco).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Colunas {
    pub num_a: (f32, f32),
    pub txt_a: (f32, f32),
    pub num_b: (f32, f32),
    pub txt_b: (f32, f32),
    /// Centro do vão (o `--compare-center-x`).
    pub centro: f32,
}

/// Geometria de um modo numa largura: colunas e o topo de cada fileira no palco.
#[derive(Clone, Debug, Default)]
pub struct Geometria {
    pub lado: bool,
    pub largura: f32,
    pub pct: f32,
    pub colunas: Colunas,
    /// Topo de cada fileira e, no fim, a base da última.
    pub topos: Vec<f32>,
    /// Início das linhas quebradas de cada célula (bytes do texto exibido), sem o 0.
    quebras: Vec<[Vec<usize>; 2]>,
    /// Altura do conteúdo (com as margens).
    pub altura: f32,
}

impl Geometria {
    /// Fileira no y do palco.
    pub fn fileira_em(&self, y: f32) -> Option<usize> {
        let n = self.topos.len().saturating_sub(1);
        if n == 0 || y < self.topos[0] || y >= self.topos[n] {
            return None;
        }
        Some(self.topos.partition_point(|&t| t <= y).saturating_sub(1).min(n - 1))
    }
}

/// Medidas da fonte que a grade usa.
#[derive(Clone, Copy, Debug)]
struct Medidas {
    ch: f32,
    sinal: f32,
    ascent: f32,
    descent: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Ponto {
    pub fileira: usize,
    pub byte: usize,
}

/// Seleção de texto num lado (`b` é a coluna modificada; no unificado, sempre `false`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selecao {
    pub b: bool,
    pub ancora: Ponto,
    pub foco: Ponto,
}

impl Selecao {
    pub fn ordenada(&self) -> (Ponto, Ponto) {
        if self.ancora <= self.foco { (self.ancora, self.foco) } else { (self.foco, self.ancora) }
    }
}

enum Arraste {
    Texto { inicio: Point<Pixels>, moveu: bool },
    Divisor { x: f32, pct: f32, largura: f32 },
}

/// Estado do comparador, guardado pela vista entre os quadros.
pub struct Comparador {
    pub modelo: Rc<Modelo>,
    exibidos: Vec<Exibido>,
    /// Unidade de cada linha do patch.
    unidade_de: Vec<Option<usize>>,
    pub unidades: Vec<FaixaUnidade>,
    pub caminho: String,
    pub lado: bool,
    pub pct: f32,
    pub rolagem: f32,
    pub atual: Option<usize>,
    /// Rolar até a unidade atual assim que houver geometria.
    pub pular: bool,
    realce_antigo: Option<Rc<Realce>>,
    realce_novo: Option<Rc<Realce>>,
    pub geo: Option<Rc<Geometria>>,
    larguras: Option<Vec<f32>>,
    medidas: Option<Medidas>,
    pub caixa: Bounds<Pixels>,
    pub selecao: Option<Selecao>,
    arraste: Option<Arraste>,
}

impl Comparador {
    /// `unidades` é a unidade de cada linha `+`/`-`/` ` do patch, na ordem (a de `comparar::comparar`).
    pub fn new(patch: &str, caminho: &str, unidades: &[Option<usize>], total: usize) -> Comparador {
        let modelo = modelo::montar(patch);
        let exibidos = modelo.linhas.iter().map(|l| Exibido::de(&l.texto)).collect();
        let unidade_de: Vec<Option<usize>> = (0..modelo.linhas.len()).map(|i| unidades.get(i).copied().flatten()).collect();
        let mut faixas = vec![FaixaUnidade { lado: None, unificado: None }; total];
        for (i, u) in unidade_de.iter().enumerate() {
            let Some(u) = *u else { continue };
            let Some(f) = faixas.get_mut(u) else { continue };
            let (l, un) = (modelo.fileira_lado[i], modelo.fileira_unificado[i]);
            f.lado = Some(f.lado.map_or((l, l), |(a, b)| (a.min(l), b.max(l))));
            f.unificado = Some(f.unificado.map_or((un, un), |(a, b)| (a.min(un), b.max(un))));
        }
        let lado = !modelo.arquivo_novo;
        Comparador {
            modelo: Rc::new(modelo),
            exibidos,
            unidade_de,
            unidades: faixas,
            caminho: caminho.to_string(),
            lado,
            pct: 50.,
            rolagem: 0.,
            atual: None,
            pular: true,
            realce_antigo: None,
            realce_novo: None,
            geo: None,
            larguras: None,
            medidas: None,
            caixa: Bounds::default(),
            selecao: None,
            arraste: None,
        }
    }

    /// Os dois textos completos (original e modificado) para o realce em segundo plano.
    pub fn textos(&self) -> (String, String) {
        let (mut a, mut b) = (String::new(), String::new());
        for l in &self.modelo.linhas {
            if l.tipo != Tipo::Incluida {
                a.push_str(&l.texto);
                a.push('\n');
            }
            if l.tipo != Tipo::Removida {
                b.push_str(&l.texto);
                b.push('\n');
            }
        }
        (a, b)
    }

    /// Realce de sintaxe dos dois lados (`realce::realcar` sobre `textos()`), por linha.
    pub fn realcar(caminho: &str, modelo: &Modelo, antigo: &str, novo: &str) -> (Realce, Realce) {
        let por_linha = |texto: &str, lado: Tipo| -> Realce {
            let faixas = realce::realcar(caminho, texto);
            let mut v: Realce = vec![Vec::new(); modelo.linhas.len()];
            let mut k = 0;
            let mut base = 0;
            for (i, l) in modelo.linhas.iter().enumerate() {
                if l.tipo == lado {
                    continue;
                }
                let fim = base + l.texto.len();
                while k < faixas.len() && faixas[k].0.end <= base {
                    k += 1;
                }
                let mut j = k;
                while j < faixas.len() && faixas[j].0.start < fim {
                    let (r, s) = &faixas[j];
                    v[i].push((r.start.max(base) - base..r.end.min(fim) - base, *s));
                    j += 1;
                }
                base = fim + 1;
            }
            v
        };
        (por_linha(antigo, Tipo::Incluida), por_linha(novo, Tipo::Removida))
    }

    pub fn definir_realce(&mut self, antigo: Realce, novo: Realce) {
        let mapear = |r: Realce| -> Rc<Realce> {
            Rc::new(
                r.into_iter()
                    .zip(&self.exibidos)
                    .map(|(v, e)| v.into_iter().map(|(f, s)| (e.para_exibido(f.start)..e.para_exibido(f.end), s)).collect())
                    .collect(),
            )
        };
        self.realce_antigo = Some(mapear(antigo));
        self.realce_novo = Some(mapear(novo));
    }

    pub fn definir_lado(&mut self, lado: bool) {
        let lado = lado && !self.modelo.arquivo_novo;
        if self.lado != lado {
            self.lado = lado;
            self.selecao = None;
            self.pular = true;
        }
    }

    pub fn escolher(&mut self, unidade: Option<usize>) {
        self.atual = unidade;
        self.pular = true;
    }

    /// Faixa (topo, base) da unidade no palco, no modo atual.
    pub fn faixa(&self, u: usize) -> Option<(f32, f32)> {
        let g = self.geo.as_ref().filter(|g| g.lado == self.lado)?;
        let f = self.unidades.get(u)?;
        let (a, b) = if self.lado { f.lado? } else { f.unificado? };
        Some((g.topos[a], g.topos[b + 1]))
    }

    /// Altura do palco: o conteúdo, ou a área visível se ela for maior (`min-height: 100%`).
    pub fn altura_palco(&self) -> f32 {
        let vis = f32::from(self.caixa.size.height);
        self.geo.as_ref().map_or(vis, |g| g.altura.max(vis))
    }

    pub fn max_rolagem(&self) -> f32 {
        (self.altura_palco() - f32::from(self.caixa.size.height)).max(0.)
    }

    pub fn rolar_para(&mut self, y: f32) {
        self.rolagem = y.clamp(0., self.max_rolagem());
    }

    /// Texto selecionado, com as quebras de linha (o original, com as tabulações).
    pub fn texto_selecionado(&self) -> Option<String> {
        let s = self.selecao?;
        let (a, b) = s.ordenada();
        if a == b {
            return None;
        }
        let fileiras = self.modelo.fileiras(self.lado);
        let mut partes = Vec::new();
        for f in a.fileira..=b.fileira {
            let celula = if s.b { fileiras[f].b } else { fileiras[f].a };
            let Celula::Linha(i) = celula else { continue };
            let e = &self.exibidos[i];
            let ini = if f == a.fileira { a.byte } else { 0 };
            let fim = if f == b.fileira { b.byte } else { e.texto.len() };
            let original = &self.modelo.linhas[i].texto;
            let (x, y) = (e.para_original(ini).min(original.len()), e.para_original(fim).min(original.len()));
            partes.push(original[x..y.max(x)].to_string());
        }
        Some(partes.join("\n"))
    }

    pub fn copiar(&self, cx: &mut App) {
        if let Some(t) = self.texto_selecionado() {
            cx.write_to_clipboard(ClipboardItem::new_string(t));
        }
    }

    fn medidas(&mut self, window: &mut Window) -> Medidas {
        if let Some(m) = self.medidas {
            return m;
        }
        let ts = window.text_system().clone();
        let f = fonte();
        let id = ts.resolve_font(&f);
        let tam = px(TAMANHO);
        let ch = ts.ch_width(id, tam).map(f32::from).unwrap_or(TAMANHO * 0.55);
        // O sinal (U+2212 pode vir de outra fonte) é medido moldado, como o navegador o desenha.
        let largura = |s: &str| {
            f32::from(ts.shape_line(SharedString::from(s.to_string()), tam, &[corrida(s.len(), &f, cores::numero())], None).width)
        };
        let m = Medidas {
            ch,
            sinal: largura("−").max(largura("+")),
            ascent: f32::from(ts.ascent(id, tam)),
            descent: f32::from(ts.descent(id, tam)),
        };
        self.medidas = Some(m);
        m
    }

    /// Largura de cada linha (sem quebra), uma vez por texto: com ela, só as linhas maiores que a coluna passam pelo
    /// quebrador.
    fn larguras(&mut self, window: &mut Window) -> Vec<f32> {
        if let Some(v) = self.larguras.take() {
            return v;
        }
        let ts = window.text_system().clone();
        let id = ts.resolve_font(&fonte());
        let mut cache: std::collections::HashMap<char, f32> = std::collections::HashMap::new();
        self.exibidos
            .iter()
            .map(|e| {
                e.texto
                    .chars()
                    .map(|c| *cache.entry(c).or_insert_with(|| ts.advance(id, px(TAMANHO), c).map_or(TAMANHO, |s| f32::from(s.width))))
                    .sum()
            })
            .collect()
    }

    /// Calha de uma coluna: `padding-left: 2ch`, número com no mínimo 3ch, o sinal −/+ com 4 px de margem nas linhas
    /// mudadas, `padding-right` (4 px no lado a lado, 1ch no unificado) e a borda de 2 px.
    fn largura_calha(&self, m: Medidas, b: bool) -> f32 {
        let fileiras = self.modelo.fileiras(self.lado);
        let pad_d = if self.lado { 4. } else { m.ch };
        let mut maior: f32 = 0.;
        for f in fileiras {
            let c = if b { f.b } else { f.a };
            let w = match c {
                Celula::Linha(i) => {
                    let n = self.modelo.numero(c, b, self.lado).unwrap_or(0);
                    let digitos = n.max(1).ilog10() as f32 + 1.;
                    let mudada = self.modelo.linhas[i].tipo != Tipo::Contexto;
                    2. * m.ch + (digitos * m.ch).max(3. * m.ch) + if mudada { 4. + m.sinal } else { 0. } + pad_d
                }
                // `[data-gutter-buffer]`: 2ch + 3ch + 1ch.
                Celula::SemQuebra(_) | Celula::Vazia => 6. * m.ch,
                Celula::Nada => 0.,
            };
            maior = maior.max(w);
        }
        maior + 2.
    }

    fn geometria(&mut self, window: &mut Window) -> Rc<Geometria> {
        let largura = f32::from(self.caixa.size.width);
        if let Some(g) = &self.geo {
            if g.lado == self.lado && g.largura == largura && (!self.lado || g.pct == self.pct) {
                return g.clone();
            }
        }
        let m = self.medidas(window);
        let larguras = self.larguras(window);
        let ts = window.text_system().clone();
        let mut quebrador = ts.line_wrapper(fonte(), px(TAMANHO));
        let mut quebrar = |texto: &str, util: f32| -> Vec<usize> {
            quebrador
                .wrap_line(&[gpui_kit::LineFragment::text(texto)], px(util), gpui_kit::IndentAdjustment::NoIndent)
                .map(|b| b.ix)
                .collect()
        };
        let g = self.calcular(largura, m, &larguras, &mut quebrar);
        self.larguras = Some(larguras);
        g
    }

    /// A geometria numa largura do palco. `larguras` é a largura sem quebra de cada linha; `quebrar(texto, util)` dá os
    /// inícios das linhas quebradas.
    fn calcular(&mut self, largura: f32, m: Medidas, larguras: &[f32], quebrar: &mut dyn FnMut(&str, f32) -> Vec<usize>) -> Rc<Geometria> {
        self.medidas = Some(m);
        let mut colunas = Colunas::default();
        if self.lado {
            let (na, nb) = (self.largura_calha(m, false), self.largura_calha(m, true));
            let resto = (largura - na - nb - VAO).max(0.);
            let la = resto * self.pct / 100.;
            colunas.num_a = (0., na);
            colunas.txt_a = (na, na + la);
            colunas.num_b = (na + la + VAO, na + la + VAO + nb);
            colunas.txt_b = (colunas.num_b.1, largura.max(colunas.num_b.1));
            colunas.centro = colunas.txt_a.1 + VAO / 2.;
        } else {
            let n = self.largura_calha(m, false);
            colunas.num_a = (0., n);
            colunas.txt_a = (n, largura.max(n));
            colunas.centro = largura / 2.;
        }
        // Largura útil do texto: `padding-inline: 1ch` e, à esquerda do lado a lado, a borda de 1 px.
        let util_a = (colunas.txt_a.1 - colunas.txt_a.0 - 2. * m.ch - if self.lado { 1. } else { 0. }).max(m.ch);
        let util_b = (colunas.txt_b.1 - colunas.txt_b.0 - 2. * m.ch).max(m.ch);
        let fileiras = self.modelo.fileiras(self.lado);
        let mut quebras = Vec::with_capacity(fileiras.len());
        let mut topos = Vec::with_capacity(fileiras.len() + 1);
        let margem = if self.lado { MARGEM_LADO } else { 0. };
        let mut y = margem;
        for f in fileiras {
            let mut q: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
            let mut linhas = 1;
            for (k, c) in [f.a, f.b].into_iter().enumerate() {
                let util = if k == 0 { util_a } else { util_b };
                let (texto, w) = match c {
                    Celula::Linha(i) => (self.exibidos[i].texto.as_str(), larguras[i]),
                    Celula::SemQuebra(_) => (SEM_QUEBRA, SEM_QUEBRA.len() as f32 * m.ch),
                    _ => continue,
                };
                if w > util + 0.01 {
                    q[k] = quebrar(texto, util);
                }
                linhas = linhas.max(q[k].len() + 1);
            }
            topos.push(y);
            y += linhas as f32 * LINHA;
            quebras.push(q);
        }
        topos.push(y);
        let anterior = self.geo.take();
        let g = Rc::new(Geometria { lado: self.lado, largura, pct: self.pct, colunas, topos, quebras, altura: y + margem });
        // Âncora de rolagem: a fileira do topo continua no topo quando as quebras mudam a altura das fileiras.
        if let Some(a) = anterior.filter(|a| a.lado == g.lado && a.topos.len() == g.topos.len()) {
            if let Some(r) = a.fileira_em(self.rolagem) {
                let dentro = self.rolagem - a.topos[r];
                self.rolagem = g.topos[r] + dentro.min(g.topos[r + 1] - g.topos[r]);
            }
        }
        self.geo = Some(g.clone());
        self.rolar_para(self.rolagem);
        g
    }

    /// Aplica o pedido de rolagem da navegação: a primeira linha da unidade atual a um terço da altura visível.
    pub fn aplicar_pulo(&mut self) {
        if !self.pular || self.geo.as_ref().is_none_or(|g| g.lado != self.lado) {
            return;
        }
        if let Some((topo, _)) = self.atual.and_then(|u| self.faixa(u)) {
            let alvo = (topo - f32::from(self.caixa.size.height) / 3.).max(0.);
            self.rolar_para(alvo);
        }
        self.pular = false;
    }

    /// Fileira e byte sob um ponto do palco, numa coluna.
    fn ponto_em(&self, g: &Geometria, b: bool, x: f32, y: f32, window: &mut Window) -> Ponto {
        let n = g.topos.len() - 1;
        if n == 0 {
            return Ponto { fileira: 0, byte: 0 };
        }
        if y < g.topos[0] {
            return Ponto { fileira: 0, byte: 0 };
        }
        let Some(f) = g.fileira_em(y) else {
            let fim = self.texto_celula(n - 1, b).map_or(0, |t| t.len());
            return Ponto { fileira: n - 1, byte: fim };
        };
        let Some(texto) = self.texto_celula(f, b) else { return Ponto { fileira: f, byte: 0 } };
        let q = &g.quebras[f][b as usize];
        let seg = (((y - g.topos[f]) / LINHA) as usize).min(q.len());
        let ini = if seg == 0 { 0 } else { q[seg - 1] };
        let fim = q.get(seg).copied().unwrap_or(texto.len());
        let ch = self.medidas.map_or(7.7, |m| m.ch);
        let col = if b { g.colunas.txt_b.0 } else { g.colunas.txt_a.0 };
        let linha = window.text_system().shape_line(
            SharedString::from(texto[ini..fim].to_string()),
            px(TAMANHO),
            &[corrida(fim - ini, &fonte(), cores::texto())],
            None,
        );
        let i = linha.closest_index_for_x(px(x - col - ch));
        Ponto { fileira: f, byte: ini + i }
    }

    fn texto_celula(&self, f: usize, b: bool) -> Option<&str> {
        let fileira = self.modelo.fileiras(self.lado).get(f)?;
        match if b { fileira.b } else { fileira.a } {
            Celula::Linha(i) => Some(&self.exibidos[i].texto),
            _ => None,
        }
    }

    /// Unidade de uma linha de mudança na fileira e coluna (o `onLineClick`).
    pub fn unidade_em(&self, f: usize, b: bool) -> Option<usize> {
        let fileira = self.modelo.fileiras(self.lado).get(f)?;
        match if b { fileira.b } else { fileira.a } {
            Celula::Linha(i) if self.modelo.linhas[i].tipo != Tipo::Contexto => self.unidade_de[i],
            _ => None,
        }
    }

    /// Começa a arrastar o divisor (o `pointerdown` do `.compare-sash`).
    pub fn iniciar_divisor(&mut self, x: f32) {
        let largura = self.geo.as_ref().map_or(1., |g| (g.colunas.txt_a.1 - g.colunas.txt_a.0) + (g.colunas.txt_b.1 - g.colunas.txt_b.0));
        self.arraste = Some(Arraste::Divisor { x, pct: self.pct, largura: largura.max(1.) });
    }

    pub fn arrastando_divisor(&self) -> bool {
        matches!(self.arraste, Some(Arraste::Divisor { .. }))
    }
}

pub const SEM_QUEBRA: &str = "No newline at end of file";

fn fonte() -> Font {
    font(FONTE)
}

fn corrida(len: usize, f: &Font, cor: Hsla) -> TextRun {
    TextRun { len, font: f.clone(), color: cor, background_color: None, underline: None, strikethrough: None }
}

/// Corridas de um pedaço `[ini, fim)` do texto com as faixas de realce.
fn corridas(ini: usize, fim: usize, faixas: Option<&Vec<(Range<usize>, HighlightStyle)>>) -> Vec<TextRun> {
    let base = fonte();
    let mut v = Vec::new();
    let mut pos = ini;
    let por = |a: usize, b: usize, s: Option<&HighlightStyle>, v: &mut Vec<TextRun>| {
        if b <= a {
            return;
        }
        let mut f = base.clone();
        if let Some(s) = s {
            if let Some(w) = s.font_weight {
                f.weight = w;
            }
            if let Some(st) = s.font_style {
                f.style = st;
            }
        }
        let cor = s.and_then(|s| s.color).unwrap_or_else(cores::texto);
        v.push(TextRun { len: b - a, font: f, color: cor, background_color: None, underline: None, strikethrough: None });
    };
    if let Some(faixas) = faixas {
        let k = faixas.partition_point(|(r, _)| r.end <= ini);
        for (r, s) in &faixas[k..] {
            if r.start >= fim {
                break;
            }
            let (a, b) = (r.start.max(pos), r.end.min(fim));
            if a > pos {
                por(pos, a, None, &mut v);
            }
            if b > a {
                por(a, b, Some(s), &mut v);
                pos = b;
            }
        }
    }
    por(pos, fim, None, &mut v);
    v
}

/// Elemento que desenha o comparador. Os eventos mexem no estado e avisam a vista (`vista`).
pub struct Corpo {
    estado: Rc<RefCell<Comparador>>,
    vista: EntityId,
    ao_escolher: Rc<dyn Fn(usize, &mut Window, &mut App)>,
    ao_focar: Rc<dyn Fn(&mut Window, &mut App)>,
}

impl Corpo {
    pub fn new(
        estado: Rc<RefCell<Comparador>>,
        vista: EntityId,
        ao_escolher: impl Fn(usize, &mut Window, &mut App) + 'static,
        ao_focar: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Corpo {
        Corpo { estado, vista, ao_escolher: Rc::new(ao_escolher), ao_focar: Rc::new(ao_focar) }
    }
}

impl IntoElement for Corpo {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

/// O que o `prepaint` deixa para o `paint`.
pub struct Preparo {
    hitbox: Hitbox,
    geo: Rc<Geometria>,
    visiveis: Range<usize>,
}

impl Element for Corpo {
    type RequestLayoutState = ();
    type PrepaintState = Preparo;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(&mut self, _: Option<&GlobalElementId>, _: Option<&InspectorElementId>, window: &mut Window, cx: &mut App) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = gpui_kit::relative(1.).into();
        style.size.height = gpui_kit::relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Preparo {
        let mut e = self.estado.borrow_mut();
        let mudou = e.caixa.size != bounds.size;
        e.caixa = bounds;
        let rolagem = e.rolagem;
        let g = e.geometria(window);
        let _ = &g;
        e.aplicar_pulo();
        if mudou || e.rolagem != rolagem {
            // A vista posiciona a régua e as ações do vão pela geometria deste quadro.
            cx.notify(self.vista);
        }
        let topo = e.rolagem;
        let base = topo + f32::from(bounds.size.height);
        let ini = g.topos.partition_point(|&t| t <= topo).saturating_sub(1);
        let fim = g.topos.partition_point(|&t| t < base).min(g.topos.len().saturating_sub(1));
        let hitbox = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        Preparo { hitbox, geo: g, visiveis: ini..fim.max(ini) }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        p: &mut Preparo,
        window: &mut Window,
        cx: &mut App,
    ) {
        let g = p.geo.clone();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            window.paint_quad(fill(bounds, cores::fundo()));
            let e = self.estado.borrow();
            pintar(&e, &g, p.visiveis.clone(), bounds, window, cx);
        });
        // Cursor: de redimensionar arrastando o divisor, de mão sobre as linhas (`[data-interactive-lines]`).
        {
            let e = self.estado.borrow();
            if e.arrastando_divisor() {
                window.set_window_cursor_style(CursorStyle::ResizeLeftRight);
            } else if let Some(pos) = Some(window.mouse_position()).filter(|_| p.hitbox.is_hovered(window)) {
                let (x, y) = (f32::from(pos.x - bounds.origin.x), f32::from(pos.y - bounds.origin.y) + e.rolagem);
                let c = g.colunas;
                let em_texto = (x >= c.txt_a.0 && x < c.txt_a.1) || (e.lado && x >= c.txt_b.0 && x < c.txt_b.1);
                let b = e.lado && x >= c.txt_b.0;
                if em_texto && g.fileira_em(y).is_some_and(|f| e.texto_celula(f, b).is_some()) {
                    window.set_cursor_style(CursorStyle::PointingHand, &p.hitbox);
                }
            }
        }
        let vista = self.vista;
        let estado = self.estado.clone();
        let hitbox = p.hitbox.clone();
        window.on_mouse_event(move |ev: &ScrollEvent, fase, window, cx| {
            if fase != DispatchPhase::Bubble || !hitbox.should_handle_scroll(window) {
                return;
            }
            let d = ev.delta.pixel_delta(px(PIXELS_POR_LINHA));
            let mut e = estado.borrow_mut();
            let antes = e.rolagem;
            e.rolar_para(antes - f32::from(d.y));
            if e.rolagem != antes {
                cx.stop_propagation();
                cx.notify(vista);
                window.refresh();
            }
        });
        let estado = self.estado.clone();
        let hitbox = p.hitbox.clone();
        let focar = self.ao_focar.clone();
        let g2 = g.clone();
        window.on_mouse_event(move |ev: &MouseDownEvent, fase, window, cx| {
            if fase != DispatchPhase::Bubble || ev.button != MouseButton::Left || !hitbox.is_hovered(window) {
                return;
            }
            focar(window, cx);
            let mut e = estado.borrow_mut();
            let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y) + e.rolagem);
            let b = e.lado && x >= g2.colunas.num_b.0;
            let ponto = e.ponto_em(&g2, b, x, y, window);
            let selecao = match ev.click_count {
                2 => {
                    let t = e.texto_celula(ponto.fileira, b).unwrap_or("");
                    let r = palavra_em(t, ponto.byte);
                    Selecao { b, ancora: Ponto { fileira: ponto.fileira, byte: r.start }, foco: Ponto { fileira: ponto.fileira, byte: r.end } }
                }
                n if n >= 3 => {
                    let fim = e.texto_celula(ponto.fileira, b).map_or(0, str::len);
                    Selecao { b, ancora: Ponto { fileira: ponto.fileira, byte: 0 }, foco: Ponto { fileira: ponto.fileira, byte: fim } }
                }
                _ if ev.modifiers.shift && e.selecao.is_some_and(|s| s.b == b) => Selecao { foco: ponto, ..e.selecao.unwrap() },
                _ => Selecao { b, ancora: ponto, foco: ponto },
            };
            e.selecao = Some(selecao);
            e.arraste = Some(Arraste::Texto { inicio: ev.position, moveu: ev.click_count > 1 });
            window.refresh();
        });
        let estado = self.estado.clone();
        let g3 = g.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, fase, window, cx| {
            if fase != DispatchPhase::Bubble {
                return;
            }
            let mut e = estado.borrow_mut();
            match e.arraste {
                Some(Arraste::Divisor { x, pct, largura }) => {
                    if !ev.dragging() {
                        e.arraste = None;
                        return;
                    }
                    e.pct = limitar_pct(pct + (f32::from(ev.position.x) - x) / largura * 100.);
                    cx.notify(vista);
                    window.refresh();
                }
                Some(Arraste::Texto { inicio, moveu }) => {
                    if !ev.dragging() {
                        return;
                    }
                    let moveu = moveu || (ev.position - inicio).magnitude() > 3.0;
                    e.arraste = Some(Arraste::Texto { inicio, moveu });
                    // Fora da área, rola na direção do ponteiro, como o navegador ao selecionar.
                    let y_vis = f32::from(ev.position.y - bounds.origin.y);
                    let alto = f32::from(bounds.size.height);
                    if y_vis < 0. {
                        let r = e.rolagem + y_vis;
                        e.rolar_para(r);
                    } else if y_vis > alto {
                        let r = e.rolagem + (y_vis - alto);
                        e.rolar_para(r);
                    }
                    let Some(s) = e.selecao else { return };
                    let x = f32::from(ev.position.x - bounds.origin.x);
                    let c = g3.colunas;
                    // A seleção fica na coluna em que começou.
                    let x = if s.b { x.max(c.txt_b.0) } else if e.lado { x.min(c.txt_a.1 - 1.) } else { x };
                    let ponto = e.ponto_em(&g3, s.b, x, y_vis + e.rolagem, window);
                    e.selecao = Some(Selecao { foco: ponto, ..s });
                    cx.notify(vista);
                    window.refresh();
                }
                None => {}
            }
        });
        let estado = self.estado.clone();
        let escolher = self.ao_escolher.clone();
        let g4 = g.clone();
        window.on_mouse_event(move |ev: &MouseUpEvent, fase, window, cx| {
            if fase != DispatchPhase::Bubble || ev.button != MouseButton::Left {
                return;
            }
            let mut e = estado.borrow_mut();
            match e.arraste.take() {
                Some(Arraste::Divisor { .. }) => {
                    cx.notify(vista);
                    window.refresh();
                }
                Some(Arraste::Texto { moveu: false, .. }) => {
                    // Clique sem arrastar: limpa a seleção e escolhe a unidade da linha de mudança.
                    e.selecao = None;
                    let (x, y) = (f32::from(ev.position.x - bounds.origin.x), f32::from(ev.position.y - bounds.origin.y) + e.rolagem);
                    let c = g4.colunas;
                    let em_a = x >= c.txt_a.0 && x < c.txt_a.1;
                    let em_b = e.lado && x >= c.txt_b.0 && x < c.txt_b.1;
                    let u = g4.fileira_em(y).filter(|_| em_a || em_b).and_then(|f| e.unidade_em(f, em_b));
                    drop(e);
                    if let Some(u) = u {
                        escolher(u, window, cx);
                    }
                    window.refresh();
                }
                _ => {}
            }
        });
    }
}

/// Palavra sob o byte (duplo clique), pelas mesmas fichas do realce por palavra.
fn palavra_em(t: &str, byte: usize) -> Range<usize> {
    let mut pos = 0;
    for f in palavras::fichas(t) {
        let fim = pos + f.len();
        if byte < fim || fim == t.len() {
            return pos..fim;
        }
        pos = fim;
    }
    t.len()..t.len()
}

fn pintar(e: &Comparador, g: &Geometria, visiveis: Range<usize>, bounds: Bounds<Pixels>, window: &mut Window, cx: &mut App) {
    let Some(m) = e.medidas else { return };
    let ox = f32::from(bounds.origin.x);
    let oy = f32::from(bounds.origin.y) - e.rolagem;
    let caixa = |x0: f32, x1: f32, y0: f32, y1: f32| Bounds::new(point(px(ox + x0), px(oy + y0)), size(px(x1 - x0), px(y1 - y0)));
    let fileiras = e.modelo.fileiras(e.lado);
    let c = g.colunas;
    let colunas: &[(bool, (f32, f32), (f32, f32))] =
        if e.lado { &[(false, c.num_a, c.txt_a), (true, c.num_b, c.txt_b)] } else { &[(false, c.num_a, c.txt_a)] };
    let f = fonte();
    let tam = px(TAMANHO);
    let ts = window.text_system().clone();
    let altura_caixa = m.ascent + m.descent;
    let meio = (LINHA - altura_caixa) / 2.;
    let selecao = e.selecao.map(|s| (s.b, s.ordenada()));
    for r in visiveis.clone() {
        let (y0, y1) = (g.topos[r], g.topos[r + 1]);
        for &(b, num, txt) in colunas {
            let celula = if b { fileiras[r].b } else { fileiras[r].a };
            let tipo = e.modelo.tipo(celula);
            // Fundo da calha e do texto.
            let (fundo_calha, fundo_texto) = match tipo {
                Some(Tipo::Removida) => (cores::calha_removida(), cores::linha_removida()),
                Some(Tipo::Incluida) => (cores::calha_incluida(), cores::linha_incluida()),
                _ => (cores::fundo(), cores::fundo()),
            };
            if tipo.is_some_and(|t| t != Tipo::Contexto) {
                window.paint_quad(fill(caixa(num.0, num.1 - 2., y0, y1), fundo_calha));
                let fim_txt = if e.lado && !b { txt.1 - 1. } else { txt.1 };
                window.paint_quad(fill(caixa(txt.0, fim_txt, y0, y1), fundo_texto));
            }
            if celula == Celula::Vazia {
                hachurar(caixa(txt.0, if e.lado && !b { txt.1 - 1. } else { txt.1 }, y0, y1), window);
            }
            let x_txt = txt.0 + m.ch;
            match celula {
                Celula::Linha(i) => {
                    let ex = &e.exibidos[i];
                    let realce = if b || (!e.lado && e.modelo.linhas[i].tipo != Tipo::Removida) || e.modelo.linhas[i].tipo == Tipo::Incluida {
                        e.realce_novo.as_ref()
                    } else {
                        e.realce_antigo.as_ref()
                    };
                    let faixas = realce.and_then(|r| r.get(i));
                    let q = &g.quebras[r][b as usize];
                    let enfase: Vec<Range<usize>> =
                        e.modelo.enfase[i].iter().map(|x| ex.para_exibido(x.start)..ex.para_exibido(x.end)).collect();
                    let cor_enfase = if e.modelo.linhas[i].tipo == Tipo::Removida { cores::enfase_removida() } else { cores::enfase_incluida() };
                    for s in 0..=q.len() {
                        let ini = if s == 0 { 0 } else { q[s - 1] };
                        let fim = q.get(s).copied().unwrap_or(ex.texto.len());
                        let yl = y0 + s as f32 * LINHA;
                        if oy + yl > f32::from(bounds.bottom()) || oy + yl + LINHA < f32::from(bounds.top()) {
                            continue;
                        }
                        let linha = ts.shape_line(SharedString::from(ex.texto[ini..fim].to_string()), tam, &corridas(ini, fim, faixas), None);
                        let xi = |i: usize| x_txt + f32::from(linha.x_for_index(i.saturating_sub(ini).min(fim - ini)));
                        // Seleção (a altura da linha inteira).
                        if let Some((sb, (a, z))) = selecao {
                            if sb == b && r >= a.fileira && r <= z.fileira {
                                let sa = if r == a.fileira { a.byte } else { 0 };
                                let sz = if r == z.fileira { z.byte } else { usize::MAX };
                                let (sa, sz) = (sa.max(ini), sz.min(fim));
                                let quebra_fim = r < z.fileira && s == q.len();
                                if sz > sa || (quebra_fim && sa <= fim) {
                                    let x1 = xi(sz) + if quebra_fim { m.ch / 2. } else { 0. };
                                    window.paint_quad(fill(caixa(xi(sa), x1, yl, yl + LINHA), cores::selecao()));
                                }
                            }
                        }
                        // O trecho alterado: caixa arredondada na altura da fonte, repetida em cada pedaço quebrado.
                        for x in &enfase {
                            let (a, z) = (x.start.max(ini), x.end.min(fim));
                            if z > a {
                                window.paint_quad(
                                    fill(caixa(xi(a), xi(z), yl + meio, yl + meio + altura_caixa), cor_enfase).corner_radii(px(3.)),
                                );
                            }
                        }
                        let _ = linha.paint(point(px(ox + x_txt), px(oy + yl)), px(LINHA), TextAlign::Left, None, window, cx);
                    }
                }
                Celula::SemQuebra(_) => {
                    let mut cor = cores::texto();
                    cor.a *= 0.6;
                    let linha = ts.shape_line(SharedString::from(SEM_QUEBRA), tam, &[corrida(SEM_QUEBRA.len(), &f, cor)], None);
                    let _ = linha.paint(point(px(ox + x_txt), px(oy + y0)), px(LINHA), TextAlign::Left, None, window, cx);
                }
                _ => {}
            }
            // Número e sinal na calha, alinhados à direita.
            if let Some(n) = e.modelo.numero(celula, b, e.lado) {
                let pad_d = if e.lado { 4. } else { m.ch };
                let mudada = tipo.is_some_and(|t| t != Tipo::Contexto);
                let direita = num.1 - 2. - pad_d;
                let cor_num = match tipo {
                    Some(Tipo::Removida) => cores::numero_removido(),
                    Some(Tipo::Incluida) => cores::numero_incluido(),
                    _ => cores::numero(),
                };
                let fim_num = if mudada { direita - m.sinal - 4. } else { direita };
                let texto = n.to_string();
                let linha = ts.shape_line(SharedString::from(texto.clone()), tam, &[corrida(texto.len(), &f, cor_num)], None);
                let _ = linha.paint(point(px(ox + fim_num - f32::from(linha.width)), px(oy + y0)), px(LINHA), TextAlign::Left, None, window, cx);
                if mudada {
                    let s = if tipo == Some(Tipo::Removida) { "−" } else { "+" };
                    let sinal = ts.shape_line(SharedString::from(s), tam, &[corrida(s.len(), &f, cores::numero())], None);
                    let _ = sinal.paint(point(px(ox + direita - m.sinal), px(oy + y0)), px(LINHA), TextAlign::Left, None, window, cx);
                }
            }
        }
    }
    // A borda direita do texto original (só onde há fileiras).
    if e.lado && !g.topos.is_empty() {
        let (y0, y1) = (g.topos[0], *g.topos.last().unwrap());
        window.paint_quad(fill(caixa(c.txt_a.1 - 1., c.txt_a.1, y0, y1), cores::borda()));
    }
    // A unidade atual: barra azul de 3 px à esquerda.
    if let Some((topo, base)) = e.atual.and_then(|u| e.faixa(u)) {
        window.paint_quad(fill(caixa(0., 3., topo, base), cores::atual()));
    }
}

/// Espaço de alinhamento: listras diagonais de 2 px a cada 8 px (`linear-gradient(-45deg, #cccccc33 12.5%, ...)` em
/// ladrilhos de 8 px a partir de `background-position: 5px 0`).
fn hachurar(b: Bounds<Pixels>, window: &mut Window) {
    let (x0, y0) = (f32::from(b.origin.x), f32::from(b.origin.y));
    let (w, h) = (f32::from(b.size.width), f32::from(b.size.height));
    let mut p = PathBuilder::fill();
    // Listra onde (x - 5 + y) mod 8 ∈ [6, 8): retas x + y = c com c ≡ 3 (mod 8), 2 px de largura em x.
    let mut c = 3. - 8. * ((h / 8.).ceil() + 1.);
    while c < w + h {
        let (a, z) = (c, c + 2.);
        p.move_to(point(px(x0 + a), px(y0)));
        p.line_to(point(px(x0 + z), px(y0)));
        p.line_to(point(px(x0 + z - h), px(y0 + h)));
        p.line_to(point(px(x0 + a - h), px(y0 + h)));
        p.close();
        c += 8.;
    }
    window.with_content_mask(Some(ContentMask { bounds: b }), |window| {
        window.paint_quad(fill(b, cores::fundo()));
        if let Ok(path) = p.build() {
            window.paint_path(path, cores::hachura());
        }
    });
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tabulacao_com_tab_size_2() {
        let e = Exibido::de("\tx\ty");
        assert_eq!(e.texto, "  x y");
        assert_eq!(e.para_exibido(1), 2);
        assert_eq!(e.para_exibido(3), 4);
        assert_eq!(e.para_original(1), 0);
        assert_eq!(e.para_original(4), 3);
    }

    #[test]
    fn unidades_por_fileira() {
        let patch = "--- a/x\n+++ b/x\n@@ -1,6 +1,7 @@\n a\n-b\n+B\n+B2\n c\n d\n-e\n f\n";
        let c = crate::comparar::comparar(patch, None);
        let u: Vec<_> = c.unificado.iter().map(|l| l.unidade).collect();
        let cmp = Comparador::new(patch, "x.ts", &u, c.unidades.len());
        assert_eq!(cmp.unidades[0], FaixaUnidade { lado: Some((1, 2)), unificado: Some((1, 3)) });
        assert_eq!(cmp.unidades[1], FaixaUnidade { lado: Some((5, 5)), unificado: Some((6, 6)) });
        assert_eq!(cmp.unidade_em(1, true), Some(0));
        assert_eq!(cmp.unidade_em(0, false), None);
    }

    /// Fonte monoespaçada de 8 px e quebra por contagem de caracteres.
    fn medir(c: &mut Comparador, largura: f32, alto: f32) -> Rc<Geometria> {
        let m = Medidas { ch: 8., sinal: 8., ascent: 11., descent: 3. };
        let larguras: Vec<f32> = c.exibidos.iter().map(|e| e.texto.chars().count() as f32 * 8.).collect();
        c.caixa = Bounds::new(point(px(0.), px(0.)), size(px(largura), px(alto)));
        let mut quebrar = |t: &str, util: f32| -> Vec<usize> {
            let n = (util / 8.).floor().max(1.) as usize;
            t.char_indices().map(|(i, _)| i).skip(n).step_by(n).collect()
        };
        c.calcular(largura, m, &larguras, &mut quebrar)
    }

    fn comparador(patch: &str, blocos: Option<&[crate::comparar::Intervalo]>) -> Comparador {
        let cmp = crate::comparar::comparar(patch, blocos);
        let u: Vec<_> = cmp.unificado.iter().map(|l| l.unidade).collect();
        Comparador::new(patch, "x.ts", &u, cmp.unidades.len())
    }

    const LONGO: &str = "--- a/x
+++ b/x
@@ -1,5 +1,5 @@
 a
-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
+c
 d
 e
-f
";

    #[test]
    fn geometria_do_lado_a_lado() {
        let mut c = comparador(LONGO, None);
        let g = medir(&mut c, 500., 300.);
        // Calha: 2ch + 3ch + 4 + sinal + 4 + borda 2 = 58; o resto dividido 50/50 depois do vão de 40.
        assert_eq!(g.colunas.num_a, (0., 58.));
        assert_eq!(g.colunas.txt_a, (58., 58. + 172.));
        assert_eq!(g.colunas.num_b, (270., 328.));
        assert_eq!(g.colunas.centro, 250.);
        // Margem de 8 px; a linha de 50 caracteres quebra em 172 - 16 - 1 = 155 px úteis (19 caracteres): 3 linhas.
        assert_eq!(g.topos[0], 8.);
        assert_eq!(g.topos[1], 27.);
        assert_eq!(g.topos[2] - g.topos[1], 3. * LINHA, "a fileira tem a altura do lado mais alto");
        assert_eq!(g.altura, g.topos[g.topos.len() - 1] + 8.);
        // Divisor a 30%: o original encolhe e o vão acompanha.
        c.pct = 30.;
        let g = medir(&mut c, 500., 300.);
        assert!((g.colunas.txt_a.1 - (58. + 344. * 0.3)).abs() < 0.01);
        assert!((g.colunas.centro - (g.colunas.txt_a.1 + 20.)).abs() < 0.01);
    }

    #[test]
    fn geometria_do_unificado_sem_margem() {
        let mut c = comparador(LONGO, None);
        c.definir_lado(false);
        let g = medir(&mut c, 500., 300.);
        // Calha do unificado: padding-right de 1ch.
        assert_eq!(g.colunas.num_a, (0., 16. + 24. + 4. + 8. + 8. + 2.));
        assert_eq!(g.topos[0], 0.);
        assert_eq!(g.topos.len(), 7);
        assert_eq!(g.altura, g.topos[6]);
    }

    #[test]
    fn navegacao_rola_ao_terco_e_ancora_a_rolagem() {
        let mut linhas = String::from("--- a/x
+++ b/x
@@ -1,60 +1,60 @@
");
        for i in 0..60 {
            if i == 40 {
                linhas.push_str("-velha
+nova
");
            } else {
                linhas.push_str(&format!(" linha {i}
"));
            }
        }
        let mut c = comparador(&linhas, None);
        medir(&mut c, 500., 300.);
        c.escolher(Some(0));
        c.aplicar_pulo();
        let (topo, base) = c.faixa(0).unwrap();
        assert_eq!(topo, 8. + 40. * LINHA);
        assert_eq!(base - topo, LINHA);
        assert_eq!(c.rolagem, topo - 100.);
        assert!(!c.pular, "o pulo acontece uma vez");
        // Estreitar a coluna quebra linhas: a fileira do topo continua no topo.
        let antes = c.geo.as_ref().unwrap().fileira_em(c.rolagem).unwrap();
        medir(&mut c, 200., 300.);
        assert_eq!(c.geo.as_ref().unwrap().fileira_em(c.rolagem).unwrap(), antes);
        // Rolagem limitada à altura do palco.
        c.rolar_para(1e9);
        assert_eq!(c.rolagem, c.altura_palco() - 300.);
    }

    #[test]
    fn selecao_copia_o_original() {
        let mut c = comparador("--- a/x
+++ b/x
@@ -1,2 +1,2 @@
-	a
+	b
 x
", None);
        medir(&mut c, 500., 300.);
        c.selecao = Some(Selecao { b: true, ancora: Ponto { fileira: 0, byte: 0 }, foco: Ponto { fileira: 1, byte: 1 } });
        assert_eq!(c.texto_selecionado().as_deref(), Some("	b
x"));
    }

    #[test]
    fn palavra_do_duplo_clique() {
        assert_eq!(palavra_em("import { useMemo }", 11), 9..16);
        assert_eq!(palavra_em("a  b", 1), 1..3);
    }
}
