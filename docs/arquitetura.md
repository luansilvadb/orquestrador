# Arquitetura

## Regras

O compilador não impõe estas regras; quem altera o código as mantém.

- Em `crates/core/src/`, `ide/` não depende de `agents/` nem de `core/`.
- Chamadas ao sistema operacional no `orquestrador-core` ficam só em `platform/`.
- Comando externo com prazo roda só por `proc.rs`.
- Permissões dos agentes ficam só em `agents/cli.rs`, e a regra de cada papel só em `agents/roles.rs`.
- Telas ficam em crates que não dependem de `crates/app`.

## Decisões

- O centro são os agentes em paralelo; o produto não é chat com IA nem IDE com assistente.
- GPUI nativo no lugar de React, Tauri e WebView2 (v0.2.0): uso baixo de memória, CPU e GPU.
- Mídia pelo Media Foundation no lugar do FFmpeg (v0.2.3): menos manutenção e dependência menor; mídia fica fora do centro do produto e o Windows a mantém.
- Sem memória entre sessões: o executor começa do zero e a sessão do orquestrador não é restaurada.
- Executores na mesma pasta, sem worktree: a independência das tarefas de um lote é responsabilidade do orquestrador.
- Executor de turno único: o primeiro fim de rodada o encerra; pergunta ou parada precoce volta como relato.
- Alçada só pelo texto dos papéis, sem trava no app (ver [segurança](seguranca.md)).
- A bancada é apoio aos agentes: LSP, depurador, tarefas de build e extensões ficam fora.
- A revisão do trabalho dos agentes usa a tela Git; não há diff por lote.
- Portão bloqueante: o app e os dois CLIs na última versão a cada abertura; sem internet não abre.
