---
target: issues
total_score: 27
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 1
target_identity: "file:D:\\orquestrador\\src\\issues-page.tsx"
target_fingerprint: "sha256:23dd4f39f5fb3d94b57f33229bf328b12ab46973393db72762315c1497b2d034"
target_path: "D:\\orquestrador\\src\\issues-page.tsx"
timestamp: 2026-10-16T20-48-53Z
slug: src-issues-page-tsx
---
**Método:** dual-agent (A: revisão de design · B: detector e overlay). Os dois avaliadores rodaram isolados um do outro e em sequência, porque usam o mesmo navegador.

# Crítica da tela Issues

## Veredito de especificidade de design

**Avaliação do design.** O comportamento é deste produto. A aparência é genérica de propósito, como o brief pede ("familiar antes de original").

A composição é a esperada:
- faixa no estilo do Controle do Código-Fonte do VS Code;
- detalhe numa aba do editor;
- orquestrador ao lado.

Qualquer IDE usaria esse arranjo. O que só este produto tem está na camada de estado:
- a issue acompanha o ciclo do orquestrador: "colada, aguardando envio" → "trabalhando" → "livre";
- a Próxima é colada sozinha quando a rodada termina;
- o menu de ocupado diz a consequência: "Colar agora (#8 volta às abertas)".

Nenhum gerenciador de issues genérico faz isso. A tela escorrega para o genérico onde copia o vocabulário de outro produto:
- as bolinhas de label do GitHub nas linhas de 24 px;
- o ▷ verde, que no VS Code é "executar", num botão que não executa;
- o Markdown com o espaçamento frouxo de chat do Lobe UI (entrelinha 1,8), numa bancada com densidade de IDE.

**Varredura determinística.**
- **Linha de comando:** 0 achados nos 5 arquivos. Isso não prova que está limpo: em `.tsx` o detector só aplica regras de regex, e as regras de elemento (contraste, texto pequeno) ficam de fora. Não há config nem ignores; com `--no-config` o resultado também é 0.
- **Navegador, em 5 vistas:** 2 achados distintos.
  - **`low-contrast` na dica "Ctrl+Shift+I" da captura** (`src/issues-sidebar.tsx:488`): `#6f6f6f` sobre `#0d0d0d`, 3,9:1. É real, de baixa gravidade. A revisão de design chegou ao mesmo número sozinha (3,87:1).
  - **`blinking-cursor`:** falso positivo. É o cursor real do xterm do orquestrador, fora dos arquivos das Issues. Única ressalva: o piscar não respeita `prefers-reduced-motion`.
- **O que o detector não pegou:**
  - o corpo do modal "Descartar esta issue?", a 3,46:1 (ele não abriu o modal);
  - o ▷ verde fora da exceção de cor;
  - todos os problemas de comportamento, o P1 inclusive.

  O detector atesta o acabamento visual; os problemas desta tela estão no fluxo.

**Overlay visual.** A injeção funcionou. O overlay ficou visível na aba **[Human] Issues — Orquestrador** do navegador do Playwright, na issue #8, marcando a dica "Ctrl+Shift+I". Se a aba for recarregada, ou se o Vite subir de novo (o que a recarrega), o overlay some.

## Nota de saúde do design

