# Releases

Cada versão publicada tem aqui seu arquivo de notas, `vX.Y.Z.md`, fonte única do corpo da release no GitHub e do `notes` do `latest.json`, sem edição manual.

## Política de versão

- A versão nova precisa ser maior que a última publicada: não há downgrade.
- Versão publicada não é refeita. Correção depois da publicação vira `X.Y.(Z+1)`.
- Mudança que quebra quem usa o app (dados, configuração ou atualização) precisa dizer isso em `Impacto da atualização`.

## Modelo do arquivo de notas

Copie para `docs/releases/vX.Y.Z.md`. O título e as sete seções são obrigatórios, nesta ordem, com texto em cada uma:

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

## Como escrever

- Em português, com acentos. Cada afirmação vem do código ou dos arquivos da versão (`git diff <anterior>..HEAD`); nada de funcionalidade que não existe.
- Links para a própria release usam a URL pública `https://github.com/luansilvadb/orquestrador/...`; caminhos relativos quebram na página da release.
- Descreva fallback com o limite real: por exemplo, "se a memória não puder ser lida, o teto fica em no máximo 2".

## Checklist de uma versão

1. Suba a versão em `Cargo.toml` (`[workspace.package]`) e no `Cargo.lock` (`orquestrador-core` e `orquestrador-app`).
2. Escreva `docs/releases/vX.Y.Z.md` a partir do modelo.
3. Confira localmente: `cargo xtask release check-versions X.Y.Z`, `cargo xtask release check-notes X.Y.Z <versão anterior>`, `cargo test -p xtask` e `cargo test --workspace`.
4. Faça o commit, crie a tag `vX.Y.Z` e envie a tag. Commit e tag são do mantenedor; o workflow `.github/workflows/release.yml` faz o resto.
5. Confira a última release publicada pela API: `gh api repos/luansilvadb/orquestrador/releases/latest --jq .tag_name`. Não use o `isLatest` de `gh release view`.

## Correção editorial do corpo publicado

Corpo de release publicada errado ou fora do formato pode ser corrigido sem versão nova; só o corpo muda:

```powershell
cargo xtask release release-body X.Y.Z <pasta-temporária>\corpo.md
gh release edit vX.Y.Z --repo luansilvadb/orquestrador --notes-file <pasta-temporária>\corpo.md
cargo xtask release verify-body <pasta-temporária>\corpo.md docs/releases/vX.Y.Z.md
```

Não mexa em tag, instalador, `.sig`, `latest.json`, versão nem nos flags de publicação. Se o conteúdo estiver errado, corrija antes a fonte. Depois confira pela API que nome, tag e assets continuam os mesmos.

## Se algo der errado

- **Antes da publicação** (checks, build, conferência do rascunho ou CI não verde): a release fica em rascunho ou nem é criada, e o app não a vê. Corrija a causa e rode o job de novo. Se o código mudou, o mantenedor pode mover a tag enquanto não houver release publicada.
- **Durante a publicação** (`gh release edit --draft=false --latest`): a release pode já estar publicada mesmo com o job falho. Não rode o job de novo nem mova a tag; confira `releases/latest` na API e o `latest.json` público. Se apontam para a tag, ela está publicada e a correção vira versão nova.
- **Chave de assinatura ausente**: o build do instalador falha. O secret e o backup estão em [atualização e release](../atualizacao.md). Não escreva a chave em arquivo versionado.
