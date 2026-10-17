# Arquitetura

O front-end (React) fica em `src/` e o backend (Rust, Tauri) em `src-tauri/src/`. O mesmo `orquestrador.exe` atende também os modos `--mcp`, `--hook` e `--allow`, que os CLIs chamam. Mapa dos arquivos principais; os caminhos são relativos à raiz do repositório.

| Caminho | Papel |
| --- | --- |
| `src/` | Frontend em React: tela de atualização, a tela principal (árvore de pastas, área do arquivo, painel Terminal e painel do Orquestrador), a tela Git, terminais (xterm), formulário de escolha. `mock.ts` é um backend simulado, só em `npm run dev` fora do Tauri; ele simula os modos sem Git pela URL (`?git=no-repo`, `no-git`, `repo-above`, e `?subfolder=1`, ver [desenvolvimento](desenvolvimento.md)). O design system (Lobe UI) está em `docs/design-system.md`. |
| `src/terminal-screen.tsx` | O painel do Orquestrador, à direita da tela principal: faixa de abas (orquestrador e um executor por aba), a barra da aba ativa e os terminais, com o da aba ativa visível. |
| `src/file-view.tsx` | O editor de texto (CodeMirror com as cores do app e as medidas do Monaco, guias de recuo e minimap), usado pela área central: não lê nem grava o disco; guarda o estado do CodeMirror por arquivo e reporta cursor e rolagem. |
| `src/media-view.tsx` | A aba de mídia da área central: imagem (Ajustar e Tamanho real) e áudio (controles nativos). Os bytes vêm de `read_media` e entram num blob local; não lê texto e não grava. |
| `src/breadcrumbs.tsx` | Os breadcrumbs embaixo das abas de arquivo: pastas e arquivo, com o seletor do conteúdo da pasta. |
| `src/source-control-icon.tsx` | O ícone do Git na coluna de ícones: o codicon `source-control` do Antigravity/VS Code (de `@vscode/codicons`, CC BY 4.0, licença em `src/assets/codicons/LICENSE`). |
| `src/git-graph.ts` | O layout do gráfico de commits da barra lateral do Git: faixas, linhas de merge e cores. Testado por `scripts/check-git-graph.mjs`. |
| `src/bracket-colors.ts` | A cor dos parênteses, colchetes e chaves pelo nível de aninhamento, lida da árvore de sintaxe. |
| `src/editor-workspace.tsx` | A área central: até dois grupos lado a lado, com abas de arquivo, diff, commit e conflito; a comparação com o disco antes de gravar; e a recuperação por projeto (rascunhos, cursor, rolagem e layout). Recuperações de versões antigas que trazem abas `issues` ou `new-issue` as descartam ao ler. |
| `src/workspace-model.ts` | O modelo puro das abas e dos grupos (prévia, fixadas, fechamento com confirmação, reabertura e carga do estado salvo). Sem React. |
| `src/workspace-store.ts` | Buffers, rascunhos, conflitos e o texto da recuperação, validado ao ler. Sem React e sem disco. |
| `src/quick-open.tsx` | O arquivo rápido (Ctrl+P): campo e lista do `backend.quickOpen`, com Enter, Shift+Enter e Ctrl+Enter. |
| `src/git-guard.ts` | A trava das operações Git da interface, compartilhada pela barra lateral e pelo editor. |
| `scripts/check-workspace.mjs` | Checagem pura do modelo de abas. |
| `scripts/check-editor-store.mjs` | Checagem pura dos rascunhos, conflitos e recuperação. |
| `scripts/check-file-tabs.mjs` | Checagem pura de caminhos copiados, de nomes recusados e de “dentro de pasta”. |
| `scripts/check-explorer.mjs` | Checagem pura da escolha de ícones (os casos do Explorer do Antigravity) e das decorações do Git na árvore. |
| `scripts/sync-file-icons.mjs` | Copia o tema de ícones Symbols (MIT, com a LICENSE) da pasta `extensions` de uma IDE do VS Code para `src/assets/symbols` e gera o `theme.json`; rode de novo para atualizar os ícones. |
| `src-tauri/icons/` | O ícone do app: `icon.svg` é a fonte; `icon.ico` (o do executável e do instalador) e `icon.png` saem dele. Para regenerar, rode `npx tauri icon src-tauri/icons/icon.svg -o <pasta temporária>` e copie só `icon.ico` e `icon.png` para cá; sem `-o`, o comando cria arquivos que o projeto não usa. |
| `scripts/editor-ui-harness.mjs` | Loader e helpers dos checks de interface: sobe o Vite na porta `SMOKE_PORT` (padrão 1421), usa o Chromium já instalado, sem instalar nada, e dá a cada cenário um contexto novo (`browser.newContext`) no mesmo Chromium. |
| `scripts/check-editor-ui.mjs` | Check de interface da área central: abas, grupos, fechamento, recuperação, comparação com o disco e atalhos. |
| `scripts/check-files-search-ui.mjs` | Check de interface da árvore e da pesquisa: criar, renomear, recortar e colar; pesquisa com `./pasta`, prévia na lista, troca por arquivo e Substituir Tudo, árvore e só nos editores abertos. |
| `scripts/check-media-ui.mjs` | Check de interface da aba de mídia: imagem (Ajustar e Tamanho real) e áudio, com o app simulado em `read_media` (um PNG e um WAV de teste). |
| `scripts/check-git-ui.mjs` | Check de interface do Git, do diff, do histórico, dos conflitos, do painel Terminal e do editor (mover aba, Ctrl+G), e os cenários do painel sem Git (sem repositório, sem Git, repositório acima e subpasta). |
| `src/shell-panel.tsx` | O painel Terminal: os shells do usuário embaixo da área do arquivo, na coluna central, com a faixa de abas (uma por shell, com o `×` de cada uma e o `+` para abrir outro), o botão de esconder e a borda que redimensiona. |
| `src/explorer.tsx` | A coluna de ícones da navegação, a árvore de pastas (pasta abre e fecha; arquivo abre na área central; menu com renomear, excluir, criar, copiar, recortar, colar e mover, inclusive em lote; barra com as ações da view e seção do projeto) e a barra de status com a branch, o Sincronizar e o botão Terminal. |
| `src/file-icons.ts`, `src/file-icon.tsx` | A escolha do ícone do tema Symbols pela ordem do VS Code (nome, extensão, linguagem, padrão) e o componente `FileIcon`, usado na árvore, nas abas, no Git, na pesquisa e no arquivo rápido. |
| `src/explorer-decorations.ts` | As decorações do Git na árvore: cor e letra por arquivo, herança na pasta e o cinza do ignorado, com as cores do tema escuro do VS Code. |
| `src-tauri/src/files.rs` | Os comandos de arquivos: `list_dir`, que lista uma pasta do projeto (com `ignored` por entrada, num `git check-ignore` por pasta), e `read_file`, que lê o arquivo (binário e acima de 1 MiB voltam sem texto), `read_media`, que devolve os bytes de imagem ou áudio para a aba de mídia (só as extensões de `MEDIA_EXTS`, sem SVG nem HTML, e até 32 MiB), e `write_file_checked`, que só grava se o arquivo não mudou desde a leitura. |
| `src/search.tsx` | A pesquisa de texto nos arquivos, na faixa da árvore, no layout do Antigravity: o cabeçalho, o campo com as três opções e o histórico, o Substituir (com Preservar maiúsculas), os filtros de caminho, a lista ou árvore de ocorrências (com dispensar e menu de contexto) e a prévia da troca na própria lista. |
| `src-tauri/src/search.rs` | O comando da pesquisa, `search`: os candidatos vêm de `git ls-files` (rastreados e não ignorados, sem `.git`) quando há repositório; sem repositório, vêm da pasta percorrida com a crate `ignore` (respeita `.gitignore` e `.ignore` da pasta e das subpastas, não os de pastas acima; inclui ocultos; não segue links; pula `.git` e `node_modules`), e o texto é casado em Rust pelo motor `regex`, o mesmo da prévia e da substituição. |
| `src/git.tsx` | A barra lateral do Git (alterações e histórico). Os comandos vêm do backend, e as operações passam pela trava compartilhada (`src/git-guard.ts`). |
| `src-tauri/src/git.rs` | Os comandos do painel Git: status, diff, preparar, commit, fetch, pull, push e branches. Também o modo do projeto (com repositório, sem Git, sem repositório ou repositório acima) e o `git init` do botão Inicializar repositório. |
| `src-tauri/src/core.rs` | O núcleo da sessão: PTYs, lotes, executores e a fila. Ver [delegação](delegacao.md). |
| `src-tauri/src/capacity.rs` | A política automática de simultâneos: teto pela CPU e pela memória física (`GlobalMemoryStatusEx`), com a reserva de 2 GiB e 1 GiB por executor. |
| `src-tauri/src/` | Backend: PTYs, sessão e travas do orquestrador, lotes, executores e configuração. |
| `src-tauri/src/delegate.rs` | A lógica de delegação: publica o lote na pasta da sessão, espera o resultado, imprime o lote e grava as respostas às perguntas de executor. |
| `src-tauri/src/mcp.rs` | O servidor MCP da sessão, `orquestrador.exe --mcp <pasta da sessão>`: fala JSON-RPC pela entrada e saída padrão e expõe `delegar` e `aguardar`. |
| `src-tauri/src/hook.rs` | O modo `--hook` do executável (fim de rodada), chamado pelos CLIs dos executores e também pelo orquestrador, no início e no fim de cada rodada. O `--allow` imprime `cli::ALLOW` direto do `main.rs`; o `--mcp`, do orquestrador, está em `mcp.rs`. |
| `src-tauri/src/session.rs` | A sessão do orquestrador: pasta, `AGENTS.md`, `mcp.json` e `claude-settings.json` do Claude, e a limpeza das sobras, inclusive das regras do Codex que versões antigas deixavam em `CODEX_HOME` (até 2027-02-01). |
| `src-tauri/src/roles.rs` | Os textos enviados ao orquestrador (no `AGENTS.md` da sessão) e a cada executor, antes da tarefa: a única fonte da regra de cada papel, sem trava no app. O texto do orquestrador é o mesmo para Claude Code e Codex; o protocolo das ferramentas (marcadores e próxima chamada) fica nas saídas delas, em `mcp.rs`, `delegate.rs` e `core.rs`. |
| `src-tauri/src/update.rs` | Atualização na abertura: o portão do uso, a fonte do plugin oficial e os testes dos cenários. |
| `scripts/release.mjs` | As regras do fluxo de release: versões iguais nos manifests, notas completas, o `latest.json` que o app lê e a conferência do instalador. Detalhes em [notas de release](releases/README.md). |
| `.github/workflows/release.yml` | Release por tag: instalador assinado, `latest.json` e conferência, em rascunho. |

O que fica no `localStorage` do app (nada disso vai ao disco do projeto):

- `orquestrador.editor:<projeto>`: a recuperação da área central (abas, grupos, rascunhos, cursor, rolagem e layout; ver [design system](design-system.md#recuperação-e-comparação)).
- `orquestrador.search.history` e `orquestrador.search.view`: o histórico de consultas da pesquisa e a escolha entre lista e árvore.
