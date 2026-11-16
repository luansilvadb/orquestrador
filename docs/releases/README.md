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
- **Corpo da release** (no GitHub, sem o H1): `cargo xtask release release-body X.Y.Z <arquivo temporário>` grava esse texto, em UTF-8, a partir da linha `## Resumo`. O workflow usa o mesmo formato, e `verify-body` confere o corpo publicado contra ele. Um H1 repetido, uma seção faltando ou um texto alterado são recusados.

## Como escrever

- Em português, com acentos. Cada afirmação deve vir do código ou dos arquivos da versão: `git diff <anterior>..HEAD` e os arquivos atuais. Nada de funcionalidade que não existe.
- Não deixe `TODO`, `TBD`, `XXX`, `{{ }}` nem comentários HTML: o check recusa.
- Não use o texto genérico de release automática. O check recusa o texto antigo.
- Mínimo de 1500 caracteres no total, e cada seção com texto de verdade.
- Links para a própria release (instalador, página da tag, comparação) usam a URL pública `https://github.com/luansilvadb/orquestrador/...`. Não use caminhos relativos (`docs/...`, `../`): quebram na página da release no GitHub.
- Descreva fallback de recursos com o limite real: por exemplo, "se a memória não puder ser lida, o teto fica em no máximo 2".

## Checklist de uma versão

1. Suba a versão em `Cargo.toml` (`[workspace.package]`) e no `Cargo.lock` (entradas `orquestrador-core` e `orquestrador-app`).
2. Escreva `docs/releases/vX.Y.Z.md` a partir do modelo.
3. Rode as checagens locais, sem publicar nada:
   - `cargo xtask release check-versions X.Y.Z`
   - `cargo xtask release check-notes X.Y.Z <versão anterior>`
   - `cargo test -p xtask` (regras de versão, notas, `latest.json` e a verificação Minisign, com assinaturas de teste geradas na hora)
   - `cargo test --workspace`
4. Faça o commit, crie a tag `vX.Y.Z` e envie a tag. O commit e a tag são do mantenedor; o workflow não cria nem move tag.
5. O workflow `.github/workflows/release.yml` faz o resto, nesta ordem:
   - confere tag e manifests; recusa release já publicada; verifica a ordem em relação à última publicada e as notas;
   - restaura o cache de compilação, com os crates locais, (aquecido em `main` por `.github/workflows/aquecer-release.yml`) e o `cargo-packager`; os testes ficam com a CI, que roda em paralelo;
   - gera por `crates/app/empacotamento/empacotar.ps1` (cargo-packager) o instalador NSIS assinado e o `latest.json` com as notas completas (com o H1) e a URL pública da tag, e sobe tudo num rascunho com o texto das notas sem o título no corpo;
   - confere o rascunho: baixa o instalador, o `.sig` e o `latest.json`; verifica a assinatura Minisign do instalador com a pubkey embutida no app, pelo assinador e pelo `tauri-plugin-updater` 2.14.1 do 0.1.48 (`compat-tauri`); confere versão, URL, assinatura e notas do `latest.json`; e confere que o corpo da release é o texto das notas sem o título (só diferença de CRLF/LF e espaço final é ignorada; título repetido, seção faltando ou qualquer mudança de conteúdo é recusada; o campo `notes` do `latest.json` segue a regra de conteúdo, com o H1);
   - exige CI verde de `push` em `main` para o SHA de `git rev-parse HEAD` (o commit checkout da tag), esperando até 30 minutos, imediatamente antes de publicar como a última release estável; PR, SHA diferente, falha, cancelamento, erro de consulta ou timeout deixam a release em rascunho.

## Conferências fora do workflow

- Última release publicada, sempre pela tag e pela API: `gh api repos/luansilvadb/orquestrador/releases/latest --jq .tag_name`. Não use o campo `isLatest` de `gh release view`.

## Correção editorial do corpo publicado

Se o texto do corpo de uma release já publicada estiver errado ou fora do formato (por exemplo, um H1 repetido), o corpo pode ser corrigido sem versão nova. Só o corpo muda:

```powershell
cargo xtask release release-body X.Y.Z <pasta-temporária>\corpo.md
gh release edit vX.Y.Z --repo luansilvadb/orquestrador --notes-file <pasta-temporária>\corpo.md
cargo xtask release verify-body <pasta-temporária>\corpo.md docs/releases/vX.Y.Z.md
```

Não mexe em tag, instalador, `.sig`, `latest.json`, versão nem nos flags de publicação (rascunho, pré-release, latest). Se o conteúdo das notas estiver errado, a fonte é corrigida; a release só recebe o corpo novo. Confira depois, pela API, que o nome, a tag e os assets continuam os mesmos.

## Se algo der errado

A situação da release depende do momento da falha:

- **Gate da CI** (sem execução ou ainda em andamento): cada uma das duas conferências pode aguardar até 30 minutos. Se a CI falhar ou for cancelada, a consulta der erro ou o prazo acabar, corrija a causa e execute a release de novo; cada conferência consulta a tentativa atual para o SHA checkout, sem reaproveitar sucesso anterior.
- **Antes da publicação** (checks, build, instalador, conferência do rascunho): a release fica em rascunho, ou nem chega a ser criada. O app não a vê. Corrija a causa e rode o job de novo: o rascunho é refeito. Se o código mudou, a tag só pode ser movida enquanto não houver release publicada, e quem move é o mantenedor.
- **Durante a publicação** (`gh release edit --draft=false --latest`): se esse passo chegou a rodar, a release já pode estar publicada e ser a última, mesmo que o job tenha falhado depois. Não rode o job de novo e não mova a tag. Confira a API (`releases/latest`) e o `latest.json` público: se apontam para a tag, a release está publicada, e qualquer correção vira versão nova.
- Release já publicada: nenhum passo a altera. Para corrigir, publique uma versão nova.
- Chave de assinatura: o secret `TAURI_SIGNING_PRIVATE_KEY` é o conteúdo da chave privada, fora do repositório ([atualização e release](../atualizacao.md)). Sem ela, o build do instalador falha, porque o updater exige assinatura. Nunca escreva a chave em arquivo versionado.
