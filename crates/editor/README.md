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

## Dados gerados

`node crates/editor/gerar-dados.mjs` refaz `assets/linguagens.json` e `assets/dark-plus.json`; `node
crates/editor/examples/gerar-dados-vitrine.mjs` refaz os textos da vitrine. Vitrine: `cargo run -p orquestrador-editor --example
vitrine -- arquivos|varias|menu|lista|dividido|rapido|md-preview|md-codigo|binario|vazio`.
