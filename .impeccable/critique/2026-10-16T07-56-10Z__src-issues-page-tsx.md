---
target: tela Issues
total_score: 25
max_score: 40
na_heuristics: 
p0_count: 0
p1_count: 3
target_identity: "file:D:\\orquestrador\\src\\issues-page.tsx"
target_fingerprint: "sha256:0f72ef6c41a4e14f65572088b93d165bf454fc2a2dc1fa2effd41fac29658974"
target_path: "D:\\orquestrador\\src\\issues-page.tsx"
timestamp: 2026-10-16T07-56-10Z
slug: src-issues-page-tsx
---
# Crítica: tela Issues (src/issues-page.tsx, área, faixa e Nova issue)

## Nota de saúde do design: 25/40 (Aceitável)

| # | Heurística | Nota | Problema principal |
|---|---|---|---|
| 1 | Visibilidade do estado | 3 | "Open 8" na página contra "Abertas 7" + 1 em andamento na faixa |
| 2 | Linguagem do usuário | 3 | New issue/Open/Closed/Labels ao lado de Nova issue/Criar issue |
| 3 | Controle e liberdade | 3 | Desfazer do Concluir só num toast de 8 s |
| 4 | Consistência | 2 | Enter na faixa = Iniciar; clique no título da página = navegador; Concluir/Reabrir só na faixa |
| 5 | Prevenção de erros | 3 | Iniciar não envia; Concluir sem rodada e Descartar confirmam |
| 6 | Reconhecer em vez de lembrar | 2 | Iniciar só no hover; "Labels 1" não diz qual; labels truncadas na faixa |
| 7 | Flexibilidade e eficiência | 2 | Página sem atalhos, ~22 paradas de Tab, sem lote |
| 8 | Estética e minimalismo | 2 | Mesma lista duas vezes; offline com mensagem e Tentar de novo dobrados |
| 9 | Recuperação de erros | 3 | Mensagens claras, Copiar comando, criação falhada com Tentar de novo |
| 10 | Ajuda e documentação | 2 | Nenhum atalho visível na página |

## Veredito de especificidade
Faixa específica (colada vs. trabalhando, próxima, ocupado com #N, captura Ctrl+Shift+I, ideias guardadas offline). Página central é clone do GitHub.
Varredura: CLI 1 achado real (side-tab issues-sidebar.tsx:576). Manual: placeholders #555 a 2,6-2,8:1 (issues-page.tsx:106, issues-sidebar.tsx:243/502/510, new-issue.tsx:78/97); #007fd4 literal em new-issue.tsx:84; fora da tela: Text secondary #6f6f6f 3,9:1 (App.tsx, terminal-screen.tsx:278, titlebar.tsx:75), badge Git 9 px 3,6:1 (explorer.tsx:122). Erros agora a 9,0:1/5,8:1. Overlay em 4 vistas: 5-6 low-contrast cada.

## O que funciona
1. Estado honesto: colada vs. trabalhando; confirmação do Concluir só sem rodada (issues-sidebar.tsx:561-571).
2. Captura sem atrito: Ctrl+Shift+I, Esc devolve foco, Enter cria em série, guardada offline, reduced-motion.
3. Teclado da faixa e foco #007fd4 a 5,1:1.

## Problemas prioritários
- [P1] Página central duplica a faixa; a 1100×720 fica com ~240 px e títulos em 3 linhas (issues-page.tsx:195-232, App.tsx:909-926). Correção: papel próprio, detalhe da issue ou tabela de triagem densa. Comando: distill, layout.
- [P1] Hierarquia invertida: Iniciar opacity-0 até hover (issues-page.tsx:27, issues-sidebar.tsx:696) e cobre o título na faixa; New issue verde é o mais saturado. Correção: Iniciar sempre visível em slot fixo; New issue padrão. Comando: bolder, quieter.
- [P1] Leitor de tela: role="status" com relógio de 1 s (issues-sidebar.tsx:377, :83); aria-label da linha esconde metadados (:724); placeholders 2,6:1 como único rótulo; Desfazer em toast de 8 s. Correção: anunciar só mudanças, aria-labelledby/describedby, placeholder text-secondary, "Fechada · Reabrir" persistente. Comando: harden.
- [P2] Faixa e página inconsistentes: título abre navegador (issues-page.tsx:206-211), sem ações na linha em andamento, fechadas sem Reabrir, contagem 8 vs 7+1. Correção: ações compartilhadas em issue-ui.tsx, ExternalLink secundário. Comando: clarify.
- [P2] Idioma misto (issues-page.tsx:91,131-132), pílula "em andamento" com forma de label, filete âmbar (issues-sidebar.tsx:576). Correção: pt-BR, Tag com ícone, tom em vez de borda. Comando: clarify, polish.

## Pontos fracos por persona
- Alex: sem / c j/k na página; 2 Tabs por linha; Labels (10) sem filtro digitando; sem lote; uma só "próxima".
- Sam: relógio anunciando; linha esconde metadados; Open/Closed aria-pressed; "Abra o orquestrador…" lido 8 vezes; foco pós-Criar no contêiner.
- Dev VS Code/GitHub: título leva ao navegador; busca sem qualificadores; sem descrição/comentários na página.

## Observações menores
Anel ring-ring/50 da aba Issues (issues-area.tsx:119); "Nenhuma issue corresponde à busca" com só o filtro de labels; issue criada sem destaque; captura + filtro ocupam meia faixa a 720 px; motivo do New issue desabilitado só no title.

## Perguntas para pensar
- Se a faixa é a fila, o que só a página faz?
- Uma fila com uma só "próxima" é fila?
- Por que o Iniciar é hover-only e o New issue é o mais saturado?
- Lugar de ler e decidir uma issue, ou atalho para o navegador?