| # | Heurística | Nota | Problema principal |
|---|---|---|---|
| 1 | Visibilidade do estado | 3 | A cobertura é boa, mas dois avisos afirmam o que não aconteceu: a troca de issue no rascunho e o "será colada" sem efeito. |
| 2 | Correspondência com o mundo real | 3 | O ▷ do "Run" marca uma ação que não executa. O mesmo estado aparece com dois textos. |
| 3 | Controle e liberdade | 3 | Há Desfazer, Reabrir e Esc. Não há como desfazer uma colagem: o app manda apagar no rascunho. |
| 4 | Consistência e padrões | 2 | Linhas abertas e fechadas com anatomias diferentes. ▷ verde na faixa e branco no detalhe. Tag de estado com forma de label. Abas sem setas. "Abertas 10" no cabeçalho e "9 abertas" no rodapé. |
| 5 | Prevenção de erros | 2 | Colar com outra issue ainda não enviada junta as duas no rascunho, sem aviso. |
| 6 | Reconhecimento em vez de memória | 3 | A 280 px, a linha escolhida mostra só 14 a 17 caracteres do título. As labels viram cor sem nome. |
| 7 | Flexibilidade e eficiência | 3 | A lista tem bons atalhos. Faltam atalho para Concluir, ↓ do filtro para a lista e saída do terminal pelo teclado. |
| 8 | Estética e minimalismo | 3 | Tela calma e densa. O ruído vem das bolinhas, do "sincronizado há N s" a cada segundo e dos 8 chips do filtro. |
| 9 | Reconhecer e recuperar erros | 3 | A indisponibilidade do GitHub é exemplar. O título vazio não gera texto, e uma falha é reenviada a cada ~15 s, sem fim. |
| 10 | Ajuda e documentação | 2 | As dicas no contexto são boas, mas não há lista de atalhos nem caminho para a documentação. |
| **Total** | | **27/40** | **Aceitável, no topo da faixa (20–27)** |

## Impressão geral

A tela é forte no que é dela:
- o ciclo da issue com o orquestrador;
- o contrato "colar não é enviar", dito na própria tela;
- a indisponibilidade do GitHub tratada como um fluxo, não como um erro.

O maior problema e a maior oportunidade são o mesmo. O app afirma o que há no rascunho do orquestrador, mas não rastreia esse rascunho. Na ação principal da tela, o que a interface diz e o que o orquestrador vai receber podem divergir.

## O que funciona

1. **"Colar não é enviar", dito e cumprido.**
   - O contrato aparece visível no detalhe ("Cola no rascunho do orquestrador; você envia."), na descrição acessível e no `title`.
   - O Rodar no Terminal também cola `gh auth login` sem Enter.
   - É o princípio 4 do PRODUCT.md (explícito sobre risco) aplicado num produto em que os executores têm permissão total na pasta.
2. **Indisponibilidade de primeira.**
   - A mensagem é exata e o comando aparece como código, com Copiar comando, Rodar no Terminal e Tentar de novo.
   - Sem GitHub, a captura guarda a ideia e envia sozinha quando ele volta.
3. **Teclado no padrão do VS Code na lista.**
   - Uma só parada de Tab, com ↑/↓/Home/End nas linhas e ←/→ nos cabeçalhos.
   - Enter escolhe a issue sem tirar o foco da faixa; Ctrl+Enter escolhe e cola.
   - O anel de foco `#007fd4` é visível.

## Problemas prioritários

### [P1] O Colar junta textos no rascunho, e o aviso afirma o contrário

**O quê.** Colar uma issue quando outra já foi colada e não enviada acrescenta o texto novo ao rascunho, sem separador.
- No teste, colei a #13 com a #11 pendente.
- O aviso disse "#11 voltou para as abertas; #13 colada no orquestrador.".
- O rascunho ficou com as duas grudadas: "…faixa de abas.Issue #13: …".
- O Voltar às abertas deixa o texto no rascunho em silêncio. O Concluir sem rodada manda o usuário apagar sozinho.

Conferi no código:
- `pasteIssue` cola sem olhar o rascunho (`src/App.tsx:392-413`);
- o Voltar às abertas só troca o estado (`src/issue-ui.tsx:333-341`).

**Por que importa.** É a ação principal da tela. O erro só aparece depois do Enter, com executores de permissão total já trabalhando em duas issues.

**Correção.**
- Guardar quais issues foram coladas desde a última rodada.
- Ao colar outra com o rascunho ocupado, perguntar antes.
- Fazer o aviso e o cartão Em andamento dizerem o que há no rascunho ("no rascunho: #11, #13").
- Dar ao Voltar às abertas o mesmo alerta do Concluir.
- Abrir cada colagem com uma linha em branco.

**Comando:** `$impeccable harden`

### [P2] A linha da faixa não deixa ler o título, contra a própria especificação

**O quê.** A especificação pede ícone, `#N` e título (`docs/interface.md:55` e o List Rows do `DESIGN.md`). A linha aberta tem, além disso, até 3 bolinhas de label, "+N", o `#N` à direita e o Colar (`src/issues-sidebar.tsx:623-640`).
- A 280 px, a linha escolhida mostra "Guia de contri…".
- A 200 px, mostra "Mostrar…".
- As fechadas usam outra anatomia: `#N` à esquerda e 12 px.

