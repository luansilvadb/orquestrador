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

Os testes do próprio crate não fazem parte do workspace; os do editor (`crates/editor/tests/git_gutter.rs`) cobrem as
alterações.

## gpui-pre-windows 0.3.12

Origem: o pacote `gpui-pre-windows` 0.3.12 do crates.io (a plataforma Windows do `gpui-pre`). Licença Apache-2.0
(`gpui-pre-windows/LICENSE-APACHE`). Trechos alterados levam `Orquestrador:`.

Alteração: `WindowsPlatform::new` começa a criar os dispositivos DirectX numa thread (`DirectXDevices::preparar`), em
paralelo com o DirectWrite e a janela oculta; `attach_gpu` os recebe com `tomar_preparados`. Na abertura, ~80 ms dos
~170 ms do dispositivo deixam de somar (docs/desempenho-gpui.md).
