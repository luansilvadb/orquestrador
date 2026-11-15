# Desempenho do GPUI: latência de digitação no terminal

Mesma máquina e cenário de `desempenho-base.md`: 5 pwsh de 62x30, 4 despejando 6 mil linhas/s cada (gerador `flood` da base, linhas de 120 colunas com SGR), o quinto recebendo teclas; 250 amostras por condição, intervalo de 20 a 83 ms entre teclas, Backspace limpa a linha entre amostras (Esc não: o ConPTY segura o Esc esperando o resto de uma sequência).

## Ferramentas

- `cargo test --release -p orquestrador-app bancada_latencia -- --ignored --nocapture`: core e `Registro` de verdade, sem janela. A tecla entra por `Registro::escrever` (o caminho da vista) e o carimbo é tirado no `TermSink` quando o pedaço com a letra chega (antes e depois de o `Term` processá-lo). Mede também a RAM do processo.
- `cargo run --release -p orquestrador-core --example eco_conpty -- <xtask.exe>`: sem o app; o gerador é `xtask despejar` (`cargo build -p xtask`). Abre os ConPTYs com `portable_pty` (o mesmo do core), escreve a letra e carimba quando ela volta na leitura. `ORQ_SHELL` troca o programa do eco, `ORQ_TIMER=1` pede 1 ms de resolução de timer, `ORQ_DEPURAR=1` imprime os pedaços.
- `ORQ_MEDIR` no app (`%TEMP%\orq-latencia.txt`): agora conta como eco a mudança da linha do cursor. Antes contava qualquer saída, e o pwsh manda `ESC[?25l` 0,3 ms após a tecla e o caractere 10 a 25 ms depois; o p50 de 2,4 ms medido antes era esse `ESC[?25l`.

## Onde está o atraso

Pedaços que voltam de uma tecla no pwsh: `ESC[?25l` em +0,4 ms, `ESC[93ma ESC[?25h` em +8 a +27 ms. O app entrega e processa a letra 0,01 ms depois de ela ser lida; os 4 despejos não mudam nada.

Eco direto (ms, 250 amostras; "carga" = 4 ConPTYs despejando):

| Programa do eco | Timer | sem carga p50 / p95 / p99 / máx | com carga p50 / p95 / p99 / máx |
|---|---|---|---|
| node em modo raw | padrão | 0,23 / 0,33 / 0,37 / 0,46 | 0,24 / 0,37 / 0,40 / 0,46 |
| cmd | padrão | 0,07 / 0,11 / 0,14 / 0,14 | 0,07 / 0,12 / 0,15 / 0,16 |
| pwsh | padrão | 12,4 / 24,7 / 26,8 / 30,9 | 12,4 / 25,6 / 31,4 / 35,4 |
| pwsh sem PSReadLine | padrão | 12,5 / 26,1 / 27,0 / 35,1 | 12,3 / 24,7 / 30,9 / 35,3 |
| powershell 5.1 | padrão | 12,3 / 25,6 / 30,6 / 35,3 | 15,0 / 25,6 / 31,1 / 34,2 |
| pwsh | 1 ms | 11,8 / 11,9 / 12,3 / 12,9 | 11,8 / 11,9 / 12,4 / 12,9 |
| pwsh sem PSReadLine | 1 ms | 11,8 / 11,9 / 12,3 / 12,3 | 11,8 / 11,9 / 12,4 / 12,4 |
| powershell 5.1 | 1 ms | 11,7 / 11,8 / 11,9 / 12,3 | 11,8 / 11,9 / 11,9 / 11,9 |

Conclusões: o ConPTY e a leitura ecoam em menos de 0,5 ms mesmo com carga. O atraso é do PowerShell (5.1 e 7, com ou sem PSReadLine), que só ecoa no tique seguinte do timer do Windows (15,6 ms por padrão). Com 1 ms sobram ~11,8 ms fixos, também do PowerShell.

## Correção

`Registro::novo` (`crates/app/src/terminal/motor.rs`) chama `timeBeginPeriod(1)` uma vez, pela vida do processo. O agrupamento do core (`INTERACTIVE_GAP`, 4 ms) só junta o que chega logo depois de um envio; o eco chega mais de 8 ms depois do `ESC[?25l` e sai na hora, então o core ficou como estava.

## Resultado no app (bancada, ms)

