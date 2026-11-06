# OMNI AGENTS: oito melhorias (plano de implementação)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** a IA trabalha sozinha do começo ao fim e o usuário vê, a qualquer momento, o que está rodando, onde e o que aquilo está fazendo.

**Architecture:** seis fases que funcionam sozinhas, na ordem do spec. Regras puras em `omni-core`, execução e rotas em `omni-engine`, MCP e memória em `omni-rag`, tela em `src/`. O `omni-engine` **não** depende do `omni-rag`: o que precisa dos dois (grafo, importar tarefas) passa pelo shell Tauri em `src-tauri/`.

**Tech Stack:** Rust (axum, Tauri 2, rusqlite), React/TS (Vite, vitest, `@xyflow/react`).

**Spec:** `docs/superpowers/specs/2026-11-06-omni-oito-itens-design.md`

**Estado:** plano apenas. Nada aqui foi executado, e a execução depende de pedido do usuário.

## Global Constraints

- Textos de tela em pt-BR. `src/lib/changelog.ts` em linguagem de usuário (um teste reprova jargão).
- Mexer em `App.tsx`, `Sidebar.tsx` e `index.css` só em hunks pequenos; nunca reverter mudança que não é sua.
- Arquivo novo em vez de crescer os gigantes: `mobile.rs` (3961 linhas), `FluxosView.tsx` (923), `fluxos.rs` do engine (953).
- Publicar release exige pedido explícito. Nenhuma tarefa publica.
- Não matar processos do app instalado. Testar `npm run dev` só com o usuário ciente.
- Tela nova de CLI: capturar em `crates/omni-engine/tests-fixtures/telas/` (ver `LEIA.md`) antes de mexer na detecção.
- A busca de tarefas externas nasce sempre de um clique. Nenhum temporizador chama essa API.
- Dado que a fonte não tem aparece como "não disponível", nunca como 0.
- Dados de memória e de tarefas chegam à tela por `sem_segredos`.

## Ciclo padrão de cada tarefa

Todas seguem TDD, sem repetir isto em cada uma:

1. Escrever os testes listados e rodar: devem **falhar** pelo motivo esperado.
2. Implementar o mínimo nas interfaces indicadas.
3. Rodar de novo: devem **passar**.
4. Rodar a suíte do pacote tocado: `cargo test -p <pacote>` ou `npx vitest run <arquivo>`, mais `npm run typecheck` se tocou TS.
5. Commitar só os arquivos da tarefa (`git add <arquivos>`, nunca `-A`), com a mensagem indicada.
6. Marcar no `roadmaps/omni-agents/roadmap.md` (fase 2 também em `roadmaps/orquestracao/roadmap.md`).

## Review Focus

1. **Mesmo nome em várias pastas:** `README.md` citado sem pasta mostra a lista, não abre um qualquer. Teste na Tarefa 1.
2. **Engine reinicia com subfluxo em andamento:** o nó pai volta e reencontra a filha, sem abrir uma segunda. Teste na Tarefa 9.
3. **Parar em X com Z rodando:** o processo de Z morre, e nada da cadeia continua. Teste na Tarefa 9.
4. **Migração do Kanban com engine fora do ar ou `localStorage` corrompido:** nada se perde e a chave antiga fica intacta. Teste na Tarefa 19.
5. **Tokens contados em dobro:** a CLI repete a linha da mesma mensagem durante o streaming; soma uma vez por `message.id`. Teste na Tarefa 17.

---

## Fase 0: base

### Tarefa 0: commit do trabalho aberto

Autorizado pelo usuário, é o único commit sem tarefa própria.

- [ ] Conferir `git status` e `git diff --stat`. Confirmar com `find . -mmin -30 -type f -not -path './.git/*' -not -path './target/*' -not -path './node_modules/*'` que nenhuma sessão está editando agora. Se estiver, parar e avisar o usuário.
- [ ] Procurar segredos no diff (`git diff | grep -iE 'token|secret|senha|api[_-]?key'`) e olhar cada acerto.
- [ ] Rodar `cargo test --workspace` e `npm run test`. Falha já existente é registrada no commit e informada ao usuário, sem conserto aqui.
- [ ] Commitar tudo como está, no branch `feat/conversas-por-projeto`. Mensagem: `feat: limites de fluxo, ensaio, busca e memória pessoal`.
- [ ] Commit separado de `docs/superpowers/`: `docs: design e plano das oito melhorias`.

## Fase 1: bugs

### Tarefa 1A: enviar mensagem sem erro de contexto (item 10)

Primeira a fazer depois da Tarefa 0: é o que mais atrapalha no dia a dia.

**Causa, pelo código, ainda não reproduzida ao vivo:** a mensagem leva no `If-Match` a versão da tela da última leitura. Essa versão é um hash da tela inteira (`interaction.rs:19`), que muda a cada redesenho. Qualquer mudança entre a leitura e o envio devolve 412 `O contexto mudou; atualize antes de responder` (`mobile.rs:1767`), e o cliente não tenta de novo (`MobileApp.tsx:885`). A mesma conferência se repete na execução (`mobile.rs:2661`, `Contexto mudou antes da execução`). A Fila já foi dispensada disso pelo mesmo motivo (`mobile.rs:1763-1765`).

**Files:**
- Modify: `crates/omni-engine/src/mobile.rs:1750-1767`, `:2660-2670`, `src/mobile/MobileApp.tsx:869-900`
- Test: testes em `crates/omni-engine/src/mobile.rs`, `src/test/MobileApp.test.tsx`

**Interfaces:**
- Produces: `exige_versao_da_tela(kind: &ActionKind) -> bool`, verdadeiro só para `Approval`, `Escolha`, `Modo` e `Ajustes`. Substitui as duas conferências, em `enqueue` e em `execute_checked`.
- `ActionKind::Prompt` passa a valer com `capability.prompt || capability.enfileirar`, como a Fila. Na execução, o mesmo braço aceita `ready` ou `pode_enfileirar`.

