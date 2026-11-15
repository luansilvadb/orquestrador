# orquestrador-hunks

Blocos de mudança linha a linha, sem UI, para o git gutter do editor e para o `orquestrador-git` (que o reexporta como
`preparo`): porte de `src/editor/git-stage.ts` e da parte pura de `git-hunk.ts`.

`trechos(a, b)`, `tocam`, `aplicar`, `preparar_bloco(indice, buffer, bloco)`, `tirar_bloco(head, indice, buffer, bloco)`,
`linhas_antigas(head, trecho)`, `ir_para_bloco(inicios, aberto, linha_cursor, direcao)`. Textos com `\n`; posições em
bytes, com o fim do último trecho em tamanho + 1, como os `Chunk` do `@codemirror/merge`.