| Condição | Medida | p50 | p95 | p99 | máx |
|---|---|---|---|---|---|
| Antes, sem carga | eco no `Term` | 21,6 | 66,9 | 96,9 | 128,9 |
| Antes, 4 despejando | eco no `Term` | 20,2 | 57,5 | 96,3 | 102,8 |
| Depois, sem carga | eco no `Term` | 11,33 | 11,37 | 11,39 | 11,42 |
| Depois, 4 despejando | eco no `Term` | 11,34 | 11,39 | 11,43 | 11,88 |

"Antes" variou entre rodadas (outra deu p95 41 ms). Somando a parte da janela medida antes (desenho, p95 3,3 ms), tecla até o quadro fica em ~14,7 ms no p95, abaixo dos 16 ms; essa soma não foi medida de ponta a ponta na janela.

RAM do processo da bancada (sem janela; MB privados / working set): 13 / 10 ocioso, 155 / 136 com os 4 despejando, 155 / 136 10 s depois de a saída parar (não cresce). Shells, conhost e node ficam de fora.

## Medição completa do app (`cargo xtask medir`)

`cargo xtask medir` (cerca de 9 min, sem intervenção; `--exe` pula o build, `--out` escolhe a pasta, `--aberturas`, `--amostras`, `--taxa`, `--despejo`). Compila `orquestrador` em dev com opt-level 3 (`<CARGO_TARGET_DIR>\medir`): o app com `debug_assertions` (sem instância única e sem atualização de CLIs), as dependências sem elas, como no release (com elas o GPUI compila os shaders HLSL na abertura, ~200 ms, e liga a camada de depuração do DXGI). `--so-abertura` mede só a abertura. Grava `resultado.json` em `%TEMP%\orq-medir-gpui-<data>\`.

- Isolamento: cada execução tem `LOCALAPPDATA`, `TEMP` e `TMP` próprios; repositório descartável de 40 commits com `src/app.js` de 3000 linhas modificado e `novo.txt`. Os shells desligam histórico e predição do PSReadLine numa linha marcada `# secret`, que o PSReadLine não grava; o script confere que `ConsoleHost_history.txt` ficou byte a byte igual.
- Gancho no app (`crates/app/src/medir.rs`, por `ORQ_MEDIR_DIR`): grava o instante do primeiro quadro desenhado e, com `roteiro.txt`, abre o painel Terminal com um shell por linha (um por vez, porque o shell só nasce quando a aba é desenhada) e escreve cada linha no seu shell.
- Abertura: do spawn ao primeiro quadro da bancada com o projeto aberto (o marco é o `on_next_frame` do primeiro `render`). 5 execuções, perfil novo em cada.
- RAM: Private Bytes e working set por processo, pela árvore do exe. "App" é só `orquestrador.exe` (um processo; não há WebView). pwsh, conhost e o gerador ficam à parte.
- Cenário: 5 pwsh no painel; 4 rodam `xtask despejar` (o `flood` da base em Rust: 6000 linhas/s, 70 s, 420 mil linhas cada), o 5º, ativo e focado, recebe as teclas. Ocioso 15 s, RAM; com carga: RAM aos 15 s, série a cada 3 s, RAM no fim e 30 s depois.
- Digitação: teclas postas na fila da janela (`PostMessageW` de `WM_KEYDOWN`/`WM_KEYUP`, sem mexer no teclado do usuário), letra e Backspace alternados com 20 a 83 ms entre elas, até 500 amostras do `ORQ_MEDIR` (cada tecla que muda a linha do cursor conta uma). "Total" é tecla até o quadro, "pty" até a saída processada, "janela" a diferença. Sem carga e com carga em execuções separadas do app, porque `orq-latencia.txt` acumula.