- [ ] **Reproduzir primeiro.** Teste Rust que falha hoje: `prompt_com_versao_velha_e_aceito` (POST com `If-Match` antigo depois de a tela mudar devolve 202, não 412).
- [ ] Testes Rust: `prompt_nao_falha_se_a_tela_muda_na_fila_de_acoes`; `agente_comecou_a_trabalhar_prompt_entra_na_fila_da_cli`; `campo_com_rascunho_continua_recusando` (a proteção real, `composer_vazio`, continua); `aprovacao_com_versao_velha_continua_412`; `escolha_com_versao_velha_continua_412`; `prompt_repetido_com_a_mesma_chave_nao_duplica`.
- [ ] Testes TS: `envio não mostra erro quando a tela mudou`; `aprovação com a mesma pergunta relê e aprova`; `aprovação com pergunta trocada continua recusando`.
- [ ] Implementar. Aprovação (escolha do Claude): relê a versão e tenta uma vez se `approval_text` for igual, como a escolha já faz. Texto trocado continua recusando: é onde a trava protege de verdade.
- [ ] Commit: `fix: mensagem não é mais recusada por tela redesenhada`

### Tarefa 1: arquivo citado encontrado pelo nome (item 8)

**Files:**
- Create: `src/features/files/resolverCitado.ts`, `src-tauri/src/localizar.rs`
- Modify: `src/App.tsx:182-200`, `src/mobile/Chat.tsx:270-292`, `src-tauri/src/lib.rs` (registrar comando)
- Test: `src/test/resolverCitado.test.ts`, `src/test/ArquivoCitado.test.tsx`, testes em `localizar.rs`

**Interfaces:**
- Produces: `resolverCitado(citado: string, bases: string[], deps: { exists(p: string): Promise<boolean>; homeDir(): Promise<string>; localizar(raiz: string, nome: string): Promise<string[]> }): Promise<{ tipo: "um"; caminho: string } | { tipo: "varios"; caminhos: string[] } | { tipo: "nenhum"; nome: string; raiz: string }>`
- Produces: comando Tauri `localizar_arquivo(raiz: String, nome: String) -> Result<Vec<String>, String>`, no máximo 20 resultados, ignorando `node_modules`, `.git`, `target`, `dist`.

- [ ] **Confirmar a causa antes de corrigir.** Com o usuário, em `npm run dev`, logar `absoluto` em `App.tsx:192` e clicar num `financeiro.md` e num `docs/analise-360/financeiro.md`. Registrar no roadmap qual hipótese do spec valeu. Se o caminho com pasta falhou (hipótese 2), `bases` começa pela pasta da conversa: estender `usarAbrirArquivo` para `(caminho, pasta?)`, com a pasta vinda do tipo da conversa em `src/mobile/api.ts`.
- [ ] Testes de `resolverCitado`:

```ts
test("nome solto acha o arquivo dentro de subpasta", async () => {
  const r = await resolverCitado("analise-academico.md", ["/p"], deps({ achados: ["/p/docs/analise-360/analise-academico.md"] }));
  expect(r).toEqual({ tipo: "um", caminho: "/p/docs/analise-360/analise-academico.md" });
});
test("caminho parcial acha o arquivo pelo fim do caminho", async () => {
  // Caso real do print de 06/11: citado `plans/x.md`, arquivo em `docs/superpowers/plans/x.md`.
  const r = await resolverCitado("plans/x.md", ["/p"], deps({ achados: ["/p/docs/superpowers/plans/x.md", "/p/outro/x.md"] }));
  expect(r).toEqual({ tipo: "um", caminho: "/p/docs/superpowers/plans/x.md" });
});
test("mesmo nome em duas pastas devolve a lista", ...);      // tipo "varios", 2 caminhos
test("caminho relativo que existe não dispara busca", ...);  // localizar não é chamado
test("absoluto e ~ valem como estão", ...);
test("relativo com pasta inexistente busca só pelo nome final", ...);
test("nada encontrado informa nome e raiz", ...);            // tipo "nenhum"
```
- [ ] Teste em `ArquivoCitado.test.tsx`: rejeição mostra `Não achei analise.md em EBPOS_360`; tipo `varios` mostra um botão por caminho, e clicar abre aquele.
- [ ] Testes Rust: acha em subpasta; ignora `node_modules`; para em 20; recusa `nome` com separador de pasta.
- [ ] Implementar. `App.tsx` chama `resolverCitado`; `falha` recebe a mensagem nova. Para `varios`, a promessa de `abrir` resolve com a lista e `ArquivoCitado` mostra a escolha.
- [ ] Commit: `fix: arquivo citado pelo nome é encontrado dentro do projeto`

### Tarefa 2: servidores locais no Linux e macOS (item 4)

**Files:**
- Create: `src-tauri/src/servidores_leitura.rs`, `crates/omni-core/src/processos.rs`
- Modify: `src-tauri/src/servidores.rs`, `crates/omni-core/src/lib.rs`
- Test: em cada arquivo

**Interfaces:**
- Produces (omni-core): `processos::descendentes(raiz: u32, pares: &[(u32, u32)]) -> Vec<u32>`, pares `(pid, ppid)`, filhos antes dos pais.
- Produces: `parse_ss(saida: &str) -> Vec<Escuta>`, `parse_lsof(saida: &str) -> Vec<Escuta>`, `parse_proc_net_tcp(saida: &str) -> Vec<(u16, u64)>` (porta, inode).
- `Processo` ganha `pasta: Option<String>`, vinda de `/proc/<pid>/cwd`.

- [ ] Testes:

