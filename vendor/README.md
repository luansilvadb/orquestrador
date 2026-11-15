# vendor

Cópias de crates de terceiros com alterações mínimas, ligadas por `[patch.crates-io]` no `Cargo.toml` da raiz.

## gpui-base 0.9.1

Origem: o pacote `gpui-base` 0.9.1 do crates.io (repositório `longbridge/gpui-kit`, `crates/base`, commit
`39ab849a93b5f15507b969164d49891db8d5b470`), parte do GPUI Kit 0.9.1. Licença Apache-2.0 (`gpui-base/LICENSE-APACHE`).
Cada trecho alterado leva o comentário `Orquestrador:`.

Alterações:

- Blocos entre linhas (o widget do bloco do Git do editor): `DisplayMap::set_blocks`, com a altura reservada acima de
  uma linha do buffer (ou abaixo da última). `display_row_top`, `display_row_at_y` e `buffer_line_box_top` trocam a
  conta `linha × altura` nos pontos que convertem linha e posição vertical (faixa visível, altura rolável, cursor,
  rolar até o cursor, clique, régua, marcas da calha, dobras, guias de recuo, números de linha, fundo da linha atual,
  seleção). `LineLayout` ganha `block_top`/`block_bottom`, somados no tamanho e no desenho. No estado: `set_blocks`,
  `block_bounds(linha)` (onde desenhar o widget) e `gutter_width()`.
- Quebra de linha como a do Chromium com o `white-space: break-spaces` do CodeMirror (`browser_linebreaks`): UAX #14
  mais a tabela ASCII do Blink (quebra antes de `(` `<` `[` `{`, depois de `-` e `?`); cada espaço é ponto de quebra e
  conta na largura. O recuo de continuação conta espaços e tabs, como `wrap-indent.ts`. `wrap_line_boundaries` expõe a
  conta para testes.
- `RangeDecorationStyle::Lines`: fundo na largura toda das linhas que o trecho toca (o fundo das linhas novas do bloco
  aberto).
- Cor do texto selecionado nos campos: `InputEditorStyle::selection_foreground` (`None` mantém a cor); o `TextElement`
  parte os runs nos limites da seleção (`recolor_selected_runs`) com o campo focado e a janela ativa.
- Código inline do texto (`text/inline.rs`, `text/inline_flow.rs`): a margem horizontal do estilo (`margin-inline`) soma
  aos recuos do trecho e o fundo a desconta (`code_margins`).

Os testes do próprio crate não fazem parte do workspace; os do editor (`crates/editor/tests/git_gutter.rs`) cobrem as
alterações.

## gpui-component 0.9.1

Origem: o pacote `gpui-component` 0.9.1 do crates.io (repositório `longbridge/gpui-kit`), parte do GPUI Kit 0.9.1.
Licença Apache-2.0 (`gpui-component/LICENSE-APACHE`). Trechos alterados levam `Orquestrador:`.

Alterações:

- Menus (`menu/popup_menu.rs`, `menu/menu_item.rs`, `menu/dropdown_menu.rs`) nas medidas do menu do React
  (`src/index.css` sobre o DropdownMenu do Lobe UI): itens de 26 px, texto de 13 px a 26 px da borda, raio de 4 px, sem
  espaço entre itens; menu com 4 px de folga, borda de 1 px, anel claro por fora, raio de 5 px e 220 px de largura
  mínima; separador de 1 px com 5 px de margem; ícone absoluto a 6 px da borda; submenu sem seta, encostado no menu e
  4 px acima; desabilitado em branco a 25% com opacidade de 50%; dropdown 6 px abaixo do gatilho.
- Tema (`theme/mod.rs`, `input/input.rs`): `Theme::selection_foreground`, a cor do texto selecionado repassada ao
  `InputEditorStyle` do `gpui-base`.
- Dica (`tooltip.rs`): some quando o mouse aperta e quando um menu ou modal desenha no mesmo quadro
  (`marcar_sobreposicao`, chamado pelo `PopupMenu` e pela máscara de `orquestrador-ui`); a dica pendente do dono clicado
  nasce escondida (`registrar_clique`). Escondida, só volta num novo hover.

## gpui-pre-windows 0.3.12

Origem: o pacote `gpui-pre-windows` 0.3.12 do crates.io (a plataforma Windows do `gpui-pre`). Licença Apache-2.0
(`gpui-pre-windows/LICENSE-APACHE`). Trechos alterados levam `Orquestrador:`.

Alteração: `WindowsPlatform::new` começa a criar os dispositivos DirectX numa thread (`DirectXDevices::preparar`), em
paralelo com o DirectWrite e a janela oculta; `attach_gpu` os recebe com `tomar_preparados`. Na abertura, ~80 ms dos
~170 ms do dispositivo deixam de somar (docs/desempenho-gpui.md).
