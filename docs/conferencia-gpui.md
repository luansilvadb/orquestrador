# Conferência visual do GPUI

Comparação do `orquestrador-gpui` (release, janela 1400x900, escala 1) com as 45 referências de
[`referencia-visual/`](referencia-visual/README.md). O app rodou sobre um repositório Git real em `%TEMP%` que reproduz o
cenário de `src/backend/mock.ts` (mesma árvore, preparadas, alterações, conflitos, grafo com `main`, `feat/painel-git`,
`origin/main` e a tag `v0.1.7`), com `LOCALAPPDATA` isolado. Executores e o portão dos CLIs usaram o build de debug com o
CLI falso (`src-tauri/examples/fake_cli.rs`, `ORQ_CLAUDE_EXE`, `ORQ_FAKE_DELEGAR`) e `ORQ_PORTAO=clis`.

Diferenças só de fixture (hashes, datas, um commit a mais no grafo, nomes reais do Git) não contam. "Corrigido" quer dizer
que a diferença era da bancada (`crates/app`) e foi resolvida nesta conferência.

| Referência | Situação | Diferença | Crate |
| --- | --- | --- | --- |
| `inicial-sem-projeto.png` | igual | | |
| `inicial-projeto.png` | diferença | Botões "dividir" e "mais" da área vazia do editor 7 px à direita. | editor |
| `titlebar-abas-projeto.png` | igual | | |
| `titlebar-tooltip.png` | corrigido | A dica herdava a margem de 12 px do Kit e ficava baixa e à esquerda. | app |
| `menu-arquivo.png` | diferença | Itens do `PopupMenu` 1 px mais baixos e menos respiro no topo; "Projetos recentes" com seta e 8 px menos de recuo. | ui (Kit) |
| `arquivos-editor.png` | igual | | |
| `arvore-hover-selecao.png` | diferença | Ctrl+clique troca a seleção em vez de somar (App.tsx perde o destaque); a dica da linha aparece no hover. | arvore |
| `arvore-menu-contexto.png` | diferença | A dica da linha fica por cima do menu de contexto. | arvore |
| `arvore-menu-pasta.png` | diferença | Idem. | arvore |
| `arvore-renomear.png` | diferença | Seleção do nome em azul; no React é amarela com texto escuro. | arvore |
| `editor-varias-abas.png` | diferença | Ponto de "sujo" maior; abas 8 px mais largas. | editor |
| `editor-menu-aba.png` | diferença | A dica da aba fica por cima do menu; menu 22 px mais estreito, itens 1 px mais baixos, desabilitados menos apagados. | editor, ui (Kit) |
| `editor-lista-abas.png` | igual | Lista 6 px à direita e 3 px acima; minimapa mais largo. | editor |
| `editor-dividido.png` | igual | | |
| `abrir-rapido.png` | diferença | Texto do campo em 14 px (React 16 px); linha de resultado 3 px mais alta. | editor |
| `markdown-codigo.png` | diferença | O bloco ```` ```ts ```` ganha realce embutido (React deixa sem realce); o `>` da citação fica cinza (React verde). | editor |
| `markdown-preview.png` | diferença | Código inline e bloco com fonte menor; tabela 3 px mais baixa. | editor |
| `conflito-merge.png` | igual | O Git juntou os dois trechos do fixture num só. | |
| `midia-imagem.png` | igual | | |
| `midia-audio.png` | igual | | |
| `midia-video.png` | igual | | |
| `midia-erro.png` | igual | O texto da falha vem do sistema. | |
| `arquivo-binario.png` | igual | Aviso 3 px mais abaixo. | editor |
| `pesquisar-resultados.png` | igual | | |
| `pesquisar-substituir.png` | igual | | |
| `pesquisar-confirmar.png` | diferença | A dica "Substituir Tudo" fica visível sobre o modal; máscara preta (o Lobe usa a cor do painel a 60%, que clareia o preto); corpo quebra uma palavra antes; botões 5 px mais largos. | pesquisa |
| `git-alteracoes.png` | diferença | Ordem dos arquivos por bytes (README.md antes de `assets/`); o React ordena com `localeCompare` pt-BR, sem caixa. Botão Commit 2 px mais baixo. | git |
| `git-diff-lado-a-lado.png` | diferença | Calha dos números 7 px mais estreita (a quebra das linhas muda); destaque de palavra mais fraco. | git |
| `git-diff-unificado.png` | diferença | Idem. | git |
| `git-grafo.png` | igual | | |
| `git-historico.png` | igual | | |
| `git-commit-diff.png` | diferença | Layout igual; arquivo de commit só renomeado (100% de similaridade) mostra "Não foi possível interpretar o diff." em vez do arquivo sem mudanças. | git |
| `git-sem-repositorio.png` | diferença | O texto quebra em pontos diferentes (linha ~4 px mais estreita). | git |
| `git-ausente.png` | igual | | |
| `orquestrador-escolha.png` | corrigido | Máscara, fundo do diálogo (`#1a1a1a`), borda do segmentado, respiro de topo e base, e o cursor do terminal que aparecia atrás antes do processo existir. | app |
| `orquestrador-select-modelo.png` | igual | | |
| `orquestrador-terminal.png` | igual | | |
| `executores-abas.png` | igual | Sem aba "na fila": os três executores do CLI falso rodam juntos. | |
| `executor-aguardando-escolha.png` | igual | | |
| `terminal-shells.png` | diferença | A dica "Novo terminal" fica no lugar antigo do botão depois que a faixa cresce. | ui |
| `terminal-lista-abas.png` | igual | Lista 4 px mais baixa. | ui |
| `fechar-confirmacao.png` | corrigido | Máscara e largura dos botões (preenchimento de 15 para 12 px). | app |
| `toast-sincronizado.png` | igual | | |
| `toast-subpasta.png` | corrigido | O aviso de pasta dentro de um repositório não existia. | app |
| `atualizacao-clis.png` | igual | | |