```rust
#[test] fn ss_le_porta_e_pid() {
    let linha = r#"LISTEN 0 511 *:5173 *:* users:(("node",pid=4321,fd=23))"#;
    assert_eq!(parse_ss(linha), vec![Escuta { p: 5173, pid: 4321 }]);
}
#[test] fn ss_ignora_linha_sem_dono() { /* socket de outro usuário: sem users:(...) */ }
#[test] fn caminho_unix_vira_projeto() { /* node /home/e/p/node_modules/.bin/vite -> /home/e/p */ }
#[test] fn comando_relativo_usa_a_pasta_do_processo() { /* `node server.js` com pasta /home/e/p */ }
#[test] fn fora_de_projeto_no_linux() { /* /usr/lib, ~/.nvm, ~/.cargo, ~/.local, ~/.npm caem fora */ }
#[test] fn windows_continua_igual() { /* os testes atuais passam sem mudança */ }
#[test] fn proprio_omni_marcado_no_linux() { /* `proprio` verdadeiro quando a pasta contém o executável */ }
#[test] fn descendentes_vem_antes_do_pai() { assert_eq!(descendentes(1, &[(2,1),(3,2),(9,8)]), vec![3,2]); }
```
- [ ] Implementar. `ler()` vira `#[cfg(windows)]` com PowerShell e `#[cfg(unix)]` com `ss -ltnpH`, caindo para `/proc/net/tcp` se `ss` faltar; macOS usa `lsof -nP -iTCP -sTCP:LISTEN`. `projeto_do_comando` aceita caminho `/` e `C:\`, sem a troca de `/` por `\` fora do Windows, e cai em `Processo.pasta`. `programa` tira `.exe`, e `python3.12` conta como `python`.
- [ ] `servidor_matar` em unix: TERM em `descendentes` + pid, espera 2 s, KILL no que sobrou. A recusa de `proprio` continua.
- [ ] Erro de leitura sem a palavra PowerShell fora do Windows: `Não deu para ler as portas: <motivo>`.
- [ ] Commit: `fix: servidores locais funcionam no Linux e no macOS`

### Tarefa 3: demais pontos que presumem Windows (item 4)

**Files:**
- Modify: `crates/omni-engine/src/mobile.rs:1849-1853`, `crates/omni-core/src/mestra.rs:58`, `crates/omni-engine/src/main.rs:1487-1497`, `crates/omni-core/Cargo.toml`
- Test: nos mesmos arquivos

**Interfaces:**
- Consumes: `processos::descendentes` (Tarefa 2).

- [ ] Testes: `agent_models_sem_cmd_fora_do_windows` (o programa do `Command` é `agent`); `lixeira_move_o_arquivo` em pasta temporária (`#[cfg(target_os = "linux")]`), que confere que o original sumiu e não houve apagamento definitivo; `matar_arvore_derruba_o_filho` com `sh -c 'sleep 60 & wait'`.
- [ ] Implementar: `saida_agent_models` com `cmd` só em `cfg(windows)`. Lixeira com a crate `trash` em `cfg(not(windows))`, depois de conferir licença e manutenção. Se a Lixeira falhar, devolve erro: nunca apaga em definitivo. `matar_arvore` em `cfg(not(windows))` usa `descendentes`.
- [ ] Commit: `fix: modelos do Cursor, Lixeira e fim de processo fora do Windows`

## Fase 2: Fluxos

### Tarefa 4: linha do tempo da execução (2.1)

**Files:**
- Create: `crates/omni-engine/src/fluxos_eventos.rs`
- Modify: `crates/omni-engine/src/fluxos.rs` (`mexer`, `processar`, `historico`), `crates/omni-engine/src/mobile.rs:575-580`
- Test: `crates/omni-engine/src/fluxos_testes.rs`

**Interfaces:**
- Produces: `struct Evento { seq: u64, ms: u64, no: Option<String>, tipo: String, texto: String }`, com `tipo` em `inicio`, `fim`, `falha`, `atividade`, `aprovacao`, `subfluxo`, `aviso`.
- Produces: `registrar(state: &EngineState, e: &mut Execucao, no: Option<&str>, tipo: &str, texto: &str)` e `ler_desde(state: &EngineState, id: &str, desde: u64) -> Result<Vec<Evento>>`, com no máximo 500 por leitura.
- Produces: `GET /fluxos/execucoes/{id}/eventos?desde=N` e o campo `Execucao.evento_seq: u64` com `#[serde(default)]`.

- [ ] Testes: `eventos_saem_em_ordem_e_com_cursor`; `ler_desde_devolve_so_o_novo`; `ler_desde_para_em_500`; `id_invalido_recusa` (mesma regra de `historico`); `execucao_antiga_sem_eventos_devolve_vazio`; `atividade_repetida_nao_vira_evento`; `volta_nova_separa_o_log` (`historico` devolve item de separação com o número da volta).
- [ ] Implementar. Grava em `execucoes/<id>/eventos.jsonl`, só por acréscimo. `processar` registra início e fim de cada nó; a mudança de `atividade` registra uma linha. A cada volta de nó, escrever separador no `<no>.log`.
- [ ] Commit: `feat: linha do tempo das execuções de fluxo`

### Tarefa 5: tela mostra tudo o que está ativo (2.1)

**Files:**
- Create: `src/features/fluxos/LinhaDoTempo.tsx`, `src/features/fluxos/execucaoModel.ts`
- Modify: `src/features/fluxos/FluxosView.tsx:136`, `:681`, `:708-730`, `src/features/fluxos/fluxosService.ts`, `src/features/terminal/terminalService.ts` (`engineApi`)
- Test: `src/test/execucaoModel.test.ts`, `src/test/LinhaDoTempo.test.tsx`

**Interfaces:**
- Consumes: rota de eventos (Tarefa 4).
- Produces: `nosAtivos(execucao: Execucao): NoAtivo[]`, com `{ id, rotulo, status, atividade, inicioMs }`.
- Produces: `lerEventos(id: string, desde: number): Promise<EventoDeFluxo[]>`

- [ ] Testes: `nosAtivos devolve os dois ramos em paralelo`; `nó aguardando conta como ativo`; `LinhaDoTempo acrescenta sem repetir`; `LinhaDoTempo pede a partir do último seq`; `LinhaDoTempo para de consultar quando a execução termina`.
- [ ] Implementar. Polling de 1,5 s, já usado na tela. O log fica sempre visível, sem precisar selecionar nó. O painel "Agora" lista `nosAtivos`, e o foco do canvas enquadra todos.
- [ ] Commit: `feat: acompanhamento ao vivo com todos os nós ativos`

### Tarefa 6: selo global de execução (2.1)

**Files:**
- Create: `src/features/fluxos/useExecucoesAtivas.ts`, `src/features/fluxos/SeloDeExecucao.tsx`, `src/mobile/ExecucoesAtivas.tsx`
- Modify: `src/App.tsx:281` (uma linha), tela inicial do celular em `src/mobile/`
- Test: `src/test/SeloDeExecucao.test.tsx`

**Interfaces:**
- Produces: `useExecucoesAtivas(): { ativas: Execucao[]; aguardando: Execucao[] }`, uma consulta a cada 4 s, compartilhada.

- [ ] Testes: `sem execução o selo não aparece`; `uma execução mostra "Fluxo X: etapa Y"`; `duas execuções mostram a contagem e listam ao abrir`; `aprovação pendente muda o selo e o texto`; `clicar leva à execução`; `execução iniciada pela IA aparece igual`.
- [ ] Implementar. A consulta usa ETag, que já existe em `GET /fluxos/execucoes`.
- [ ] Commit: `feat: selo mostra o fluxo rodando em qualquer tela`

### Tarefa 7: aprovação em destaque (2.2)

