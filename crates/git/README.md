# orquestrador-git

Tela Git da lateral e as vistas de comparação e de conflito do editor, porte de `src/git/` e de
`src/editor/{diff-view,compare-rows,conflict-view}.tsx`.

## Montagem

```rust
orquestrador_git::init(cx); // uma vez, depois de gpui_kit::init: F8/Shift+F8 e Ctrl+S, e aquece o syntect
let fonte: Fonte = Arc::new(FonteCore(projetos.core(&id)?)); // os comandos do Tauri, pelo Core do projeto
let guarda = GuardaGit::new();                               // uma por projeto, compartilhada entre as telas
let tela = cx.new(|cx| TelaGit::new(fonte.clone(), guarda.clone(), window, cx));
```

`FonteGit` é o trait com os comandos (`estado`, `stage`, `commit`, `review_diff`, `conflict`...); `simulado::Simulado`
é a implementação com os dados de `src/backend/mock.ts`, para testes e para a vitrine.

## `TelaGit` (lateral)

- Métodos: `definir_ativa(bool)` (tela Git aberta: grafo e histórico só consultam assim), `definir_travado(bool)`
  (executores vivos), `definir_selecionado(Option<Alvo>)` (aba aberta no editor, marca a linha), `pedir_historico(Option<String>)`
  (abre o Histórico do projeto ou do arquivo), `cutucar()` (evento de executor: relê o status já), `atualizar()`, `estado()`.
- Eventos `EventoGit`: `AbrirAlvo { alvo, permanente }` (clique abre em prévia, duplo clique de vez),
  `PreCarregar { alvo, hover }`, `AbrirArquivo { caminho, permanente }`, `Estado(Option<Estado>)` (badge e barra de
  status; só quando muda).

## `VisaoDiff` (aba do editor)

`VisaoDiff::new(fonte, guarda, alvo, window, cx)` com `Alvo::Diff { lado, caminho }` (relido a cada 2,5 s enquanto ativa)
ou `Alvo::Commit { hash, caminho }` (lido uma vez). Métodos: `definir_ativa`, `definir_travado`, `definir_rascunho`
(rascunho aberto do arquivo bloqueia descartar), `alternar_modo`, `modo() -> diff::Modo` (`LadoALado`/`Unificado`; antes `DiffMode` do Kit),
`passo`, `escolher`, `proporcao`/`definir_proporcao` (divisor, 15 a 85), `rolagem`, `faixa_atual`, `selecao`. Eventos `EventoDiff`: `AtualizarGit`,
`AbrirLinha { caminho, linha }`.

O corpo é o comparador próprio (`comparador::{Comparador, Corpo}`), porte do `FileDiff` do `@pierre/diffs` 1.2.12 como o
React o configura: Consolas 14/19 px, calha com número e −/+, vão de 40 px com divisor arrastável e ações do bloco,
células vazias hachuradas, `word-alt` (`comparador::palavras`), régua e F8/Shift+F8 pela geometria calculada.
Ctrl+C copia a seleção; setas, PageUp/PageDown, Home/End rolam.

## `VisaoConflito` (aba do editor)

`VisaoConflito::new(fonte, guarda, caminho, rascunho, window, cx)`; `rascunho` vem da recuperação. Métodos: `mostrar`
(a aba voltou a ser a atual), `definir_travado`, `definir_arquivo_sujo`, `rascunho()`. Eventos `EventoConflito`:
`Rascunho(RascunhoConflito)` (sujo e recuperação), `AtualizarGit`, `Fechar` (resolvido ou removido). O texto final usa o
`Documento` e o `Minimapa` do `orquestrador-editor`.

Na tela Git, Shift+F10 e a tecla de menu abrem o menu de contexto da linha focada.

## Puro, para o git gutter

`preparo::{trechos, preparar_bloco, tirar_bloco, aplicar, tocam, linhas_antigas, ir_para_bloco}` (porte de
`git-stage.ts` e da parte pura de `git-hunk.ts`): o texto novo do índice para `git::write_index`.

## Vitrine

`cargo run -p orquestrador-git --example vitrine -- <cena>`, cena = `alteracoes`, `historico`, `commit-diff`,
`lado-a-lado`, `unificado`, `sem-repositorio`, `ausente` ou `conflito`.
