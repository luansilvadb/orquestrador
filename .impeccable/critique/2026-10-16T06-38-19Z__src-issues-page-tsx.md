---
target: issues
total_score: 23
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 2
target_identity: "file:D:\\orquestrador\\src\\issues-page.tsx"
target_fingerprint: "sha256:6a1dcc2103e538a9ee4232314cda5151d97b1fee76c3a201d822526678221cc9"
target_path: "D:\\orquestrador\\src\\issues-page.tsx"
timestamp: 2026-10-16T06-38-19Z
slug: src-issues-page-tsx
---
# Crítica: tela Issues (src/issues-page.tsx e faixa lateral)

## Nota de saúde do design: 23/40 (Aceitável)

| # | Heurística | Nota | Problema principal |
|---|---|---|---|
| 1 | Visibilidade do estado | 3 | "Em andamento" começa ao colar, não ao enviar; falha de atualização é só um ponto âmbar de 11 px |
| 2 | Linguagem do usuário | 2 | Inglês e português misturados; "Na fila" colide com a fila dos executores |
| 3 | Controle e liberdade | 2 | Concluir fecha no GitHub sem confirmação nem Desfazer; descartar criação falha sem perguntar; Esc não limpa busca/filtro |
| 4 | Consistência | 2 | Dois filtros independentes; linhas de 44 px contra 22-24 px do Git; 10,5/11/12,5 px fora da escala |
| 5 | Prevenção de erros | 2 | Dá para Concluir sem envio; a troca automática pela próxima devolve a anterior em silêncio |
| 6 | Reconhecer em vez de lembrar | 3 | Iniciar só no hover; motivo de desabilitado só no `title` |
| 7 | Flexibilidade e eficiência | 3 | Página sem setas; sem atalho para Concluir; menu Labels não filtra digitando |
| 8 | Estética e minimalismo | 2 | Mesma lista duas vezes lado a lado; 10 chips no filtro |
| 9 | Recuperação de erros | 2 | Crases literais, cinza terciário, sem copiar; sem gh não dá para anotar |
| 10 | Ajuda e documentação | 2 | Nada explica fila/próxima/em andamento nem que o Iniciar não envia |

## Veredito de especificidade
A especificidade está na faixa: o Iniciar sabe o estado do orquestrador, a "próxima" é colada sozinha e a captura é otimista. A página central replica o GitHub e repete a lista da faixa.
Varredura automática: 12 achados no CLI (11 de font-size 11 px reais; 1 side-tab tolerado em issues-sidebar.tsx:539). Faltaram as props fontSize={11}/{10.5}. No navegador: a label "question" fica em 2,8:1 por causa de labelTextColor (issue-text.ts:42-48); Text type="secondary" resolve para #6f6f6f, inclusive nas mensagens de erro; placeholders em #555 (vem do tema).

## O que funciona
1. Iniciar sensível ao estado do orquestrador (issue-ui.tsx:171-203).
2. Captura sem atrito e sem perda (criação otimista, falha guardada, confirmação ao descartar).
3. Teclado na faixa (uma parada de Tab, setas, Home/End, prévia com aria-describedby).

## Problemas prioritários
- [P1] A página central quebra em 1100×720: títulos letra por letra, Labels e ordenação fora do overflow-hidden (issues-page.tsx:75,87-117). Correção: @container, segunda linha abaixo de ~520 px, Iniciar sobreposto em vez de START_SLOT. Comando: adapt.
- [P1] "Em andamento" e Concluir não correspondem ao que aconteceu: em andamento ao colar; Concluir fecha no GitHub sem confirmação ou Desfazer; a próxima substitui em silêncio. Correção: estado "colada, aguardando envio", Desfazer no aviso, aviso de troca. Comando: harden.
- [P2] Duas listas iguais e uma "fila" que não é fila; inglês e português misturados. Correção: papéis distintos (faixa = fila de trabalho, página = triagem com fixadas no topo) ou "Abertas"; tudo em pt-BR. Comando: distill, clarify.
- [P2] Contraste e foco: ring-ring/50 a ~1,8:1; foco do Input invisível; Iniciar com visibility:hidden; labelTextColor por YIQ; erros em terciário. Correção: anel focus-blue de 2 px, opacity-0, escolha preto/branco por contraste WCAG, erros em text-secondary. Comando: audit, polish.
- [P2] A issue criada pela aba Nova issue some atrás da busca e dos filtros antigos (new-issue.tsx:42-46, issues-page.tsx:32-41). Correção: aviso "Issue #N criada", limpar filtros ou faixa "1 oculta pelos filtros". Comando: harden.

## Pontos fracos por persona
- Alex: duas paradas de Tab por issue na página; sem atalhos para busca ou Concluir; Esc não limpa; Labels sem filtro digitando; sem ações em lote.
- Sam: foco a ~1,8:1; Iniciar fora da árvore de acessibilidade; "Na fila9"; motivo de desabilitado só no title; o placeholder é o único rótulo.
- Dev vindo do VS Code/GitHub: título sai do app; sem "Clear filters"; linhas de 44 px; nada diz que o Iniciar só cola.

## Observações menores
Tipografia fora da escala e mono na descrição (new-issue.tsx:93); hover espreme as labels (issues-sidebar.tsx:684); rodapé "9 na fila" contra 1 com filtro; Closed limitado a 50 sem aviso; o verde de "New issue" é o elemento mais saturado; itens do menu Labels com ~33 px.

## Perguntas para pensar
- O que justifica a página central repetir a faixa na coluna do editor?
- "Em andamento" começa ao colar ou quando o orquestrador aceita a rodada?
- Uma fila com uma só "próxima" é uma fila?
- Por que o elemento mais saturado é o "New issue" e não o Iniciar?
