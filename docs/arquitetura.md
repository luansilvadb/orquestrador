# Arquitetura

O front-end (React) fica em `src/` e o backend (Rust, Tauri) em `src-tauri/src/`. O mesmo `orquestrador.exe` atende também os modos `--mcp`, `--hook` e `--allow`, que os CLIs chamam. Os caminhos são relativos à raiz do repositório.

## Front-end (`src/`)

Uma linha por pasta; os arquivos não óbvios de cada uma vêm logo abaixo da tabela.

| Pasta | Papel |
| --- | --- |
| `src/` | Raiz: `main.tsx` monta o app (o `ThemeProvider`, os hosts de aviso e de diálogo); `App.tsx` integra as telas e os painéis (a tela de atualização, a tela principal com árvore, área do arquivo, painel Terminal e painel do Orquestrador, e a tela Git); `index.css` é o CSS global. O design system (Lobe UI) está em [design system](design-system.md). |
| `src/assets/` | Os codicons e o tema de ícones Symbols, com as licenças. |
| `src/backend/` | O contato com o backend: `backend.ts` é o único ponto que fala com o Tauri, e o backend simulado fica aqui. |
| `src/editor/` | A área central: abas, grupos, diff, commit, conflito, comparação com o disco, recuperação, o editor de texto, a aba de mídia, os breadcrumbs e o arquivo rápido. |
| `src/explorer/` | A coluna de ícones da navegação, a árvore de pastas, a barra de status e as decorações do Git na árvore. |
| `src/git/` | A barra lateral do Git (alterações e histórico), o gráfico de commits e a trava das operações Git. |
| `src/search/` | A pesquisa de texto nos arquivos, na faixa da árvore. |
| `src/terminal/` | Os terminais (xterm), o painel do Orquestrador, o painel Terminal, a colagem de arquivo e imagem e o formulário de escolha. |
| `src/window/` | A barra de título, o menu principal e os controles da janela. |
| `src/shared/` | O que é usado por três ou mais áreas sem dono óbvio: caminhos, atualização periódica, faixa de abas, abas de arquivo, ícones e aviso para abrir pasta. |

