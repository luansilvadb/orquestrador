# Desempenho: medição de base

Medição antes de qualquer otimização. Reexecutar: `node scripts/medir-desempenho.mjs` (cerca de 6 min, sem intervenção; `--build` recompila o exe). O script grava `resultado.json`, `renderer.heapsnapshot` e `heap-analise.json` em `%TEMP%\orq-medir-<data>\` (ou em `--out`). Os números abaixo são da rodada `C:\Users\Luan\AppData\Local\Temp\orq-medir-base\`; o heap snapshot (79,8 MB) está lá, fora do repositório.

## Ambiente e método

- Windows 11 26200, Ryzen 9 5900X (24 threads), 15,9 GB de RAM, RTX 2060 SUPER, monitor a 240 Hz, WebView2 156.0.4314.58. A máquina é de uso diário: outras instâncias do app e CLIs rodavam ao mesmo tempo.
- Exe: `CARGO_PROFILE_DEV_OPT_LEVEL=3 npx tauri build --debug --no-bundle` (2 min 43 s), com o `dist` de produção embutido (React em modo produção). Rust otimizado como o release, mas com `debug_assertions`: sem o plugin single-instance e sem a verificação de atualização, que agiriam sobre a instância instalada. O release foi descartado por isso.
- Isolamento: `WEBVIEW2_USER_DATA_FOLDER` e `LOCALAPPDATA` do filho apontam para pastas temporárias (perfil do WebView2 e `config.json` do app). O PSReadLine grava o histórico no `%APPDATA%` real (a pasta vem da API do Windows, o `APPDATA` do ambiente não vale): os 4 terminais desligam gravação e predição (`Set-PSReadLineOption -HistorySaveStyle SaveNothing -PredictionSource None`) e o script remove do arquivo as 4 linhas desse comando no fim. Verificado: o script confere que o `ConsoleHost_history.txt` atual começa pela cópia tirada antes da rodada (e a rodada anterior deu `diff` vazio); `config.json` da instalada com o mesmo hash e data; PIDs 6172, 30308 e 34868 (instalada) vivos antes e depois; nenhum processo da árvore medida sobrou.
- CDP pelo `--remote-debugging-port` (porta livre), com o playwright-core do cache do npx. Nada foi instalado nem mudou em `package.json`, `src/` ou `src-tauri/src/`.
- Uma janela de 1400x900 aparece na tela durante a medição.

### Cenário (igual em toda execução)

1. Pasta descartável em `%TEMP%`: repositório git com 40 commits, `src/app.js` de 3000 linhas modificado e `novo.txt` não rastreado. O exe abre com essa pasta como argumento.
2. Aberto: `src/app.js` no CodeMirror; tela Git visitada e retorno a Arquivos; painel Terminal com 4 shells pwsh (o painel tem ~62 colunas no layout padrão; `scrollback: 10000`).
3. Gerador de saída: `node flood.mjs <linhas> <tag> 6000`, linhas de 120 colunas com cor SGR, conteúdo determinístico, 6000 linhas/s por terminal (~780 KB/s).
4. Sequência: ocioso 15 s, RAM; latência sem carga (250 amostras); fase A: terminais 1 a 3 despejam por 70 s (420 mil linhas cada) e o 4º, ativo, recebe a digitação (250 amostras); GC; fase B: os 4 despejam por 40 s (240 mil linhas cada, o 4º visível), RAM aos 15 s e quadros; espera de 30 s, RAM; GC forçado, RAM, heap snapshot; fase C: 4 x 20 s com os perfis de CPU e de alocações do V8.
5. "Saída consumida" = a última linha do gerador aparece no DOM do xterm daquele terminal. A carga esteve ativa durante toda a coleta de latência (conferido no início, no meio e no fim).

### Métricas

- Abertura: `Date.now()` antes do spawn até o marco no DOM, medido dentro da página por MutationObserver: `nav[aria-label=Telas]`, aba de projeto selecionada, um `treeitem` e o botão "Painel Terminal" presentes. Em nenhuma execução o marco já existia ao conectar o CDP.
- RAM: por processo, Private Bytes (`PrivateMemorySize64`, commit), working set e working set privado (contador `WorkingSetPrivate`), mais contadores `GPU Process Memory`. Árvore: `orquestrador.exe` e todo `msedgewebview2.exe` com a pasta de dados desta execução. Shells (pwsh, conhost) e o gerador (node) ficam à parte. O serviço utility de `tracing` (8 MB), criado pelos dumps do próprio script, é excluído.
- Latência: `Input.dispatchKeyEvent` entrega uma letra ao xterm ativo. `t0` = `event.timeStamp` do keydown dentro da página. "DOM" = instante em que o MutationObserver vê a letra na linha do prompt (o xterm atualiza o DOM no seu rAF, o paint segue no mesmo quadro). "Quadro" = o rAF seguinte a isso. Depois de cada amostra, Esc limpa a linha e o script espera o prompt voltar; intervalos entre teclas de 20 a 80 ms vêm de um PRNG fixo. Sem eco em 20 s, a amostra entra como 20 s (nenhuma ocorreu).

## Resultados

### Abertura (ms, do spawn ao marco)

| Condição | Execuções | Mediana | Mín-máx |
|---|---|---|---|
| Perfil WebView2 novo (5) | 1399, 1258, 1190, 1214, 1400 | 1258 | 1190-1400 |
| Perfil existente (5) | 1328, 1270, 1278, 1189, 1297 | 1278 | 1189-1328 |
| Execução do cenário | 1203 | | |

Marcos de uma execução típica (perfil novo, mediana): WebView navega em +436 ms, primeiro paint em +615 ms, DOMContentLoaded em +857 ms, interface interativa em +1258 ms.

### RAM por processo (MB: privada / working set / working set privado)

| Processo | Ocioso | Durante (15 s de saída) | 30 s depois | Após GC forçado |
|---|---|---|---|---|
| orquestrador.exe | 7 / 31 / 5 | 84 / 90 / 66 | 84 / 96 / 71 | 84 / 96 / 71 |
| WebView2 browser | 47 / 133 / 37 | 47 / 130 / 33 | 45 / 130 / 33 | 49 / 133 / 36 |
| WebView2 renderer | 418 / 415 / 350 | 928 / 918 / 864 | 2024 / 1995 / 1929 | 581 / 560 / 495 |
| WebView2 gpu | 86 / 82 / 29 | 90 / 85 / 32 | 87 / 84 / 30 | 102 / 86 / 33 |
| WebView2 utility (rede, storage) | 20 / 60 / 10 | 20 / 60 / 10 | 20 / 60 / 10 | 21 / 61 / 11 |
| WebView2 crashpad | 3 / 19 / 2 | 3 / 12 / 2 | 3 / 12 / 2 | 3 / 12 / 2 |
| **Total do app** | **582 / 738 / 433** | **1172 / 1296 / 1007** | **2264 / 2376 / 2076** | **839 / 949 / 648** |

- Pico da série durante a saída (amostras a cada ~3 s): 2221 MB privados (t = 42 s). Depois de a saída acabar a memória continuou subindo até os 2264 MB acima.
- Shells, fora do total: 4 pwsh 124 MB privados (364 de working set) em repouso e 157 (400) depois; 5 conhost 7 (42); 4 node do gerador 120 (256) durante.
- GPU: processo gpu com 86 MB privados; contadores do Windows 32 MB dedicados e 3 MB compartilhados. O xterm usa o renderizador DOM, sem canvas nem WebGL.
- CPU média na fase B (1 núcleo = 100%): renderer 123%, orquestrador.exe 28%, browser 30%, gpu 20%.

### Latência de digitação no terminal (ms, 250 amostras cada)

| Condição | Medida | p50 | p95 | p99 | máx |
|---|---|---|---|---|---|
| Sem saída concorrente | eco no DOM | 23,2 | 24,7 | 26,2 | 26,7 |
| | até o quadro seguinte | 26,2 | 27,7 | 29,0 | 29,3 |
| 3 terminais despejando (18 mil linhas/s) | eco no DOM | 24,1 | 36,6 | 46,3 | 52,4 |
| | até o quadro seguinte | 27,6 | 40,3 | 49,7 | 59,1 |

Uma rodada completa anterior, idêntica, deu com carga p50 23,1, p95 31,5, p99 38,6, máx 42,3 (DOM) e RAM 30 s depois de 2557 MB (renderer 2318): a variação entre execuções é de uns 5 a 8 ms em p95 e p99 e de algumas centenas de MB na RAM.

### Quadros durante a saída (fase B, 4 terminais, o 4º visível)

| | fps médio | tempo de quadro p50 / p95 / p99 / máx | quadros > 50 ms |
|---|---|---|---|
| Ocioso (referência, monitor de 240 Hz) | 240 | 4,2 / 4,3 / 4,3 / 4,4 ms | 0 |
| Durante a saída | 158 | 4,2 / 12,5 / 16,7 / 75 ms | 1 |

(Rodada anterior: 173 fps, p99 16,5 ms, máx 41,7 ms.)

## Diagnóstico de memória

### Heap JS x resto do renderer

| Momento | Private Bytes do renderer | Heap JS (total) | Fora do heap JS |
|---|---|---|---|
| Ocioso | 418 MB | 36 MB (32 usados), 9% | 382 MB, 91% |
| 30 s depois da saída | 2024 MB | 607 MB (547 usados), 30% | 1417 MB, 70% |
| Após GC forçado | 581 MB | 55 MB (53 usados), 9% | 526 MB, 91% |

Dump de memória do Chromium (renderer, MB), ocioso: footprint 312; `blink_gc` 110 (DOM, CSS e wrappers do Blink), `partition_alloc` 52, `malloc` 45, `v8` 38 (heap 33), memória compartilhada 19 (buffer de transferência da GPU); ~106 MB dos 418 ficam fora do dump (código, pilhas, imagens mapeadas). Após GC com 4 terminais cheios: footprint 575; `blink_gc` 210, `partition_alloc` 173, `v8` 58, `malloc` 48. Entre o ocioso e o estado após GC com os 4 terminais cheios, o renderer vivo cresce 163 MB de Private Bytes.

Limite do método: pedir o dump força um GC no renderer, então ele só descreve o estado vivo. A composição dos ~1,4 GB que o GC devolveu não foi capturada por dump; o que a explica vem dos perfis abaixo e das contagens: `Performance.getMetrics` registrou 69,6 mil nós DOM contra 4,4 mil em repouso (o documento tem 2,3 mil vivos), provavelmente ~65 mil nós soltos esperando coleta.

### De onde vem o lixo (fase C, 4 x 120 mil linhas = ~63 MB de texto, 20 s)

Perfil de alocações do V8 (inclui objetos já coletados): 2294 MB alocados no heap JS, 36 vezes o texto recebido.

| Função (bundle) | MB | % |
|---|---|---|
| `createRow` (renderizador DOM do xterm) | 1775 | 77,4 |
| (V8 API) | 127 | 5,6 |
| (PARSER) | 110 | 4,8 |
| `_sync` (xterm) | 73 | 3,2 |
| `copyFrom` (xterm) | 50 | 2,2 |

Perfil de CPU da thread principal (26,4 s amostrados): `(program)` 53,5% (trabalho nativo do navegador: estilo, layout, paint, DOM), ocioso 18,1%, coletor de lixo 7,2%, `createRow` 4,6%, `replaceChildren` 4,6%, `print` 2,0%, `parse` do xterm 0,9%.

### Heap vivo (snapshot após 30 s e GC): 118,1 MB, 886 mil nós

`HeapProfiler.takeHeapSnapshot` coleta lixo antes, então o snapshot só tem o que está retido. Atribuição pelo dono na árvore de dominadores (o bundle é minificado, o dono sai das propriedades dos objetos):

| Dono | MB (tamanho raso) |
|---|---|
| xterm: buffers e estado dos 4 terminais | 36,7 |
| V8: código compilado e fonte dos scripts | 20,9 |
| React: fibras, estado e filas de atualização | 20,3 |
| shiki / Oniguruma: memória do WASM | 16,5 |
| Blink nativo (DOM, CSS, estilos; 884 CSSStyleSheet, 882 RuleSet) | 10,5 |
| Outros objetos (app, antd/lobehub, libs) | 7,1 |
| Strings sem dono identificado | 6,0 |
| CodeMirror (1 editor aberto) | 0,2 |

Dez maiores consumidores (tamanho retido; os aninhados se sobrepõem):

| # | Consumidor | Retido MB | Parte do app |
|---|---|---|---|
| 1 | Escopo do módulo que guarda a instância do Oniguruma | 19,1 | shiki |
| 2 | Fila de updates pendentes de um `useState` (`pending` com 29 closures `action`, cada uma segura 0,63 MB) | 18,3 | React, `useGitBase` em `src/editor/editor-workspace.tsx` |
| 3 | Objeto do WASM do Oniguruma (`HEAPU8`, ArrayBuffer de 16 MB) | 16,5 | shiki |
| 4 | `Terminal` do xterm #1 (CircularList de 10 mil linhas, `BufferLine` com `Uint32Array`) | 9,1 | xterm |
| 5 | `Terminal` #2 | 9,1 | xterm |
| 6 | `Terminal` #3 | 9,1 | xterm |
| 7 | `Terminal` #4 | 9,1 | xterm |
| 8 | Script `index-*.js` (fonte de 8,5 MB mantida como string externa) | 8,7 | bundle principal |
| 9 | `SourceTextModule` | 1,6 | bundle (módulos ES) |
| 10 | `BytecodeArray` com pool de constantes (mapas de ícones em JSON) | 1,5 | bundle (ícones) |

Item 2: a cada 2,5 s o `load` do `useGitBase` chama `setBase((prev) => ...)` com o `{head, index}` do arquivo aberto (0,63 MB para o `app.js` de 3000 linhas: duas strings de 0,32 MB). Como o resultado é igual ao estado, o React sai sem renderizar e mantém o update, com sua closure, na fila do hook até o próximo render do componente. Em ~340 s de sessão havia 29 pendentes. A inspeção está em `heap-analise.json` (`inspecao`). O custo cresce com o tamanho do arquivo aberto e com o tempo sem renderização do `FileBody`.

Também no processo Rust: `orquestrador.exe` passa de 7 para 84 MB com os 4 terminais cheios (cada terminal guarda até 16 MB de saída bruta, `src-tauri/src/core/pty.rs`).

## Conclusão

- Memória: o app ocioso usa 582 MB privados, 72% no renderer (418 MB), e só 9% do renderer é heap JS (36 MB); o resto é Blink (110 MB de `blink_gc`), alocadores e código. Com saída intensa o total vai a 1,2 GB em 15 s e a 2,3 GB meio minuto depois, quase tudo lixo: o GC forçado devolve 1,4 GB e o app volta a 839 MB. A origem é o renderizador DOM do xterm (`createRow` responde por 77% das alocações e `createRow` mais `replaceChildren` por ~9% da CPU da thread principal, fora o layout e o paint nativos), que recria linhas de DOM a cada quadro, inclusive nos terminais escondidos (`visibility: hidden` não pausa o render). O que fica retido é pequeno (118 MB): 37 MB de buffers do xterm, 16,5 MB do WASM do Oniguruma, 18 MB de updates acumulados do `useGitBase`.
- Latência: sem carga a digitação ecoa em 23 ms (p50) e 27 ms (p99); com 3 terminais despejando 18 mil linhas/s o p50 não muda (24 ms) mas a cauda sobe: p95 37 ms, p99 46 ms, máx 52 ms. Os quadros caem de 240 para 158 fps, com p99 de 16,7 ms e um quadro de 75 ms. palpite: o piso de ~23 ms vem do caminho PTY e PSReadLine, e a cauda sob carga vem da thread principal do renderer ocupada com o renderizador DOM (123% de CPU do processo, 54% da thread em trabalho nativo de DOM/layout).
- Abertura: 1,2 s, dividida entre WebView2 (primeiro paint em 0,6 s) e bundle e inicialização do React (0,6 s).

## Limitações

- Exe de debug (opt-level 3, `debug_assertions` e overflow checks ligados), não o release; o frontend é o mesmo `dist`. O custo do lado Rust pode ser um pouco maior que no release.
- Máquina compartilhada, janela visível, monitor de 240 Hz: os fps e as caudas dependem disso. Há variação entre execuções (ver latência e RAM acima).
- A latência não inclui teclado físico, SO nem o paint na tela; termina no DOM e no rAF seguinte. Não foi medida a digitação no terminal que ele próprio despeja (o eco se perde entre as linhas que rolam). PSReadLine sem predição nem histórico.
- A carga é limitada a 6000 linhas/s por terminal para durar e ser repetível. Numa rodada preliminar com o gerador sem limite (4 x 300 mil linhas, ~100 mil linhas/s no total), o app foi a 2,96 GB (heap JS 1,27 GB) 30 s depois e a 791 MB após o GC; esse modo não está no script.
- O dump do Chromium perturba o GC (ver acima), então a composição pré-GC do renderer não foi medida. O snapshot do heap só vê objetos vivos. `blink_gc` de 110 MB em repouso não foi decomposto.
- Os contadores de GPU do Windows são do processo gpu inteiro; o uso de VRAM é pequeno (32 MB) e não foi detalhado.
- Pico de RAM: a série tem amostras a cada ~3 s, o pico real pode ser maior.
