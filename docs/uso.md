# Uso

## Visão geral

Um app nativo para Windows (Rust e GPUI) que põe CLIs de IA para trabalhar em papéis separados sobre a pasta de um projeto, de preferência um repositório Git. O orquestrador alinha o objetivo com o usuário, lê o projeto, delega, faz stage e commit do trabalho da sessão e consulta o usuário antes de push; os executores recebem as tarefas e trabalham na pasta do projeto em paralelo. As ferramentas `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## Como se usa

1. Ao abrir, o app confere a versão e atualiza sozinho se houver uma nova, e depois instala ou atualiza `claude` e `codex` ([atualização](atualizacao.md)); na primeira vez de cada CLI, o login é pedido no terminal dele. Enquanto isso, por trás dessa verificação, os projetos da última vez reabrem, na mesma ordem e com a mesma aba ativa, se as pastas ainda existirem. Nenhum orquestrador inicia sozinho. Uma pasta dentro de um repositório abre a raiz dele; uma pasta fora de repositório abre sem os recursos de Git (ver [interface](interface.md)).
2. Há um só orquestrador por projeto, e é no terminal dele que você conversa; os executores só mostram o que fazem. Sem Git na pasta, o que os executores mudarem não fica registrado e não pode ser desfeito pelo Git.
3. Converse com o orquestrador e feche o objetivo.
4. Na primeira delegação de cada sessão do orquestrador, o app pede ferramenta, modelo e esforço dos executores; a última escolha feita vem marcada. Os executores seguintes da sessão usam essa escolha, que o botão `Executores` troca para os próximos.
5. O orquestrador delega um lote e fica ocupado até todos voltarem. Acima do teto de simultâneos, os excedentes ficam na fila. O teto não é ajustado à mão (ver [configuração](#configuração)), e a fila anda sozinha quando a memória livre volta.
6. A resposta de cada executor volta sozinha ao orquestrador. O botão `parar` interrompe só aquele executor; o orquestrador recebe `interrompido pelo usuário` no lugar da resposta.
7. Fechar o app com terminais vivos encerra todos, inclusive os shells do painel Terminal. Shells e CLIs não voltam na próxima abertura; abas, layout e alterações não salvas do editor voltam, inclusive os rascunhos sem projeto.

O instalador põe o app no Abrir com do Explorer para extensões de texto e código sem torná-lo padrão. Com o app aberto, um arquivo ou pasta aberto pelo Explorer chega à mesma janela.

## Configuração

A última escolha de cada papel fica em `%LOCALAPPDATA%\orquestrador\config.json`. As abas abertas, a aba ativa, até 10 projetos recentes, as medidas dos painéis e o histórico da pesquisa ficam em `%LOCALAPPDATA%\orquestrador\interface.json`; fechar a última aba limpa a lista aberta e mantém os recentes.

```json
{
  "orchestrator": { "tool": "codex", "model": "", "effort": "" },
  "executor": { "tool": "claude", "model": "", "effort": "" }
}
```

Modelo e esforço vazios deixam o CLI usar seus padrões. Um JSON ilegível ou valor inválido é um erro; não há fallback silencioso para os padrões. Uma chave `maxParallel` de versões antigas é ignorada e some do arquivo no próximo salvamento.

O teto de simultâneos é automático (`crates/core/src/core/capacity.rs`): a CPU é lida uma vez, ao abrir o app, e a RAM e o commit a cada passe da fila, na mesma leitura. É um orçamento aproximado, não uma medida do peso de cada tarefa. Executor que já roda nunca é parado nem reiniciado por falta de memória: o teto só deixa de abrir novos. Se nada está vivo, um executor começa mesmo sem vaga, para a fila não parar. Se a memória não puder ser lida, o teto é o menor entre 2 e o da CPU, e o app abre do mesmo jeito.