Arquivos não óbvios:
- src/editor/format.ts: carrega Prettier e os plugins oficiais sob demanda; combina as opções devolvidas pelo backend e formata com posição do cursor.
- `src/backend/backend.ts`: o único ponto de contato com o Tauri, pela interface `Transport`. Fora do app (no navegador, em desenvolvimento), entra o simulado.
- `src/backend/mock.ts` e `mock-fixtures.ts`: o backend simulado, só em `npm run dev` fora do Tauri. Simula o modo sem projeto por `?project=none` e os modos sem Git por `?git=no-repo`, `no-git`, `repo-above` e `?subfolder=1` (ver [desenvolvimento](desenvolvimento.md)).
- `src/backend/types.ts`: os tipos do contrato com o Rust e o mapa `Commands`, com cada comando do Tauri, seus argumentos e o resultado. `backend.ts` e `mock.ts` são tipados por ele, e `scripts/check-commands.mjs` confere as chaves com `generate_handler!` em `lib.rs`.
- `src/shared/open-project.tsx`: aviso e botão `Abrir pasta` nos painéis que precisam de projeto.
- `src/editor/editor-workspace.tsx`: até dois grupos lado a lado, com abas de arquivo, diff, commit e conflito; a comparação com o disco antes de gravar; recuperação por projeto e recuperação sem projeto, com arquivos soltos e rascunhos.
- `src/editor/workspace-model.ts` e `workspace-store.ts`: modelos puros, sem React. O modelo cuida das abas e dos grupos (prévia, fixadas, fechamento com confirmação, reabertura e carga do estado salvo); o store cuida de buffers, rascunhos, conflitos e do texto da recuperação, validado ao ler, sem disco.
- `src/editor/file-view.tsx`: o editor de texto (CodeMirror com as cores do app e as medidas do Monaco, guias de recuo e minimap). Não lê nem grava o disco; guarda o estado do CodeMirror por arquivo e reporta cursor e rolagem.
- `src/editor/media-view.tsx`: a aba de mídia, com imagem (Ajustar e Tamanho real) e áudio (controles nativos). Os bytes vêm de `read_media` e entram num blob local; não lê texto e não grava.
- `src/editor/breadcrumbs.tsx`: os breadcrumbs embaixo das abas de arquivo, com pastas e arquivo, e o seletor do conteúdo da pasta.
- `src/editor/compare-rows.ts`: o modelo do comparador contínuo, em lado a lado e unificado.
- `src/editor/git-gutter.ts`: os marcadores de mudança do Git no editor (gutter, barra de rolagem e minimap), pelo diff do buffer, mesmo não salvo, com o HEAD e o índice que `git_file_base` devolve.
- `src/editor/git-hunk.ts`: o bloco aberto de um marcador, com as linhas antigas do HEAD, Preparar ou Tirar do preparo, Restaurar (edição no buffer) e a navegação entre alterações.
- `src/editor/git-stage.ts`: o cálculo puro do texto novo do índice para preparar ou tirar do preparo um bloco, que `git_write_index` grava; testado por `scripts/check-git-stage.mjs`.
- `src/editor/bracket-colors.ts`: a cor dos parênteses, colchetes e chaves pelo nível de aninhamento, lida da árvore de sintaxe.
- `src/editor/quick-open.tsx`: o arquivo rápido (Ctrl+P), com Enter, Shift+Enter e Ctrl+Enter.
- `src/explorer/explorer.tsx`: a coluna de ícones, a árvore de pastas (pasta abre e fecha; arquivo abre na área central; menu com renomear, excluir, criar, copiar, recortar, colar e mover, inclusive em lote; barra com as ações da view e seção do projeto) e a barra de status com a branch, o Sincronizar e o botão Terminal.
- `src/explorer/explorer-decorations.ts`: as decorações do Git na árvore, com cor e letra por arquivo, herança na pasta e o cinza do ignorado, nas cores do tema escuro do VS Code.
- `src/explorer/source-control-icon.tsx`: o ícone do Git na coluna de ícones, o codicon `source-control` do Antigravity/VS Code (de `@vscode/codicons`, CC BY 4.0, licença em `src/assets/codicons/LICENSE`).
- `src/git/git.tsx`: a barra lateral do Git. Os comandos vêm do backend, e as operações passam pela trava compartilhada.
- `src/git/git-guard.ts`: a trava das operações Git da interface, compartilhada pela barra lateral e pelo editor.
- `src/git/git-graph.ts`: o layout puro do gráfico de commits (faixas, linhas de merge e cores), testado por `scripts/check-git-graph.mjs`.
- `src/git/git-graph-view.tsx`: o gráfico e o hover de commit; corpo, estatísticas e URL do GitHub são pedidos uma vez por hash ao abrir o hover.
- `src/search/search.tsx`: a pesquisa de texto, com o cabeçalho, o campo com as três opções e o histórico, o Substituir (com Preservar maiúsculas), os filtros de caminho, a lista ou árvore de ocorrências (com dispensar e menu de contexto) e a prévia da troca na própria lista.
- `src/terminal/terminal-screen.tsx`: o painel do Orquestrador, à direita da tela principal: faixa de abas (orquestrador e um executor por aba), a barra da aba ativa e os terminais, com o da aba ativa visível.
- `src/terminal/shell-panel.tsx`: o painel Terminal, com os shells do usuário embaixo da área do arquivo, na coluna central, com a faixa de abas (uma por shell, com o `×` de cada uma e o `+` para abrir outro), o botão de esconder e a borda que redimensiona.
- `src/terminal/term.ts`: o tema dos terminais xterm (cores em `theme`).
- `src/terminal/paste.ts`: a colagem de arquivo e imagem no terminal.
- `src/terminal/choice-form.tsx`: o formulário de escolha de ferramenta, modelo e esforço.
- `src/shared/file-icons.ts` e `file-icon.tsx`: a escolha do ícone do tema Symbols pela ordem do VS Code (nome, extensão, linguagem, padrão) e o componente `FileIcon`, usado na árvore, nas abas, no Git, na pesquisa e no arquivo rápido.
- `src/shared/file-tab-actions.ts`: as regras puras de caminho e nome das abas de arquivo.

