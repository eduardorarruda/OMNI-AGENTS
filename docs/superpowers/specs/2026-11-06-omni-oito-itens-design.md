# OMNI AGENTS: oito melhorias (design)

Data: 2026-11-06. Estado: aguardando aprovação do usuário. Depois disto vem o plano de implementação.

## Por que

Hoje o OMNI faz trabalho que o usuário não enxerga: fluxos e subagentes rodam sem sinal na tela. Fluxos não se
encadeiam e param por limites que o usuário não quer. Em Linux, parte do app presume Windows. Um arquivo citado
pelo agente aparece como inexistente mesmo estando no disco.

O objetivo é um OMNI em que a IA trabalha sozinha do começo ao fim e o usuário vê, a qualquer momento, o que está
rodando, onde e o que aquilo está fazendo.

## Metas

Os oito itens, mais o item 9 (tirar mensagem da fila) e o item 10 (enviar sem erro de contexto), todos entregues. Nada fica de fora ("Quero que seja resolvido tudo").

## Fora do escopo

- Webhook de entrada e túnel para a internet: o usuário escolheu que o OMNI **busca** as tarefas.
- Busca automática de tarefas: a busca nasce sempre de um clique. Por isso as tarefas não entram "automaticamente",
  como dizia o pedido original: o usuário escolheu proteger a Central.
- Abrir arquivo citado na web/celular: lá o caminho continua sendo texto.
- Eventos em tempo real (SSE) para fluxos: o polling de 1,5 s com ETag já atende.

## O que seria fracasso mesmo funcionando

- Número de uso ou de tokens inventado: quando a fonte não tem o dado, a tela diz "não disponível".
- Tarefa vinda de fora executar sozinha.
- Execução que ainda fica invisível fora da tela de Fluxos.
- Apagar ou perder cartões do Kanban na migração.

## Decisões do usuário

| Assunto | Decisão |
|---|---|
| Autonomia da IA pelo MCP | Total, sem exceção. Cria e roda sem confirmação; nem etapa marcada para a execução iniciada pela IA |
| Limites nos fluxos | Remover todos: tempo, tokens, paralelo e proteção de pastas |
| Ciclo entre subfluxos | Permitido, só com aviso. A parada é manual, pelo botão Parar |
| Origem das tarefas externas | API genérica, consultada pelo OMNI (o OMNI busca, nada fica exposto) |
| Quando buscar | Só no clique. A regra "polling proibido" continua valendo, porque a Central é frágil |
| Tarefa externa ao chegar | Entra na fila e espera. Só executa quando o usuário ou um agente já iniciado a pegar |
| Projeto da tarefa externa | Caixa de triagem: o usuário escolhe o projeto de cada tarefa |
| Execução nascida de tarefa externa | Única exceção à autonomia total: para nos nós de Aprovação, mesmo vinda da IA |
| Trabalho não commitado | O Claude faz um commit dele como está, no branch atual, antes de começar |
| Tirar mensagem da fila | Por CLI: fila da própria CLI só onde o X por tecla foi medido e é confiável; nas outras, fila no OMNI |
| Perguntas do Claude | A caixa de alternativas já aparece para o usuário: não é item de produto |

**Risco aceito pelo usuário:** com autonomia total e sem proteção de pastas, uma etapa errada age sem revisão e pode
escrever em `.git`, `.claude` ou pastas do sistema. Laço sem fim gasta tokens até alguém clicar em Parar. Por isso o
acompanhamento ao vivo e o Parar são parte do mesmo pacote, não um extra.

## Ordem de entrega (escolha do Claude)

Cada fase funciona sozinha.

1. Erro de contexto ao enviar (item 10), bug dos arquivos (item 8) e reconhecimento do Linux (item 4).
2. Fluxos (item 2), nesta ordem: acompanhamento ao vivo, botão Aprovar, subfluxos, remoção de limites, autonomia.
   O acompanhamento vem antes da remoção de limites de propósito: nada roda sem freio antes de ficar visível.
3. Tarefas em Segundo Plano (item 3) e panorama do projeto (item 5).
4. Uso da conta (item 1).
5. Kanban e tarefas externas (item 7).
6. Grafo de conhecimento (item 6).

## Fase 1 (primeiro): enviar mensagem sem erro de contexto (item 10)

