# Design system

A interface é GPUI com os componentes do gpui-kit (vendorizado em `vendor/`, patches em `vendor/README.md`), reproduzindo o Lobe UI da versão React (tema escuro fixo, fonte Inter) e as medidas do VS Code / Antigravity. Tokens e regras visuais em [DESIGN.md](../DESIGN.md).

## Onde fica

| Arquivo | Papel |
| --- | --- |
| `crates/ui/src/tema.rs` | Cores (`cor::*`), fontes (Inter, Cascadia Mono, Consolas no código), tamanhos de texto, raios e medidas da janela, das faixas e dos painéis. `aplicar` liga o tema do Kit. |
| `crates/ui/src/widgets.rs` | Peças comuns: `botao`, `botao_pequeno`, `acao`, `icone`, `botao_janela`, `item_menu`, `mascara` dos modais, `TooltipTexto::tooltip_texto`, `texto_pretty` e a `alca` dos divisores. |
| `crates/ui/src/faixa.rs`, `lista.rs` | Faixa de abas com arrasto (`FaixaAbas`) e a lista de abas em menu (`ListaAbas`), as mesmas no Orquestrador, no Terminal e no editor. |
| `crates/ui/src/avisos.rs` | Avisos (toasts) no topo ao centro. |
| `crates/ui/src/recursos.rs` e `assets/` | Ícones (codicons, Material Icon Theme), marca do app e fontes embutidas. |
| `crates/app/src/terminal/cores.rs` | Tema e paleta de 256 cores dos terminais. |

## Regras

- Componentes do Kit (`Button`, `PopupMenu`, `Input`, `Tooltip`) antes de desenho próprio; o que é comum a duas ou mais telas vai para `crates/ui`.
- Cores e medidas só pelos tokens de `tema.rs`; o único token próprio do app é o fundo dos terminais.
- Dicas pelo `tooltip_texto` comum: somem ao clicar e quando um menu ou modal abre (`marcar_sobreposicao`), e só voltam num novo hover.
- Modais sobre a `mascara` comum; texto corrido de diálogo com a quebra `pretty` (`texto_pretty`).
- Estado de execução sempre com texto e ícone, não só cor.

## Telas e terminais montados

Cada projeto mantém árvore, editor, pesquisa, Git e terminais vivos enquanto a aba existe; trocar de tela ou de projeto só esconde. Os terminais (`alacritty_terminal`) recebem o tamanho da área visível e só avisam o PTY quando ele muda.

## Recuperação e comparação

A recuperação da área central fica em `%LOCALAPPDATA%\orquestrador\editor-recuperacao.json`, uma chave por projeto, validada ao ler: versão, projeto, caminhos relativos e seguros, tamanhos. O que não passa é descartado com aviso. Ela guarda abas e grupos, rascunhos sujos (de arquivo e de conflito), cursor e rolagem por arquivo e a proporção dos grupos. Nada disso vai ao disco do projeto.

Antes de gravar sobre um arquivo que mudou no disco, o editor mostra as duas versões lado a lado (disco e edição) num modal. A confirmação grava a edição com a versão do disco que foi comparada como base; se o arquivo mudou de novo depois da comparação, o backend recusa e nada é gravado. “Usar o disco” pede a mesma confirmação, porque descarta a edição.

## Fechamento

Fechar a janela com alterações não salvas, em qualquer projeto aberto (os de outro projeto vêm com o nome da pasta na frente), abre um modal com Salvar, Descartar e Cancelar; fechar a aba de um projeto pergunta pelos arquivos dele. Salvar grava tudo e só fecha se nada falhou e nenhum texto novo ficou sujo. Descartar descarta os rascunhos, grava a recuperação de imediato (sem rascunho, com abas e layout) e encerra. Cancelar mantém tudo. Terminais vivos têm a mesma confirmação de antes.
