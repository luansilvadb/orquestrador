# Delegação

O orquestrador delega por duas ferramentas MCP de um servidor da própria sessão, `orquestrador.exe --mcp <pasta da sessão>` (`src-tauri/src/mcp.rs`), que fala JSON-RPC pela entrada e saída padrão:

- `delegar { tarefas: [textos] }`: publica o lote e espera o resultado.
- `aguardar {}`: volta a esperar o lote mais recente ainda não entregue, sem publicar nada.

Os argumentos vão em JSON. Cada tarefa chega ao executor como está: aspas duplas, barras, acentos, crases, `$`, apóstrofos e quebras de linha passam sem escape, e o canal não tem limite de tamanho. O canal e o texto do orquestrador (`src-tauri/src/roles.rs`) são os mesmos para o Claude Code e para o Codex.

Cada chamada espera até 5 minutos. A resposta termina de um de dois jeitos:

- `--- FIM ---`: o lote acabou. A saída traz as seções `--- EXECUTOR i/N ---`, `executor_exit` e `--- GIT ---` (abaixo).
- `--- EM ANDAMENTO ---`: a janela passou e os executores continuam. O orquestrador chama `aguardar`.

Uma chamada interrompida ou que deu erro de tempo também se resolve com `aguardar`, porque os executores não param. Erros do próprio comando (lista vazia, tarefa em branco, nenhum lote aguardando, sessão encerrada) voltam como erro da ferramenta, sem publicar nada.

Cada tarefa abre um executor novo, sem memória das tarefas anteriores. Todos trabalham na mesma pasta; quem garante que as tarefas de um lote não se cruzam é o orquestrador, pelas instruções de `src-tauri/src/roles.rs`.

Quando o lote termina, a saída traz, em UTF-8:

- Para cada tarefa, na ordem: `--- EXECUTOR i/N ---`, o título, a última resposta do executor e `executor_exit` (`0`, ou o motivo seguido de `(FALHOU)`).
- `--- GIT ---` com a saída de `git status --porcelain --untracked-files=all`: tudo o que está alterado no repositório naquele momento, de todos os executores e também o que já estava pendente antes. São até 200 linhas; passando disso, a seção traz a linha “(… e mais N arquivos; rode git status para ver todos)”. Sem Git, a seção tem só uma linha, sem a do `git status`: “(sem Git: a pasta não é um repositório Git)”, “(sem Git: o Git não foi encontrado neste computador)” ou “(sem Git: a pasta está dentro do repositório <raiz>, que não é a raiz do projeto)”. Nesse caso, a conferência fica só nos arquivos, que o texto do orquestrador (`src-tauri/src/roles.rs`) trata como a prova.
- `--- FIM ---`: a linha que fecha toda saída completa.

Limites:

- Tamanho: o canal levou um texto de 31.679 caracteres, byte a byte (medido em 2026-10-14). O limite prático é a saída do modelo, não o canal: o Haiku não conseguiu escrever um argumento de cerca de 32 mil caracteres numa só chamada.
- Tarefa vazia ou só de espaços é recusada e não publica nada.
- Medido em 2026-10-14 com servidores MCP descartáveis:
  - **Claude Code 2.1.300**: um texto com apóstrofo solto, acentos, crases e `$` chegou byte a byte. Chamadas de 180 s (sem ajuste) e de 400 s (com `MCP_TOOL_TIMEOUT = 900000`) voltaram normalmente.
  - **Codex 0.162.1**, com `approval_policy` `never`: toda chamada MCP é negada (`MCP tool call requires approval, but approval policy is never`), a menos que o servidor tenha `default_tools_approval_mode = "approve"`. Os valores `auto`, `writes` e `prompt` e o `enabled_tools` não liberam. A anotação `readOnlyHint` liberaria, mas `delegar` não é só leitura. Com `approve`, o texto chegou exato. Chamadas de 90 s (sem ajuste) e de 400 s (com `tool_timeout_sec = 900`) voltaram normalmente. Ver `codex_mcp`, em `src-tauri/src/cli.rs`.

O fim da rodada de um executor é detectado pelo hook `Stop` do Claude Code e pelo `notify` do Codex, que chamam o próprio executável do app.

Limite: interromper a rodada não manda sinal em nenhum dos dois CLIs (nem o `Stop`, nem o `notify` do Codex; medido com o Esc). Por isso o app conta o Esc sozinho no terminal do orquestrador, com ele ocupado, como fim da rodada, sem distinguir se o Esc de fato interrompeu algo. Outra forma de interromper, como o Ctrl+C, não foi medida nem é reconhecida: depois dela, o app continua vendo o orquestrador ocupado até o próximo `Stop`, um Esc ou o fim do orquestrador.