**Pedido:** poder mandar prompt livremente, sem o erro de que o contexto não está atualizado.

**Causa, pelo código, ainda não reproduzida ao vivo:** a mensagem leva a versão da tela da última leitura, e essa versão é
um hash da tela inteira (`crates/omni-engine/src/interaction.rs:19`). Qualquer redesenho entre a leitura e o envio
recusa a mensagem com "O contexto mudou; atualize antes de responder" (`mobile.rs:1767`), sem nova tentativa
(`src/mobile/MobileApp.tsx:885`). A conferência se repete na execução (`mobile.rs:2661`). A fila já foi dispensada
disso pelo mesmo motivo (`mobile.rs:1763-1765`). O envio normal ficou de fora.

- Mensagem deixa de conferir a versão da tela. O que protege de verdade continua: campo sem rascunho, sem aprovação
  aberta, e a mesma chave não duplica.
- Agente que começou a trabalhar nesse meio tempo: a mensagem vira fila, sem erro.
- Aprovação e escolha **mantêm** a trava, porque ali um clique velho pode responder outra pergunta. Se a pergunta for a
  mesma, relê e tenta uma vez (escolha do Claude). Pergunta trocada continua recusando.
- Tela não reconhecida: a fila do OMNI (fase 7) guarda a mensagem em vez de dar erro.

## Fase 1a: arquivo citado que "não existe" (item 8)

**Fato:** `analise-academico.md`, `geral-matriculas-midia.md` e `tarefas-termos-inventario.md` estão no disco.

**Causa provável, ainda não reproduzida no app:** o agente cita só o nome do arquivo, e `src/App.tsx:189` junta esse
nome à raiz do projeto ativo. O caminho vira `EBPOS_360/analise-academico.md`, que não existe. Segunda hipótese: a
base é o projeto da aba ativa (`App.tsx:179`), não a pasta da conversa.

**Evidência nova (print de 06/11):** `plans/2026-11-06-omni-oito-itens.md`, citado sem `docs/superpowers/`, também deu
"Não existe no disco" com o arquivo no lugar. O bug pega caminho parcial, não só nome solto.

**Primeiro passo:** confirmar. Logar `absoluto` em `App.tsx:192` e comparar `financeiro.md` com
`docs/analise-360/financeiro.md`. Se o caminho com pasta também falhar, a hipótese 2 é a causa.

**Comportamento desejado**, nesta ordem:

1. Caminho absoluto: usa como está.
2. Relativo à pasta da conversa, depois à raiz do projeto.
3. Busca pelo nome exato dentro do projeto, ignorando `node_modules`, `.git`, `target` e `dist`.
   Se a citação traz parte das pastas (`plans/x.md`), ficam só os achados que terminam nesse trecho.
   Um resultado abre direto. Vários mostram a lista para escolher.
4. Nenhum: "Não achei `<nome>` em `<projeto>`", no lugar de "Não existe no disco".

Teste de regressão em `src/test/ArquivoCitado.test.tsx`.

## Fase 1b: reconhecer o sistema operacional (item 4)

