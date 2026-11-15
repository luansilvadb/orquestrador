# orquestrador-ui

Tema, recursos embutidos, estado salvo, widgets e a faixa de abas, comuns à bancada (`crates/app`) e aos crates de tela.

## Contrato dos crates de tela

Cada tela (editor, árvore, mídia, pesquisa, Git) é um crate próprio que:

- depende de `orquestrador-ui` e `orquestrador-core`, e de `gpui-kit = "=0.9.1"`; não depende de `crates/app`;
- expõe uma `Entity` com `Render` (a View) e um `EventEmitter` com os eventos que a bancada trata (abrir arquivo, foco, avisos);
- recebe da bancada o que precisa do core e do projeto ativo no construtor ou por métodos, sem globais próprias;
- guarda a própria preferência numa seção versionada: `impl estado::Secao` com `NOME` único e `VERSAO`, lida e gravada por
  `estado::ler::<S>(cx)` e `estado::gravar(&s, cx)`;
- usa `tema::cor`, as medidas de `tema` e `tema::FONTE`; ícones novos entram em `assets/icons/` e em `recursos.rs`;
- usa `FaixaAbas` e `ListaAbas` para abas;
- usa os menus do Kit (`PopupMenu`, com `widgets::item_menu` para rótulo e atalho), já nas medidas do React;
- usa `TooltipTexto::tooltip_texto` para dicas e `widgets::mascara` sob os modais; menu ou camada própria que deve
  esconder a dica visível chama `widgets::marcar_sobreposicao()` ao desenhar.
- ordena como o `localeCompare` do React com `colagem::comparar` e quebra texto corrido com `quebra::linhas` (o
  `text-wrap: pretty` do Lobe) ou com `widgets::texto_pretty`.
