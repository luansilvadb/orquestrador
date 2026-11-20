# Desempenho do GPUI: latência de digitação no terminal

## Eco do ConPTY (15/11/2026, antes da v0.2.0)

Cenário: 5 pwsh de 62x30, 4 despejando 6 mil linhas/s cada (linhas de 120 colunas com SGR), o quinto recebendo teclas; 250 amostras por condição, 20 a 83 ms entre teclas. Medido com `crates/core/examples/eco_conpty.rs`, sem o app.

Pedaços que voltam de uma tecla no pwsh: `ESC[?25l` em +0,4 ms, `ESC[93ma ESC[?25h` em +8 a +27 ms. Na bancada abaixo, o app processa a letra 0,01 ms depois de lê-la, com ou sem os 4 despejos.

Eco direto (ms; "carga" = 4 ConPTYs despejando):

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

O ConPTY e a leitura ecoam em menos de 0,5 ms mesmo com carga. O atraso é do PowerShell (5.1 e 7, com ou sem PSReadLine), que só ecoa no tique seguinte do timer do Windows (15,6 ms por padrão). Com 1 ms sobram ~11,8 ms fixos, também do PowerShell.

O agrupamento do core (`INTERACTIVE_GAP`) ficou como estava: o eco chega mais de 8 ms depois do `ESC[?25l` e sai na hora.

## Bancada sem janela (15/11/2026, antes da v0.2.0)

Teste `bancada_latencia` de `crates/app/src/terminal/motor.rs`, mesmo cenário, tecla até o eco no `Term` (ms):

| Condição | p50 | p95 | p99 | máx |
|---|---|---|---|---|
| Timer padrão, sem carga | 21,6 | 66,9 | 96,9 | 128,9 |
| Timer padrão, 4 despejando | 20,2 | 57,5 | 96,3 | 102,8 |
| Timer de 1 ms, sem carga | 11,33 | 11,37 | 11,39 | 11,42 |
| Timer de 1 ms, 4 despejando | 11,34 | 11,39 | 11,43 | 11,88 |

O timer padrão variou entre rodadas (outra deu p95 41 ms).

RAM do processo da bancada (MB privados / working set): 13 / 10 ocioso, 155 / 136 com os 4 despejando, 155 / 136 10 s depois de a saída parar. Shells, conhost e node ficam de fora.

## `cargo xtask medir` na v0.2.0 (15/11/2026, `target\par-versao\medir-final\`)

Máquina quieta (nenhum outro build).

| Medida | GPUI 0.2.0 | Meta | |
|---|---|---|---|
| Abertura, mediana (ms) | 224 (820, 242, 224, 224, 222) | < 400 | ok |
| RAM ociosa (MB privados / working set) | 129 / 82 (118 sem shells) | < 150 | ok |
| RAM com 4 despejando (15 s); pico | 264 / 206; 265 | < 500 | ok |
| RAM 30 s depois do fim | 263 / 207 (fim: 263) | não crescer | ok (+0,1) |
| Digitação sem carga, p50 / p95 / máx (ms) | 12,66 / 13,41 / 18,50 | | |
| Digitação com carga, p50 / p95 / máx (ms) | 12,71 / 13,67 / 17,76 | p95 < 16 | ok |

Decomposição com carga (p50 / p95): pty 11,36 / 12,19, janela 1,35 / 1,77. Histórico do PSReadLine intacto.

## Não medido

- O exe de release: a medição usa o de dev otimizado, com `debug_assertions`.
- Editor aberto e tela Git: o gancho só abre terminais.
- Do teclado físico ao compositor: a latência começa quando o app trata a tecla e termina no quadro seguinte ao eco.