Regras do layout:

- Imports relativos, sem `index.ts` que reexporta a pasta e sem alias de caminho.
- Uma pasta pode importar de outra.
- `shared/` só recebe o que serve a três ou mais áreas.

## Backend (`src-tauri/src/`)

Plano: cada módulo é uma unidade.

| Caminho | Papel |
| --- | --- |
| `src-tauri/src/lib.rs` | Registra os comandos Tauri. `open_project`/`close_project`, `pick_file`/`pick_save_path` e `read_file_at`/`read_media_at`/`write_file_at_checked` abrem projeto, escolhem caminhos e atendem arquivos absolutos do editor. |
| `src-tauri/src/files.rs` | Comandos de arquivos do projeto e absolutos: `read_file_at`, `read_media_at` e `write_file_at_checked` atendem arquivos escolhidos pelo usuário, com limites, BOM e checagem de revisão. |
| `src-tauri/src/search.rs` | O comando da pesquisa, `search`: os candidatos vêm de `git ls-files` (rastreados e não ignorados, sem `.git`) quando há repositório; sem repositório, vêm da pasta percorrida com a crate `ignore` (respeita `.gitignore` e `.ignore` da pasta e das subpastas, não os de pastas acima; inclui ocultos; não segue links; pula `.git` e `node_modules`), e o texto é casado em Rust pelo motor `regex`, o mesmo da prévia e da substituição. |
| `src-tauri/src/git.rs` | Os comandos do painel Git: status, diff, preparar, commit, fetch, pull, push e branches. Também `git_commit_hover`, que lê os detalhes de um commit sob demanda, e os do editor: `git_file_base`, o texto do arquivo no HEAD e no índice para os marcadores de mudança, e `git_write_index`, que grava no índice o texto de um arquivo para preparar ou tirar do preparo um bloco. E o modo do projeto (com repositório, sem Git, sem repositório ou repositório acima) e o `git init` do botão Inicializar repositório. |
| `src-tauri/src/core.rs` | O núcleo das sessões e projetos: abrir, fechar (com recusa durante sessão), PTYs, lotes, executores e fila. Sem projeto, shells usam a pasta pessoal. Ver [delegação](delegacao.md). |
| `src-tauri/src/platform/` | O único lugar das chamadas ao sistema operacional: Job Objects, consulta de processo, COM e a Lixeira, o Explorer, a abertura de URLs do GitHub, o clipboard de arquivos, a memória física, as pastas do usuário e os nomes dos shells. `windows.rs` implementa; `other.rs` (macOS e Linux) responde "Ainda não suportado neste sistema." ou devolve `None`, conforme o chamador já trata a ausência. Fora daqui não há `windows_sys` nem `std::os::windows`. |
| `src-tauri/src/capacity.rs` | A política automática de simultâneos: teto pela CPU e pela memória física (`platform::memoria`), com a reserva de 2 GiB e 1 GiB por executor. |
| `src-tauri/src/` | Backend: PTYs, sessão e travas do orquestrador, lotes, executores e configuração. |
| `src-tauri/src/delegate.rs` | A lógica de delegação: publica o lote na pasta da sessão, espera o resultado, imprime o lote e grava as respostas às perguntas de executor. |
| `src-tauri/src/mcp.rs` | O servidor MCP da sessão, `orquestrador.exe --mcp <pasta da sessão>`: fala JSON-RPC pela entrada e saída padrão e expõe `delegar` e `aguardar`. |
| `src-tauri/src/hook.rs` | O modo `--hook` do executável (fim de rodada), chamado pelos CLIs dos executores e também pelo orquestrador, no início e no fim de cada rodada. O `--allow` imprime `cli::ALLOW` direto do `main.rs`; o `--mcp`, do orquestrador, está em `mcp.rs`. |
| `src-tauri/src/session.rs` | A sessão do orquestrador: a pasta do funcionamento interno (o orquestrador trabalha na pasta do projeto), `AGENTS.md` com o papel, `mcp.json` e `claude-settings.json` do Claude, e a limpeza das sobras, inclusive das regras do Codex que versões antigas deixavam em `CODEX_HOME` (até 2027-02-01). |
| `src-tauri/src/roles.rs` | Os textos enviados ao orquestrador (no Claude, pelo `AGENTS.md` da sessão com `--append-system-prompt-file`; no Codex, por `developer_instructions`) e a cada executor, antes da tarefa: a única fonte da regra de cada papel, sem trava no app. O texto do orquestrador é o mesmo para Claude Code e Codex; o protocolo das ferramentas (marcadores e próxima chamada) fica nas saídas delas, em `mcp.rs`, `delegate.rs` e `core.rs`. |
| `src-tauri/src/update.rs` | Atualização na abertura: o portão do uso, a rodada pelo plugin oficial do Tauri e os testes do portão. |