Rodada de 15/11/2026 (`C:\orq-target\medir-r1\`), mesma máquina da base, monitor a 240 Hz:

| Medida | Base React (`desempenho-base.md`) | GPUI | Meta | |
|---|---|---|---|---|
| Abertura, mediana (ms) | 1258 | 489 (1659, 479, 481, 489, 675) | < 400 | falha |
| RAM ociosa, total do app (MB privados / working set) | 582 / 738 | 131 / 89 (5 shells abertos; 118 sem shells) | < 150 | ok |
| RAM com 4 despejando (15 s) | 1172 / 1296 | 265 / 209; pico da série 266 | < 500 | ok |
| RAM 30 s depois do fim | 2264 / 2376 | 264 / 210 (fim do despejo: 265) | não crescer | ok |
| Digitação sem carga, p50 / p95 / máx (ms) | 26,2 / 27,7 / 29,3 (até o quadro) | 14,45 / 16,76 / 23,94 | | |
| Digitação com carga, p50 / p95 / máx (ms) | 27,6 / 40,3 / 59,1 (3 despejando) | 14,72 / 16,81 / 19,10 | p95 < 16 | falha por 0,8 |

Decomposição da digitação com carga (p50 / p95): pty 11,12 / 12,16, janela 3,67 / 5,46. Shells fora do total: 5 pwsh 152 a 164 MB privados, conhost 7 a 8.

### Onde está o custo das metas que falharam

- Abertura (marcos temporários na inicialização, 3 execuções quentes, ms desde o spawn): `main` em 31; `gpui_kit::application()` até o callback de `run` 240; `open_window` até a view ser construída 200; `Bancada::new` 0,5; primeiro quadro 7 depois. A bancada e o restauro do projeto não pesam: ~440 ms são da plataforma do GPUI (criação da aplicação e da janela com o renderizador). palpite: DirectWrite enumerando as fontes do sistema e a criação do dispositivo DirectX/DirectComposition. Carregar o exe (87 MB, FFmpeg estático) e sair sem janela (`--allow`) leva ~65 ms. A primeira execução (1659 ms) é cache frio do disco.
- Digitação: 11 ms do p50 são do PowerShell (ver "Onde está o atraso" acima: eco no tique seguinte, ~11,8 ms fixos mesmo com timer de 1 ms). A parte do app (janela) é 3,7 ms no p50 e 5,5 no p95, perto de um quadro a 240 Hz; a carga não muda nada. A meta de 16 ms no p95 fica fora de alcance com pwsh enquanto o eco dele custar 12 ms no p95.

### Limitações

- Exe de dev otimizado (com `debug_assertions`), não o release.
- Sem editor aberto nem visita à tela Git (a base abria `src/app.js` e o Git): o gancho só abre terminais.
- A latência começa quando o app trata a tecla (`digitar`), não no teclado físico; termina no quadro seguinte ao eco, sem o tempo do compositor.

## Rodada de 15/11/2026, depois das correções (`target\par-abertura\final4\`)

Correções:

- Abertura: os dispositivos DirectX nascem numa thread em paralelo com o DirectWrite (`vendor/gpui-pre-windows`, ver `vendor/README.md`); dependências sem `debug_assertions` na medição (shaders pré-compilados, como no release).
- Digitação: a saída que chega até 50 ms depois de uma tecla pede `WM_PAINT` na hora (`InvalidateRect`, `terminal/motor.rs`), sem esperar o vsync; o processo pede que o Windows 11 não ignore o timer de 1 ms com a janela encoberta (`PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION`). Sem o segundo, o eco do pwsh voltou a 15,9 ms no p50.
- `bancada_latencia` usa `xtask despejar` no lugar do `node flood.mjs`.

Marcos da abertura (ms desde `main`, execução quente): antes, DirectWrite 80, dispositivo 180 depois dele, pipelines 200 (shaders compilados na hora), primeiro quadro ~600; depois, DirectWrite 80 com o dispositivo em paralelo (pronto em ~175), pipelines 1, view em ~205.

| Medida | Antes (r1) | Depois | Meta | |
|---|---|---|---|---|
| Abertura, mediana (ms) | 489 | 230 (553, 236, 230, 226, 220) | < 400 | ok |
| RAM ociosa (MB privados / working set) | 131 / 89 | 130 / 81 | < 150 | ok |
| RAM com 4 despejando (15 s); pico | 265 / 209; 266 | 265 / 205; 266 | < 500 | ok |
| RAM 30 s depois do fim | 264 / 210 | 265 / 207 (fim: 265) | não crescer | ok |
| Digitação sem carga, p50 / p95 / máx | 14,45 / 16,76 / 23,94 | 13,32 / 19,39 / 21,21 | | |
| Digitação com carga, p50 / p95 / máx | 14,72 / 16,81 / 19,10 | 13,47 / 19,84 / 20,92 | p95 < 16 | falha |

Decomposição com carga (p50 / p95): janela 3,67 / 5,46 → 1,27 / 1,73; pty 11,12 / 12,16 → 12,18 / 18,36.

A parte do app caiu 3,7 ms no p95, mas o eco do pwsh nos shells do app subiu: a letra volta do pwsh em ~17,4 ms (o Backspace em ~11,5), medido no byte que chega do PTY, a partir da 27ª tecla (as 26 primeiras, uma de cada letra, ficam em ~11,5). O app escreve só a tecla. Fora do app o mesmo pwsh ecoa em 11,8 ms em todas as teclas (`eco_conpty` e `bancada_latencia`, inclusive com `LOCALAPPDATA`/`TEMP` vazios, na pasta do repositório de teste, em 200x50, com o roteiro de comandos do `medir` e com o processo num Job). Causa (achada depois): o próprio `cargo xtask medir`. `App::latencia` compilava a regex de `orq-latencia.txt` a cada par de teclas, no xtask de dev sem otimização, logo antes de postar a letra; só passa a fazer isso quando o arquivo existe, e o app o grava a cada 50 amostras, a primeira na 26ª letra. Provas, com um registro temporário de escritas e leituras no PTY: com o alfabeto de 10 letras o salto continuou na 26ª tecla; adiando a primeira gravação para 150 amostras, ele foi para a 76ª; com a regex compilada uma vez, sumiu. O Backspace não sofria porque vem depois de uma pausa sem leitura do arquivo. palpite: a rajada de CPU do xtask desloca o pwsh do núcleo ou da fase do tique. O app e o core não mudaram.

## Rodada de 15/11/2026, com a regex do xtask compilada uma vez (`target\par-latencia\final3\`)

Rodadas com outros executores compilando (até 16 `rustc`) deram abertura de 364 e 908 ms e digitação com carga p95 de 38 e 58 ms; a tabela é a rodada com a máquina quieta (0 a 2 `rustc`).

| Medida | Antes | Depois | Meta | |
|---|---|---|---|---|
| Abertura, mediana (ms) | 230 | 270 (267, 249, 270, 305, 452) | < 400 | ok |
| RAM ociosa (MB privados / working set) | 130 / 81 | 131 / 84 | < 150 | ok |
| RAM com 4 despejando (15 s); pico | 265 / 205; 266 | 265 / 208; 266 | < 500 | ok |
| RAM 30 s depois do fim | 265 / 207 | 265 / 209 (fim: 265) | não crescer | ok |
| Digitação sem carga, p50 / p95 / máx | 13,32 / 19,39 / 21,21 | 12,69 / 13,73 / 37,35 | | |
| Digitação com carga, p50 / p95 / máx | 13,47 / 19,84 / 20,92 | 12,58 / 13,41 / 16,36 | p95 < 16 | ok |

Decomposição com carga (p50 / p95): pty 12,18 / 18,36 → 11,34 / 12,14; janela 1,27 / 1,73 → 1,20 / 1,64.

## Tabela final da v0.2.0 (15/11/2026, `target\par-versao\medir-final\`)

Máquina quieta (nenhum outro build). Mesmo exe de medição (dev com opt-level 3) da versão 0.2.0.

| Medida | Base React (`desempenho-base.md`) | GPUI 0.2.0 | Meta | |
|---|---|---|---|---|
| Abertura, mediana (ms) | 1258 | 224 (820, 242, 224, 224, 222) | < 400 | ok |
| RAM ociosa (MB privados / working set) | 582 / 738 | 129 / 82 (118 sem shells) | < 150 | ok |
| RAM com 4 despejando (15 s); pico | 1172 / 1296; 2221 | 264 / 206; 265 | < 500 | ok |
| RAM 30 s depois do fim | 2264 / 2376 | 263 / 207 (fim: 263) | não crescer | ok (+0,1) |
| Digitação sem carga, p50 / p95 / máx (ms) | 26,2 / 27,7 / 29,3 | 12,66 / 13,41 / 18,50 | | |
| Digitação com carga, p50 / p95 / máx (ms) | 27,6 / 40,3 / 59,1 (3 despejando) | 12,71 / 13,67 / 17,76 | p95 < 16 | ok |

Decomposição com carga (p50 / p95): pty 11,36 / 12,19, janela 1,35 / 1,77. Histórico do PSReadLine intacto.
