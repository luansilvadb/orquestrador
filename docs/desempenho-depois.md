# Desempenho: depois das otimizações

Remedição com as mudanças não commitadas do working tree (xterm com `@xterm/addon-webgl` no terminal visível, saída dos terminais por `tauri::ipc::Channel` binário com coalescência de 4 ms, `term_write` fora do lock do Core, correções de updates retidos em hooks React) contra a base de `docs/desempenho-base.md`. Duas rodadas completas, `node scripts/medir-desempenho.mjs`, mesmo cenário, mesma carga (4 terminais, 6000 linhas/s cada), mesmos momentos, 250 amostras de latência por condição. Resultados brutos: `%TEMP%\orq-medir-depois1\` e `%TEMP%\orq-medir-depois2\` (`resultado.json`, `renderer.heapsnapshot`, `heap-analise.json`).

## Mudança de método (script)

Com WebGL o terminal visível não tem `.xterm-rows` no DOM, e a base detectava eco e fim da saída por esse DOM. O script agora tem dois métodos, escolhidos a cada `watch()`:

- `dom` (o da base): se o terminal visível tem `.xterm-rows`, usa MutationObserver e `textContent`. Continua funcionando para builds sem WebGL.
- `buffer` (novo): se o visível está em WebGL, usa o buffer do xterm e o evento `onRender`.
  - Gancho no app: `src/terminal/term.ts` chama `globalThis.__orqMedir?.(id, term)` logo após `term.open()`. Sem a função definida, nada acontece (uma leitura de propriedade por terminal criado).
  - O script define `__orqMedir` por `Page.addScriptToEvaluateOnNewDocument` e recarrega a página (`page.reload()`); `--sem-gancho` desliga. A abertura do cenário (1186/1176 ms) é medida antes do reload; as aberturas dedicadas rodam sem gancho.
  - Eco "renderizado": `onRender` do xterm do terminal visível cobre a linha de tela do prompt e o buffer dessa linha já começa por prompt + letra. t0 = `keydown` (`event.timeStamp`), igual à base. "Quadro" = o rAF seguinte, como na base.
  - "Saída consumida": a última linha do gerador aparece nas últimas 60 linhas do buffer do terminal (processada pelo parser). Na base era "presente no DOM renderizado". Diferença: aqui conta o processamento, não o desenho.
  - "Quieto" (esperas entre amostras): última escrita processada (`onWriteParsed`) do terminal ativo, no lugar de mutações do DOM (o piscar do cursor no WebGL dispara `onRender` e impediria a espera).
- Nas chaves do JSON, `eco_dom_ms` continua sendo o eco renderizado (mesmo nome, para comparar); `metodo` diz qual foi usado. Nas duas rodadas: `buffer`.
- Verificação do WebGL ativo: no DOM da página há 39 linhas de xterm (3 terminais escondidos x 13 linhas, no renderizador DOM) e nenhuma do visível.
- Sanidade do método: sem carga, p50 de 22,3 a 22,5 ms contra 23,2 ms da base por DOM. Não prova equivalência; sugere que o piso está fora do renderizador.

## Ambiente

- Mesma máquina da base (Ryzen 9 5900X, 15,9 GB, RTX 2060 SUPER, monitor 240 Hz, WebView2 156). Uso diário: instância instalada do app (PIDs 6172, 30308, 34868), CLIs `claude` e esta sessão rodando em paralelo, janela de 1400x900 visível durante a medição.
- RAM livre do sistema antes de cada rodada: 7717 MB (rodada 1) e 8006 MB (rodada 2), de 16292 MB. A base não registrou. Nenhuma rodada chegou perto de falta de memória (pico do app 1,2 GB). Build, testes e medição rodaram um de cada vez.
- Exe: `CARGO_PROFILE_DEV_OPT_LEVEL=3 npx tauri build --debug --no-bundle` (exit 0), feito imediatamente antes de medir. Mesmo perfil da base.
- Proteções: histórico do PSReadLine com o mesmo SHA-256 antes e depois (`322648f4…95cb`; o script removeu as 4 linhas que digitou em cada rodada); PIDs da instância instalada iguais antes e depois, `config.json` dela intacto; nenhum processo restante da árvore medida; nenhum CLI de IA aberto (só 4 pwsh e o gerador node).

## Resultados (base | depois 1 | depois 2)

### Abertura (ms, do spawn ao marco)

| Condição | Base (mediana; mín-máx) | Depois 1 | Depois 2 |
|---|---|---|---|
| Perfil WebView2 novo (5) | 1258; 1190-1400 | 1189; 1157-1413 | 1215; 1158-1330 |
| Perfil existente (5) | 1278; 1189-1328 | 1163; 1135-1252 | 1168; 1129-1175 |
| Execução do cenário | 1203 | 1186 | 1176 |
| Primeiro paint, perfil novo (mediana) | ~615 | 567 | 583 |

Sem diferença que ultrapasse a dispersão da base (a faixa de cada condição se sobrepõe), com leve tendência a menos. A abertura não usa o terminal.

### RAM privada por processo (MB): base | depois 1 | depois 2

| Processo | Ocioso | Durante (15 s de saída) | 30 s depois | Após GC forçado |
|---|---|---|---|---|
| orquestrador.exe | 7 / 8 / 8 | 84 / 86 / 84 | 84 / 85 / 85 | 84 / 85 / 84 |
| WebView2 browser | 47 / 47 / 47 | 47 / 54 / 53 | 45 / 54 / 54 | 49 / 56 / 56 |
| **WebView2 renderer** | **418 / 446 / 449** | **928 / 796 / 785** | **2024 / 885 / 976** | **581 / 644 / 656** |
| WebView2 gpu | 86 / 87 / 87 | 90 / 89 / 90 | 87 / 88 / 88 | 102 / 107 / 109 |
| WebView2 utility | 20 / 20 / 20 | 20 / 24 / 25 | 20 / 24 / 25 | 21 / 25 / 25 |
| WebView2 crashpad | 3 / 3 / 3 | 3 / 3 / 3 | 3 / 3 / 3 | 3 / 3 / 3 |
| **Total do app, privada** | **582 / 611 / 614** | **1172 / 1051 / 1039** | **2264 / 1140 / 1230** | **839 / 921 / 934** |
| Total, working set | 738 / 776 / 777 | 1296 / 1212 / 1196 | 2376 / 1306 / 1382 | 949 / 1074 / 1077 |
| Total, working set privado | 433 / 463 / 468 | 1007 / 893 / 891 | 2076 / 983 / 1066 | 648 / 750 / 760 |

- Pico da série durante a saída (amostras a cada ~3 s): base 2221 MB (t = 42 s); depois 1123 MB (t = 27 s) e 1115 MB (t = 45,5 s).
- Renderer, working set privado (MB): ocioso 350 / 377 / 380; durante 864 / 728 / 728; 30 s depois 1929 / 815 / 899; após GC 495 / 578 / 588.
- Fora do total do app: shells (4 pwsh + conhost) 132 MB em repouso e 269-271 durante (base: 124 em repouso); contadores de GPU do Windows: 30 MB dedicados e 3-4 compartilhados no processo gpu (base: 32 e 3). O WebGL não aumentou o uso de GPU de forma mensurável por esses contadores.
- CPU média na fase B (1 núcleo = 100%), base | depois 1 | depois 2: renderer 123 / 101 / 113; orquestrador.exe 28 / 32 / 31; browser 30 / 51 / 50; gpu 20 / 12 / 13. O processo browser do WebView2 gasta mais CPU (palpite: é o lado de IPC do Channel binário).

### Heap JS do renderer (MB, total / usado)

| Momento | Base | Depois 1 | Depois 2 |
|---|---|---|---|
| Ocioso | 36 / 32 | 33 / 27 | 39 / 31 |
| Durante | n/d | 109 / 49 | 110 / 61 |
| 30 s depois | 607 / 547 | 61 / 48 | 62 / 51 |
| Após GC | 55 / 53 | 36 / 34 | 36 / 34 |

No ponto de 30 s depois, fora do heap JS ficam 824 MB (92%) na rodada 1 e 914 MB (94%) na 2 (base: 1417 MB, 70%). A memória que sobra não é JS: é Blink e alocadores. Dump do Chromium após GC (footprint do renderer 642 e 649 MB): `partition_alloc` 285 MB (buffers 246-249), `blink_gc` 198-200, `malloc` 54. Na base, após GC: footprint 575, `blink_gc` 210, `partition_alloc` 173, `malloc` 48. Ou seja: `partition_alloc` subiu ~110 MB; palpite: texturas e buffers do atlas WebGL e do canal binário do terminal visível, mais o DOM dos três escondidos.

### Latência de digitação (ms, 250 amostras cada)

| Condição | Medida | | p50 | p95 | p99 | máx |
|---|---|---|---|---|---|---|
| Sem saída concorrente | eco renderizado | base | 23,2 | 24,7 | 26,2 | 26,7 |
| | | depois 1 | 22,5 | 24,3 | 26,2 | 30,8 |
| | | depois 2 | 22,3 | 24,1 | 24,9 | 26,8 |
| | até o quadro seguinte | base | 26,2 | 27,7 | 29,0 | 29,3 |
| | | depois 1 | 26,2 | 27,8 | 29,2 | 32,0 |
| | | depois 2 | 26,2 | 27,8 | 28,6 | 36,3 |
| 3 terminais despejando (18 mil linhas/s) | eco renderizado | base | 24,1 | 36,6 | 46,3 | 52,4 |
| | | depois 1 | 26,2 | 126,8 | 160,5 | 201,8 |
| | | depois 2 | 25,7 | 42,5 | 82,8 | 110,1 |
| | até o quadro seguinte | base | 27,6 | 40,3 | 49,7 | 59,1 |
| | | depois 1 | 29,6 | 147,3 | 176,9 | 232,4 |
| | | depois 2 | 29,2 | 46,6 | 93,3 | 122,9 |

- Nenhuma amostra sem eco no prazo (20 s). A carga esteve ativa nas 250 amostras (conferido no início, no meio e no fim: os 3 geradores não tinham sido consumidos).
- Amostras acima de 50 ms sob carga: base 2; depois 1: 29; depois 2: 7. Na rodada 1, 20 amostras consecutivas (índices 83 a 102, ~5 s) ficaram entre 95 e 202 ms e depois voltaram ao normal: um episódio de travamento da thread principal ou de pausa, de causa não identificada. Excluindo as amostras acima de 50 ms (exclusão a posteriori, só para ver o corpo da distribuição), as duas rodadas dão p50 25,8 / 25,6, p95 36,6 / 36,8, p99 44,8 / 46,1. Esse corpo é parecido com a base (24,1 / 36,6 / 46,3 com tudo incluído), mas a cauda piorou.
- Menor eco observado: 13,0 ms (sem carga, rodada 1), 14,6 (rodada 2); sob carga 17,9 e 16,3. Nenhuma amostra sob carga ≤ 16 ms.

### Quadros durante a saída (fase B)

| | fps médio | tempo de quadro p50 / p95 / p99 / máx | quadros > 50 ms |
|---|---|---|---|
| Base | 158 | 4,2 / 12,5 / 16,7 / 75 ms | 1 |
| Depois 1 | 214,8 | 4,2 / 4,4 / 24,9 / 66,6 ms | 4 |
| Depois 2 | 208,2 | 4,2 / 8,3 / 20,8 / 66,7 ms | 6 |

Referência em ocioso: 240 fps, 4,2 ms (as três medições). Tarefas longas (> 50 ms) no período: 2 e 1 (a base não registrou). O fps médio e o p95 melhoraram; o p99 e a contagem de quadros acima de 50 ms ficaram piores ou iguais.

### Variação entre as duas rodadas

| Métrica | Rodada 1 | Rodada 2 | Diferença |
|---|---|---|---|
| Total do app, ocioso (privada) | 611 MB | 614 MB | 0,5% |
| Total, durante | 1051 MB | 1039 MB | 1% |
| Total, 30 s depois | 1140 MB | 1230 MB | 8% |
| Total, após GC | 921 MB | 934 MB | 1% |
| Renderer, 30 s depois | 885 MB | 976 MB | 10% |
| Eco sem carga, p95 | 24,3 ms | 24,1 ms | 1% |
| Eco com carga, p95 | 126,8 ms | 42,5 ms | 3x (episódio de ~5 s na rodada 1) |
| Eco com carga, p99 | 160,5 ms | 82,8 ms | 2x |
| fps durante | 214,8 | 208,2 | 3% |
| Abertura, mediana (perfil novo) | 1189 ms | 1215 ms | 2% |

A latência com carga na cauda varia muito entre rodadas; a memória, pouco (o ponto de 30 s é o mais variável, como na base).

## Diagnóstico

### Origem do lixo (fase C, 4 x 120 mil linhas)

Perfil de alocações do V8: 1691 e 1735 MB alocados (base: 2294).

| Função (bundle) | MB (rodada 1 / 2) | % | Base % |
|---|---|---|---|
| `createRow` (renderizador DOM do xterm) | 1249 / 1285 | 74 | 77 |
| `resolve` | 104 / 117 | 6-7 | n/d |
| `_sync` (xterm) | 102 / 100 | 6 | 3 |
| `setScrollPosition` | 63 / 63 | 4 | n/d |

`createRow` continua dominando as alocações: os 3 terminais escondidos seguem no renderizador DOM e recebem a mesma saída. CPU da thread principal (26 s): ocioso 44-47%, `(program)` 36-39%, coletor de lixo 7%, `createRow` 0,9-1,2% (base 4,6%), `replaceChildren` 1,1-1,3% (base 4,6%).

### Heap vivo (snapshot após 30 s e GC): 107,0 MB, 915 mil nós (base 118,1 MB, 886 mil)

| Dono | Base MB | Depois 1 | Depois 2 |
|---|---|---|---|
| xterm: buffers e estado dos 4 terminais | 36,7 | 37,2 | 37,1 |
| V8: código compilado e fonte dos scripts | 20,9 | 21,2 | 21,2 |
| React: fibras, estado e filas de atualização | 20,3 | 1,9 | 1,9 |
| shiki / Oniguruma: memória do WASM | 16,5 | 16,5 | 16,5 |
| Blink nativo | 10,5 | 16,8 | 16,5 |
| Outros objetos | 7,1 | 7,5 | 7,5 |
| Strings sem dono identificado | 6,0 | 6,0 | 6,0 |
| CodeMirror | 0,2 | 0,2 | 0,2 |

A fila de updates pendentes do `useGitBase` (18,3 MB na base) desapareceu: o nó com `pending`/`lastRenderedReducer` retém 0 MB. O aumento de Blink nativo (+6 MB) é em grande parte `blink::InspectorNetworkAgent` (7,1 MB, `NetworkResourcesData`), que guarda o conteúdo dos scripts enquanto o CDP está anexado. Palpite: aparece agora por causa do `Page.enable` e do reload do gancho; é custo da medição, não do app.

Dez maiores consumidores (tamanho retido; os aninhados se sobrepõem), rodada 2 (a 1 é igual até a 2ª casa):

| # | Consumidor | Retido MB | Parte do app |
|---|---|---|---|
| 1 | Escopo do módulo que guarda a instância do Oniguruma | 19,3 | shiki |
| 2 | Objeto do WASM do Oniguruma (`HEAPU8`, ArrayBuffer de 16 MB) | 16,5 | shiki |
| 3 | `Terminal` do xterm #1 (CircularList de 10 mil linhas, `Uint32Array`) | 9,1 | xterm |
| 4 | `Terminal` #2 | 9,1 | xterm |
| 5 | `Terminal` #3 | 9,1 | xterm |
| 6 | `Terminal` #4 | 9,1 | xterm |
| 7 | Script `index-*.js` (fonte de 8,8 MB como string externa) | 8,9 | bundle principal |
| 8 | `blink::InspectorNetworkAgent` | 7,1 | instrumento (CDP) |
| 9 | `SourceTextModule` | 1,6 | bundle |
| 10 | `BytecodeArray` com pool de constantes | 1,4 | bundle |

## Conclusão

Metas:

- **Meta 1, renderer < 250 MB: não atingida em nenhum momento.** Private Bytes do renderer: ocioso 446-449 MB, durante a saída 785-796, 30 s depois 885-976, após GC 644-656. Também não em working set privado (377-380, 728, 815-899, 578-588). O piso está no ocioso: 446 MB já excede a meta em 78%, e ficou ~30 MB acima da base (418).
- **Meta 2, p95 do eco < 16 ms: não atingida.** Sem carga 24,1-24,3 ms (base 24,7); com carga 42,5 ms (rodada 2) e 126,8 ms (rodada 1), contra 36,6 ms da base. Nem a menor amostra sob carga chegou a 16 ms (16,3 ms). O eco do PSReadLine no PTY puro é ~10,8 ms (`src-tauri/examples/eco_pty.rs`), então o limite teórico é ~11 ms; o app adiciona ~12 ms sem carga, e esse acréscimo não mudou com o WebGL.

O que as mudanças fizeram:

- Melhorou: pico e residual de memória durante a saída intensa. Total do app 30 s depois da saída: 2264 MB para 1140 e 1230 MB (-50% e -46%); renderer 2024 MB para 885 e 976 MB; heap JS 607 MB para 61 e 62 MB; pico da série 2221 MB para ~1120 MB. A fila retida do `useGitBase` (18,3 MB) sumiu. fps durante a saída 158 para 208-215; p95 de quadro 12,5 para 4,4 e 8,3 ms.
- Sem mudança: abertura (~1,2 s), eco sem carga (~22,4 ms p50), `orquestrador.exe` (~85 MB com 4 terminais cheios).
- Piorou: ocioso +29 a +32 MB no total (renderer +28 a +31 MB); após GC forçado +82 a +95 MB (839 para 921-934 MB); processo browser do WebView2 +7 MB e de 30% para 50% de CPU; p99 de quadro 16,7 para 20,8-24,9 ms e quadros > 50 ms de 1 para 4-6; latência sob carga na cauda: p99 46,3 para 82,8-160,5 ms e máx 52,4 para 110,1-201,8 ms (p50 24,1 para 25,7-26,2 ms). Na rodada 2 o p95 é 42,5 ms (+16%); na 1, um episódio de ~5 s elevou tudo. Causa não identificada; palpite: pausas da thread principal (GC maior ou trabalho dos 3 terminais escondidos no DOM) somadas ao tratamento do Channel.
- O ganho de memória vem sobretudo de a RAM não inflar durante a saída (menos lixo do DOM do visível e dos hooks), não de um piso menor. Os 3 terminais escondidos continuam no renderizador DOM e continuam gerando 74% das alocações (`createRow`); se a meta 1 depende de baixar o ocioso e o residual, o próximo alvo é o que o Blink segura (`blink_gc` ~100 MB em repouso, ~200 MB com os terminais cheios e `partition_alloc`) e o custo dos escondidos.

## Limitações

- Mesmas da base (exe de debug com opt-level 3, máquina compartilhada, janela visível, monitor de 240 Hz, sem teclado físico nem paint do SO, PSReadLine sem predição nem histórico, carga limitada a 6000 linhas/s por terminal, dump do Chromium perturba o GC, pico de RAM amostrado a cada ~3 s).
- Método de eco diferente (WebGL: `onRender` + buffer; base: DOM). Equivalência só sugerida pelo eco sem carga. "Consumido" passou a significar "processado no buffer", não "desenhado".
- O gancho e o reload (`Page.enable`, init script, página recarregada antes dos terminais) são parte do cenário medido nesta remedição; a base não os tinha. Efeito conhecido: `InspectorNetworkAgent` retido no heap (7 MB) e o reload antes dos terminais. A abertura dedicada não usa o gancho.
- Duas rodadas apenas; a cauda da latência sob carga variou 3x entre elas. Duas rodadas não bastam para afirmar a cauda real do app novo.
- Contadores de GPU do Windows dedicados a 30 MB; VRAM do atlas WebGL não detalhada.

## Validação (após as medições, uma por vez)

| Comando | Exit | Contagem |
|---|---|---|
| `npm run build` (`tsc && vite build`) | 0 | sem erros de tipo |
| `npm run check` | 0 | 163 testes, 163 ok |
| `npm run check:ui` | 0 | 174 testes, 174 ok |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 0 | 327 ok (312 lib + 10 `lote` + 5 `prazo`), 5 ignorados, 0 falhas |

O `cargo test` sobrescreve `src-tauri/target/debug/orquestrador.exe` sem o frontend embutido; refazer o `tauri build` antes de qualquer nova medição.
