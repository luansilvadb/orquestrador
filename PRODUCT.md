# Product

<!-- impeccable:product-schema 1 -->

## Platform

web

App de desktop para Windows em Tauri: a interface é web (React no WebView2), sem linguagem visual nativa do sistema.

## Users

Hoje, o próprio autor: um desenvolvedor no Windows que já usa os CLIs `claude` e `codex` no terminal e quer pôr vários deles para trabalhar sobre o mesmo projeto. A intenção é abrir depois para outros desenvolvedores com o mesmo perfil (CLIs instalados e autenticados, projetos em repositório Git).

## Product Purpose

Pôr CLIs de IA para trabalhar sobre a pasta de um projeto. Um orquestrador conversa com o usuário, lê o projeto e delega tarefas; os executores recebem essas tarefas, cada um numa aba, e trabalham na mesma pasta, em paralelo. A resposta de cada executor volta sozinha ao orquestrador. Sucesso é o usuário fechar um objetivo com o orquestrador e acompanhar os executores sem sair do app.

## Positioning

Orquestração local de CLIs de IA reais (`claude` e `codex`, cada um podendo ser orquestrador ou executor) sobre a pasta do projeto, com editor, Git, pesquisa, terminais e issues do GitHub no mesmo lugar. Não é um chat com IA nem um IDE com assistente: o centro são os agentes trabalhando em paralelo.

## Operating Context

- Sessões longas, com o app aberto o dia todo ao lado do trabalho do usuário.
- Fluxo: escolher a pasta, abrir o terminal do orquestrador, escolher ferramenta, modelo e esforço, conversar, acompanhar executores, revisar diffs e fazer commit.
- Ferramentas em volta: Git, GitHub CLI (`gh`) para a tela Issues, os CLIs de IA no `PATH`.

## Capabilities and Constraints

- Telas: Arquivos (árvore e editor CodeMirror), Pesquisar, Git (controle de versão e comparador) e Issues (GitHub); painel do Orquestrador à direita e painel Terminal embaixo.
- A concorrência dos executores é automática (CPU e RAM), sem ajuste manual.
- Os executores têm permissão total na pasta do projeto; os limites estão em `docs/seguranca.md`.
- Terminais xterm ficam sempre montados: trocar de tela não pode desmontar a coluna central nem o painel do Orquestrador.
- Atualização automática pelo próprio app; instalação por usuário, sem administrador.
- Termos do produto: orquestrador, executor, delegação, fila, ferramenta, modelo, esforço.

## Brand Commitments

- Nome: Orquestrador. Interface e documentação em português do Brasil.
- Sem identidade visual própria além do tema: a interface segue o Lobe UI e as convenções do VS Code / Antigravity.

## Evidence on Hand

- Captura da interface: `docs/screenshot-v1.png`.
- Documentação: `README.md` e `docs/` (uso, interface, delegação, arquitetura, segurança, design system, notas de versão).
- Não há depoimentos, usuários externos nem métricas de uso; não inventar.

## Product Principles

1. Os agentes são o centro: tudo serve a delegar, acompanhar e revisar o trabalho deles.
2. Nunca perder estado: trocar de tela, redimensionar ou fechar não pode quebrar terminais nem apagar rascunhos.
3. Familiar antes de original: quem vem do VS Code deve reconhecer cada painel.
4. Explícito sobre risco: permissões e ações destrutivas são ditas em texto, não escondidas.