**Files:**
- Create: `src/features/fluxos/FaixaDeAprovacao.tsx`
- Modify: `crates/omni-engine/src/fluxos.rs:781-791`, `src/features/fluxos/FluxosView.tsx:764-765`, `src/mobile/AprovacoesDeFluxo.tsx:17,79`, CSS dos nós
- Test: `crates/omni-engine/src/fluxos_testes.rs`, `src/test/FaixaDeAprovacao.test.tsx`

**Interfaces:**
- Consumes: `useExecucoesAtivas` (Tarefa 6).

- [ ] Teste Rust `duas_aprovacoes_seguidas_soltam_as_duas`: dois nós `aguardando`, aprovar um, aprovar o outro; o segundo não devolve "Este nó não está esperando aprovação". **Se o teste passar antes da correção, a suspeita do spec era falsa:** registrar e seguir sem mexer no engine.
- [ ] Testes TS: `faixa mostra o texto do que será aprovado`; `Aprovar e Recusar têm nome acessível`; `duas pendências aparecem nas duas faixas`; `celular lista todas as aprovações da execução`; `sem pendência a faixa some`.
- [ ] Implementar. `aprovar` olha só o status do nó; a execução volta para `rodando` apenas quando não restar nó `aguardando`. Faixa fixa no topo da tela de Fluxos, e o nó parado pulsa, respeitando `prefers-reduced-motion`.
- [ ] Commit: `feat: aprovação de fluxo em destaque`

### Tarefa 8: nó Subfluxo nas regras (2.3)

**Files:**
- Modify: `crates/omni-core/src/fluxos.rs:21`, `:152`, `crates/omni-rag/src/mcp_fluxos.rs:125-131`, `src-tauri/src/fluxos.rs`, `crates/omni-core/prompts/criar-fluxo.md`
- Test: `crates/omni-core/src/fluxos.rs`

**Interfaces:**
- Produces: `TipoDeNo::Subfluxo`, com `config.fluxo: String` (id do fluxo chamado).
- Produces: `ciclos_de_subfluxo(raiz: &Fluxo, todos: &[Fluxo]) -> Vec<Vec<String>>`, cada ciclo como lista de ids.
- Produces: comando Tauri `fluxos_avisos(fluxo: Fluxo, project_path: Option<String>) -> Vec<String>`

- [ ] Testes: `subfluxo_sem_fluxo_nao_valida`; `subfluxo_tem_uma_saida`; `x_chama_y_chama_x_e_ciclo`; `x_chama_a_si_mesmo_e_ciclo`; `cadeia_sem_volta_nao_e_ciclo`; `fluxo_chamado_inexistente_nao_trava_a_busca`; `ciclo_nao_impede_salvar`.
- [ ] Implementar. Até a Tarefa 12, `etapa_arriscada` trata `Subfluxo` como arriscada: sem isso, `_ => false` deixaria o nó passar.
- [ ] Commit: `feat: nó Subfluxo e detecção de ciclos`

### Tarefa 9: execução filha no engine (2.3)

**Files:**
- Create: `crates/omni-engine/src/fluxos_subfluxo.rs`
- Modify: `crates/omni-engine/src/fluxos.rs` (`Execucao`, `EstadoDoNo`, `processar`, pausar, cancelar, limpeza de worktree)
- Test: `crates/omni-engine/src/fluxos_testes.rs`

**Interfaces:**
- Consumes: `fluxos::listar` (omni-core), com a mesma pasta global que `src-tauri/src/fluxos.rs` usa; `registrar` (Tarefa 4).
- Produces: `Execucao.pai: Option<RefDoPai>`, com `RefDoPai { execucao: String, no: String }`, e `Execucao.pilha: Vec<String>` (nomes de `X` até a atual); `EstadoDoNo.filha: Option<String>`. Todos com `#[serde(default)]`.
- Produces: `rodar_subfluxo(state: &Arc<EngineState>, v: &Viva, no: &No, ctx: &Contexto) -> Result<String>`, que devolve a saída do `Fim` da filha.
- Produces: `filhas_de(id: &str) -> Vec<String>`

**Algoritmo que os testes não fixam:** se `EstadoDoNo.filha` já existe, espera por ela em vez de criar outra (é o que cobre o engine religado). A espera consulta a cada 500 ms e sai quando a filha termina ou o pai deixa de estar `rodando`. A filha nasce com `pasta`, `branch` e `base` do pai e nunca remove a pasta: só a execução sem `pai` limpa o worktree. Entrada da filha: `ctx.anterior`.

- [ ] Testes: `pai_recebe_a_saida_da_filha`; `falha_na_filha_e_falha_no_no`; `cadeia_x_y_z_termina`; `filha_usa_a_pasta_do_pai`; `filha_nao_remove_o_worktree`; `cancelar_x_mata_o_processo_de_z`; `pausar_x_pausa_a_cadeia_e_retomar_continua`; `engine_religado_nao_abre_segunda_filha`; `fluxo_apagado_falha_com_mensagem_clara` ("O fluxo chamado “Y” não existe mais"); `pilha_mostra_x_y_z`; `ciclo_roda_e_cancelar_para` (X chama X, `Comando` inofensivo; cancelar encerra todas).
- [ ] Implementar.
- [ ] Commit: `feat: fluxo chama fluxo em cadeia`

### Tarefa 10: subfluxo no editor (2.3)

**Files:**
- Create: `src/features/fluxos/TrilhaDeSubfluxo.tsx`
- Modify: `src/features/fluxos/fluxosService.ts` (`TIPOS`), `src/features/fluxos/FluxosView.tsx`
- Test: `src/test/TrilhaDeSubfluxo.test.tsx`, `src/test/fluxosService.test.ts`

**Interfaces:**
- Consumes: `fluxos_avisos` (Tarefa 8), `Execucao.pilha`, `EstadoDoNo.filha` (Tarefa 9).

- [ ] Testes: `Subfluxo aparece entre os tipos`; `seletor lista os fluxos sem o atual marcado como ciclo`; `salvar com ciclo mostra aviso e salva`; `trilha mostra X › Y › Z`; `clicar no nó abre a filha`; `clicar na trilha volta ao pai`.
- [ ] Commit: `feat: subfluxos no editor de fluxos`

### Tarefa 11: remover limites (2.4)

Só depois das Tarefas 4 a 6: nada roda sem freio antes de ficar visível.