**Por que importa.** A faixa é a superfície que se varre com os olhos. A linha recém-escolhida é justamente a mais ilegível.

**Correção.**
- Ícone, `#N` (secundário, largura fixa) e título, nessa ordem.
- Sem bolinhas: os nomes das labels já estão no `title` e na descrição acessível.
- Colar compacto só com ícone e o nome acessível completo.
- A mesma anatomia, a 13 px, nas fechadas.

**Comando:** `$impeccable layout`

### [P2] Teclado e leitor de tela perdem o lugar nos momentos de risco

**O quê.**
- Depois de Concluir pelo modal, de Voltar às abertas e de Descartar a Nova issue, o foco cai no `body`, e o Tab seguinte vai para "Minimizar".
- Nas abas Issues e Nova issue, a aba inativa tem `tabindex=-1` e não há setas (`src/issues-area.tsx:127`). Com a Nova issue ativa, a aba Issues fica inalcançável pelo teclado.
- O Desfazer do Concluir dura 8 s, só se chega a ele com F6, e o toast se anuncia em inglês ("Notifications", "Close").
- O título vazio só pisca e sacode, sem texto e sem `aria-invalid`.

**Por que importa.** Concluir, Desfazer e Descartar mexem no GitHub ou jogam texto fora. É justamente nelas que quem usa o teclado perde o lugar ou o prazo.

**Correção.**
- Devolver o foco à linha da issue, como o Reabrir já faz.
- Fazer ←/→/Home/End andarem nas abas.
- Traduzir os rótulos do `ToastHost` e pausar o prazo do toast enquanto ele tem foco.
- Mostrar e anunciar "Escreva um título para criar.".

**Comando:** `$impeccable harden`. Os anúncios do leitor de tela foram deduzidos do código e pedem um `$impeccable audit`.

### [P2] Avisos que afirmam o que não aconteceu, e uma linha que pula a cada 15 s

**O quê.**
- **"Colar quando o orquestrador terminar" na própria issue em andamento.** O menu oferece a opção e o aviso confirma "#8 será colada…", mas nada é marcado. O `markNext` recusa a issue em andamento (`src/issue-text.ts:168`), e o aviso sai mesmo assim (`src/App.tsx:416-419`).
- **Reenvio sem fim.** Uma criação que falhou é reenviada a cada atualização boa, sem espaçar as tentativas nem limitá-las (`src/issues-store.ts:167`). A cada tentativa:
  - a linha cai de 70 para 42 px;
  - Tentar de novo e Descartar somem;
  - a lista pula 28 px.
- **Dois relógios na mesma tela.** O detalhe atualiza a cada 30 s e a faixa a cada 1 s (`src/issues-page.tsx:43`, `src/issues-sidebar.tsx:100`). Por isso o painel mostra "no orquestrador agora" ao lado de "há 1 min" no cartão.

**Por que importa.** A força desta tela é dizer a verdade sobre o estado, e cada aviso falso gasta essa confiança. Num app aberto o dia todo, um pulo a cada 15 s provoca clique errado.

**Correção.**
- Não oferecer "colar depois" na issue em andamento.
- Espaçar os reenvios (15 s, 1 min, 5 min, depois só manual), mantendo a altura e os botões e mostrando a contagem.
- Usar um relógio só para a tela.

**Comando:** `$impeccable harden`

### [P2] Contraste e as regras de cor do próprio DESIGN.md

**O quê.**
- **Corpo do modal "Descartar esta issue?":** usa `type="secondary"`, que resolve para `#6f6f6f`. Sobre o `#1a1a1a` do modal dá 3,46:1 (`src/issues-area.tsx:191`). O modal irmão usa o `SECONDARY` (`src/issue-ui.tsx:98`), como o comentário de `src/issue-ui.tsx:202` manda.
- **O ▷ do Colar na faixa é verde `#3fb950`** (`src/issue-ui.tsx:255`). A exceção de cor do GitHub vale só para ícones de estado, e o `docs/design-system.md:55` diz "Não há botão verde".
- **A Tag "Aberta" tem a forma do chip de label**, contra a regra "Nunca pílula com a forma de uma label".
- **A dica "Ctrl+Shift+I"** fica a 3,9:1. O detector e a revisão concordam.

