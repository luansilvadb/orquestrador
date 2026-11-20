# Delegação

Decisões:

- Executores do mesmo lote trabalham na mesma pasta sem isolamento, por decisão de projeto; a independência das tarefas de um lote é responsabilidade do orquestrador.
- O executor é de turno único: o primeiro fim de rodada o encerra com `executor_exit: 0`, sem canal de pergunta nem retomada, e uma pergunta ou parada precoce volta como relato para o orquestrador julgar.
- O relato do executor traz o resultado em uma linha e só os desvios.

Limites aceitos:

- Não há prazo total para um lote.
- Uma chamada cortada por tempo se resolve com `aguardar`, porque os executores não param. Uma chamada cancelada cuja resposta ainda é escrita marca o lote como entregue, e o `aguardar` seguinte não o encontra.
- Se o resultado do lote não puder ser gravado, o `delegar` segue devolvendo `--- EM ANDAMENTO ---`.

Medições:

- 2026-10-14: o canal levou um texto de 31.679 caracteres byte a byte. O limite prático é a saída do modelo: o Haiku não conseguiu escrever um argumento de cerca de 32 mil caracteres numa só chamada.
- 2026-10-14, com servidores MCP descartáveis:
  - **Claude Code 2.1.300**: apóstrofo solto, acentos, crases e `$` chegaram byte a byte. Chamadas de 180 s (sem ajuste) e de 400 s (com `MCP_TOOL_TIMEOUT = 900000`) voltaram normalmente.
  - **Codex 0.162.1**: além de `approve`, testaram-se `auto`, `writes`, `prompt` e `enabled_tools`, que não liberam a chamada; `readOnlyHint` liberaria, mas `delegar` não é só leitura. Com `approve`, o texto chegou exato, e chamadas de 90 s (sem ajuste) e de 400 s (com `tool_timeout_sec = 900`) voltaram normalmente.
- 2026-10-26, com um servidor que responde após 840 s: **Claude Code 2.1.310** voltou em 847 s e **Codex 0.164.0** em 853 s, os dois com a resposta completa.
