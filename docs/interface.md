# Interface

Cada aba de projeto mantém a área completa dele, com editor, Pesquisa, Git, Orquestrador e shells; trocar de aba esconde sem desmontar, e os processos continuam rodando. Fechar uma aba com sessão do Orquestrador ou executores ativos encerra a sessão do projeto. O que volta ao reabrir o app está em [uso](uso.md#configuração).

## Tela principal

Sem projeto, o editor abre arquivos por caminho absoluto, mas Git gutter, árvore e pesquisa consideram só arquivos do projeto ativo.

As medidas dos painéis e o Terminal aberto ou fechado valem por projeto enquanto o app roda; o `interface.json` guarda só as últimas, e todos os projetos reabrem com elas.

## Painel do Orquestrador

Trocar de aba entre orquestrador e executores não desmonta nenhum terminal.

## Painel Terminal

Os shells não dependem do orquestrador: abrem com ou sem ele e continuam vivos quando ele termina. Esconder o painel não encerra os shells; fechar ou encerrar a última aba fecha o painel. Ao reabrir o app, o painel volta aberto ou fechado como estava, mas nenhuma sessão de shell é restaurada.

## Arquivos e editor

Formatar Documento altera só o buffer, numa edição que um Ctrl+Z desfaz, e não grava. Python, Rust e Go dependem de `ruff format` ou `black`, `rustfmt` e `gofmt` no PATH. As configurações vêm de `.prettierrc*`, `package.json` e `.editorconfig`; `package.json` com a chave `prettier` vence os `.prettierrc*` da mesma pasta, e o `.prettierignore` mais próximo acima do arquivo é respeitado. Overrides em YAML não são suportados, e arquivos de configuração JavaScript (`.prettierrc.js`, `prettier.config.*`) não são lidos.

Renomear não é recusado por rascunho: as abas e os rascunhos abertos seguem o arquivo para o novo caminho.

As medidas e cores do editor são fixas no código e vêm do Antigravity instalado (tema Dark Modern), lido sem alterar nada; a sintaxe usa o Shiki Dark Plus, sem semantic highlighting. Arquivos muito grandes ficam sem cor, sem minimap e sem marcadores do Git.

Os marcadores de mudança do Git comparam o buffer, inclusive não salvo, com o HEAD e com o índice, e a base é relida periodicamente, o que pega commit, stage e checkout feitos pelo painel Git, pelos terminais ou pelos executores; trocar a base não mexe no texto nem no Desfazer. Ficam sem marcadores arquivo fora de repositório, não rastreado, binário ou grande, o rascunho sem título, o editor de conflito e a mídia; erro do Git só tira os marcadores, sem aviso. Preparar e Tirar do preparo de um bloco gravam o índice calculado pela interface e são recusados se o índice mudou desde a leitura ou se há executores trabalhando. Restaurar um bloco é uma edição comum: entra no Desfazer e não grava o disco nem o índice.

## Imagem, áudio e vídeo

Mídia abre numa aba só de leitura, sem rascunho. Áudio e vídeo tocam pelo Media Foundation do Windows (`crates/midia`), então os formatos aceitos são os que o Windows decodifica; quando falta uma extensão da Microsoft Store (HEVC, VP9, AV1, MPEG-2 ou Web Media Extensions), a aba diz qual. A imagem é decodificada pelo conteúdo, não pela extensão. SVG e HTML abrem como texto no editor.

## Pesquisar

Com repositório, a lista de arquivos vem do `git ls-files`, sem os ignorados e sem `.git`. Sem repositório, vem da pasta percorrida com a crate `ignore`: respeita `.gitignore` e `.ignore` da pasta do projeto e das subpastas, mas não os de pastas acima; inclui ocultos; não segue links; pula `.git`, `node_modules` e subpasta ilegível. O Ctrl+P e a substituição usam a mesma lista. Binários e arquivos com CR solto (fora de um CRLF) ficam fora da pesquisa e da substituição.

A expressão regular usa a sintaxe do Rust (`regex`): sem lookaround e sem referências como `\1`; a substituição aceita `$1` e `${nome}`. Nos filtros de incluir e excluir, um trecho sem `./` casa em qualquer ponto do caminho (`src/` pega `docs/src/`); com `./`, vale a partir da raiz (`./src` não pega `src-tauri` nem `outro/src`).

Dispensar um resultado só o tira da tela, sem mexer no disco. A substituição é por arquivo inteiro e fica de fora no arquivo com alteração não salva no editor, no que tem ocorrência dispensada e no que passa do limite da prévia; o arquivo que mudou no disco depois da prévia é recusado e não é gravado.

## Git

Preparadas e Alterações são abas diferentes do mesmo arquivo: a versão preparada e a não preparada não se misturam. Descartar arquivo e Descartar bloco seguem os [limites de segurança](seguranca.md).

O comparador mostra o arquivo inteiro, sem esconder linhas, mas as ações por bloco agem sobre os blocos de 3 linhas de contexto do Git. Arquivo acima do limite do diff não abre no comparador. Commit é imutável: não tem ações, e a navegação é por trecho de mudança.

Com executor trabalhando, Receber, Sincronizar, trocar de branch e criar branch ficam travados até o lote terminar. O commit pelo painel é seu; o orquestrador também commita o trabalho da sessão, dentro da alçada dele. Preparar, tirar um bloco, descartar um bloco e gravar a resolução de um conflito rodam um por vez na interface; essa trava não substitui o bloqueio de executores do backend.

Inicializar repositório roda só `git init`, sem `-b` (vale a branch padrão do Git do usuário), sem commit e sem stage. Quando o Git recusa uma pasta que tem `.git` (dono diferente, repositório corrompido), o painel mostra o erro e não oferece inicializar. Um `.git` criado ou removido pelo terminal ou pelo orquestrador é percebido sozinho.

## Colar nos terminais

Em todos os terminais, Ctrl+clique abre URL, arquivo (na linha e coluna de `:l`, `:l:c` ou `(l,c)`, relativo à pasta do projeto) e pasta; hiperlinks OSC 8 também.

Num shell do painel Terminal, o app decide como colar pelos processos do próprio terminal a cada colagem: com `claude.exe` ou `codex.exe` rodando, cola como naquele CLI (com os dois, vale o de fora); sem nenhum, como shell comum. O CLI instalado pelo npm, que roda como `node.exe`, não é reconhecido.

Com uma imagem no clipboard, o app envia o gesto de colar imagem do CLI (`Ctrl+V` no Codex, `Alt+V` no Claude Code), e o CLI a anexa sem enviar a mensagem. Arquivos copiados no Explorer viram caminhos completos, que o CLI anexa como imagem ou deixa como caminho. Vários arquivos vão do jeito que cada CLI aceita (medido no Claude Code 2.1.302 e no Codex 0.163.0): no Claude, num paste só, um caminho por linha, e ele põe as imagens primeiro; no Codex, um paste por caminho, separados por F12, que ele ignora, na ordem da seleção. Num shell sem CLI, os caminhos vão separados por espaço, sem Enter, e uma imagem solta não tem o que colar.
