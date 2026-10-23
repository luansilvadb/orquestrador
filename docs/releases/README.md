# Releases

Cada versão publicada tem um arquivo de notas aqui: `vX.Y.Z.md`. Ele é a única fonte das notas: o corpo da release no GitHub e o campo `notes` do `latest.json` saem dele, sem edição manual. A fonte tem o título `# Orquestrador vX.Y.Z`, que identifica a versão; o corpo da release no GitHub não o repete (o GitHub já mostra o nome da release), então começa em `## Resumo`. O app ainda não mostra essas notas; o campo fica no manifesto para quem o consulta.

## Critério de versão

- Versão é `X.Y.Z`, e a tag é `vX.Y.Z` com a mesma versão.
- A versão nova precisa ser maior que a última release publicada. O app não instala versão menor (não há downgrade), e o workflow recusa a tag que não é maior.
- Uma versão publicada nunca é refeita. Correção depois da publicação vira `X.Y.(Z+1)`.
- Mudança que quebra quem usa o app (dados, configuração ou atualização) precisa dizer isso em `Impacto da atualização`.

## Modelo do arquivo de notas

Copie para `docs/releases/vX.Y.Z.md`. O título e as sete seções são obrigatórias, nesta ordem, com texto em cada uma:

```markdown
# Orquestrador vX.Y.Z

## Resumo
Uma ou duas frases sobre o que a versão entrega e por que vale a atualização.

## Novidades
O que o usuário passa a ter. Uma entrada por funcionalidade, com o comportamento real.

## Melhorias e correções
Ajustes e correções, com o efeito para quem usa.

## Impacto da atualização
O que muda para quem já tem a versão anterior: configuração, arquivos, comportamento padrão.

## Requisitos e limitações
Sistema, dependências externas e limites reais (inclusive o que ainda não faz).

## Instalação
Link da release, nome do instalador e como quem já tem o app recebe a atualização.

## Comparação com a versão anterior
Tabela: área, versão anterior, versão nova. Cite a versão anterior pelo nome (por exemplo, v0.1.6).
```

Dois formatos, da mesma fonte:

- **Fonte** (`docs/releases/vX.Y.Z.md`, com o H1): é o que `check-notes` confere e o que vai inteiro no `notes` do `latest.json`.
- **Corpo da release** (no GitHub, sem o H1): `node scripts/release.mjs release-body X.Y.Z <arquivo temporário>` grava esse texto, em UTF-8, a partir da linha `## Resumo`. O workflow usa o mesmo formato, e `verify-body` confere o corpo publicado contra ele. Um H1 repetido, uma seção faltando ou um texto alterado são recusados.

## Como escrever

- Em português, com acentos. Cada afirmação deve vir do código ou dos arquivos da versão: `git diff <anterior>..HEAD` e os arquivos atuais. Nada de funcionalidade que não existe.
- Não deixe `TODO`, `TBD`, `XXX`, `{{ }}` nem comentários HTML: o check recusa.
- Não use o texto genérico de release automática. O check recusa o texto antigo.
- Mínimo de 1500 caracteres no total, e cada seção com texto de verdade.
- Links para a própria release (instalador, página da tag, comparação) usam a URL pública `https://github.com/luansilvadb/orquestrador/...`. Não use caminhos relativos (`docs/...`, `../`): quebram na página da release no GitHub.
- Descreva fallback de recursos com o limite real: por exemplo, "se a memória não puder ser lida, o teto fica em no máximo 2".

## Checklist de uma versão

