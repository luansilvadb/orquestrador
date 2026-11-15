# Arquitetura

App nativo em Rust: a interface em GPUI (com o Kit vendorizado em `vendor/`) e a lógica no crate `orquestrador-core`, num workspace Cargo na raiz. O mesmo `orquestrador.exe` atende também os modos `--mcp`, `--hook` e `--allow`, que os CLIs chamam. Os caminhos são relativos à raiz do repositório.

## Crates

| Crate | Papel |
| --- | --- |
| `crates/app` (`orquestrador-app`, bin `orquestrador`) | A bancada: janela, barra de título, abas de projeto, navegação, painéis, terminais (`terminal/`, sobre `alacritty_terminal`), portão de abertura, atualização (`atualizacao/`), instância única (`instancia.rs`), fechamento, migração do `localStorage` do WebView2 da 0.1.48 (`migracao.rs`) e empacotamento (`empacotamento/`). |
| `crates/core` (`orquestrador-core`) | Backend sem interface (ver abaixo). |
| `crates/ui` | Tema, recursos embutidos (ícones, fontes), widgets comuns, avisos, colagem pt-BR, quebra `pretty` e o `interface.json`. |
| `crates/arvore` | Árvore de arquivos com as decorações do Git. |
| `crates/editor` | Área central: abas, grupos, editor, realce, formatação, Markdown, comparação, recuperação e abrir rápido. |
| `crates/midia` | Aba de mídia: imagem, áudio e vídeo com FFmpeg estático, e picture-in-picture. |
| `crates/pesquisa` | Pesquisa e substituição nos arquivos. |
| `crates/git` | Tela Git: alterações, histórico, grafo, diff e conflito. |
| `crates/hunks` | Cálculo puro dos blocos do Git para preparar e tirar do preparo. |
| `crates/xtask` | Tarefas do projeto (`cargo xtask`). |
| `vendor/` | Cópias alteradas do gpui-kit e do gpui-windows, com os patches em `vendor/README.md`. |

