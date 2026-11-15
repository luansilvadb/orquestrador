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
| `inicial-projeto.png` | corrigido | Botão "mais" do editor com 24 px e ícone de 14 px, como o "dividir": os dois no lugar. | editor |
| `titlebar-abas-projeto.png` | igual | | |
| `titlebar-tooltip.png` | corrigido | A dica herdava a margem de 12 px do Kit e ficava baixa e à esquerda. | app |
| `menu-arquivo.png` | corrigido | Menus do Kit (`vendor/gpui-component`) nas medidas do React: itens, folga, borda, anel, submenu sem seta encostado no menu e atalho a 70%. Fica 1 px acima porque o gatilho "Arquivo" da bancada está 1 px acima. | ui (Kit) |
| `arquivos-editor.png` | igual | | |
| `arvore-hover-selecao.png` | corrigido | Ctrl+clique soma ao destaque exibido (o arquivo aberto), como o `explorer-tree.tsx`. A dica da linha fica: o React mostra o `title` do nome, com o atraso de 500 ms do navegador. | arvore |
| `arvore-menu-contexto.png` | corrigido | A dica some quando o menu abre (ponto comum no `Tooltip` do Kit). | arvore |
| `arvore-menu-pasta.png` | corrigido | Idem (mesmo menu; não capturado à parte). | arvore |
| `arvore-renomear.png` | corrigido | Seleção como o `::selection` do Lobe em todos os campos: texto preto sobre `#ffef5c` (`Theme::selection_foreground`, patch no `gpui-base`). O editor de código mantém a seleção `#264F78` do CodeMirror sem trocar a cor do texto. | arvore, gpui-base |
| `editor-varias-abas.png` | corrigido | Ponto de "sujo" desenhado com 6,5 px (o ● da Inter era maior); faixa no tamanho certo com o botão "mais" corrigido. | editor |
| `editor-menu-aba.png` | corrigido | A dica some com o menu; largura (360 px), itens e desabilitados iguais ao React. | ui (Kit) |
| `editor-lista-abas.png` | corrigido | Lista no lugar (±1 px) com o botão "mais" corrigido; o minimapa só desloca o texto 4 px com a faixa do Git (com base), como o React. | editor |
| `editor-dividido.png` | igual | | |
| `abrir-rapido.png` | corrigido | Campo em 16 px; linha de resultado com 28 px. | editor |
| `markdown-codigo.png` | corrigido | Bloco cercado só ganha realce com a linguagem já carregada na sessão, como o Shiki (`langs: []`); o `>` já estava verde. | editor |
| `markdown-preview.png` | diferença | Corrigidos: fonte do código inline (13,5 px) e do bloco (12,6 px), contorno da tabela e da imagem por fora e a margem de 0,25 em ao lado do código inline (patch no texto do Kit). Resta: o parágrafo com código inline 2 px mais alto embaixo na referência; o mesmo CSS no Chromium (Playwright, Cascadia/Consolas, `line-height: 1`) não cresce a linha, então a causa não foi reproduzida. | editor, Kit |
| `conflito-merge.png` | igual | O Git juntou os dois trechos do fixture num só. | |
| `midia-imagem.png` | igual | | |
| `midia-audio.png` | igual | | |
| `midia-video.png` | igual | | |
| `midia-erro.png` | igual | O texto da falha vem do sistema. | |
| `arquivo-binario.png` | corrigido | Aviso com 16 px de preenchimento e linha de 22 px, como o `Note`. | editor |
| `pesquisar-resultados.png` | igual | | |
| `pesquisar-substituir.png` | igual | | |
| `pesquisar-confirmar.png` | corrigido | Sem dica sobre o modal; máscara do Lobe (`widgets::mascara`); caixa nas partes do `Modal` do Lobe (cabeçalho de 48 px, conteúdo, rodapé), fundo `#1a1a1a`, quebras e botões iguais. O corpo usa a quebra `pretty` comum (`widgets::texto_pretty`), com as mesmas quebras. O modal do editor (`modal` em `area.rs`) também usa a máscara comum. | pesquisa, editor |
| `git-alteracoes.png` | igual | Corrigido: a lista usa a colagem do ICU pt-BR (`orquestrador_ui::colagem`, como o `localeCompare` do Chromium; também na árvore, no abrir rápido e nas pastas da pesquisa); o campo da mensagem tinha 26 px (React 28) e subia o botão Commit. | git |
| `git-diff-lado-a-lado.png` | corrigido | Calhas, colunas e régua batem no pixel; a diferença era a quebra do lado novo: a calha dele leva 1 px a mais (números do lado novo ~0,5 px à direita na referência), e todas as quebras visíveis batem. Também: destaque de palavra com 18 px de altura, `type` de `import { type X }` em #C586C0 e divisor em 2 px a meia força. | git |
| `git-diff-unificado.png` | igual | Corrigidos o destaque de palavra e o `type`, como no lado a lado. | git |
| `git-grafo.png` | igual | | |
| `git-historico.png` | igual | | |
| `git-commit-diff.png` | igual | Corrigido: patch com cabeçalho e sem blocos (renomeação pura, modo trocado, arquivo vazio) abre o comparador vazio, como o `parsePatchFiles`. | git |
| `git-sem-repositorio.png` | igual | Corrigido: a quebra era o `text-wrap: pretty` global do Lobe (redistribui as últimas linhas quando a última fica com uma palavra), portado em `orquestrador_ui::quebra` e aplicado aos corpos de diálogo, avisos, estados vazios e erros (`widgets::texto_pretty`). Texto 1 px mais baixo. | git |
| `git-ausente.png` | igual | | |
| `orquestrador-escolha.png` | corrigido | Máscara, fundo do diálogo (`#1a1a1a`), borda do segmentado, respiro de topo e base, e o cursor do terminal que aparecia atrás antes do processo existir. | app |
| `orquestrador-select-modelo.png` | igual | | |
| `orquestrador-terminal.png` | igual | | |
| `executores-abas.png` | igual | Sem aba "na fila": os três executores do CLI falso rodam juntos. | |
| `executor-aguardando-escolha.png` | igual | | |
| `terminal-shells.png` | corrigido | A dica some ao clicar e não volta até um novo hover (como o Base UI). Vale também para as dicas da barra de status e das abas de projeto, agora no `tooltip_texto` comum. | ui, app |
| `terminal-lista-abas.png` | corrigido | A lista ancora no gatilho, 6 px abaixo (`sideOffset` do Lobe), com borda e anel do menu do React; fica 1 px abaixo porque o botão da bancada está 1 px abaixo. | ui |
| `fechar-confirmacao.png` | corrigido | Máscara e largura dos botões (preenchimento de 15 para 12 px). | app |
| `toast-sincronizado.png` | igual | | |
| `toast-subpasta.png` | corrigido | O aviso de pasta dentro de um repositório não existia. | app |
| `atualizacao-clis.png` | igual | | |
