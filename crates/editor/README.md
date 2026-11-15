# orquestrador-editor

Coluna central do editor (porte de `src/editor/` e `src/shared/file-tabs.tsx`/`file-tab-actions.ts`): até dois grupos de abas, editor de
texto do GPUI Kit com realce `syntect`/dark-plus, Markdown com Preview, abrir rápido, formatação e recuperação de rascunhos.

## View

`AreaEditor` (`Entity` com `Render`, `Focusable`, `EventEmitter<EventoEditor>`).

- `AreaEditor::new(projeto, window, cx)`: `projeto` é a raiz absoluta (vazia sem projeto). Recuperação em
  `%LOCALAPPDATA%\orquestrador\editor-recuperacao.json`, chave por projeto, com os limites e a validação de `workspace-store.ts`.
- `AreaEditor::com_armazenamento(projeto, Rc<RefCell<dyn armazem::Chaves>>, window, cx)`: outro armazenamento (testes).

### Métodos

| Método | O que faz |
| --- | --- |
| `abrir_arquivo(caminho, Abertura { permanente, linha, coluna, lado }, w, cx)` | Abre (prévia sem `permanente`), revela linha:coluna (1-based). Caminho relativo ou absoluto. |
| `abrir_ao_lado(caminho, Abertura, w, cx)` | Abre no grupo da direita. |
| `abrir_alvo(Alvo, Abertura, w, cx)` | Abre diff, commit, conflito ou arquivo com `doc_id`. |
| `abrir_pelo_dialogo(w, cx)` | Arquivo > Abrir arquivo. |
| `novo_arquivo(w, cx)` | Arquivo > Novo (rascunho "Sem título-N"). |
| `salvar_ativa`, `salvar_ativa_como`, `salvar_tudo(w, cx, ao_fim)` | Arquivo > Salvar, Salvar como, Salvar tudo (`ao_fim` recebe se ficou tudo limpo). |
| `abrir_rapido(w, cx)` | Ctrl+P. |
| `fechar(Escopo, w, cx)`, `reabrir_fechada`, `dividir(id)`, `unir_grupos`, `ativar(id)` | Operações de aba. |
| `remapear(de, para, cx)`, `remover(&[caminhos], cx)` | Depois de renomear/mover e de excluir. |
| `ha_rascunho(caminho, cx)`, `nao_salvos()`, `descartar_tudo(w, cx)` | Guardas do Git e do fechamento da janela. |
| `caminhos_abertos()`, `texto_selecionado(cx)`, `atualizar(w, cx)`, `gravar_recuperacao(cx)` | `openPaths`, `selectedText`, `refresh`, `flushRecovery`. |
| `definir_projeto`, `definir_ativo`, `definir_layout` | Projeto ativo, tela visível (liga a consulta ao disco de 2,5 s), layout opaco da bancada. |
| `definir_fabrica_conteudo(FabricaConteudo)` | Vista das abas de diff, commit, conflito e mídia (`Fn(&Alvo, &mut Window, &mut App) -> Option<AnyView>`). |
| `ao_abrir_documento(AoAbrirDocumento, w, cx)`, `decoracoes(caminho)`, `editor_de(caminho)` | Para o gutter do Git: `InfoDocumento` traz `doc_id`, caminho e as coleções `gutter` (16 px), `regua` e `trechos` do Kit. |
| `definir_widgets(doc_id, Vec<WidgetInline>, cx)` | Widgets sob uma linha (bloco do Git aberto). |
| `definir_tem_alteracoes(doc_id, bool, cx)` | Liga Próxima/Anterior Alteração (Alt+F5) no menu. |

### Eventos (`EventoEditor`)

`AbaAtiva(Option<Alvo>)`, `Salvo`, `AtualizarGit`, `RevelarNoExplorer(caminho)`, `Grupos(n)`, `RestaurarLayout(texto)`,
`IrParaAlteracao { doc_id, passo }`.

## Módulos puros