O módulo src-tauri/src/format.rs escolhe o motor pela extensão, procura as configurações pelo caminho absoluto e executa os formatadores externos com timeout. Os comandos formatter_info e format_cli são registrados em lib.rs e tipados em src/backend/types.ts.

## Scripts

Agrupados por tipo.

**Checagens puras** (`npm run check`): Node, sem dependências, com `node:test`. Os 9 arquivos rodam um de cada vez.

- `scripts/check-workspace.mjs`: o modelo de abas.
- `scripts/check-editor-store.mjs`: rascunhos, conflitos e recuperação.
- `scripts/check-git-stage.mjs`: o cálculo do texto novo do índice para preparar ou tirar do preparo um bloco do editor (`src/editor/git-stage.ts`), com o índice já diferente do HEAD em outro bloco, linhas apagadas e fim de arquivo sem quebra final.
- `scripts/check-file-tabs.mjs`: caminhos copiados, nomes recusados e “dentro de pasta”.
- `scripts/check-explorer.mjs`: a escolha de ícones (os casos do Explorer do Antigravity) e as decorações do Git na árvore.
- `scripts/check-git-graph.mjs`: o layout do gráfico de commits.
- `scripts/check-paste.mjs`: a colagem de arquivo e imagem no terminal. O efeito real nos CLIs fica no teste ignorado `src-tauri/tests/colagem.rs`.
- `scripts/check-release.mjs`: as regras do fluxo de release.
- `scripts/check-commands.mjs`: o contrato dos comandos, com as chaves de `Commands` (`types.ts`) iguais às de `generate_handler!` (`lib.rs`).

**Checks de interface** (`npm run check:ui`): Chrome headless contra o app simulado (`src/backend/mock.ts`, só no `vite` de desenvolvimento). Todos sobem o Vite na mesma porta (1421, `SMOKE_PORT`), então rodam em sequência. Saem com 0 se todos os cenários passam.