**Correção.**
- `SECONDARY` no corpo do modal.
- Um ícone de colar, por exemplo `ClipboardPaste`, em `currentColor`.
- Tag de estado sem o preenchimento de chip.
- A dica em texto secundário.

**Comando:** `$impeccable polish`

## Alertas por persona

**Alex (usuário avançado)**
- Do ícone Issues até a primeira linha são 12 Tabs. A captura se abre ao receber foco e tem 8 controles no caminho.
- No filtro, ↓ não leva aos resultados (são ~17 Tabs até a primeira linha), e não há atalho para abrir o filtro.
- Depois do Colar, fica preso no terminal do orquestrador, onde o Ctrl+Shift+I não vale (decisão da especificação).
- O Concluir só existe no painel do detalhe, e o Desfazer exige F6.

**Sam (teclado e leitor de tela)**
- A recusa do título vazio é só cor e movimento, e a criação pela captura não é anunciada.
- O título (`h1`) do detalhe é lido "…barra de título#13", sem espaço antes do número.
- Depois de Concluir, o foco cai no `body`. O toast fala inglês e é um `dialog` sem nome.
- O Colar compacto mede 20 px, dentro de uma linha clicável de 24 px.

**Rafa (o desenvolvedor do PRODUCT.md, com o app aberto o dia todo)**
- Enquanto aparece "Conectando ao GitHub…", a captura não existe. Justamente quando ele quer anotar no meio de outra tarefa.
- Escolhe a issue pelo número e pela cor, porque o título não cabe.
- Muda de ideia antes de enviar e cai no P1.
- Com uma falha permanente, vê a lista pulando a cada 15 s.
- Quando a rodada termina, nada diz "pronto para revisar": o painel só troca "trabalhando" por "livre".

## Observações menores

- O cabeçalho "Abertas 10" conta as criações pendentes, e o rodapé diz "9 abertas" (`src/issues-sidebar.tsx:621-622` contra 115-120).
- A Tag "Em andamento" repete o nome da seção dentro do próprio cartão Em andamento.
- No detalhe, o Abrir no GitHub fica entre os metadados, e o Reabrir é botão com texto. A especificação põe os dois na linha de ações, como `ActionIcon` (`docs/interface.md:65`).
- O placeholder especificado, "Anotar uma ideia… (Ctrl+Shift+I)", virou "Anotar uma ideia…" mais uma dica à parte, que é a de 3,9:1.
- "Conectando ao GitHub…" aparece duas vezes, na faixa e no detalhe.
- O Criar issue fica desabilitado sem dizer que falta o título.
- O Markdown do detalhe usa entrelinha de 1,8; o corpo no `DESIGN.md` é 1,5714.
- A 200 px, o nome do repositório vira "orque…", mas o contador de segundos não encolhe (`shrink-0`, `src/issues-sidebar.tsx:354`).
- O estado offline não diz que tenta de novo sozinho.
- Na indisponibilidade, o botão principal (Rodar no Terminal) fica entre os dois secundários.
- A alça de redimensionar a faixa é um `div` focável sem nome.
- O `.impeccable/design.json` está defasado em relação ao `DESIGN.md`: libera cor fixa nas telas das Issues inteiras, e o `DESIGN.md` limita isso aos ícones de estado. O `$impeccable document` refaz o par.

## Perguntas para pensar

1. Se colar não é enviar, de quem é o rascunho? Ou o app rastreia o que colou e diz o que há lá, ou para de anunciar trocas que o rascunho não reflete.
2. As bolinhas de label valem um terço do título numa linha de 24 px? O VS Code mostraria isso?
3. Num produto cujo centro são agentes em paralelo, uma issue em andamento por vez é a abstração certa?
4. O fim da rodada não deveria ser um momento ("Rodada concluída: revisar e concluir a #9?"), em vez de uma troca silenciosa para "livre"?