**Files:**
- Modify: `crates/omni-core/src/fluxos.rs:100-105`, `:176-177`, `crates/omni-core/src/fluxos_limites.rs:14-15`, `:21-22`, `:25`, `:49`, `:95-120`, `crates/omni-engine/src/fluxos.rs` (`:84-89`, `:153`, `:213-226`, `:241-245`, `:311`, `:330`, `:686`, `:764`), `crates/omni-rag/src/mcp_fluxos.rs:20`, `:196-213`, `src/features/fluxos/FluxosView.tsx:629`, `:662`
- Test: nos mesmos arquivos e em `crates/omni-rag/src/testes_fluxos_mcp.rs`

**Interfaces:**
- `limite_paralelo(f) -> usize`: `max_paralelo` definido vale como está; ausente é `usize::MAX`. Sem teto.
- `max_voltas` ausente é `u32::MAX`: a saída `esgotado` só acontece se o autor definir um número.

- [ ] Testes novos: `custo_alto_nao_pausa`; `fluxo_antigo_com_custo_maximo_abre_e_ignora`; `execucao_antiga_pausada_por_custo_retoma` (JSON gravado com `parou_por_custo: true`); `vinte_ramos_rodam_juntos`; `max_paralelo_definido_ainda_vale`; `condicao_sem_max_voltas_nao_esgota_em_3`; `max_voltas_definido_ainda_esgota`; `sala_aceita_20_rodadas`; `escrita_aceita_git_e_claude`; `pasta_extra_aceita_pasta_do_sistema`; `mcp_aceita_quarta_execucao`.
- [ ] Atualizar os testes atuais que travam o oposto (`fluxos.rs:463-479`). Fica registrado no commit que eles mudaram de propósito.
- [ ] Implementar. Sai: `custo_maximo` do `validar` e da pausa, `custo_aceito`, `parou_por_custo`, `MAX_PARALELO_TETO`, `PASTAS_PROTEGIDAS`, `PASTAS_DO_SISTEMA`, `EXECUCOES_SIMULTANEAS`. `custo_maximo` continua aceito na leitura de fluxo antigo. Fica o que o spec manda manter em 2.4.
- [ ] Commit: `feat: fluxos rodam sem limite de custo, voltas, paralelo ou pastas`

### Tarefa 12: autonomia pelo MCP (2.5)

**Files:**
- Modify: `crates/omni-rag/src/mcp_fluxos.rs` (`:25`, `:28`, `:56`, `:120-155`, `:163`, `:201`, `:217`, `:388`), `crates/omni-rag/src/mcp.rs:106-107`, `crates/omni-engine/src/fluxos.rs` (`Execucao`, `iniciar`, `processar` no `Aprovacao`, env dos agentes), `crates/omni-engine/src/mobile.rs:924`, `crates/omni-core/prompts/criar-fluxo.md`
- Test: `crates/omni-rag/src/testes_fluxos_mcp.rs`, `crates/omni-engine/src/fluxos_testes.rs`

**Interfaces:**
- Produces: `Execucao.origem: String` (`manual` ou `mcp`, padrão `manual`) e `Execucao.veio_de_fora: bool`, os dois com `#[serde(default)]`. Corpo de `POST /fluxos/execucoes` ganha os mesmos dois campos, opcionais.
- Produces: constante `VARIAVEL_DE_FORA = "OMNI_VEIO_DE_FORA"` em omni-core.
- Regra: `aprovacao_automatica(e: &Execucao) -> bool` = `e.origem == "mcp" && !e.veio_de_fora`.

**Como a marca nasce:** o processo do MCP vive com a sessão da CLI. `ler_tarefa` liga uma marca nesse processo, e `rodar` a manda no corpo. Agente rodando dentro de execução marcada recebe `OMNI_VEIO_DE_FORA=1` no ambiente, e o MCP dele começa marcado. Subfluxo herda `origem` e `veio_de_fora`.

- [ ] Testes: `mcp_cria_fluxo_sem_aprovacao`; `mcp_roda_fluxo_de_projeto_com_comando`; `mcp_retoma_pausado`; `aprovacao_passa_sozinha_com_origem_mcp` (segue por `sim`, evento `aprovacao` na linha do tempo); `aprovacao_para_com_origem_manual`; `corpo_sem_origem_vale_manual`; `aprovacao_para_quando_veio_de_fora`; `ler_tarefa_marca_a_proxima_execucao`; `subfluxo_herda_origem_e_marca`; `agente_de_execucao_marcada_recebe_a_variavel`.
- [ ] Implementar. Sai `etapa_arriscada`, `etapa_sem_aprovacao`, `exigir_aprovacao`, `exige_aprovacao`, `FERRAMENTAS_DE_RISCO` e o texto `USO`. `PREFIXO_DO_AGENTE` fica: ele só protege fluxo do usuário de ser sobrescrito.
- [ ] Atualizar `criar-fluxo.md` e as descrições das ferramentas.
- [ ] Commit: `feat: IA cria e roda fluxos sem confirmação`

## Fase 3: segundo plano e panorama

### Tarefa 13: fonte de dados do que roda em segundo plano (itens 3 e 5)

**Files:**
- Create: `crates/omni-core/src/segundo_plano.rs`, `crates/omni-engine/src/segundo_plano.rs`
- Modify: `crates/omni-engine/src/mobile.rs` (rota), `crates/omni-engine/src/interaction.rs:330` (expor por sessão)
- Test: nos arquivos novos e em `crates/omni-engine/tests-fixtures/telas/`

**Interfaces:**
- Consumes: execuções ativas (fase 2).
- Produces: `struct ItemDeFundo { tipo: String, titulo: String, atividade: String, desde_ms: u64, projeto_id: String, sessao: Option<String>, execucao: Option<String> }`, com `tipo` em `sessao`, `subagente`, `fluxo`.
- Produces: `subagentes_ativos(pasta: &Path, agora_ms: u64) -> Vec<SubagenteAtivo>`, que considera ativo o `.jsonl` alterado nos últimos 2 minutos.
- Produces: `GET /segundo-plano?projeto=<id>`

- [ ] Capturar a tela real com agente em segundo plano no acervo, seguindo o `LEIA.md`. Precisa de rede e do Claude Code: combinar com o usuário. Fecha a pendência do roadmap (linha 407).
- [ ] Testes: `transcript_recente_e_ativo`; `transcript_velho_nao_entra`; `linha_corrompida_nao_derruba`; `nome_e_ultima_acao_vem_do_jsonl`; `tela_capturada_marca_segundo_plano`; `rota_filtra_por_projeto`; `sessao_fora_do_omni_nao_entra`; `rota_exige_token` (mesma regra de `same_origin`).
- [ ] Commit: `feat: rota do que roda em segundo plano`