`src-tauri/src/servidores.rs` é todo Windows, sem nenhum `cfg`. Trocar só o comando devolve lista vazia, porque a
linha 53 descarta o que não começa com `C:\`.

- Separar a coleta por sistema com `cfg(target_os)`: Windows mantém o PowerShell, Linux usa `ss -ltnpH` e
  `/proc/<pid>/{comm,cmdline,cwd}`.
- Filtros de pasta por sistema (`servidores.rs:22`, `:53-55`).
- Derrubar servidor (`:127`): no Linux, `kill` na árvore do processo, primeiro TERM e depois KILL.
- Nunca listar nem derrubar os processos do próprio OMNI: isso fecharia os terminais vivos do usuário.
- Sem `ss` instalado, cair para `/proc/net/tcp`.
- macOS (escolha do Claude): a release também gera esse sistema, que não tem `ss` nem `/proc`. Usa
  `lsof -iTCP -sTCP:LISTEN`. Sem máquina para testar, fica coberto só por teste com saída gravada, e isso é dito no
  fechamento da fase.

Outros pontos que presumem Windows, corrigidos na mesma fase:

| Onde | Problema | Equivalente |
|---|---|---|
| `crates/omni-engine/src/mobile.rs:1851` | `cmd /c agent models` | chamar `agent` direto |
| `crates/omni-core/src/mestra.rs:58` | sem Lixeira no Linux | Lixeira do XDG |
| `crates/omni-engine/src/main.rs:1489` | `matar_arvore` não faz nada fora do Windows | matar a árvore de verdade |

## Fase 2: Fluxos (item 2)

### 2.1 Acompanhamento ao vivo

Já existe: nó ativo centralizado, aresta animada, painel "Agora", lista de etapas, log do nó selecionado.

Falta:

- **Vários nós ativos:** "Agora" e o foco usam `.find` (`FluxosView.tsx:136`, `:681`). Passam a mostrar todos.
- **Linha do tempo geral:** novo `execucoes/<id>/eventos.jsonl`, só de acréscimo, com hora, nó, tipo e texto. Leitura
  por cursor (`?desde=`), sem reler o arquivo inteiro. O log fica sempre visível, sem precisar selecionar nó.
- **Voltas separadas:** log de voltas e participantes da Sala com separador.
- **Indicador global:** selo na barra do topo em qualquer tela ("Fluxo X: etapa Y"), que leva à execução. Resolve
  "o sistema continua normal sem dar indicativos". Execução iniciada pela IA também aparece aqui.
- **Celular:** mesma lista de execuções ativas.

### 2.2 Botão Aprovar

Hoje são dois botões pequenos em `FluxosView.tsx:764-765` e um em `AprovacoesDeFluxo.tsx:79`.

- Faixa fixa no topo da tela de Fluxos, com cor de destaque, o texto do que precisa de aprovação e botões grandes
  Aprovar e Recusar.
- O nó parado pulsa no canvas.
- O selo global muda de cor quando há aprovação pendente.
- Corrigir: duas aprovações simultâneas deixam a segunda presa (`crates/omni-engine/src/fluxos.rs:784-785`, inferido do
  código, confirmar com teste) e o celular mostra só uma por execução (`AprovacoesDeFluxo.tsx:17`).

### 2.3 Subfluxos

- Novo tipo de nó `Subfluxo` com o id do fluxo chamado. Entra no enum (`crates/omni-core/src/fluxos.rs:21`), em `saidas`,
  `validar`, no `match` de `processar`, em `TIPOS` e no editor.
- Roda como `Execucao` filha, com referência à execução e ao nó pai. O nó pai espera, e o resultado da filha vira a
  saída dele. Falha na filha é falha no nó.
- Parar e Pausar valem para a cadeia inteira. Ao religar o engine, a cadeia volta pausada, como hoje.
- A filha usa a mesma pasta de trabalho do pai, para X, Y e Z verem o mesmo código.
- Tela: trilha `X › Y › Z`, e clicar no nó abre a filha.
- **Ciclos:** ao salvar, busca entre fluxos. Havendo ciclo, salva e mostra aviso, sem recusar.
- Fluxo chamado que foi apagado: falha no nó com mensagem clara.

### 2.4 Remover limites

Sai:

- `custo_maximo` e a pausa por custo (`crates/omni-engine/src/fluxos.rs:241`, `:764`).
- Teto de paralelo: `MAX_PARALELO_PADRAO` e `MAX_PARALELO_TETO`.
- `EXECUCOES_SIMULTANEAS` no MCP.
- `max_voltas` padrão 3: ausente passa a ser sem limite.
- Teto de 1-10 rodadas da Sala e de 1-10 voltas na tela.
- Recusas de `PASTAS_PROTEGIDAS`, `PASTAS_DO_SISTEMA` e `pastas_extras`.

**Fica, por não ser limite de execução (escolha do Claude):**

- Tempos de espera do MCP (`mcp_fluxos.rs:31-36`): limitam quanto a ferramenta espera a resposta, não a execução.
- Corte de tamanho de saída nos logs e corpo de 1 MiB nas rotas.
- `proibidas` e `escrita`: só existem se o autor do fluxo pedir naquela etapa. São opção, não imposição.
- Pasta de trabalho separada por padrão: é isolamento, não trava nada.

Fluxos antigos com `custo_maximo` continuam abrindo: o campo é ignorado.

### 2.5 Autonomia pelo MCP

- `Execucao` ganha `origem` (`Mcp` ou `Manual`). Subfluxo herda de quem chamou.
- Sai: `etapa_sem_aprovacao`, `etapa_arriscada`, `FERRAMENTAS_DE_RISCO`, a exigência em `criar` e `rodar`
  (`mcp_fluxos.rs:135`, `:163`, `:201`, `:217`) e os avisos "Use SOMENTE quando...".
- Com origem `Mcp`, nó de Aprovação segue pelo `sim` sozinho e registra isso na linha do tempo.
- Com origem `Manual`, a Aprovação continua parando, e é para isso que serve o botão da 2.2.
- **Exceção, tarefa externa:** `Execucao` ganha a marca `veio_de_fora`, herdada pelos subfluxos. Com a marca, a
  Aprovação para mesmo com origem `Mcp`. A marca nasce quando o agente pega um cartão externo pelo MCP: as execuções
  que aquela conversa iniciar depois herdam a marca. Só cobre o que o OMNI rastreia: um agente que age fora do MCP
  não é marcado.
- Atualizar `crates/omni-core/prompts/criar-fluxo.md` e os testes do MCP que travam o comportamento antigo.

## Fase 3: Segundo plano e panorama (itens 3 e 5)

### Fonte de dados única

Nova rota `GET /segundo-plano`, com filtro opcional por projeto, juntando:

- **Sessões com agente em segundo plano:** `agente_em_segundo_plano` (`interaction.rs:330`) existe, mas devolve só
  verdadeiro ou falso e não é exposto.
- **Subagentes do Claude:** `<id>/subagents/*.jsonl` alterados recentemente dão nome e última ação.
- **Execuções de fluxo ativas:** vêm da fase 2.

Antes de mexer na detecção, capturar a tela real no acervo `crates/omni-engine/tests-fixtures/telas/`, como pede o
`LEIA.md` (já é pendência no roadmap). Codex, Cursor e Antigravity mostram só o estado da sessão, sem detalhar
subagentes.

### Painel "Tarefas em Segundo Plano"

Nova aba em `src/components/Sidebar.tsx`, junto de Skills, Containers e Git. Cada linha traz o que é, o que está
fazendo, desde quando, e botões para abrir a conversa e parar. Atualiza por polling e mostra um contador na aba.

Escolhas do Claude: só entram sessões abertas pelo OMNI, porque as outras não têm como ser paradas daqui. Parar uma
linha de subagente para a sessão inteira, como o Parar atual, e o botão diz isso.

### Panorama do projeto

Ao entrar num projeto com algo rodando, uma faixa compacta em `WorkspaceView.tsx` diz o que está em execução. O clique
abre uma aba em `PaneView.tsx` com o esquema: cada fluxo como mini-grafo somente leitura com o nó ativo aceso, mais as
tarefas em segundo plano. Reaproveita `Panorama.tsx` e `@xyflow/react`.

## Fase 4: Uso da conta (item 1)

**Fato:** o nome da conta fica no rodapé (`AgentUsageStatus.tsx:210`), não no topo. O botão "Uso" do topo já abre um
painel (`UsoDeTodasAsContas.tsx`), e janela de 5h e semana já existem.

- Clicar no nome da conta abre o painel de detalhe da conta. O painel do topo ganha os mesmos dados.
- Mostra janela de 5h e limite semanal como **dois medidores separados** (interpretação do pedido "limite semanal de
  5 horas"), com hora de renovação, situação e motivo.
- **Tokens:** não existe contagem por conta. Somar dos históricos locais os tokens de hoje e dos últimos 7 dias, com
  cache por arquivo para não reler tudo a cada abertura. Provedor sem dado mostra "não disponível".

## Fase 5: Kanban e tarefas externas (item 7)

- **Kanban vai para o engine.** Hoje vive só no `localStorage` (`src/services/kanbanService.ts`), que o engine não
  alcança. Novas rotas com operação por cartão, sem repetir o problema de `POST /tarefas`, que substitui a lista
  inteira. Migração única do `localStorage`, preservando a cópia antiga.
- **Busca:** botão Atualizar no Kanban, usando a busca atual de Tarefas Externas (`src-tauri/src/tarefas.rs:148`) e o
  bloqueio gravado no cache. Nada repete sozinho. Não duplica: chave é origem + id externo.
- **Caixa de triagem:** a tarefa nova cai numa fila geral "Externas", fora dos projetos, com selo "Externa". O usuário
  escolhe o projeto de cada uma, e aí ela vira cartão em Backlog daquele projeto.
- **Dispatcher (escolha do Claude):** `useKanbanDispatcher` continua na tela. Tarefa em triagem nunca é despachada.
  Depois que o usuário a coloca num projeto, mover para Pronta é o gesto de "pegar" e segue o fluxo normal, com a
  marca `veio_de_fora` da fase 2.5.
- **MCP:** ferramentas para listar, pegar e mover cartões. Texto de tarefa externa chega ao agente marcado como dado
  vindo de fora, nunca como instrução.
- Tarefa que sumiu da origem fica no Kanban, marcada.

## Fase 6: Grafo de conhecimento (item 6)

**Fato:** a tabela `notas` não tem ligações (`crates/omni-rag/src/memoria.rs:23-29`), e não há rota que liste notas.

- Nova rota com notas e ligações, calculadas na leitura, sem tabela nova.
- Ligações, da mais forte para a mais fraca: `[[wikilink]]` no corpo, mesma conversa, tags em comum, textos parecidos
  (`notas_relacionadas`, as 3 mais próximas). Mesmo projeto vira cor, não ligação, para não formar um novelo.
- Nova aba "Grafo" na barra lateral, com opção de abrir em aba grande. Filtro por projeto e por tipo de ligação. Clicar
  na nota abre o texto e destaca as vizinhas.
- `@xyflow/react` não faz layout de forças: entra uma biblioteca pequena para isso (a escolher pelo construtor).

## Fase 7: tirar mensagem da fila (item 9, pedido depois dos oito)

**Pedido:** um X para tirar a mensagem enquanto ela ainda está na fila.

**Fato:** hoje a mensagem é colada na CLI na hora (`mobile.rs:2669`). O que aparece como "na fila" é só um espelho local
(`MobileApp.tsx:569`), e o acervo de telas não tem nenhuma tela dessa fila.

- **Fila do OMNI:** o engine guarda a mensagem e entrega quando o agente fica livre, uma por vez, na ordem. Sobrevive à
  janela fechada e aparece igual no PC e no celular. O agente só lê ao terminar o turno.
- **Fila da CLI:** mantém o envio de hoje, e o X manda teclas. Só vale para a CLI medida numa tela real, com confirmação
  pela tela. Sem confirmação, a bolha fica e aparece o erro: nunca dizer que saiu sem ter saído.
- **Padrão seguro (escolha do Claude):** toda CLI começa na fila do OMNI e só muda depois de medida.
- **Parar (escolha do Claude):** depois de Parar, a fila não entra sozinha. Fica pausada até o usuário enviar algo novo
  ou tirar os itens, para não recomeçar um trabalho recém-interrompido.
- Mensagem que já foi entregue não pode ser tirada: a tela avisa.

## Restrições

- Textos de tela em pt-BR. `src/lib/changelog.ts` em linguagem de usuário.
- TDD, cobertura de 80% e regras de `~/.claude/rules`.
- Há trabalho não commitado em boa parte destes arquivos, e duas sessões costumam editar ao mesmo tempo: só hunks
  pequenos em `App.tsx`, `Sidebar.tsx` e `index.css`, e nunca reverter o que não é seu. As proteções que a fase 2.4
  remove estão nesse trabalho ainda não commitado (`fluxos_limites.rs`, `mcp_fluxos.rs`). Primeiro passo da
  implementação: commit desse trabalho como está, no branch atual, depois de conferir o `git status`.
- Registrar progresso em `roadmaps/omni-agents/roadmap.md` e `roadmaps/orquestracao/roadmap.md`.
- Texto novo de CLI quebra a detecção em silêncio: capturar a tela antes de mexer.
- Mudança no engine só chega ao app instalado numa release, e publicar exige pedido explícito a cada vez.

## Fica a critério do construtor

- Nomes de rotas, campos e componentes.
- Biblioteca de layout do grafo, depois de pesquisar.
- Formato exato do `eventos.jsonl`.
- Visual dos componentes, dentro do padrão do app.
