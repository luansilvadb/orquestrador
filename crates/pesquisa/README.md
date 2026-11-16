# orquestrador-pesquisa

Tela Pesquisar, com busca, prévia e substituição pelo motor de `orquestrador_core::ide::search`.

## View

`TelaPesquisa` (`Entity` com `Render` e `Focusable`):

- `TelaPesquisa::new(raiz: Option<PathBuf>, window, cx)`: `raiz` é a pasta do projeto (`core.repo()`).
- `definir_raiz(raiz, cx)`: troca de projeto; refaz a busca.
- `definir_abertos(Vec<String>, cx)`: caminhos com aba no editor, para "só nos editores abertos".
- `definir_alterados(HashSet<String>, cx)`: caminhos com alteração não salva (marcados "alterado" e fora da substituição).
- `focar(window, cx)`: Ctrl+Shift+F e entrada na tela; foca o campo com o texto selecionado.
- `semear(texto, window, cx)`: seleção do editor vira a consulta e busca na hora.
- `pesquisar_na_pasta(pasta, window, cx)`: "Localizar na Pasta"; põe `./pasta` no incluir.
- `atualizar_apos_gravar(cx)`: chamar a cada gravação no editor.
- `passo(1 | -1, cx)`: F4 e Shift+F4.
- `definir_substituicao(texto, window, cx)`, `substituir_tudo(cx)`, `consulta()`, `status()`.

Preferência (visão lista/árvore e histórico) na seção `pesquisa` do `estado` (`PreferenciaPesquisa`).

## Eventos (`EventoPesquisa`)

- `AbrirArquivo { caminho, linha, coluna, permanente }`: coluna UTF-16 a partir de 1; clique simples e F4 pedem prévia, duplo clique e Enter pedem aba permanente.
- `PreLer { caminho }`: botão desceu sobre um resultado.
- `ArquivosAlterados { caminhos }`: a substituição gravou; recarregar o que estiver aberto.
- `Aviso(String)`: mensagem curta (cópia feita).