Estado salvo em `%LOCALAPPDATA%\orquestrador\`: `interface.json` (abas, recentes, painéis, pesquisa; uma seção por crate) e `editor-recuperacao.json` (recuperação da área central).

## Backend (`crates/core/src/`)

A raiz guarda a infraestrutura (configuração, processos, atualização e caminhos de abertura); `agents/`, `core/`, `ide/` e `platform/` agrupam os subsistemas, e `ide/` não depende de `agents/` nem de `core/`.

| Caminho | Papel |
| --- | --- |
| `crates/core/src/ide/files.rs` | Comandos de arquivos do projeto e absolutos: `read_file_at`, `read_media_at` e `write_file_at_checked` atendem arquivos escolhidos pelo usuário, com limites, BOM e checagem de revisão. `MEDIA_EXTS` (imagem), `AUDIO_EXTS` e `VIDEO_EXTS` (áudio e vídeo, lidos pelo `orquestrador-midia`) listam as extensões de mídia aceitas. |
| `crates/core/src/ide/search.rs` | O comando da pesquisa, `search`: os candidatos vêm de `git ls-files` (rastreados e não ignorados, sem `.git`) quando há repositório; sem repositório, vêm da pasta percorrida com a crate `ignore` (respeita `.gitignore` e `.ignore` da pasta e das subpastas, não os de pastas acima; inclui ocultos; não segue links; pula `.git` e `node_modules`), e o texto é casado em Rust pelo motor `regex`, o mesmo da prévia e da substituição. |
| `crates/core/src/ide/git.rs` | Os comandos do painel Git: status, diff, preparar, commit, fetch, pull, push e branches. Também `git_commit_hover`, que lê os detalhes de um commit sob demanda, e os do editor: `git_file_base`, o texto do arquivo no HEAD e no índice para os marcadores de mudança, e `git_write_index`, que grava no índice o texto de um arquivo para preparar ou tirar do preparo um bloco. E o modo do projeto (com repositório, sem Git, sem repositório ou repositório acima) e o `git init` do botão Inicializar repositório. |
| `crates/core/src/core/mod.rs` | Um núcleo por projeto, com sua sessão, PTYs, lotes, executores, fila e trava Git. O Core sem projeto mantém os shells da pasta pessoal. Ver [delegação](delegacao.md). |
| `crates/core/src/core/projects.rs` | Registro dos Cores, ids estáveis durante a abertura, configuração compartilhada e eventos identificados pelo projeto. |
| `crates/core/src/platform/` | O único lugar das chamadas ao sistema operacional: Job Objects, consulta de processo, COM e a Lixeira, o Explorer, a abertura de URLs do GitHub, o clipboard de arquivos, a memória física, as pastas do usuário, os nomes dos shells, as pastas do `PATH` registrado, o Windows PowerShell do sistema e o local padrão de cada CLI. `windows.rs` implementa; `other.rs` (macOS e Linux) responde "Ainda não suportado neste sistema." ou devolve `None`, conforme o chamador já trata a ausência. Fora daqui não há `windows_sys` nem `std::os::windows`. |
| `crates/core/src/core/capacity.rs` | Política compartilhada de simultâneos: teto pela CPU e memória física (`platform::memoria`), com reserva de 2 GiB e 1 GiB por executor, e fila global FIFO. |
| `crates/core/src/agents/delegate.rs` | A lógica de delegação: publica o lote na pasta da sessão, espera o resultado, imprime o lote e grava as respostas às perguntas de executor. |
| `crates/core/src/agents/mcp.rs` | O servidor MCP da sessão, `orquestrador.exe --mcp <pasta da sessão>`: fala JSON-RPC pela entrada e saída padrão e expõe `delegar` e `aguardar`. |
| `crates/core/src/agents/hook.rs` | O modo `--hook` do executável, chamado pelos CLIs dos executores no fim de cada rodada. O `--allow` imprime `cli::ALLOW` direto do `main.rs`; o `--mcp`, do orquestrador, está em `mcp.rs`. |
| `crates/core/src/agents/session.rs` | A sessão do orquestrador: a pasta do funcionamento interno (o orquestrador trabalha na pasta do projeto), `AGENTS.md` com o papel, `mcp.json` e `claude-settings.json` do Claude, e a limpeza das sobras, inclusive das regras do Codex que versões antigas deixavam em `CODEX_HOME` (até 2027-02-01). |
| `crates/core/src/agents/roles.rs` | Os textos enviados ao orquestrador (no Claude, pelo `AGENTS.md` da sessão com `--append-system-prompt-file`; no Codex, por `developer_instructions`) e a cada executor, antes da tarefa: a única fonte da regra de cada papel, sem trava no app. O texto do orquestrador é o mesmo para Claude Code e Codex; o protocolo das ferramentas (marcadores e próxima chamada) fica nas saídas delas, em `mcp.rs`, `delegate.rs` e `core.rs`. |
| `crates/core/src/clis.rs` | A etapa dos CLIs no portão: localiza `claude.exe` e `codex.exe`, instala pelo script oficial, confere `--version`, roda `update` e devolve os caminhos absolutos; no debug, só localiza. |
| `crates/core/src/open_paths.rs` | Extrai caminhos dos argumentos; `crates/app/empacotamento/hooks.nsh` registra e remove menus e extensões no HKCU. |

O módulo crates/core/src/ide/format.rs escolhe o motor pela extensão, procura as configurações pelo caminho absoluto e executa os formatadores externos com timeout. O editor o chama por `orquestrador-editor::formatar`.

`crates/core/src/proc.rs` é o único lugar que roda comando externo com prazo (`saida_com_prazo`), e o prazo cobre execução, escrita na entrada e leitura dos dois pipes. No Windows o processo nasce suspenso, entra no Job Object antes de qualquer descendente rodar e só então é liberado; no macOS e no Linux, o comando abre um grupo de processos próprio. No estouro, a árvore inteira é encerrada e o que saiu até ali volta com `estourou`. `crates/core/tests/prazo.rs` cobre pai que sai e deixa descendente segurando os pipes, pai vivo, entrada de 16 MiB sem destino e saída grande nos dois pipes.

Cada comando dependente de projeto recebe o id devolvido por `open_project` (ou `choose_project`); reabrir a mesma raiz normalizada devolve o id existente. Os eventos do Core também carregam `project`; ids de terminal são locais ao Core e a interface chaveia por `(project, id)`. O limite de executores é global, com fila FIFO entre projetos. Opções e escolhas padrão ficam na configuração global; a escolha dos executores, cada sessão em execução, seus shells e sua trava Git pertencem ao Core do projeto. O Core sem projeto é separado e sobrevive à abertura e ao fechamento de projetos.

## Testes

Ficam nos módulos (`#[cfg(test)]`) e em `crates/*/tests/`. As telas são testadas numa janela sem tela do Kit (`gpui_kit::test`). `crates/app/tests/lote.rs` sobe o `orquestrador.exe --mcp` com o CLI falso de `crates/app/examples/fake_cli.rs`; `crates/app/tests/real.rs` e `crates/core/tests/colagem.rs` usam os CLIs reais e ficam ignorados (ver [desenvolvimento](desenvolvimento.md)).

## Tarefas em Rust (`crates/xtask`, `cargo xtask`)

- `cargo xtask release <comando>`: as regras do fluxo de release: versões iguais no `[workspace.package]` e no `Cargo.lock`, notas completas, o `latest.json` que o app lê e a conferência do instalador. Testes: `cargo test -p xtask`. Detalhes em [notas de release](releases/README.md).
- `cargo xtask limpar-target`: `cargo clean` quando `target` passa do teto (30 GB; `ORQUESTRADOR_TARGET_MAX_GB`).
- `cargo xtask medir`: medição de desempenho ([desempenho do GPUI](desempenho-gpui.md)).
- `cargo xtask despejar`: o gerador de saída das medições (também do exemplo `eco_pty`).

## Workflows (`.github/workflows/`)

- `ci.yml`: a cada push em `main` e em pull request, `cargo test --workspace` e `cargo clippy` no Windows, e `cargo check -p orquestrador-core` em `ubuntu-latest` e `macos-latest`. Sem segredos, sem assinatura e sem publicação.
- `release.yml`: release por tag: instalador assinado, `latest.json` e conferência, em rascunho.

## Ícone do app

`crates/app/empacotamento/icon.ico` é o do instalador; a marca da janela é `app-mark.svg`, em `crates/ui/assets/`.