`modelo` (workspace-model), `armazem` (workspace-store, conflitos, patch), `caminhos` (file-tab-actions), `linguagem`
(detecção igual à do Shiki), `realce` (tema dark-plus, colchetes), `formatar` (Prettier via dprint/malva/markup_fmt/pretty_yaml;
outros motores pelo core).

## Preview de Markdown

`markdown::previa` reproduz o `Markdown` do Lobe UI 5.58 (`fontSize` 14, tema escuro): o `TextView` do Kit (`gpui_kit::base`)
desenha o fluxo com seleção e cópia; `markdown::blocos` troca blocos de código e tabelas por elementos próprios (`Snippet`
para código de uma linha até 32 caracteres, `Highlighter` com copiar e linguagem no hover, tabela na largura do conteúdo).
O realce dos blocos usa `assets/lobe-theme.json`: o `lobe-theme` do `@lobehub/ui` 5.58 com os tokens escuros resolvidos
(`colorInfo` #60b1ff, `colorSuccess` #c4f042, `colorWarning` #ffb224, `purple10` #d590da, `volcano10` #ff9480, `geekblue10`
#9abeff), extraído uma vez. Imagens relativas ao `.md` vêm do disco; http(s) pela URL. `markdown::instalar_realce_de_codigo`
continua colorindo com dark-plus os `TextView` de fora do Preview.

## Dados gerados

Ficam versionados; os pacotes Node de onde vieram saíram do projeto.

- `assets/linguagens.json` (detecção de linguagem): `bundledLanguagesInfo` do Shiki, `linguist-languages` e
  `@codemirror/language-data`, nas versões do `package.json` do React.
- `assets/dark-plus.json`: `colors` e `tokenColors` de `shiki/themes/dark-plus`.
- `assets/gramaticas/*.sublime-syntax`: gramáticas TextMate do `@shikijs/langs` 4.5.0 convertidas mecanicamente
  (`begin`/`end` viram `push` com `meta_scope`, `while` vira `pop` na linha que não casa, `repository` vira contextos,
  escopos cortados em 8 átomos, limite do `syntect`). Só onde a conversão acerta mais que o `two-face`: c, cpp, csharp,
  go, html, php, shellscript e vue continuam com o `two-face`. `assets/sintaxes.packdump` é o conjunto `two-face` +
  convertidas já ligado (carrega em ~20 ms); depois de mudar as gramáticas: `cargo test -p orquestrador-editor --lib
  gerar_sintaxes -- --ignored` (o teste `packdump_em_dia_com_as_gramaticas` acusa o esquecimento).
- `examples/dados/*`: textos do mock do React (`src/backend/mock.ts`, `mock-fixtures.ts`) para a vitrine.

Vitrine: `cargo run -p orquestrador-editor --example vitrine --
arquivos|varias|menu|lista|dividido|rapido|md-preview|md-codigo|binario|vazio`.

## Cores comparadas com o Shiki

`tests/shiki/amostra.<ext>` e `.cores` (cor de cada token do `codeToTokensBase` dark-plus, gerada uma vez) medem a
fração de caracteres visíveis, fora colchetes, com a cor do React. Atual (antes da conversão): ts 100 (97,6), tsx 100,
js 100 (81,0), jsx 100 (85,1), json 100 (54,7), css 100 (80,9), scss 98,7 (91,9), html 93,5, md 93,4 (82,1), rs 100
(60,6), py 100 (83,8), go 86,9, java 94,9 (68,4), c 93,9, cpp 72,0, cs 77,0, sh 76,5, ps1 100, yaml 99,3 (97,4), toml
90,8 (67,2), sql 96,4 (89,4), xml 100 (97,7), php 90,0, rb 94,5 (74,0), kt 84,2 (72,4), swift 74,8 (71,9), lua 98,3
(88,6), dart 100 (98,1), vue 88,9. O teste `cores_iguais_as_do_shiki` exige esses valores; `SHIKI_DETALHE=<ext>` lista
as diferenças.
