# orquestrador-arvore

A tela Arquivos sem o editor: porte de `src/explorer/` (árvore, decorações do Git, menus, renomear e criar na linha,
excluir, arrastar, copiar, recortar e colar). Chama o core direto (`ide::files`) numa thread de fundo.

## View

`Explorador` (`Entity<Explorador>`, `Render`, `Focusable`, `EventEmitter<EventoArvore>`):

```rust
let arvore = cx.new(|cx| Explorador::new(raiz_do_projeto, "nome-da-pasta", window, cx));
```

A primeira leitura sai na hora; depois relê a raiz e as pastas abertas a cada 1,5 s com a janela em foco e 10 s sem
foco, só com a tela ativa e a seção do projeto aberta.

Métodos para a bancada:

| Método | Uso |
| --- | --- |
| `definir_git(Option<&[GitFile]>, cx)` | status do Git (None fora do modo `repo`): cores, letras e "•" |
| `definir_aberto(Option<String>, cx)` | arquivo da aba ativa: marcado sem seleção; trocar limpa a seleção |
| `definir_sujos(Vec<String>)` | arquivos com rascunho: o diálogo de excluir avisa |
| `definir_abas(Vec<String>)` | arquivos com aba: mover ou colar por cima é recusado antes do disco |
| `definir_chat(bool)` | Orquestrador rodando: "Adicionar ao Chat" habilitado |
| `definir_historico(bool)` | modo `repo`: o menu ganha "Histórico Git" |
| `definir_ativa(bool, cx)` | tela à vista: liga a atualização periódica |
| `atualizar(cx)` | rodada imediata (o `pokePolling('files')`) |
| `revelar(caminho, window, cx)` | abre as pastas, seleciona e rola até o arquivo |
| `expandir`, `selecionar`, `renomear`, `criar`, `excluir`, `abrir_menu_de_contexto` | os mesmos fluxos do mouse e do teclado |

## Eventos (`EventoArvore`)

| Evento | A bancada |
| --- | --- |
| `Abrir { caminho, fixo, ao_lado }` | abre no editor: `fixo` falso é a prévia; `ao_lado`, na área da direita |
| `PreCarregar(caminho)` | pode ler o arquivo antes do clique terminar |
| `AbrirNoTerminal(dir)` | shell na pasta |
| `AdicionarAoChat(caminho)` | cola o caminho no rascunho do Orquestrador |
| `HistoricoGit(caminho)` | abre o histórico na tela Git |
| `Pesquisar(dir)` | vai à pesquisa com a pasta no incluir ("" é o projeto) |
| `Renomeado { de, para }` | troca o prefixo das abas e dos rascunhos |
| `Excluidos(caminhos)` | fecha as abas (pastas incluem o que está dentro) |
| `Movidos(Vec<Moved>)` | troca o prefixo das abas (arrastar e colar um recorte) |

## Peças em `orquestrador-ui`

- `icone_arquivo(caminho, pasta, aberta) -> SharedString`: o ícone do Material Icon Theme, desenhado com `img()`.
- `avisos::mostrar(Aviso, cx)`: os toasts. A bancada desenha `avisos::camada(window, cx)` uma vez, por cima de tudo.

## Vitrine

`cargo run -p orquestrador-arvore --example vitrine -- <cenário>` (ver o cabeçalho de `examples/vitrine.rs`).
