# Desempenho: terminais escondidos sem renderizar

Remedição com o working tree atual: o que `docs/desempenho-depois.md` mediu, mais `termPane` (`src/terminal/terminals.tsx`: invisible, deslocado para fora da viewport, inert) e `deferSelectionRedraw` (`src/terminal/term.ts`), que fazem os terminais escondidos pararem de renderizar. Comparação com a base (`docs/desempenho-base.md`) e com a rodada anterior (média das duas rodadas dela). Duas rodadas completas, `node scripts/medir-desempenho.mjs` sem argumentos (mesmo cenário, carga e momentos; 250 amostras por condição). `scripts/medir-desempenho.mjs` não foi alterado. Brutos: `%TEMP%\orq-medir-depois2-r1\` e `%TEMP%\orq-medir-depois2-r2\` (`resultado.json`, `renderer.heapsnapshot`, `heap-analise.json`); abertura extra em `%TEMP%\orq-medir-depois2-ab\`.

## Ambiente e proteções

- Mesma máquina, WebView2 156, janela 1400x900 visível. Uso diário em paralelo: instância instalada (PIDs 6172, 30308, 34868, uma delas hospeda esta sessão) e CLIs. CPU do sistema em ~51% com o app medido parado.
- RAM livre do sistema (de 16292 MB): 6321 antes do build, 6347 antes da rodada 1, 6030 antes da rodada 2, 5410 antes da abertura extra. Pico do app 1,1 GB; sem pressão de memória. Build e medições um de cada vez.
- Build: `CARGO_PROFILE_DEV_OPT_LEVEL=3 npx tauri build --debug --no-bundle`, exit 0, 43 s, exe de 04:28 (UTC-3), medido sem `cargo test` depois.
- Histórico do PSReadLine: SHA-256 `A1CB7B4B…2AA8` antes e depois de cada execução (o script removeu as 4 linhas que digitou; backup apagado pelo script). PIDs da instalada iguais antes e depois; `config.json` dela intacto; nenhum processo da árvore medida sobrou; só 4 pwsh e o gerador node; nenhum CLI de IA.
- Nenhuma amostra de eco sem resposta no prazo; carga ativa nas 250 amostras (geradores ainda não consumidos no início, meio e fim).

## Resultados (base | depois = média das 2 rodadas anteriores | agora r1 | agora r2)

### Abertura (ms, do spawn ao marco; mediana, mín-máx)

| Condição | Base | Depois | Agora r1 | Agora r2 | Extra (só abertura) |
|---|---|---|---|---|---|
| Perfil novo (5) | 1258; 1190-1400 | 1202; 1157-1413 | 1451; 1219-1941 | 1265; 1260-1452 | 1728; 1326-2178 |
| Perfil existente (5) | 1278; 1189-1328 | 1166; 1129-1252 | 1335; 1247-1386 | 1319; 1236-1403 | 1376; 1270-1723 |
| Execução do cenário | 1203 | 1181 | 1298 | 1245 | |
| Primeiro paint, perfil novo | ~615 | 575 | 628 | 585 | |

A abertura ficou 5 a 15% acima da rodada anterior nas medianas e dentro da faixa da base na rodada 2. A dispersão no mesmo exe (1219 a 2178) é maior que a diferença, e a máquina estava mais carregada que na rodada anterior (RAM livre 5,4 a 6,3 GB contra 7,7 a 8,0). palpite: ruído da máquina; o marco não passa pelos terminais, e `termPane` e `deferSelectionRedraw` só atuam depois que eles montam. Não tratar como regressão sem repetir com a máquina parada.

### RAM privada (MB)

| Momento | Processo | Base | Depois | Agora r1 | Agora r2 |
|---|---|---|---|---|---|
| Ocioso | Renderer | 418 | 447,5 | 454,8 | 449,4 |
| | **Total do app** | 582 | 612,5 | 619,4 | 612,8 |
| Durante a saída (15 s) | Renderer | 928 | 790,5 | 777,2 | 809,6 |
| | **Total do app** | 1172 | 1045 | 1052,8 | 1060,1 |
| 30 s depois | Renderer | 2024 | 930,5 | 940,3 | 835,5 |
| | **Total do app** | 2264 | 1185 | 1215,4 | 1084,5 |
| Após GC forçado | Renderer | 581 | 650 | 613,8 | 637,6 |
| | **Total do app** | 839 | 927,5 | 908,7 | 911,2 |

- Total, working set privado (MB), ocioso / durante / 30 s / após GC: base 433 / 1007 / 2076 / 648; depois 465,5 / 892 / 1024,5 / 755; r1 472,8 / 877,6 / 1026,3 / 716,1; r2 465,7 / 896,6 / 924,5 / 733,5.
- Pico da série durante a saída (amostras a cada ~3 s): base 2221; depois ~1119; r1 1122,3 (t = 44,6 s); r2 1114,4 (t = 44,1 s).
- `orquestrador.exe` durante / após GC: base 84 / 84; depois 85 / 85; r1 105,5 / 105,1; r2 83,2 / 83,0. A r1 ficou 21 MB acima nas três leituras depois da saída e não voltou; causa não investigada (palpite: retenção do alocador no Rust, p.ex. fila do `Channel`).
- WebView2 browser durante: base 47; depois 54; agora 53,2 / 51,1. GPU (contador do Windows, r2): 29,5 MB dedicados, 4 compartilhados.
- CPU média na fase B (1 núcleo = 100%; da série de amostras, ~38 s): renderer base 123, depois 107, agora 90 / 101; browser 30, 50,5, 36 / 46; `orquestrador.exe` 28, 31,5, 27 / 33; gpu 20, 12,5, 15 / 14. (Método de cálculo pode diferir do da rodada anterior em poucos pontos.)

### Heap JS do renderer (MB, total / usado)

| Momento | Base | Depois | Agora r1 | Agora r2 |
|---|---|---|---|---|
| Ocioso | 36 / 32 | 36 / 29 | 32 / 28,2 | 36,3 / 29,5 |
| Durante | n/d | 109,5 / 55 | 100,2 / 54,7 | 104,7 / 55,4 |
| 30 s depois | 607 / 547 | 61,5 / 49,5 | 59,6 / 51,3 | 60,2 / 52,8 |
| Após GC | 55 / 53 | 36 / 34 | 36 / 33,9 | 35,5 / 33,9 |

Fora do heap JS, aos 30 s: r1 881 MB (94%), r2 775 MB (93%). Dump do Chromium após GC (footprint do renderer 616 e 615 MB): `partition_alloc` 284-285 (buffers 246-248), `blink_gc` 168-172. Igual à rodada anterior (285 / 198-200). O piso do renderer (ocioso ~450 MB) e os ~250 MB de `partition_alloc/buffer` não mudaram com esta alteração; são o próximo alvo de memória.

### Latência de digitação (ms, 250 amostras)

| Condição | Medida | | p50 | p95 | p99 | máx |
|---|---|---|---|---|---|---|
| Sem saída concorrente | eco renderizado | base | 23,2 | 24,7 | 26,2 | 26,7 |
| | | depois | 22,4 | 24,2 | 25,6 | 28,8 |
| | | agora r1 | 22,0 | 24,0 | 24,5 | 27,9 |
| | | agora r2 | 21,6 | 24,9 | 34,9 | 37,5 |
| | até o quadro seguinte | base | 26,2 | 27,7 | 29,0 | 29,3 |
| | | depois | 26,2 | 27,8 | 28,9 | 34,2 |
| | | agora r1 | 26,0 | 27,9 | 28,5 | 29,7 |
| | | agora r2 | 25,6 | 29,0 | 38,0 | 41,4 |
| 3 terminais despejando (18 mil linhas/s) | eco renderizado | base | 24,1 | 36,6 | 46,3 | 52,4 |
| | | depois (r1 / r2) | 26,2 / 25,7 | 126,8 / 42,5 | 160,5 / 82,8 | 201,8 / 110,1 |
| | | **agora r1** | 22,8 | 27,0 | 35,3 | 49,2 |
| | | **agora r2** | 23,1 | 26,9 | 30,8 | 37,4 |
| | até o quadro seguinte | base | 27,6 | 40,3 | 49,7 | 59,1 |
| | | depois (r1 / r2) | 29,6 / 29,2 | 147,3 / 46,6 | 176,9 / 93,3 | 232,4 / 122,9 |
| | | **agora r1** | 26,8 | 31,0 | 39,7 | 50,8 |
| | | **agora r2** | 27,1 | 31,1 | 39,5 | 41,9 |

- Sob carga a cauda ficou abaixo da base nas duas rodadas: p95 27,0 / 26,9 (base 36,6), p99 35,3 / 30,8 (46,3), máx 49,2 / 37,4 (52,4). Amostras acima de 40 ms sob carga: 1 e 0; acima de 50 ms: 0 e 0 (base 2; rodada anterior 29 e 7). Nenhum episódio de amostras consecutivas altas (a maior fora do corpo foi a amostra 83 da r1, 49,2 ms, isolada).
- Sem carga a r2 tem 5 amostras acima de 30 ms (índices 35, 107, 109, 118, 183; 30,7 a 37,5 ms), a r1 nenhuma; ver ruído da máquina. p50 e p95 estão na faixa da base.
- Menor eco: 12,8 (r1) e 7,0 ms (r2) sem carga; 16,8 e 17,7 sob carga. Meta 2 (p95 < 16 ms) não atingida.

### Quadros durante a saída (fase B)

| | fps médio | tempo de quadro p50 / p95 / p99 / máx (ms) | quadros > 50 ms | tarefas longas (n; total ms) |
|---|---|---|---|---|
| Base | 158 | 4,2 / 12,5 / 16,7 / 75 | 1 | n/d |
| Depois (média) | 211,5 | 4,2 / 6,4 / 22,9 / 66,7 | 5 | 1,5 |
| Agora r1 | 236,8 | 4,2 / 4,3 / 4,4 / 66,7 | 1 | 1; 64 |
| Agora r2 | 234,4 | 4,2 / 4,3 / 8,3 / 62,5 | 2 | 3; 163 |

Ocioso: 240 fps, 4,2 ms. Um quadro de 62 a 75 ms aparece em todas as medições, inclusive na base; causa não investigada.

### Variação entre as duas rodadas

| Métrica | r1 | r2 |
|---|---|---|
| Total do app, ocioso / durante / 30 s / após GC | 619 / 1053 / 1215 / 909 | 613 / 1060 / 1085 / 911 |
| Renderer, 30 s depois | 940 | 836 (-11%) |
| Eco sob carga p95 / p99 | 27,0 / 35,3 | 26,9 / 30,8 |
| fps durante | 236,8 | 234,4 |
| Abertura, mediana (perfil novo) | 1451 | 1265 (-13%) |

Memória e latência sob carga repetiram; abertura e o ponto de 30 s variam mais.

## Diagnóstico

### Alocações (fase C, 4 x 120 mil linhas)

Perfil de alocações do V8: 451,5 e 452,2 MB alocados (rodada anterior 1691 e 1735; base 2294): **-74%**.

| Função (bundle) | Base % / MB | Depois % / MB | Agora r1 % / MB | Agora r2 % / MB |
|---|---|---|---|---|
| `createRow` (renderizador DOM do xterm) | 77 / 1775 | 74 / 1249-1285 | **0,4 / 1,7** | **0,3 / 1,6** |
| `resolve` (bundle:6516) | n/d | 6-7 / 104-117 | 27 / 122,1 | 26,2 / 118,6 |
| `_sync` (Viewport do xterm) | 3 / 73 | 6 / 100-102 | 23 / 104 | 23,1 / 104,2 |
| `setScrollPosition` | n/d | 4 / 63 | 13,9 / 62,6 | 13,9 / 62,8 |
| (anônima) bundle:6589 | n/d | n/d | 13,6 / 61,3 | 14,4 / 65,1 |
| `sendIpcMessage` (Tauri) | n/d | n/d | 4,5 / 20,3 | 4,5 / 20,5 |

Fração ainda vinda de terminais escondidos: o renderizador DOM só existe nos escondidos (o visível está em WebGL), então `createRow` mede o piso: **0,3 a 0,4%** (74% antes). Todo o ganho é `createRow`: sem ele, o restante é igual ao da rodada anterior (442 e 450 MB antes, 450 e 452 agora). O que sobra (`resolve`, `_sync`, `setScrollPosition`, anônima) é código por terminal que roda a cada rolagem em todos os 4; não distingue visível de escondido e o perfil não guarda pilhas por terminal. palpite: ~3/4 desse resto vem dos 3 escondidos (a mesma taxa de linhas nos 4), ~340 MB, sem custo de CPU relevante (`_sync` 0,1% da thread).

CPU da thread principal (26 s): ocioso 64,6% / 54,6% (rodada anterior 44-47%, base 18%); `(program)` 25,5% / 32,3% (36-39%; base 53,5%); coletor de lixo 4,2% / 4,3% (7%; base 7,2%). `createRow` e `replaceChildren` não aparecem mais entre os 25 maiores.

### Heap vivo (snapshot após 30 s e GC): 106,7 MB, 914 mil nós (rodada anterior 107,0 / 915 mil; base 118,1 / 886 mil)

| Dono | Base MB | Depois | Agora r1 | Agora r2 |
|---|---|---|---|---|
| xterm: buffers e estado dos 4 terminais | 36,7 | 37,2 | 37,0 | 36,9 |
| V8: código compilado e fonte dos scripts | 20,9 | 21,2 | 21,2 | 21,3 |
| React: fibras, estado e filas | 20,3 | 1,9 | 1,9 | 1,9 |
| shiki / Oniguruma (WASM) | 16,5 | 16,5 | 16,5 | 16,5 |
| Blink nativo | 10,5 | 16,5 | 16,3 | 16,4 |
| Outros objetos | 7,1 | 7,5 | 7,5 | 7,5 |
| Strings sem dono | 6,0 | 6,0 | 6,0 | 6,0 |
| CodeMirror | 0,2 | 0,2 | 0,2 | 0,2 |

Fila de updates do `useGitBase`: 0 MB (igual à rodada anterior). Blink nativo inclui `blink::InspectorNetworkAgent` 6,8 MB (custo do CDP anexado, não do app).

### Dez maiores consumidores do heap (tamanho retido; aninhados se sobrepõem; r1 e r2 iguais até a 1ª casa)

| # | Consumidor | Retido MB | Parte do app |
|---|---|---|---|
| 1 | Escopo do módulo que guarda a instância do Oniguruma | 19,3 | shiki |
| 2 | Objeto do WASM do Oniguruma (`HEAPU8`, ArrayBuffer de 16 MB) | 16,5 | shiki |
| 3 | `Terminal` do xterm #1 (CircularList de 10 mil linhas) | 9,1 | xterm |
| 4 | `Terminal` #2 | 9,1 | xterm |
| 5 | `Terminal` #3 | 9,1 | xterm |
| 6 | `Terminal` #4 | 9,1 | xterm |
| 7 | Script `index-*.js` (fonte de 8,8 MB como string externa) | 8,9 | bundle principal |
| 8 | `blink::InspectorNetworkAgent` | 6,8 | instrumento (CDP) |
| 9 | `SourceTextModule` | 1,6 | bundle |
| 10 | `BytecodeArray` com pool de constantes | 1,4 | bundle |

## Investigação da cauda sob carga

**A condição de investigar não ocorreu**: sob carga a cauda ficou abaixo da base nas duas rodadas (tabela acima), então não capturei trace/perfil de CPU com `Tracing` nem alterei o script. O que vale registrar:

1. **Causa provável da cauda anterior: renderização dos 3 terminais escondidos na thread principal.** Antes: `createRow` 74% das alocações, 44-47% de ociosidade da thread, p95 sob carga 42,5 a 126,8 ms. Agora: `createRow` 0,3%, ociosidade 55-65%, p95 26,9 e 27,0 ms. A mudança de `termPane`/`deferSelectionRedraw` é a única diferença relevante entre as medições, e a cauda acompanhou. Não é prova: o episódio de ~5 s da rodada anterior não se reproduziu (nem a cauda alta), e duas rodadas não esgotam uma cauda que antes variou 3x. palpite: o episódio era uma coincidência de pausa de GC grande com layout/estilo de 3 DOMs; o GC hoje consome 4,2% contra 7%.
2. **Hipóteses descartadas pelos dados desta medição**: GC (4,2 a 4,3% da thread, sem mudança que explique a cauda); decodificação do quadro binário (`decodeTermFrame`, `src/backend/term-frame.ts`, fora das 25 maiores funções por tempo próprio; `subarray` 0,3 a 0,4% das alocações); entrega do `Channel` (`fetch` 0,4 a 0,5% da thread, `sendIpcMessage` 4,5% das alocações, 20 MB); layout/React (`(program)` caiu de 36-39% para 25-32%).
3. **Lado do backend (`src-tauri/src/core/pty.rs`), por cálculo, não medido.**
   - Neste cenário o coalescer quase não agrupa. O gerador escreve 100 linhas (~142 B cada, ~14 KB) 60 vezes por segundo por terminal, ou seja, uma leitura a cada ~16,7 ms; `MIN_GAP` de 4 ms (`pty.rs:35`) vence antes da leitura seguinte e cada leitura sai como um envio, igual ao comportamento anterior (uma emissão por leitura de até 16 KiB, `HEAD:pty.rs`). Lote típico ~14 KB, nunca perto de `MAX_BATCH` (256 KiB, `pty.rs:37`). Frequência: ~60 envios/s por terminal, ~240/s no total.
   - Cada envio ≥ 1024 B vai pelo caminho `eval` + `fetch` do Tauri (`tauri-2.12.3/src/ipc/channel.rs:302`, limite `MAX_RAW_DIRECT_EXECUTE_THRESHOLD`); o eco de uma tecla (< 1024 B) vai direto por `eval`. Custo está no processo browser do WebView2 (CPU 36-46%, base 30%).
   - **Risco real, fora do cenário medido**: o xterm 6 só confere o limite de 12 ms entre escritas (`_innerWrite`, constante de 12 ms no bundle); uma escrita única é processada inteira, sem ceder à thread. Com saída muito mais rápida que 6000 linhas/s (`cat` de arquivo grande), `pty.rs:104-110` pode montar um lote de até 256 KiB e o xterm o parseia de uma vez (palpite: 5 a 25 ms de thread parada, durante os quais o eco não renderiza). Antes cada escrita tinha no máximo 16 KiB.
4. **Correção proposta (não aplicada)**: baixar `MAX_BATCH` em `src-tauri/src/core/pty.rs:37` de `256 << 10` para `64 << 10` (4 leituras de 16 KiB). Mantém o ganho de IPC (um envio por 4 ms) e limita cada parse a ~1-3 ms (palpite). Validar com o script em carga bem acima da atual (p.ex. `--taxa` alto) e observar `tarefasLongas` e p99 do eco. Sem pressa: no cenário padrão não muda nada.
5. Para reproduzir a cauda alta caso volte: rodar mais 3 rodadas completas e, na fase com carga, ligar CDP `Tracing` (`devtools.timeline`, `v8.execute`) ou `Profiler` no renderer; o script não tem essa opção.

## Conclusão

- **Metas**: meta 1 (renderer < 250 MB) não atingida: ocioso 449-455 MB, durante 777-810, 30 s 836-940, após GC 614-638. Meta 2 (p95 do eco < 16 ms) não atingida: 24,0 a 24,9 ms sem carga, 26,9 a 27,0 sob carga; piso de ~11 ms no PTY (`src-tauri/examples/eco_pty.rs`).
- **Melhorou sobre a rodada anterior**: cauda do eco sob carga (p95 42,5-126,8 para 26,9-27,0; p99 82,8-160,5 para 30,8-35,3; máx 110-202 para 37-49) e agora abaixo da base (36,6 / 46,3 / 52,4); alocações -74% (1700 para 452 MB); fps durante a saída 208-215 para 234-237 (base 158) e p95 de quadro 4,3 ms (base 12,5); CPU da thread principal mais livre (ociosa 55-65%).
- **Sem mudança**: RAM privada do total em todos os momentos (±5% da rodada anterior; 30 s depois 1085-1215 MB contra 2264 da base); heap JS; heap vivo; eco sem carga (p50 ~22 ms).
- **Pior que a base, igual à rodada anterior**: ocioso +31 a +37 MB no total (613-619 contra 582) e no renderer (449-455 contra 418); após GC +70 a +72 MB no total (909-911 contra 839).
- **Atenção para a release**: abertura 1245-1451 ms (cenário) contra 1176-1186 da rodada anterior; não conclusivo (ruído); `orquestrador.exe` a 105 MB na r1 contra 83-85; cauda anterior não reproduzida mas ainda sem prova de causa.

## Limitações

- Mesmas das rodadas anteriores (exe de debug com opt-level 3, máquina compartilhada com CPU de fundo em ~51%, janela visível, monitor de 240 Hz, sem teclado físico, PSReadLine sem predição, carga de 6000 linhas/s por terminal, dump do Chromium perturba o GC, pico amostrado a cada ~3 s).
- Duas rodadas completas; a cauda anterior variou 3x entre rodadas, então nada garante que a cauda baixa se repita sempre. Sem trace de CPU do renderer (a condição não se cumpriu).
- Tamanho e frequência dos lotes do backend vêm de cálculo a partir do gerador e do código, não de contagem no `Channel`.
- O perfil de alocações não separa visível de escondido; a fração "ainda vinda de escondidos" é o piso `createRow` mais um palpite para o resto.
- Método de eco `buffer` (gancho `__orqMedir`), igual ao da rodada anterior; a base usou DOM.

## Validação

| Comando | Exit | Resultado |
|---|---|---|
| `CARGO_PROFILE_DEV_OPT_LEVEL=3 npx tauri build --debug --no-bundle` | 0 | exe gerado, 43 s |
| `node scripts/medir-desempenho.mjs` (r1, r2) | 0, 0 | completas, sem erros de página, sem processos restantes |
| `node scripts/medir-desempenho.mjs --sem-cenario` (abertura extra) | 0 | |

Nenhum arquivo de código foi alterado nesta tarefa, então testes e lint não foram reexecutados (`cargo test` sobrescreveria o exe medido).
