# Limites de segurança

Arquivos e pastas passados pelo Explorer ou pela linha de comando são abertos para leitura no editor, sem executar o arquivo. O registro do instalador fica em HKCU e não torna nenhuma extensão padrão.

**Agentes sem trava.** Orquestrador e executores têm permissão total, e o app aceita sozinho toda confirmação que o CLI ainda pedir; as settings de usuário e de projeto do Claude continuam valendo, com os hooks e as regras `deny` delas. Escolher a pasta no app equivale a confiar nela, e a interface não avisa da permissão total ao abrir a pasta.

**Alçada.** A alçada de cada papel vem só do texto dos papéis, sem trava no app, inclusive quando o produto abrir a outros desenvolvedores.

**Git pelo painel.** O app lê status e base do Git sozinho, mas só muda o repositório por clique do usuário, sem `reset --hard` nem push forçado. Descartar alterações só existe no diff da área central, com confirmação mostrando o patch. Aceitar a remoção num conflito, também com confirmação, manda o arquivo para a Lixeira e roda `git rm`. Preparar e tirar do preparo, inteiro ou por bloco, pelo painel, pelo diff ou pelo editor, mudam só o índice.

**CLIs na abertura.** O app instalado atualiza os CLIs e, se um faltar ou não responder, executa o script oficial de instalação do fornecedor, como o usuário e sem administrador. O app não confere a integridade do que o script baixa.

**Painel Terminal.** Os shells do painel rodam com a permissão do próprio usuário.

**Arquivos fora do projeto.** O editor lê e grava fora da pasta do projeto apenas arquivos que o usuário abriu ou escolheu em `Abrir`/`Salvar como`.

**Rede.** O próprio app só acessa a rede para a atualização dele e para instalar os CLIs; o resto é do Git e dos CLIs.
