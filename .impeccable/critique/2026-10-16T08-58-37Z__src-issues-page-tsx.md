---
target: issues
total_score: 26
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 2
target_identity: "file:D:\\orquestrador\\src\\issues-page.tsx"
target_fingerprint: "sha256:f98d01ee664cd33733454e0fccc143f63934f72207b271549bac52020ac60d18"
target_path: "D:\\orquestrador\\src\\issues-page.tsx"
timestamp: 2026-10-16T08-58-37Z
slug: src-issues-page-tsx
---
# Crítica: tela Issues (src/issues-page.tsx + issues-sidebar, issue-ui, issues-area, new-issue)

## Nota de design: 26/40 (Aceitável)
| # | Heurística | Nota | Problema principal |
|---|---|---|---|
| 1 | Visibilidade do estado | 3 | Iniciar primário continua no detalhe da issue em andamento; "colada, aguardando envio" só na faixa |
| 2 | Linguagem do usuário | 3 | "Iniciar" só cola |
| 3 | Controle e liberdade | 3 | Colagem não se desfaz; a Próxima cola sem perguntar |
| 4 | Consistência | 2 | Dois primários de cores diferentes; linhas de 44 px num app de 24 px; erros duplicados |
| 5 | Prevenção de erros | 2 | Iniciar repetido cola de novo; Concluir deixa o texto no rascunho |
| 6 | Reconhecer em vez de lembrar | 3 | Ctrl+Enter e Ctrl+Shift+I só no title |
| 7 | Flexibilidade | 3 | Sem atalho para Nova issue e Concluir |
| 8 | Estética e minimalismo | 2 | Labels cortadas viram ruído; ações em dobro |
| 9 | Recuperação de erros | 3 | Erro de criação cortado, texto completo só no title |
| 10 | Ajuda | 2 | Ciclo Iniciar → Próxima sem explicação |

## Especificidade
Comportamento próprio do produto (cartão Em andamento, fila, Próxima); detalhe genérico, um clone do GitHub sem nada dos agentes. Detector: CLI com 0 achados; no navegador, 1 achado de low-contrast no badge "Aberta" (4,2:1), cuja cor vem do preset green do Tag do Lobe UI e não da paleta GH.

## Problemas prioritários
- [P1] Iniciar ativo e primário na issue em andamento (issues-page.tsx:99, issue-ui.tsx:219, App.tsx:391). Cola de novo. → clarify
- [P1] A linha da Próxima estoura a faixa (291 > 280) e as labels cortam (issues-sidebar.tsx:579-592). → layout
- [P2] Tags de estado abaixo do AA: Aberta 4,2:1, Fechada ~3:1 (issue-ui.tsx:388-405). → colorize
- [P2] Ações de risco sem consequência dita: Colar agora, Concluir com o texto no rascunho, markTurn em qualquer rodada (App.tsx:420). → harden
- [P2] Indisponível, Concluir/Voltar e "Limpar filtro" aparecem em dobro. → onboard, distill

## Personas
- Alex: sem atalhos para Nova issue e Concluir; sem ordenação.
- Sam: o cartão Em andamento não é parada de Tab (issues-sidebar.tsx:105-108); ~9 Tabs até a lista; contraste das Tags.
- Dev vindo do VS Code: linhas de 44 px; Iniciar que só cola; não vê executor nem arquivos no detalhe.

## Observações menores
Rodapé conta a criação que falhou; ações da linha que falhou só em ícone; coluna de ações de 26rem espreme o título; borda de foco da captura fica acesa; o link do editor completo não parece link; o repositório corta a 200 px.

## Perguntas
1. Por que o painel maior não mostra o progresso do agente?
2. Iniciar que só cola: renomear, ou fazer um Iniciar que envia?
3. A imitação do GitHub ajuda, ou cria um terceiro dialeto visual?
