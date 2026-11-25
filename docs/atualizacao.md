# Atualização e release

## Atualização automática

Decisão do dono: o app se atualiza a cada abertura e, se não consegue verificar ou instalar a atualização (por exemplo, sem internet), não abre. `claude` e `codex` são exigidos: CLI ausente cuja instalação falha, ou `--version` que falha mesmo depois de reinstalar, bloqueia. Com a versão lida, cada um roda `update` para a última versão, sem faixa fixada; se o `update` falha, a versão instalada segue em uso, a próxima abertura tenta de novo e o aviso não segura o portão: aparece como notificação na janela principal, que some sozinha em 8 s ou com um clique. Quando o instalador oficial deixa a pasta do CLI fora do PATH do usuário, o portão a acrescenta logo depois de ler a versão, antes do `update`, para `claude` e `codex` funcionarem em qualquer terminal. Como o app usa o caminho absoluto e o PATH é só conveniência, a falha ao gravar não bloqueia: vira um aviso na tela do portão, com um botão Continuar. O Continuar dispensa o aviso pela chave CLI, pasta e código do erro do Windows, não pelo texto: mudar a redação não o traz de volta, mas outra pasta ou outro código traz, e cada CLI guarda só o último dispensado. Terminais abertos dentro de um app que já estava rodando antes da gravação (por exemplo, o terminal integrado do VS Code) herdam o PATH antigo até esse app ser reiniciado; o terminal do próprio orquestrador não, porque o portable-pty remonta o ambiente, inclusive o Path, pelo registro a cada terminal aberto.

O repositório é público, então o app baixa a release sem token. Se ele virar privado, a atualização automática para e a distribuição precisa mudar.

Limites aceitos: o instalador não tem assinatura Authenticode (só Minisign, conferida pelo app) e não há canal de pré-release.

Medido em 2026-11-14, na v0.1.47, com Claude Code 2.1.325 e Codex 0.169.0 em dia: `--version` em 72 ms e 126 ms; `claude update` em 2,0 s; `codex update` em 1,5 a 2,1 s. Os dois `update` saíram com código 0 com outro processo do mesmo CLI aberto. Não medido: `update` com versão nova disponível nessa situação e instalação pelos scripts oficiais sob a política de execução `Restricted` (só o carregamento dos cmdlets foi conferido).

## Chave de assinatura

- Chave privada: `%USERPROFILE%\.tauri\orquestrador-updater.key`, fora do repositório, com acesso só para o usuário e sem senha.
- Guarde um backup. Sem ela não há como publicar atualizações que os apps instalados aceitem; trocar a chave exige nova instalação manual em todos.
- Secret do workflow, uma vez (o conteúdo vai pela entrada padrão e não aparece no terminal):

```powershell
Get-Content -Raw "$env:USERPROFILE\.tauri\orquestrador-updater.key" | gh secret set TAURI_SIGNING_PRIVATE_KEY -R luansilvadb/orquestrador
```

## Publicar uma versão

Siga a checklist em [notas de release](releases/README.md).
