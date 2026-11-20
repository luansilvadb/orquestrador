# Atualização e release

## Atualização automática

Decisão do dono: o portão de abertura é bloqueante por inteiro. Sem internet o app não abre, e `claude` e `codex` são exigidos e atualizados para a última versão a cada abertura, sem faixa de versão fixada.

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
