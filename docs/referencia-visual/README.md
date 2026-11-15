# Referência visual

Aparência da interface React, capturada contra o app simulado (`src/backend/mock.ts`), viewport 1400x900, escala 1.

Recapturar:

```
node scripts/capturar-referencia.mjs
```

Porta do Vite: 1431 (`REF_PORT` muda). Toda captura parte de uma página nova com a pasta simulada aberta (Arquivo > Abrir pasta), salvo indicação.

| Arquivo | Tela/estado | Como |
| --- | --- | --- |
| `inicial-sem-projeto.png` | Janela inicial sem projeto | `?project=none` |
| `inicial-projeto.png` | Projeto recém-aberto (tela Arquivos, sem editor) | Arquivo > Abrir pasta |
| `titlebar-abas-projeto.png` | Titlebar com duas abas de projeto | Botão "Abrir pasta" da titlebar abre o segundo projeto (simulado-2) |
| `titlebar-tooltip.png` | Tooltip de botão da titlebar | Hover em "Fechar" da titlebar |
| `menu-arquivo.png` | Dropdown do menu Arquivo | Clique em "Arquivo" na titlebar |
| `arquivos-editor.png` | Árvore expandida e código no editor com realce, git gutter e minimapa | Expande src, docs; duplo clique em src/editor-workspace.tsx (modificado no mock) |
| `arvore-hover-selecao.png` | Hover e seleção múltipla na árvore | Clique em App.tsx, Ctrl+clique em main.tsx, hover em README.md |
| `arvore-menu-contexto.png` | Menu de contexto de arquivo na árvore | Botão direito em docs/design-system.md |
| `arvore-menu-pasta.png` | Menu de contexto de pasta na árvore | Botão direito em src |
| `arvore-renomear.png` | Campo de renomear inline na árvore | Botão direito em README.md > Renomear... |
| `editor-varias-abas.png` | Várias abas de editor, uma suja | Abre App.tsx, main.tsx, longo.ts, README.md; digita no README |
| `editor-menu-aba.png` | Menu de contexto de aba do editor | Botão direito na aba App.tsx |
| `editor-lista-abas.png` | Dropdown "Lista de abas" do editor | Abre 3 arquivos e clica em "Lista de abas" |
| `editor-dividido.png` | Editor dividido em dois grupos | Abre App.tsx e Ctrl+\\ (dividir) |
| `abrir-rapido.png` | Abrir rápido (Ctrl+P) | Ctrl+P e digita "app" |
| `markdown-codigo.png` | Markdown em modo código com seletor Preview | Markdown | Cria docs/guia.md no mock e abre |
| `markdown-preview.png` | Preview de Markdown renderizado | Mesmo arquivo, clique em Preview |
| `conflito-merge.png` | Tela de conflito de merge com 2 trechos | `__mock.addConflict(src/dois.ts)`; Git > duplo clique no arquivo |
| `midia-imagem.png` | Aba de imagem (assets/logo.png) | Expande assets, clique em logo.png |
| `midia-audio.png` | Aba de áudio (WAV) | `__mock.addFile(midia/tom.wav)`; duplo clique |
| `midia-video.png` | Aba de vídeo (WebM) | `__mock.addFile(midia/clipe.webm)`; duplo clique |
| `midia-erro.png` | Mídia com falha de leitura | `__mock.failRead` em midia/quebrado.png; duplo clique |
| `arquivo-binario.png` | Arquivo grande demais para o editor | Duplo clique em dados/exportacao.json |
| `pesquisar-resultados.png` | Tela Pesquisar com resultados | Cria src/termo-*.ts; Pesquisar "termo" |
| `pesquisar-substituir.png` | Pesquisar com substituição aberta e prévia | Idem + "Ativar/Desativar Substituição" e "novo" |
| `pesquisar-confirmar.png` | Diálogo de confirmação do Substituir Tudo | Idem + "Substituir Tudo" |
| `git-alteracoes.png` | Tela Git com Preparadas e Alterações | Botão Git na navegação |
| `git-diff-lado-a-lado.png` | Comparador lado a lado | Git > duplo clique em src/editor-workspace.tsx (Alterações) |
| `git-diff-unificado.png` | Comparador unificado (inline) | Idem + "Unificado" na barra |
| `git-grafo.png` | Grafo do Git na tela Git | Git; seção Grafo |
| `git-historico.png` | Aba Histórico do Git | Git > Histórico |
| `git-commit-diff.png` | Commit aberto e diff de arquivo do commit | Histórico > 4b2e8d0 > src/lib/utils.ts |
| `git-sem-repositorio.png` | Git em pasta sem repositório | `?git=no-repo`; Git |
| `git-ausente.png` | Git ausente no sistema | `?git=no-git`; Git |
| `orquestrador-escolha.png` | Diálogo de escolha do Orquestrador (ferramenta, modelo, esforço) | Botão "Abrir terminal" |
| `orquestrador-select-modelo.png` | Dropdown do Modelo no diálogo | Idem, Claude Code no segmentado e clique no Select de Modelo |
| `orquestrador-terminal.png` | Painel do Orquestrador com terminal | Abrir terminal > confirmar |
| `executores-abas.png` | Abas de executores (executando e na fila) | Orquestrador aberto + `__mock.addNode` x2 |
| `executor-aguardando-escolha.png` | Executor aguardando escolha (diálogo dos executores) | Orquestrador aberto + `__mock.addNode` com state awaiting-choice |
| `terminal-shells.png` | Painel Terminal embaixo com 3 abas de shell | Ctrl+` e "Novo terminal" x2 |
| `terminal-lista-abas.png` | Lista de abas do painel Terminal | Idem + "Lista de abas" |
| `fechar-confirmacao.png` | Modal de fechar com alterações não salvas | Edita README e `__mock.close()` |
| `toast-sincronizado.png` | Toast de sucesso | "Sincronizar as alterações" na barra de status |
| `toast-subpasta.png` | Toast informativo (pasta dentro de repositório) | `?subfolder=1` |
| `atualizacao-clis.png` | Portão de atualização dos CLIs | `?update=clis` |

## Ausentes

- Saída real do orquestrador e dos executores no terminal: o simulado não gera texto nesses terminais (só o shell ecoa), então o painel aparece vazio.
- Diálogo de status de atualização do app (fases além de `clis`): o simulado só para na fase `clis`; as demais devolvem `ready` direto.
- Maximizada e minimizada da janela nativa: dependem do Tauri.
- Animações e transições: capturadas sempre no estado final.