1. Suba a versão nos cinco arquivos: `package.json`, `package-lock.json` (duas entradas: a raiz e `packages[""]`), `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (entrada `orquestrador`) e `src-tauri/tauri.conf.json`.
2. Escreva `docs/releases/vX.Y.Z.md` a partir do modelo.
3. Rode as checagens locais, sem publicar nada:
   - `node scripts/release.mjs check-versions X.Y.Z`
   - `node scripts/release.mjs check-notes X.Y.Z <versão anterior>`
   - `node tests/check-release.mjs` (regras de versão, notas, `latest.json` e a verificação Minisign, com assinaturas de teste geradas na hora)
   - `node tests/check-file-tabs.mjs`
   - `npm run build` e `cargo test --manifest-path src-tauri/Cargo.toml`
4. Faça o commit, crie a tag `vX.Y.Z` e envie a tag. O commit e a tag são do mantenedor; o workflow não cria nem move tag.
5. O workflow `.github/workflows/release.yml` faz o resto, nesta ordem:
   - confere a tag, as versões dos manifests, a ordem em relação à última publicada e as notas;
   - para se já existe release publicada para a tag (não altera release publicada);
   - roda `npm run build`, `check-file-tabs`, `check-release` e `cargo test`;
   - gera o instalador NSIS assinado e o `latest.json` em rascunho (`tauri-action`);
   - põe o texto das notas sem o título no corpo da release, põe o texto completo (com o H1) no `notes` do `latest.json`, e aponta a URL do instalador para a URL pública da tag;
   - confere o rascunho: baixa o instalador, o `.sig` e o `latest.json`; verifica a assinatura Minisign do instalador com a pubkey de `tauri.conf.json` (key id, assinatura do arquivo e comentário confiável); confere versão, URL, assinatura e notas do `latest.json`; e confere que o corpo da release é o texto das notas sem o título (só diferença de CRLF/LF e espaço final é ignorada; título repetido, seção faltando ou qualquer mudança de conteúdo é recusada; o campo `notes` do `latest.json` segue a regra de conteúdo, com o H1);
   - só então publica como a última release estável.

## Conferências fora do workflow

- Última release publicada, sempre pela tag e pela API: `gh api repos/luansilvadb/orquestrador/releases/latest --jq .tag_name`. Não use o campo `isLatest` de `gh release view`.
- Assinatura de um instalador real, em um diretório temporário fora do repositório (exemplo com a v0.1.6):

```powershell
gh release download v0.1.6 --repo luansilvadb/orquestrador --pattern "Orquestrador_0.1.6_x64-setup.exe*" --dir <pasta-temporária>
node scripts/release.mjs verify-installer 0.1.6 <pasta-temporária>\Orquestrador_0.1.6_x64-setup.exe <pasta-temporária>\Orquestrador_0.1.6_x64-setup.exe.sig
```

  A saída é a linha de confirmação com código 0. Instalador ou `.sig` alterados, ou assinados com outra chave, saem com código 1 e a mensagem do motivo.

## Correção editorial do corpo publicado

Se o texto do corpo de uma release já publicada estiver errado ou fora do formato (por exemplo, um H1 repetido), o corpo pode ser corrigido sem versão nova. Só o corpo muda:

```powershell
node scripts/release.mjs release-body X.Y.Z <pasta-temporária>\corpo.md
gh release edit vX.Y.Z --repo luansilvadb/orquestrador --notes-file <pasta-temporária>\corpo.md
node scripts/release.mjs verify-body <pasta-temporária>\corpo.md docs/releases/vX.Y.Z.md
```

Não mexe em tag, instalador, `.sig`, `latest.json`, versão nem nos flags de publicação (rascunho, pré-release, latest). Se o conteúdo das notas estiver errado, a fonte é corrigida; a release só recebe o corpo novo. Confira depois, pela API, que o nome, a tag e os assets continuam os mesmos.

## Se algo der errado

A situação da release depende do momento da falha. Dois casos:

- **Antes da publicação** (checks, build, instalador, conferência do rascunho): a release fica em rascunho, ou nem chega a ser criada. O app não a vê. Corrija a causa e rode o job de novo: o rascunho é refeito. Se o código mudou, a tag só pode ser movida enquanto não houver release publicada, e quem move é o mantenedor.
- **Durante a publicação** (`gh release edit --draft=false --latest`): se esse passo chegou a rodar, a release já pode estar publicada e ser a última, mesmo que o job tenha falhado depois. Não rode o job de novo e não mova a tag. Confira a API (`releases/latest`) e o `latest.json` público: se apontam para a tag, a release está publicada, e qualquer correção vira versão nova.
- Release já publicada: nenhum passo a altera. Para corrigir, publique uma versão nova.
- Chave de assinatura: o secret `TAURI_SIGNING_PRIVATE_KEY` é o conteúdo da chave privada, fora do repositório ([atualização e release](../atualizacao.md)). Sem ela, o build do instalador falha, porque o updater exige assinatura. Nunca escreva a chave em arquivo versionado.