### Tarefa 14: painel Tarefas em Segundo Plano (item 3)

**Files:**
- Create: `src/features/segundoPlano/SegundoPlanoPanel.tsx`, `src/features/segundoPlano/useSegundoPlano.ts`
- Modify: `src/components/Sidebar.tsx:52-61`, `:511-519`, `src/features/terminal/terminalService.ts`
- Test: `src/test/SegundoPlanoPanel.test.tsx`

**Interfaces:**
- Produces: `useSegundoPlano(projetoId?: string): { itens: ItemDeFundo[]; erro: string | null }`, consulta a cada 4 s.
- Aba `"segundoPlano"`, rótulo "Tarefas em segundo plano", `precisaProjeto: false`.

- [ ] Testes: `lista vazia explica que nada está rodando`; `cada linha mostra o que faz e desde quando`; `Abrir leva à conversa`; `Parar de subagente diz que para a sessão inteira`; `contador aparece na aba`; `engine fora do ar mostra aviso, não lista vazia`.
- [ ] Commit: `feat: painel de tarefas em segundo plano`

### Tarefa 15: panorama do projeto (item 5)

**Files:**
- Create: `src/features/panorama/FaixaDeExecucao.tsx`, `src/features/panorama/PainelDoProjeto.tsx`
- Modify: `src/features/workspace/WorkspaceView.tsx`, `src/features/workspace/PaneView.tsx:200`
- Test: `src/test/PainelDoProjeto.test.tsx`

**Interfaces:**
- Consumes: `useSegundoPlano(projetoId)`, `useExecucoesAtivas`.
- Tipo de aba novo: `"execucao-projeto"`.

- [ ] Testes: `faixa só aparece com algo rodando`; `faixa conta só o projeto atual`; `clique abre a aba`; `mini-grafo acende o nó ativo`; `mini-grafo não deixa editar`; `execução terminada sai sem fechar a aba`.
- [ ] Implementar. Mini-grafo com `@xyflow/react` somente leitura (`nodesDraggable={false}`, `nodesConnectable={false}`). Reaproveitar a montagem de ilhas de `panoramaModel.ts`.
- [ ] Commit: `feat: panorama do que roda no projeto`

## Fase 4: uso da conta

### Tarefa 16: painel de detalhe da conta (item 1)

**Files:**
- Create: `src/features/terminal/DetalheDaConta.tsx`
- Modify: `src/features/terminal/AgentUsageStatus.tsx:210`, `src/features/terminal/UsoDeTodasAsContas.tsx`
- Test: `src/test/DetalheDaConta.test.tsx`

**Interfaces:**
- Consumes: `AccountUsage` (`primary` = janela de 5h, `secondary` = semana).
- Produces: `<DetalheDaConta uso={AccountUsage} tokens={TokensDaConta | null} />`

- [ ] Testes: `clicar no nome da conta abre o detalhe`; `mostra 5h e semana separados com a renovação`; `status unavailable mostra o motivo`; `janela ausente mostra "não disponível", não 0%`; `Esc e clique fora fecham`; `painel do topo usa o mesmo detalhe`.
- [ ] Commit: `feat: detalhe de uso ao clicar na conta`

### Tarefa 17: tokens dos históricos locais (item 1)

**Files:**
- Create: `crates/omni-core/src/tokens.rs`
- Modify: `crates/omni-engine/src/mobile.rs:587-590`, `src/features/terminal/DetalheDaConta.tsx`
- Test: `crates/omni-core/src/tokens.rs`

**Interfaces:**
- Produces: `struct TokensDaConta { hoje: Option<Contagem>, sete_dias: Option<Contagem>, motivo: Option<String> }`, com `Contagem { entrada: u64, saida: u64, cache: u64 }`.
- Produces: `tokens_claude(pasta: &Path, agora_ms: i64, cache: &mut CacheDeArquivos) -> TokensDaConta` e `tokens_codex(...)`, com a mesma forma. `CacheDeArquivos` é chaveado por caminho + mtime + tamanho.
- Produces: `GET /contas/{id}/tokens`

- [ ] Testes: `soma_entrada_saida_e_cache`; `mesma_mensagem_repetida_conta_uma_vez` (mesmo `message.id`); `hoje_usa_o_fuso_local`; `linha_corrompida_e_ignorada`; `arquivo_sem_mudanca_nao_e_relido`; `codex_usa_o_ultimo_total_do_rollout` (os eventos `token_count` são acumulados; confirmar o formato num rollout real antes de escrever o teste); `antigravity_fica_nao_disponivel`.
- [ ] Implementar. A pasta dos históricos de cada conta é a mesma que `crates/omni-core/src/armazenamento.rs` já resolve por perfil.
- [ ] Commit: `feat: tokens por conta a partir dos históricos locais`

## Fase 5: Kanban e tarefas externas

### Tarefa 18: Kanban no engine (item 7)

**Files:**
- Create: `crates/omni-core/src/kanban.rs`, `crates/omni-engine/src/kanban.rs`
- Modify: `crates/omni-engine/src/mobile.rs` (rotas)
- Test: nos arquivos novos

**Interfaces:**
- Produces: `struct Quadro { versao: u32, tarefas: Vec<Cartao>, triagem: Vec<CartaoExterno>, ajustes: Value }`. `Cartao` tem os campos de `KanbanTask` em camelCase mais `externa: Option<RefExterna>`, com `RefExterna { origem: String, id: String, sumiu: bool }`. `ajustes` guarda `agentByProject`, `dispatcherEnabled`, `failureTimestamps`, `pausedUntil` como estão.
- Produces: `GET /kanban`, `POST /kanban/cartoes`, `PATCH /kanban/cartoes/{id}`, `DELETE /kanban/cartoes/{id}`, `PATCH /kanban/ajustes`, `POST /kanban/migrar`.

- [ ] Testes: `criar_e_ler_cartao`; `editar_um_cartao_nao_afeta_outro`; `dois_pedidos_seguidos_nao_perdem_cartao`; `gravacao_e_atomica` (arquivo temporário + rename); `migrar_so_vale_com_quadro_vazio` (segunda chamada não sobrescreve); `arquivo_corrompido_vira_copia_e_nao_apaga`; `rota_exige_token`.
- [ ] Commit: `feat: Kanban guardado no engine`

### Tarefa 19: tela do Kanban usa o engine e migra (item 7)

**Files:**
- Modify: `src/services/kanbanService.ts`, `src/features/kanban/kanbanReducer.ts`, `src/App.tsx` (onde carrega o Kanban), `src/features/terminal/terminalService.ts`
- Test: `src/test/kanbanService.test.ts`