- `scripts/editor-ui-harness.mjs`: loader e helpers dos checks de interface: sobe o Vite na porta `SMOKE_PORT` (padrão 1421), usa o Chromium já instalado, sem instalar nada, e dá a cada cenário um contexto novo (`browser.newContext`) no mesmo Chromium.
- `scripts/check-editor-ui.mjs`: a área central: abas, grupos, fechamento, recuperação, comparação com o disco e atalhos.
- `scripts/check-files-search-ui.mjs`: a árvore e a pesquisa: criar, renomear, recortar e colar; pesquisa com `./pasta`, prévia na lista, troca por arquivo e Substituir Tudo, árvore e só nos editores abertos.
- `scripts/check-git-ui.mjs`: o Git, o diff, o histórico, os conflitos, o painel Terminal e o editor (mover aba, Ctrl+G), e os cenários do painel sem Git (sem repositório, sem Git, repositório acima e subpasta).
- `scripts/check-media-ui.mjs`: a aba de mídia: imagem (Ajustar e Tamanho real) e áudio, com o app simulado em `read_media` (um PNG e um WAV de teste).
- `scripts/check-titlebar-ui.mjs`: os controles da janela na barra de título.
- `scripts/check-editor-style.mjs`: a paridade visual do editor central com o Antigravity (Dark Modern), por estilos computados e geometria, não por pixel. Os valores esperados vêm dos arquivos do Antigravity (ver [interface](interface.md), “Cores e fontes do editor”).
- `scripts/check-git-gutter-ui.mjs`: os marcadores de mudança do Git no editor (tipo, linha, cheio × vazado, geometria, barra de rolagem, minimap, digitação com Desfazer, leitura depois de salvar e a ausência em arquivo não rastreado e em rascunho) e o bloco aberto: linhas antigas, fechar, Restaurar com Ctrl+Z, Preparar e Tirar do preparo (o índice que o app simulado recebeu em `git_write_index`) e a navegação com as setas, Alt+F5 e o menu de contexto.
- `scripts/check-shell-tabs-ui.mjs`: o arrasto das abas do painel Terminal: clique sem mover, nova ordem na faixa, na lista e na vizinha ao fechar, Esc e X sem reordenar, e a faixa rolando com a aba na borda.

**Release**

- `scripts/release.mjs`: as regras do fluxo de release: versões iguais nos manifests, notas completas, o `latest.json` que o app lê e a conferência do instalador. Detalhes em [notas de release](releases/README.md).

**Sincronização de ícones**

- `scripts/sync-file-icons.mjs`: copia o tema de ícones Symbols (MIT, com a LICENSE) da pasta `extensions` de uma IDE do VS Code para `src/assets/symbols`, só os ícones que o app alcança (as chaves que `iconKey` pode devolver), e gera o `theme.json` só com essas chaves; rode de novo para atualizar os ícones.

## Workflows (`.github/workflows/`)

- `ci.yml`: a cada push em `main` e em pull request, o build do front-end, `npm run check` e `cargo test` no Windows; e o job `compilar`, que só faz `cargo check --all-targets` do backend em `ubuntu-latest` e `macos-latest`. Sem segredos, sem assinatura e sem publicação. O `check:ui` não roda aqui, porque sobe o Vite e um Chromium.
- `release.yml`: release por tag: instalador assinado, `latest.json` e conferência, em rascunho.

## Ícone do app

`src-tauri/icons/`: `icon.svg` é a fonte; `icon.ico` (o do executável e do instalador) e `icon.png` saem dele. Para regenerar, rode `npx tauri icon src-tauri/icons/icon.svg -o <pasta temporária>` e copie só `icon.ico` e `icon.png` para cá; sem `-o`, o comando cria arquivos que o projeto não usa.

## O que fica no `localStorage`

O `localStorage` do app guarda estas chaves (nada disso vai ao disco do projeto):

- `orquestrador.editor:<projeto>`: a recuperação da área central (abas, grupos, rascunhos, cursor, rolagem e layout; ver [design system](design-system.md#recuperação-e-comparação)).
- `orquestrador.editor:__sem-projeto__`: arquivos absolutos e rascunhos do modo sem projeto.
- `orquestrador.project.last` e `orquestrador.project.recent`: último projeto e até 10 projetos recentes.
- `orquestrador.search.history` e `orquestrador.search.view`: o histórico de consultas da pesquisa e a escolha entre lista e árvore.
