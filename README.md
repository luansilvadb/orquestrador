# Orquestrador

![Orquestrador](docs/screenshot-v1.png)

App para Windows (Tauri) que põe CLIs de IA para trabalhar sobre a pasta de um projeto, de preferência um repositório Git. Um **orquestrador** conversa com você, lê o projeto e delega tarefas. Os **executores** recebem essas tarefas, cada um numa aba do painel, e trabalham na mesma pasta, em paralelo. Os CLIs `claude` e `codex` podem ocupar qualquer um dos dois papéis.

## Instalar

1. Baixe o instalador `Orquestrador_<versão>_x64-setup.exe` na [página da última release](https://github.com/luansilvadb/orquestrador/releases/latest) e execute-o. A instalação é por usuário, sem administrador.
2. Abra o app. Ele confere a versão e se atualiza sozinho quando há uma nova ([detalhes](docs/atualizacao.md)).

Requisitos:

- Windows com WebView2.
- Os CLIs `claude.exe` e/ou `codex.exe` (binários nativos; o atalho do npm não serve) no `PATH`, instalados e **já autenticados**. Só os que você for usar.
- Git (recomendado). Sem ele, o app abre a pasta, mas sem os recursos de Git.

## Usar

1. Escolha a pasta do projeto. Uma pasta fora de repositório também abre, sem os recursos de Git.
2. No painel do Orquestrador, à direita, clique em `Abrir terminal`, escolha ferramenta, modelo e esforço, e converse com ele até fechar o objetivo.
3. Quando ele delega, cada executor ganha uma aba. A resposta volta sozinha ao orquestrador quando o executor termina.

O passo a passo está em [uso](docs/uso.md), a tela em [interface](docs/interface.md) e a delegação em [delegação](docs/delegacao.md).

> **Atenção:** os executores têm permissão total na pasta do projeto (Claude em `bypassPermissions`, Codex sem sandbox), e o orquestrador faz stage e commit dentro da alçada dele e consulta você antes de push. Abra só pastas em que você confia. Limites, exceções e o que foi medido estão em [limites de segurança](docs/seguranca.md).

## Desenvolvimento

```powershell
npm install
npm run tauri dev
```

Pré-requisitos: Node.js, Rust com a toolchain MSVC (exige o Microsoft C++ Build Tools) e WebView2. `npm run tauri dev` abre o app sem consultar atualizações. Para gerar o instalador, veja [desenvolvimento](docs/desenvolvimento.md).

## Testes

```powershell
npm run check
cd src-tauri
cargo test
```

As checagens de interface (`npm run check:ui`) e os demais detalhes estão em [desenvolvimento](docs/desenvolvimento.md).

## Contribuir

- Antes de mudar o código, leia [arquitetura](docs/arquitetura.md) para saber onde cada parte fica.
- Mudanças nas permissões dos agentes pedem cuidado. As permissões ficam só em `cli.rs` e a regra de cada papel só em `roles.rs`. O resumo está em [limites de segurança](docs/seguranca.md).
- Para publicar uma versão, siga [atualização e release](docs/atualizacao.md).

## Documentação

| Documento | Conteúdo |
| --- | --- |
| [uso](docs/uso.md) | Passo a passo, aviso de Git ausente, configuração e teto de executores |
| [interface](docs/interface.md) | Telas, painéis, editor, pesquisa, Git e terminal |
| [delegação](docs/delegacao.md) | Como o orquestrador delega e limites |
| [limites de segurança](docs/seguranca.md) | Permissões dos agentes e o Git do painel |
| [atualização e release](docs/atualizacao.md) | Atualização automática, chave de assinatura e publicação |
| [arquitetura](docs/arquitetura.md) | Mapa dos arquivos do código |
| [desenvolvimento](docs/desenvolvimento.md) | Build, testes e checagens |
| [design system](docs/design-system.md) | Design system (Lobe UI) do app |
| [notas de release](docs/releases/README.md) | Modelo das notas e checklist de release |