**Interfaces:**
- Consumes: rotas da Tarefa 18.
- Produces: `carregarKanban(): Promise<KanbanState>`, `aplicarNoEngine(acao: KanbanAction): Promise<void>`, `migrarSePreciso(): Promise<"migrou" | "nada" | "falhou">`

- [ ] Testes: `migra o localStorage para o engine vazio`; `mantém a chave antiga depois de migrar`; `engine fora do ar não apaga nada e avisa`; `localStorage corrompido não trava e não migra lixo`; `não migra de novo com quadro cheio`; `cada ação vira uma operação de um cartão`; `falha ao gravar desfaz na tela e mostra erro`.
- [ ] Implementar. O reducer continua dono da tela, atualizando na hora e confirmando no engine. `useKanbanDispatcher` fica como está. Web/celular não muda.
- [ ] Commit: `feat: Kanban migra para o engine sem perder cartões`

### Tarefa 20: triagem das tarefas externas (item 7)

**Files:**
- Create: `src/features/kanban/TriagemExterna.tsx`
- Modify: `crates/omni-engine/src/kanban.rs`, `crates/omni-engine/src/mobile.rs`, `src-tauri/src/tarefas.rs`, `src/features/kanban/KanbanBoard.tsx`, `src/features/kanban/useKanbanDispatcher.ts`
- Test: `crates/omni-engine/src/kanban.rs`, `src/test/TriagemExterna.test.tsx`

**Interfaces:**
- Produces: `POST /kanban/externas` (recebe a lista processada, deduplica por `origem + id`, marca `sumiu` no que saiu) e `POST /kanban/externas/{id}/projeto` (body `{ projectId }`, move da triagem para Backlog).
- Produces: comando Tauri `kanban_atualizar_externas() -> Result<ResumoDaImportacao, String>`, que usa a busca atual e o bloqueio de `espera_ms`, e envia o resultado ao engine.

- [ ] Testes: `atualizar_nao_duplica`; `tarefa_que_sumiu_fica_marcada`; `escolher_projeto_vira_backlog`; `cartao_ja_em_projeto_nao_volta_para_triagem`; `bloqueio_de_intervalo_devolve_quanto_falta`; TS: `botão Atualizar é a única forma de buscar` (nenhum `setInterval` chama a busca); `triagem mostra o selo Externa`; `usuário escolhe o projeto`; `dispatcher ignora a triagem`; `cartão externo em Pronta sobe com a marca`.
- [ ] Implementar. Sessão aberta pelo dispatcher para cartão externo leva `OMNI_VEIO_DE_FORA=1` no ambiente, como `OMNI_GH_CONTA`.
- [ ] Commit: `feat: tarefas externas entram por triagem no Kanban`

### Tarefa 21: Kanban pelo MCP (item 7)

**Files:**
- Create: `crates/omni-rag/src/mcp_kanban.rs`
- Modify: `crates/omni-rag/src/mcp.rs`
- Test: `crates/omni-rag/src/testes_kanban_mcp.rs`

**Interfaces:**
- Consumes: rotas da Tarefa 18 pelo mesmo `api(contexto, ...)` de `mcp_fluxos.rs`; marca da Tarefa 12.
- Produces: ferramentas `listar_cartoes(projeto?)`, `pegar_cartao(id)`, `mover_cartao(id, status)`.

- [ ] Testes: `lista_so_o_projeto_pedido`; `pegar_move_para_rodando`; `pegar_cartao_em_triagem_recusa` (o projeto é escolha do usuário); `pegar_cartao_externo_liga_a_marca`; `texto_externo_vem_entre_as_marcas_de_dados` (reaproveitar `MARCA` e `FIM_DOS_DADOS` de `tarefas.rs`); `status_invalido_recusa`.
- [ ] Commit: `feat: agentes veem e pegam cartões do Kanban pelo MCP`

## Fase 6: grafo

### Tarefa 22: notas e ligações (item 6)

**Files:**
- Create: `crates/omni-rag/src/grafo.rs`, `crates/omni-rag/src/testes_grafo.rs`
- Modify: `crates/omni-rag/src/lib.rs`, `src-tauri/src/` (comando no módulo que já abre a memória)

**Interfaces:**
- Produces: `grafo_de_notas(memoria: &Memoria, projeto: Option<&str>) -> Grafo`, com `Grafo { notas: Vec<NotaDoGrafo>, ligacoes: Vec<Ligacao> }`, `NotaDoGrafo { slug, titulo, resumo, tags, projeto }`, `Ligacao { de, para, tipo, peso }` e `tipo` em `wikilink`, `conversa`, `tag`, `texto`.
- Produces: comando Tauri `memoria_grafo(projeto: Option<String>) -> Result<Grafo, String>`. É comando Tauri, não rota, porque o engine não depende do `omni-rag`.
- Constantes: `TAG_COMUM_DEMAIS = 30` (tag em mais notas que isso não liga), `VIZINHOS_DE_TEXTO = 3`.

- [ ] Testes: `wikilink_liga_e_pesa_mais`; `wikilink_para_nota_inexistente_nao_liga`; `mesma_conversa_liga`; `conversa_vazia_nao_liga_tudo`; `tag_comum_demais_nao_liga`; `texto_parecido_limita_a_3_vizinhas`; `sem_ligacao_duplicada_nos_dois_sentidos`; `nota_nao_liga_a_si_mesma`; `filtro_de_projeto`; `resumo_passa_por_sem_segredos`; `banco_vazio_devolve_grafo_vazio`.
- [ ] Medir com a memória real do usuário (somente leitura). Se passar de 500 ms, guardar em cache por `max(atualizado_ms)`.
- [ ] Commit: `feat: ligações entre as notas de memória`

### Tarefa 23: aba Grafo (item 6)

**Files:**
- Create: `src/features/grafo/GrafoPanel.tsx`, `src/features/grafo/GrafoCanvas.tsx`, `src/features/grafo/grafoModel.ts`
- Modify: `src/components/Sidebar.tsx:52-61`, `:511-519`, `src/features/workspace/PaneView.tsx:200`, `package.json`
- Test: `src/test/grafoModel.test.ts`, `src/test/GrafoPanel.test.tsx`

**Interfaces:**
- Consumes: `memoria_grafo` (Tarefa 22).
- Produces: `filtrarGrafo(grafo: Grafo, filtro: { projeto: string | null; tipos: TipoDeLigacao[] }): Grafo` e `vizinhas(grafo: Grafo, slug: string): Set<string>`
- Aba `"grafo"`, rótulo "Grafo", `precisaProjeto: false`.

- [ ] Pesquisar a biblioteca de layout (`d3-force` é a candidata: pequena e sem React), conferindo tamanho, licença e versão, e registrar a escolha no roadmap.
- [ ] Testes: `filtro por tipo remove ligações e mantém notas`; `filtro por projeto tira ligações órfãs`; `vizinhas inclui os dois sentidos`; `sem notas explica como a memória nasce`; `clicar na nota mostra o texto e destaca as vizinhas`; `lista de notas acessível por teclado` (o canvas sozinho não é navegável); `abrir grande cria a aba`.
- [ ] Implementar. Desenho em canvas, pausando o layout quando a aba não está visível. Mesmo projeto vira cor, não ligação.
- [ ] Commit: `feat: grafo de conhecimento na barra lateral`

## Fase 7: tirar mensagem da fila (item 9)

### Tarefa 24: fila do OMNI no engine

**Files:**
- Create: `crates/omni-core/src/fila.rs`, `crates/omni-engine/src/fila.rs`
- Modify: `crates/omni-engine/src/mobile.rs:1613-1622`, `:2660-2670`, `src/mobile/MobileApp.tsx:568-569`, `:1171-1179`, `src/mobile/Chat.tsx` (`MessageBubble`), `src/mobile/api.ts`
- Test: nos arquivos novos, `src/test/MobileApp.test.tsx`

**Interfaces:**
- Produces: `enum ModoDeFila { Cli, Omni }` e `modo_de_fila(provider: &str) -> ModoDeFila`, que devolve `Omni` para toda CLI até a Tarefa 25 medir.
- Produces: `struct ItemDaFila { id: String, texto: String, anexos: Vec<String>, em_ms: u64 }`, guardado em disco por conversa.
- Produces: `GET /conversas/{id}/fila` e `DELETE /conversas/{id}/fila/{item}`. O `DELETE` devolve 409 se o item já foi entregue.
- Produces: `MessageBubble` ganha `onRemover?: () => void`, botão com `aria-label="Tirar da fila"`.

**Regras que os testes não fixam sozinhos:** a entrega é uma mensagem por vez, na ordem, quando `capability.prompt` fica verdadeiro, pelo mesmo caminho de `ActionKind::Prompt`. Depois de Parar, a fila não é entregue sozinha: fica marcada como pausada até o usuário enviar algo novo ou tirar os itens. Sessão que morreu mantém a fila e avisa. Mensagem que hoje seria recusada com `Sessão ocupada ou tela não reconhecida` (409) entra na fila do OMNI em vez de virar erro: é o que fecha o "mandar livremente" do item 10 (teste `tela_nao_reconhecida_guarda_na_fila`).

- [ ] Testes Rust: `mensagem_com_agente_trabalhando_fica_no_omni`; `entrega_em_ordem_quando_o_agente_termina`; `uma_por_vez_entre_turnos`; `tirar_da_fila_nao_entrega`; `tirar_item_entregue_devolve_409`; `dois_cliques_no_x_nao_quebram`; `parar_nao_solta_a_fila`; `fila_sobrevive_ao_engine_religado`; `aprovacao_pendente_segura_a_entrega`; `rota_exige_token`.
- [ ] Testes TS: `mensagem na fila mostra o X`; `clicar no X tira a mensagem`; `409 avisa que já foi entregue`; `falha mantém a mensagem e mostra o motivo`; `fila aparece igual no celular`; `mensagem enviada com o agente parado não ganha X`.
- [ ] Implementar. A tela passa a ler a fila do engine, então PC e celular veem a mesma lista.
- [ ] Commit: `feat: tirar mensagem da fila antes de ela entrar`

### Tarefa 25: X por tecla nas CLIs medidas

**Files:**
- Modify: `crates/omni-core/src/fila.rs`, `crates/omni-engine/src/interaction.rs`, `crates/omni-engine/src/fila.rs`
- Test: `crates/omni-engine/tests-fixtures/telas/`, `crates/omni-engine/src/interaction.rs`

**Interfaces:**
- Consumes: `modo_de_fila`, rotas da Tarefa 24.
- Produces: `mensagens_na_fila(tela: &str, provider: &str) -> Vec<String>`, lidas da tela real; `teclas_para_tirar(provider: &str) -> Option<&'static str>`.

- [ ] Para cada CLI (Claude, Codex, Cursor, Antigravity), com o usuário: enviar mensagem durante um turno, capturar a tela no acervo, descobrir como tirar a mensagem e capturar a tela depois. Registrar a versão da CLI e o resultado no roadmap.
- [ ] Só passa para `ModoDeFila::Cli` a CLI em que a retirada funcionou e dá para confirmar pela tela. As outras ficam em `Omni`.
- [ ] Testes, por CLI aprovada: `tela_mostra_a_mensagem_na_fila`; `x_confirma_pela_tela_antes_de_dizer_que_saiu`; `tela_nao_mudou_devolve_erro_e_mantem_a_bolha`; `texto_novo_da_cli_cai_para_erro_sem_mentir`. Sem CLI aprovada, a tarefa termina sem código, só com o registro.
- [ ] Commit: `feat: tirar da fila da CLI onde a tecla foi medida`

## Fechamento

### Tarefa 26: novidades e verificação final

**Files:**
- Modify: `src/lib/changelog.ts`, `roadmaps/omni-agents/roadmap.md`, `roadmaps/orquestracao/roadmap.md`, `CLAUDE.md` (se a armadilha do Windows mudar)

- [ ] Rodar `cargo test --workspace`, `npm run test` e `npm run typecheck`. Saída esperada: tudo verde. Falha é informada, não escondida.
- [ ] Conferir cobertura de 80% nos arquivos novos.
- [ ] Passar a revisão de código no branch inteiro, com atenção à fase 2.
- [ ] Teste de ponta a ponta em `npm run dev`, com o usuário ciente: fluxo X → Y → Z com ramo paralelo, Parar no meio, aprovação manual, execução pela IA, tarefa externa com aprovação, servidor local derrubado, arquivo citado pelo nome, mensagem tirada da fila, mensagem enviada logo depois de o agente terminar.
- [ ] Escrever as novidades em linguagem de usuário e registrar as entregas no roadmap.
- [ ] Commit: `docs: novidades das oito melhorias`. **Não publicar:** release só com pedido explícito.
