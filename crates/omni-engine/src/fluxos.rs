//! Executor de fluxos de agentes. Cada execução roda numa thread própria, num git worktree próprio
//! (branch `omni/<fluxo>-<id>`), com os agentes sem tela (`claude -p` / `codex exec`). O estado vai
//! para `<engine>/execucoes/<id>.json` a cada passo: reabrir o engine não perde nada — o que estava
//! rodando volta como pausado. O histórico de cada nó (eventos da CLI) fica em `<id>/<no>.log`.

use super::*;
use omni_core::fluxos::{self, Contexto, Fluxo, TipoDeNo};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::io::BufRead;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EstadoDoNo {
    /// `pendente`, `rodando`, `concluido`, `falhou`, `pausado`, `aguardando`.
    pub status: String,
    #[serde(default)]
    pub voltas: u32,
    #[serde(default)]
    pub inicio_ms: u64,
    #[serde(default)]
    pub fim_ms: u64,
    /// O que o nó produziu (texto final do agente, ata da sala, URL do PR…).
    #[serde(default)]
    pub saida: String,
    /// Sessão da CLI: retomar o nó numa conversa (`claude --resume`).
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub custo_usd: f64,
    #[serde(default)]
    pub erro: Option<String>,
    /// Conta e CLI usadas (para abrir a conversa na mesma conta).
    #[serde(default)]
    pub cli: Option<String>,
    #[serde(default)]
    pub conta: Option<String>,
    /// Saída escolhida (`sim`/`nao`/`esgotado`) em condição e aprovação.
    #[serde(default)]
    pub caminho: Option<String>,
    /// O que o agente está fazendo agora (última ação ou fala), para a tela mostrar ao vivo.
    #[serde(default)]
    pub atividade: String,
    /// Modelo e esforço com que rodou.
    #[serde(default)]
    pub modelo: Option<String>,
    #[serde(default)]
    pub esforco: Option<String>,
    /// Quantas vezes o nó começou a rodar (a segunda em diante é uma volta nova no log).
    #[serde(default)]
    pub corridas: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Passo { pub no: String, #[serde(default)] pub de: Option<String> }

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Execucao {
    pub id: String,
    pub fluxo: Fluxo,
    pub projeto: String,
    pub projeto_id: String,
    pub entrada: String,
    /// Onde os agentes trabalham: o worktree (projeto com git) ou a própria pasta do projeto.
    pub pasta: String,
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub base: Option<String>,
    /// `rodando`, `pausado`, `aguardando`, `concluido`, `falhou`, `cancelado`.
    pub status: String,
    #[serde(default)]
    pub erro: Option<String>,
    pub criado_ms: u64,
    pub atualizado_ms: u64,
    pub nos: HashMap<String, EstadoDoNo>,
    pub fila: Vec<Passo>,
    #[serde(default)]
    pub pr_url: Option<String>,
    #[serde(default)]
    pub custo_usd: f64,
    /// `pastas_extras` do fluxo já resolvidas contra o projeto: o que vira `--add-dir`.
    #[serde(default)]
    pub pastas_extras: Vec<String>,
    /// Custo que você já aceitou ao retomar depois do limite: `custo_maximo` conta a partir daqui.
    #[serde(default)]
    pub custo_aceito: f64,
    /// Pausada pelo `custo_maximo`. Só o usuário retoma: o MCP recusa.
    #[serde(default)]
    pub parou_por_custo: bool,
    /// Sobe a cada evento que merece aviso, como o `attention_seq` das sessões; `aviso` diz qual foi.
    #[serde(default)]
    pub aviso_seq: u32,
    #[serde(default)]
    pub aviso: Option<String>,
    /// Número do último evento da linha do tempo (`eventos.jsonl`); o próximo é este + 1.
    #[serde(default)]
    pub evento_seq: u64,
}

type Viva = Arc<Mutex<Execucao>>;
fn execucoes() -> &'static Mutex<HashMap<String, Viva>> { static M: OnceLock<Mutex<HashMap<String, Viva>>> = OnceLock::new(); M.get_or_init(Default::default) }
/// Processo rodando agora, por `<execução>/<nó>` (ramos paralelos têm um cada) — é o que pausar/cancelar mata.
fn processos() -> &'static Mutex<HashMap<String, u32>> { static M: OnceLock<Mutex<HashMap<String, u32>>> = OnceLock::new(); M.get_or_init(Default::default) }

fn pasta_execucoes(state: &EngineState) -> PathBuf { state.state_file.with_file_name("execucoes") }

fn gravar(state: &EngineState, e: &Execucao) {
    let pasta = pasta_execucoes(state);
    let _ = fs::create_dir_all(&pasta);
    let _ = atomic_write_json(&pasta.join(format!("{}.json", e.id)), e);
}

use omni_core::fluxos::id_valido;

/// Lê as execuções do disco ao subir o engine. O que estava rodando volta como pausado: o processo
/// da CLI morreu junto com o engine, e retomar é decisão sua.
pub fn carregar(state: &EngineState) {
    let Ok(entradas) = fs::read_dir(pasta_execucoes(state)) else { return };
    let mut mapa = execucoes().lock().expect("execucoes");
    for caminho in entradas.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")) {
        let Ok(mut e) = serde_json::from_slice::<Execucao>(&fs::read(&caminho).unwrap_or_default()) else { continue };
        if e.status == "rodando" {
            e.status = "pausado".into();
            for no in e.nos.values_mut().filter(|n| n.status == "rodando") { no.status = "pausado".into(); }
        }
        mapa.insert(e.id.clone(), Arc::new(Mutex::new(e)));
    }
}

pub fn listar() -> Vec<Value> {
    let mut lista: Vec<Execucao> = execucoes().lock().expect("execucoes").values().filter_map(|v| v.lock().ok().map(|e| e.clone())).collect();
    lista.sort_by_key(|e| std::cmp::Reverse(e.criado_ms));
    lista.into_iter().map(|e| json!({"id": e.id, "fluxo": e.fluxo.nome, "fluxo_id": e.fluxo.id, "projeto": e.projeto, "projeto_id": e.projeto_id,
        "entrada": e.entrada.chars().take(160).collect::<String>(), "status": e.status, "criado_ms": e.criado_ms, "atualizado_ms": e.atualizado_ms,
        "pr_url": e.pr_url, "custo_usd": e.custo_usd, "erro": e.erro, "aviso_seq": e.aviso_seq, "aviso": e.aviso})).collect()
}

pub fn obter(id: &str) -> Option<Execucao> { execucoes().lock().ok()?.get(id)?.lock().ok().map(|e| e.clone()) }

fn viva(id: &str) -> Result<Viva> { execucoes().lock().map_err(|_| anyhow!("execucoes"))?.get(id).cloned().ok_or_else(|| anyhow!("Execução não encontrada")) }

fn git(pasta: &str, args: &[&str]) -> Result<String> {
    let mut c = Command::new("git");
    c.args(args).current_dir(pasta).env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
    sem_janela(&mut c);
    let saida = c.output().context("git não encontrado")?;
    if !saida.status.success() { return Err(anyhow!("git {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&saida.stderr).trim())); }
    Ok(String::from_utf8_lossy(&saida.stdout).trim().to_owned())
}

fn sem_janela(c: &mut Command) {
    #[cfg(windows)] { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000); }
    let _ = c;
}

fn resolver_pastas(fluxo: &Fluxo, projeto: &str) -> Result<Vec<String>> {
    let home = fluxos::pasta_do_usuario();
    fluxo.pastas_extras.iter().map(|pasta| {
        fluxos::resolver_pasta_extra(Path::new(projeto), pasta, home.as_deref()).map(|real| fluxos::texto_do_caminho(&real)).map_err(|erro| anyhow!(erro))
    }).collect()
}

/// Começa uma execução: prepara o worktree e solta a thread.
pub fn iniciar(state: &Arc<EngineState>, fluxo: Fluxo, projeto: String, projeto_id: String, entrada: String) -> Result<Execucao> {
    fluxos::validar(&fluxo).map_err(|e| anyhow!(e))?;
    if entrada.trim().is_empty() { return Err(anyhow!("Escreva a tarefa")); }
    if !Path::new(&projeto).is_dir() { return Err(anyhow!("A pasta do projeto não existe neste PC")); }
    let agora = now_ms();
    let id = format!("exec-{agora}-{:04x}", rand::random::<u16>());
    let tem_git = git(&projeto, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|s| s == "true");
    let tem_pr = fluxo.nos.iter().any(|n| n.tipo == TipoDeNo::Pr);
    if tem_pr && !tem_git { return Err(anyhow!("Este fluxo abre um PR, mas o projeto não é um repositório git")); }
    let (pasta, branch, base) = if tem_git && fluxo.na_pasta_do_projeto {
        (projeto.clone(), None, Some(git(&projeto, &["rev-parse", "--abbrev-ref", "HEAD"])?))
    } else if tem_git {
        let base = git(&projeto, &["rev-parse", "--abbrev-ref", "HEAD"])?;
        let branch = format!("omni/{}-{}", fluxo.id, &id[id.len() - 4..]);
        let pasta = state.state_file.with_file_name("worktrees").join(&id);
        let _ = fs::create_dir_all(pasta.parent().unwrap());
        git(&projeto, &["worktree", "add", "-b", &branch, &pasta.to_string_lossy(), "HEAD"])?;
        (pasta.to_string_lossy().into_owned(), Some(branch), Some(base))
    } else {
        (projeto.clone(), None, None)
    };
    let pastas_extras = resolver_pastas(&fluxo, &projeto)?;
    let inicio = fluxos::entrada(&fluxo).ok_or_else(|| anyhow!("sem entrada"))?.id.clone();
    let nos = fluxo.nos.iter().map(|n| (n.id.clone(), EstadoDoNo { status: "pendente".into(), ..Default::default() })).collect();
    let e = Execucao { id: id.clone(), fluxo, projeto, projeto_id, entrada, pasta, branch, base, status: "rodando".into(), erro: None,
        criado_ms: agora, atualizado_ms: agora, nos, fila: vec![Passo { no: inicio, de: None }], pr_url: None, custo_usd: 0.0,
        pastas_extras, custo_aceito: 0.0, parou_por_custo: false, aviso_seq: 0, aviso: None, evento_seq: 0 };
    gravar(state, &e);
    execucoes().lock().map_err(|_| anyhow!("execucoes"))?.insert(id.clone(), Arc::new(Mutex::new(e.clone())));
    soltar(state.clone(), id);
    Ok(e)
}

/// Muda o que o nó está fazendo; só vira linha na linha do tempo quando o texto muda de verdade.
fn por_atividade(state: &EngineState, e: &mut Execucao, no: &str, texto: String) {
    let Some(n) = e.nos.get_mut(no) else { return };
    if n.atividade == texto { return; }
    n.atividade.clone_from(&texto);
    eventos::registrar(state, e, Some(no), "atividade", &texto);
}

fn definir_atividade(state: &EngineState, v: &Viva, no: &str, texto: &str) {
    mexer(state, v, |e| por_atividade(state, e, no, texto.to_owned()));
}

fn soltar(state: Arc<EngineState>, id: String) { std::thread::spawn(move || executar(&state, &id)); }

/// Muda a execução e grava. Devolve o que a função devolveu.
fn mexer<T>(state: &EngineState, v: &Viva, f: impl FnOnce(&mut Execucao) -> T) -> T {
    let mut e = v.lock().expect("execucao");
    let status_antes = e.status.clone();
    let r = f(&mut e);
    if e.status != status_antes { registrar_aviso(&mut e); }
    e.atualizado_ms = now_ms();
    gravar(state, &e);
    r
}

/// O que a tela e o celular precisam avisar quando a execução muda de status.
fn registrar_aviso(e: &mut Execucao) {
    let aviso = match e.status.as_str() {
        "aguardando" => "aprovacao",
        "falhou" => "falhou",
        "concluido" => "concluido",
        "pausado" if e.parou_por_custo => "custo",
        _ => return,
    };
    e.aviso = Some(aviso.into());
    e.aviso_seq += 1;
}

fn estourou_o_custo(e: &Execucao) -> bool {
    e.fluxo.custo_maximo.is_some_and(|maximo| e.custo_usd - e.custo_aceito >= maximo)
}

fn pausar_por_custo(e: &mut Execucao) {
    let maximo = e.fluxo.custo_maximo.unwrap_or_default();
    e.parou_por_custo = true;
    e.status = "pausado".into();
    e.erro = Some(format!("Pausado pelo limite de custo: US$ {:.2} gastos, limite de US$ {maximo:.2}. O custo de uma etapa só é conhecido quando ela termina, então o total pode passar do limite. Ao retomar, vale mais US$ {maximo:.2}.", e.custo_usd));
}

/// Tira da fila o que pode rodar agora. Conta o que já está rodando porque `aprovar` solta outro
/// executor enquanto um ramo ainda trabalha: quem esbarra no teto sai, e o executor daquele ramo
/// pega o resto da fila quando terminar.
fn proximo_lote(e: &mut Execucao) -> Option<Vec<Passo>> {
    if e.status != "rodando" { return None; }
    let rodando = e.nos.values().filter(|n| n.status == "rodando").count();
    if e.fila.is_empty() {
        if rodando == 0 { e.status = "concluido".into(); }
        return None;
    }
    if estourou_o_custo(e) {
        if rodando == 0 { pausar_por_custo(e); }
        return None;
    }
    let vagas = fluxos::limite_paralelo(&e.fluxo).saturating_sub(rodando).min(e.fila.len());
    if vagas == 0 { return None; }
    let lote: Vec<Passo> = e.fila.drain(..vagas).collect();
    // Já marcados aqui, para outro executor não dar a execução por concluída no meio.
    for p in &lote { if let Some(n) = e.nos.get_mut(&p.no) { n.status = "rodando".into(); } }
    Some(lote)
}

fn executar(state: &Arc<EngineState>, id: &str) {
    let Ok(v) = viva(id) else { return };
    while let Some(lote) = mexer(state, &v, proximo_lote) {
        // ponytail: ramos paralelos dividem o mesmo worktree; dois agentes no mesmo arquivo brigam.
        // Worktree por ramo + merge no Juntar se isso aparecer.
        let viva_ref = &v;
        let erros: Vec<(Passo, anyhow::Error)> = std::thread::scope(|s| {
            let tarefas: Vec<_> = lote.into_iter().map(|passo| s.spawn(move || { let r = processar(state, viva_ref, &passo); (passo, r) })).collect();
            tarefas.into_iter().filter_map(|t| t.join().ok().and_then(|(passo, r)| r.err().map(|erro| (passo, erro)))).collect()
        });
        if erros.is_empty() { continue; }
        // Tudo numa operação só: com `falhou` visível antes da segunda etapa do lote, um retomar nesse
        // intervalo perderia a etapa que ainda não tinha voltado para a fila.
        mexer(state, &v, |e| for (passo, erro) in &erros {
            let pausado = erro.to_string() == PAUSADO;
            let texto = if pausado { "Etapa pausada".to_owned() } else { erro.to_string() };
            eventos::registrar(state, e, Some(&passo.no), if pausado { "aviso" } else { "falha" }, &texto);
            if let Some(n) = e.nos.get_mut(&passo.no) {
                n.status = if pausado { "pausado".into() } else { "falhou".into() };
                if !pausado { n.erro = Some(erro.to_string()); }
                n.fim_ms = now_ms();
            }
            // Volta para o começo da fila com a origem: retomar refaz só esta etapa, com o mesmo `{{anterior}}`.
            if e.status != "cancelado" { e.fila.insert(0, passo.clone()); }
            if !pausado && (e.status == "rodando" || e.status == "aguardando") {
                e.status = "falhou".into();
                e.erro = Some(format!("{}: {erro}", passo.no));
            }
        });
        break;
    }
}

const PAUSADO: &str = "__pausado__";

fn saida_de(e: &Execucao, no: &Option<String>) -> String { no.as_ref().and_then(|n| e.nos.get(n)).map(|n| n.saida.clone()).unwrap_or_default() }

fn processar(state: &Arc<EngineState>, v: &Viva, passo: &Passo) -> Result<()> {
    let (no, ctx, pasta, projeto, run_id) = {
        let e = v.lock().map_err(|_| anyhow!("execucao"))?;
        let no = e.fluxo.nos.iter().find(|n| n.id == passo.no).cloned().ok_or_else(|| anyhow!("nó sumiu"))?;
        let ctx = Contexto { entrada: e.entrada.clone(), anterior: saida_de(&e, &passo.de),
            saidas: e.nos.iter().map(|(k, n)| (k.clone(), n.saida.clone())).collect() };
        (no, ctx, e.pasta.clone(), e.projeto.clone(), e.id.clone())
    };
    let corrida = mexer(state, v, |e| {
        let corrida = e.nos.get_mut(&no.id).map_or(0, |n| { n.status = "rodando".into(); n.inicio_ms = now_ms(); n.erro = None; n.atividade.clear(); n.corridas += 1; n.corridas });
        eventos::registrar(state, e, Some(&no.id), "inicio", &fluxos::rotulo(&no));
        corrida
    });
    if corrida > 1 { eventos::separar_log(state, &run_id, &no.id, corrida); }

    let caminho: String = match no.tipo {
        TipoDeNo::Entrada => { definir_saida(state, v, &no.id, ctx.entrada.clone()); String::new() }
        TipoDeNo::Fim => { definir_saida(state, v, &no.id, ctx.anterior.clone()); String::new() }
        TipoDeNo::Juntar => { let junto = v.lock().map(|e| fluxos::saida_do_juntar(&e.fluxo, &no.id, &ctx.saidas)).unwrap_or_default(); definir_saida(state, v, &no.id, junto); String::new() }
        TipoDeNo::Agente => {
            let retomar = v.lock().ok().and_then(|e| e.nos.get(&no.id).filter(|n| n.status == "rodando" && n.session_id.is_some() && n.saida.is_empty()).and_then(|n| n.session_id.clone()));
            let prompt = fluxos::montar_prompt(&no, &ctx);
            let ajuste = Ajuste { modelo: texto_de(&no.config, "modelo"), esforco: texto_de(&no.config, "esforco"), conta: texto_de(&no.config, "conta"), ..Ajuste::limites_de(&no) };
            let r = rodar_agente(state, v, &no.id, &no.agente, &prompt, retomar.as_deref(), &pasta, &projeto, &ajuste)?;
            definir_saida(state, v, &no.id, r);
            String::new()
        }
        TipoDeNo::Condicao => {
            let max = no.config["max_voltas"].as_u64().unwrap_or(3) as u32;
            let ok = fluxos::veredito(&ctx.anterior).unwrap_or(false);
            let saida = mexer(state, v, |e| {
                let n = e.nos.get_mut(&no.id).expect("no");
                if ok { "sim" } else { n.voltas += 1; if n.voltas >= max { "esgotado" } else { "nao" } }
            });
            definir_saida(state, v, &no.id, ctx.anterior.clone());
            saida.to_owned()
        }
        TipoDeNo::Aprovacao => {
            // Para aqui e espera: `aprovar` põe os próximos na fila e retoma.
            mexer(state, v, |e| {
                e.status = "aguardando".into();
                if let Some(n) = e.nos.get_mut(&no.id) { n.status = "aguardando".into(); n.saida = ctx.anterior.clone(); }
                eventos::registrar(state, e, Some(&no.id), "aprovacao", "Aguardando aprovação");
            });
            return Ok(());
        }
        TipoDeNo::Sala => { let ata = rodar_sala(state, v, &no, &ctx, &pasta, &projeto)?; definir_saida(state, v, &no.id, ata); String::new() }
        TipoDeNo::Pr => { let url = abrir_pr(v, &no)?; mexer(state, v, |e| e.pr_url = Some(url.clone())); definir_saida(state, v, &no.id, url); String::new() }
        // Provisório até a Tarefa 9, que roda a execução filha.
        TipoDeNo::Subfluxo => return Err(anyhow!("Subfluxo ainda não disponível nesta versão")),
        TipoDeNo::Comando => {
            let comando = no.config["comando"].as_str().unwrap_or_default().to_owned();
            let (ok, saida) = rodar_comando(state, v, &no.id, &comando, &pasta)?;
            let max = no.config["max_voltas"].as_u64().unwrap_or(3).max(1) as u32;
            let caminho = mexer(state, v, |e| {
                let n = e.nos.get_mut(&no.id).expect("no");
                if ok { "sim" } else { n.voltas += 1; if n.voltas >= max { "esgotado" } else { "nao" } }
            });
            // Falhou e ninguém trata essa saída: a execução falha (dá para retomar) em vez de
            // terminar como "concluída" com o build quebrado.
            let tratado = v.lock().map(|e| !fluxos::proximos(&e.fluxo, &no.id, caminho).is_empty()).unwrap_or(false);
            if !ok && !tratado {
                let ultima = saida.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("sem saída").to_owned();
                // Sem `definir_saida`: "concluído" aqui soltaria o Juntar de um ramo irmão antes da falha.
                mexer(state, v, |e| if let Some(n) = e.nos.get_mut(&no.id) { n.saida = saida; });
                return Err(anyhow!("o comando falhou: {ultima}"));
            }
            definir_saida(state, v, &no.id, saida);
            caminho.to_owned()
        }
    };
    seguir(state, v, &no.id, &caminho);
    Ok(())
}

fn definir_saida(state: &EngineState, v: &Viva, no: &str, saida: String) {
    mexer(state, v, |e| {
        if let Some(n) = e.nos.get_mut(no) { n.saida = saida; n.status = "concluido".into(); n.fim_ms = now_ms(); }
        eventos::registrar(state, e, Some(no), "fim", "Concluído");
    });
}

/// Os próximos de `no`. Na saída `nao` de uma Condição, o revisor pode pedir só alguns ramos
/// (`refazer`): ele escolhe entre as conexões que já existem, nunca abre caminho novo. Se nenhum id
/// bater com uma conexão, vale tudo o que está ligado.
fn proximos_da_saida(e: &Execucao, no: &str, saida: &str) -> Vec<String> {
    let ligados: Vec<String> = fluxos::proximos(&e.fluxo, no, saida).into_iter().map(|n| n.id.clone()).collect();
    let eh_condicao = e.fluxo.nos.iter().any(|n| n.id == no && n.tipo == TipoDeNo::Condicao);
    if saida != "nao" || !eh_condicao { return ligados; }
    let pedidos = fluxos::refazer(e.nos.get(no).map_or("", |n| n.saida.as_str()));
    let escolhidos: Vec<String> = ligados.iter().filter(|id| pedidos.contains(id)).cloned().collect();
    if escolhidos.is_empty() { ligados } else { escolhidos }
}

/// Voltar a um nó invalida o que vem depois dele, até o próximo Juntar. Sem isto, o fim de um ramo
/// longo que vai rodar de novo ainda contaria como pronto e o Juntar sairia antes da hora. Anda só
/// para a frente: `nao` e `esgotado` são a volta do loop, e cruzá-los reabriria ramos que já terminaram.
fn reabrir_ate_o_juntar(e: &mut Execucao, de: &str, origem: &str) {
    let mut fila = vec![de.to_owned()];
    let mut vistos: HashSet<String> = HashSet::from([origem.to_owned()]);
    while let Some(atual) = fila.pop() {
        if !vistos.insert(atual.clone()) { continue; }
        let seguintes: Vec<String> = e.fluxo.conexoes.iter().filter(|c| c.de == atual && matches!(c.saida.as_str(), "" | "sim")).map(|c| c.para.clone()).collect();
        for id in seguintes {
            if e.fluxo.nos.iter().any(|n| n.id == id && n.tipo == TipoDeNo::Juntar) { continue; }
            if let Some(n) = e.nos.get_mut(&id).filter(|n| n.status == "concluido") { n.status = "pendente".into(); }
            fila.push(id);
        }
    }
}

fn seguir(state: &EngineState, v: &Viva, no: &str, saida: &str) {
    mexer(state, v, |e| {
        if let Some(n) = e.nos.get_mut(no) { if !saida.is_empty() { n.caminho = Some(saida.to_owned()); } }
        for p in proximos_da_saida(e, no, saida) {
            // Juntar: só entra na fila quando o último ramo chega (os outros já terminaram).
            if e.fluxo.nos.iter().any(|n| n.id == p && n.tipo == TipoDeNo::Juntar) {
                let faltam = fluxos::antecessores(&e.fluxo, &p).into_iter().any(|a| a != no && e.nos.get(a).is_none_or(|n| n.status != "concluido"));
                if faltam || e.fila.iter().any(|x| x.no == p) { continue; }
            }
            // Voltar a um nó (loop QA → dev): ele roda de novo do zero, sem retomar a sessão anterior.
            if let Some(n) = e.nos.get_mut(&p) { n.status = "pendente".into(); n.session_id = None; n.saida.clear(); }
            reabrir_ate_o_juntar(e, &p, no);
            e.fila.push(Passo { no: p, de: Some(no.to_owned()) });
        }
    });
}

/// Modelo/esforço/conta escolhidos no nó, por cima dos do modelo de agente, e o que a etapa não pode.
#[derive(Default)]
struct Ajuste { modelo: Option<String>, esforco: Option<String>, conta: Option<String>, proibidas: Vec<String>, escrita: Option<Vec<String>> }

impl Ajuste {
    fn limites_de(no: &fluxos::No) -> Self { Self { proibidas: no.proibidas.clone(), escrita: no.escrita.clone(), ..Self::default() } }
}

/// Argumentos do `claude -p`. Sem `escrita`, nada muda: `acceptEdits` grava em toda a pasta. Com ela,
/// só o modo `default` respeita regra de caminho, o shell sai, e os settings do usuário e do projeto
/// ficam de fora porque um `allow` lá alargaria o limite (medido; ver `omni_core::fluxos`).
fn args_do_claude(modelo: &omni_core::agentes::ModeloDeAgente, ajuste: &Ajuste, extras: &[String], retomar: Option<&str>) -> Vec<String> {
    let modo = if ajuste.escrita.is_some() { "default" } else { "acceptEdits" };
    let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", modo].map(str::to_owned).to_vec();
    args.extend(["--append-system-prompt".to_owned(), modelo.instrucoes.clone()]);
    let liberadas = ajuste.escrita.as_ref().map_or_else(|| modelo.ferramentas.clone(), |caminhos| fluxos::ferramentas_com_escrita_limitada(&modelo.ferramentas, caminhos));
    if !liberadas.is_empty() { args.extend(["--allowedTools".to_owned(), liberadas.join(",")]); }
    let shell = ajuste.escrita.iter().flat_map(|_| fluxos::FERRAMENTAS_DE_SHELL.map(str::to_owned));
    for proibida in ajuste.proibidas.iter().cloned().chain(shell) { args.extend(["--disallowedTools".to_owned(), proibida]); }
    if ajuste.escrita.is_some() { args.extend(["--setting-sources".to_owned(), String::new()]); }
    for pasta in extras { args.extend(["--add-dir".to_owned(), pasta.clone()]); }
    if !modelo.modelo.is_empty() { args.extend(["--model".to_owned(), modelo.modelo.clone()]); }
    if !modelo.esforco.is_empty() { args.extend(["--effort".to_owned(), modelo.esforco.clone()]); }
    if let Some(sid) = retomar { args.extend(["--resume".to_owned(), sid.to_owned()]); }
    args
}

/// O `.claude.json` de onde a CLI leria os MCP desta conta.
fn estado_do_claude(perfil: Option<&omni_core::Profile>) -> Option<PathBuf> {
    match perfil.filter(|p| !p.builtin) {
        Some(p) => Some(PathBuf::from(&p.config_dir).join(".claude.json")),
        None => fluxos::pasta_do_usuario().map(|home| home.join(".claude.json")),
    }
}

/// Devolve à etapa com `escrita` os MCP que o agente declarou. Vai por arquivo: o registro pode ter
/// `env` com segredo, e argumento de linha de comando aparece para qualquer processo da máquina.
fn args_de_mcp(pasta: &Path, no: &str, ferramentas: &[String], estado: &Path) -> Result<Vec<String>> {
    let configuracao: Value = fs::read(estado).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let Some(servidores) = fluxos::mcp_das_ferramentas(ferramentas, &configuracao) else { return Ok(Vec::new()) };
    fs::create_dir_all(pasta)?;
    let arquivo = pasta.join(format!("{no}.mcp.json"));
    gravar_privado(&arquivo, &servidores)?;
    Ok(vec!["--mcp-config".to_owned(), arquivo.to_string_lossy().into_owned()])
}

/// Só o dono lê: o `env` dos servidores pode ter segredo.
fn gravar_privado(arquivo: &Path, valor: &Value) -> Result<()> {
    let _ = fs::remove_file(arquivo);
    let mut opcoes = fs::OpenOptions::new();
    opcoes.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; opcoes.mode(0o600); }
    opcoes.open(arquivo)?.write_all(&serde_json::to_vec_pretty(valor)?)?;
    Ok(())
}

/// O registro de MCP só existe enquanto a etapa roda.
struct ApagaAoSair(PathBuf);
impl Drop for ApagaAoSair { fn drop(&mut self) { let _ = fs::remove_file(&self.0); } }

/// Argumentos do `codex exec`. O Codex não bloqueia ferramenta nem limita caminho: o que ele não
/// garante falha aqui, em vez de rodar sem o limite que o fluxo pediu. Só leitura é o sandbox dele, e
/// pasta extra é raiz gravável do sandbox.
fn args_do_codex(modelo: &omni_core::agentes::ModeloDeAgente, ajuste: &Ajuste, extras: &[String], retomar: Option<&str>) -> Result<Vec<String>> {
    if !ajuste.proibidas.is_empty() { return Err(anyhow!("`proibidas` só vale para agentes do Claude: o Codex não tem como bloquear ferramenta")); }
    if ajuste.escrita.as_ref().is_some_and(|caminhos| !caminhos.is_empty()) { return Err(anyhow!("`escrita` com caminhos só vale para agentes do Claude: no Codex dá só para deixar a lista vazia (só leitura)")); }
    let mut args: Vec<String> = match retomar { Some(sid) => vec!["exec".into(), "resume".into(), sid.to_owned()], None => vec!["exec".into()] };
    let sandbox = if ajuste.escrita.is_some() { "read-only" } else { "workspace-write" };
    args.extend(["--json", "--skip-git-repo-check", "--sandbox", sandbox].map(str::to_owned));
    if !extras.is_empty() { args.extend(["-c".to_owned(), format!("sandbox_workspace_write.writable_roots={}", json!(extras))]); }
    if !modelo.modelo.is_empty() { args.extend(["--model".to_owned(), modelo.modelo.clone()]); }
    if !modelo.esforco.is_empty() { args.extend(["-c".to_owned(), format!("model_reasoning_effort={}", modelo.esforco)]); }
    args.push("-".into());
    Ok(args)
}

/// Conta da etapa: a escolhida (nó, senão modelo de agente), depois a principal da CLI, depois as
/// outras — pulando conta com login caído ou esgotada pelo uso gravado. `so_com_uso`: na troca por
/// limite, só serve conta com uso; sem nenhuma assim, a primeira da ordem (a CLI diz o motivo).
fn escolher_conta<'a>(perfis: &'a [omni_core::Profile], cli: &str, preferida: &str, fora: &[String], so_com_uso: bool,
    esgotada: impl Fn(&omni_core::Profile) -> bool) -> Option<&'a omni_core::Profile> {
    let da_cli = || perfis.iter().filter(move |p| p.provider == cli && !fora.contains(&p.id));
    let ordem = || da_cli().filter(|p| p.id == preferida).chain(da_cli().filter(|p| p.id != preferida && p.builtin)).chain(da_cli().filter(|p| p.id != preferida && !p.builtin));
    ordem().find(|p| p.authenticated && !esgotada(p)).or_else(|| (!so_com_uso).then(|| ordem().next()).flatten())
}

/// Erro da CLI que é limite de uso da conta (mesmos textos que a tela do terminal mostra).
fn eh_limite(erro: &str) -> bool {
    notice_counts(erro).0 > 0
        || { let e = erro.to_ascii_lowercase(); e.contains("usage limit") || e.contains("session limit") || e.contains("weekly limit") }
}

fn texto_de(config: &Value, chave: &str) -> Option<String> {
    config[chave].as_str().map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// Só letras, números e `._-`: modelo e esforço viram argumento de linha de comando.
fn argumento_seguro(v: &str) -> bool { !v.is_empty() && v.len() <= 64 && v.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) }

/// Uma linha curta do que o evento mostra: "Edit · src/x.ts" ou o começo da fala.
fn atividade_do_evento(ev: &Value) -> Option<String> {
    let curto = |t: &str| { let t = t.trim().replace(['\n', '\r'], " "); if t.chars().count() > 140 { format!("{}…", t.chars().take(140).collect::<String>()) } else { t } };
    match ev["type"].as_str()? {
        "assistant" => omni_core::feed::items(ev, "claude").into_iter().last().map(|i| match i.tool { Some(f) => format!("{f} · {}", curto(&i.text)), None => curto(&i.text) }),
        "item.started" | "item.completed" => match ev["item"]["type"].as_str()? {
            "command_execution" => ev["item"]["command"].as_str().map(|c| format!("Shell · {}", curto(c))),
            "agent_message" => ev["item"]["text"].as_str().map(curto),
            "file_change" => Some("Editando arquivos".into()),
            _ => None,
        },
        _ => None,
    }
}

/// Acha o modelo do agente: o do projeto vence o global de mesmo nome.
fn modelo_do_agente(nome: &str, projeto: &str) -> Result<omni_core::agentes::ModeloDeAgente> {
    let lista = omni_core::agentes::listar(omni_core::agentes::pasta_global().as_deref(), Some(Path::new(projeto)));
    lista.iter().find(|a| a.escopo == "projeto" && a.modelo.name == nome).or_else(|| lista.iter().find(|a| a.modelo.name == nome))
        .map(|a| a.modelo.clone()).ok_or_else(|| anyhow!("Modelo de agente “{nome}” não encontrado"))
}

pub(crate) fn executavel(nome: &str) -> Option<PathBuf> { fluxos::executavel(nome) }

pub(crate) fn comando_da_cli(exe: &Path) -> Command {
    if exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) { let mut c = Command::new("cmd"); c.arg("/C").arg(exe); c } else { Command::new(exe) }
}

/// Roda um agente sem tela, guardando cada evento no histórico do nó. Pausar/cancelar mata o processo
/// (e aqui vira `PAUSADO`). Devolve o texto final.
#[allow(clippy::too_many_arguments)]
fn rodar_agente(state: &Arc<EngineState>, v: &Viva, no: &str, agente: &str, prompt: &str, retomar: Option<&str>, pasta: &str, projeto: &str, ajuste: &Ajuste) -> Result<String> {
    let mut modelo = modelo_do_agente(agente, projeto)?;
    if let Some(m) = &ajuste.modelo { modelo.modelo = m.clone(); }
    if let Some(e) = &ajuste.esforco { modelo.esforco = e.clone(); }
    if let Some(c) = &ajuste.conta { modelo.conta = c.clone(); }
    if !modelo.modelo.is_empty() && !argumento_seguro(&modelo.modelo) { return Err(anyhow!("Modelo inválido: {}", modelo.modelo)); }
    if !modelo.esforco.is_empty() && !argumento_seguro(&modelo.esforco) { return Err(anyhow!("Esforço inválido: {}", modelo.esforco)); }
    let dir = state.state_file.parent().ok_or_else(|| anyhow!("engine"))?;
    let perfis = omni_core::profiles(dir);
    let mut fora: Vec<String> = Vec::new();
    loop {
        let perfil = escolher_conta(&perfis, &modelo.cli, &modelo.conta, &fora, !fora.is_empty(), |p| automatico::esgotada(state, p)).cloned();
        // ponytail: a conta nova recomeça a etapa do zero (o worktree guarda o que já foi feito); levar
        // o transcript junto, como a troca do terminal, se recomeçar custar caro.
        let retomar = if fora.is_empty() { retomar } else { None };
        match rodar_na_conta(state, v, no, &modelo, ajuste, prompt, retomar, pasta, perfil.as_ref()) {
            Err(erro) if eh_limite(&erro.to_string()) => {
                let Some(p) = perfil else { return Err(erro) };
                fora.push(p.id.clone());
                let Some(proxima) = escolher_conta(&perfis, &modelo.cli, &modelo.conta, &fora, true, |p| automatico::esgotada(state, p)) else {
                    return Err(anyhow!("Limite de uso em {} e nenhuma outra conta de {} com uso disponível. {erro}", p.name, modelo.cli));
                };
                definir_atividade(state, v, no, &format!("Limite em {}: trocando para {}…", p.name, proxima.name));
            }
            outro => return outro,
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn rodar_na_conta(state: &EngineState, v: &Viva, no: &str, modelo: &omni_core::agentes::ModeloDeAgente, ajuste: &Ajuste, prompt: &str, retomar: Option<&str>, pasta: &str, perfil: Option<&omni_core::Profile>) -> Result<String> {
    let exe = executavel(&modelo.cli).ok_or_else(|| anyhow!("CLI “{}” não encontrada neste PC", modelo.cli))?;
    let mut c = comando_da_cli(&exe);
    let (extras, run_id) = v.lock().map(|e| (e.pastas_extras.clone(), e.id.clone())).map_err(|_| anyhow!("execucao"))?;
    let log = pasta_execucoes(state).join(&run_id);
    let _registro_de_mcp = ApagaAoSair(log.join(format!("{no}.mcp.json")));
    c.args(match modelo.cli.as_str() {
        "claude" => {
            let mut args = args_do_claude(modelo, ajuste, &extras, retomar);
            if let Some(estado) = estado_do_claude(perfil).filter(|_| ajuste.escrita.is_some()) { args.extend(args_de_mcp(&log, no, &modelo.ferramentas, &estado)?); }
            args
        }
        "codex" => args_do_codex(modelo, ajuste, &extras, retomar)?,
        outra => return Err(anyhow!("A CLI “{outra}” ainda não roda sem tela nos fluxos")),
    });
    if let Some(p) = perfil { c.envs(omni_core::env_for(p)); }
    // O omni-rag herda e ignora `sem_mascara`: etapa não tem pessoa para autorizar (`busca::MARCA_DE_ETAPA`).
    c.env("OMNI_ETAPA_DE_FLUXO", "1");
    c.current_dir(pasta).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    sem_janela(&mut c);
    let mut filho = c.spawn().with_context(|| format!("não consegui abrir {}", modelo.cli))?;
    processos().lock().expect("processos").insert(format!("{run_id}/{no}"), filho.id());
    mexer(state, v, |e| if let Some(n) = e.nos.get_mut(no) {
        n.cli = Some(modelo.cli.clone()); n.conta = perfil.map(|p| p.id.clone());
        n.modelo = (!modelo.modelo.is_empty()).then(|| modelo.modelo.clone()); n.esforco = (!modelo.esforco.is_empty()).then(|| modelo.esforco.clone());
    });
    definir_atividade(state, v, no, &format!("Abrindo {}…", modelo.cli));
    {
        let mut stdin = filho.stdin.take().ok_or_else(|| anyhow!("sem stdin"))?;
        let texto = if retomar.is_some() { "Continue de onde parou.".to_owned() }
            else if modelo.cli == "codex" { format!("{}\n\n---\n\n{prompt}", modelo.instrucoes) } else { prompt.to_owned() };
        stdin.write_all(texto.as_bytes())?;
    }

    let _ = fs::create_dir_all(&log);
    let mut arquivo = fs::OpenOptions::new().create(true).append(true).open(log.join(format!("{no}.log")))?;
    let mut final_texto = String::new();
    let mut erro_cli: Option<String> = None;
    let stdout = filho.stdout.take().ok_or_else(|| anyhow!("sem stdout"))?;
    for linha in std::io::BufReader::new(stdout).lines().map_while(std::result::Result::ok) {
        let _ = writeln!(arquivo, "{linha}");
        let Ok(ev) = serde_json::from_str::<Value>(&linha) else { continue };
        let sid = ev["session_id"].as_str().or_else(|| (ev["type"] == "thread.started").then(|| ev["thread_id"].as_str()).flatten());
        if let Some(sid) = sid { let sid = sid.to_owned(); mexer(state, v, |e| if let Some(n) = e.nos.get_mut(no) { if n.session_id.as_deref() != Some(&sid) { n.session_id = Some(sid); } }); }
        if let Some(a) = atividade_do_evento(&ev) { definir_atividade(state, v, no, &a); }
        match ev["type"].as_str() {
            Some("result") => {
                final_texto = ev["result"].as_str().unwrap_or_default().to_owned();
                let custo = ev["total_cost_usd"].as_f64().unwrap_or(0.0);
                mexer(state, v, |e| { e.custo_usd += custo; if let Some(n) = e.nos.get_mut(no) { n.custo_usd += custo; } });
                if ev["is_error"] == true { erro_cli = Some(final_texto.clone()); }
            }
            Some("item.completed") if ev["item"]["type"] == "agent_message" => final_texto = ev["item"]["text"].as_str().unwrap_or_default().to_owned(),
            Some("error") => erro_cli = ev["message"].as_str().map(str::to_owned),
            _ => {}
        }
    }
    let status = filho.wait()?;
    processos().lock().expect("processos").remove(&format!("{run_id}/{no}"));
    if v.lock().map(|e| !matches!(e.status.as_str(), "rodando" | "aguardando")).unwrap_or(true) { return Err(anyhow!(PAUSADO)); }
    if let Some(erro) = erro_cli { return Err(anyhow!(erro)); }
    if !status.success() && final_texto.is_empty() {
        let mut erro = String::new();
        if let Some(mut s) = filho.stderr.take() { let _ = s.read_to_string(&mut erro); }
        return Err(anyhow!("{} saiu com erro: {}", modelo.cli, erro.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("sem detalhe")));
    }
    Ok(final_texto)
}

/// Etapa Comando: o shell do sistema roda `comando` na pasta da execução, com stdout e stderr juntos
/// no log da etapa. Devolve se saiu com 0 e o fim da saída (o erro, quando falha). O comando é o que
/// está salvo no fluxo — nada digitado ao executar entra nele, então não há injeção pela tarefa.
fn rodar_comando(state: &EngineState, v: &Viva, no: &str, comando: &str, pasta: &str) -> Result<(bool, String)> {
    #[cfg(windows)]
    let mut c = { use std::os::windows::process::CommandExt; let mut c = Command::new("cmd"); c.raw_arg(format!("/D /S /C \"{comando}\"")); c };
    #[cfg(not(windows))]
    let mut c = { let mut c = Command::new("sh"); c.arg("-c").arg(comando); c };
    c.current_dir(pasta).env("NO_COLOR", "1").env("FORCE_COLOR", "0").env("CI", "1")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    sem_janela(&mut c);
    let mut filho = c.spawn().with_context(|| format!("não consegui rodar: {comando}"))?;
    let run_id = v.lock().map_err(|_| anyhow!("execucao"))?.id.clone();
    processos().lock().expect("processos").insert(format!("{run_id}/{no}"), filho.id());
    definir_atividade(state, v, no, &format!("$ {comando}"));

    let log = pasta_execucoes(state).join(&run_id);
    let _ = fs::create_dir_all(&log);
    let mut arquivo = fs::OpenOptions::new().create(true).append(true).open(log.join(format!("{no}.log")))?;
    let _ = writeln!(arquivo, "{}", json!({"type": "comando", "text": format!("$ {comando}")}));
    // As duas saídas num canal só, na ordem em que chegam.
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let leitores: Vec<_> = [filho.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), filho.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
        .into_iter().flatten().map(|fonte| {
            let tx = tx.clone();
            std::thread::spawn(move || for linha in std::io::BufReader::new(fonte).lines().map_while(std::result::Result::ok) { if tx.send(linha).is_err() { break; } })
        }).collect();
    drop(tx);
    let mut saida = String::new();
    let mut ultima_tela = std::time::Instant::now();
    for linha in rx {
        let _ = writeln!(arquivo, "{}", json!({"type": "saida", "text": linha}));
        saida.push_str(&linha);
        saida.push('\n');
        if saida.len() > 64_000 { saida = fluxos::cauda(&saida, 32_000).to_owned(); }
        // ponytail: a atividade vai para o JSON da execução em disco — no máximo 2x por segundo.
        if !linha.trim().is_empty() && ultima_tela.elapsed() >= std::time::Duration::from_millis(500) {
            ultima_tela = std::time::Instant::now();
            definir_atividade(state, v, no, &linha.chars().take(200).collect::<String>());
        }
    }
    for l in leitores { let _ = l.join(); }
    let status = filho.wait()?;
    processos().lock().expect("processos").remove(&format!("{run_id}/{no}"));
    if v.lock().map(|e| !matches!(e.status.as_str(), "rodando" | "aguardando")).unwrap_or(true) { return Err(anyhow!(PAUSADO)); }
    let fim = format!("{}\n{}", fluxos::cauda(&saida, 12_000).trim_end(), if status.success() { "✓ terminou sem erro".to_owned() } else { format!("✗ saiu com código {}", status.code().unwrap_or(-1)) });
    let _ = writeln!(arquivo, "{}", json!({"type": "comando", "text": fim.lines().last().unwrap_or_default()}));
    Ok((status.success(), fim))
}

/// Time conversando: cada participante fala na sua vez, rodada após rodada, até todos concordarem
/// numa rodada ou acabar o limite. O primeiro participante escreve a ata.
fn rodar_sala(state: &Arc<EngineState>, v: &Viva, no: &fluxos::No, ctx: &Contexto, pasta: &str, projeto: &str) -> Result<String> {
    let participantes = fluxos::participantes(no);
    let rodadas = no.config["rodadas"].as_u64().unwrap_or(3).clamp(1, 10) as u32;
    let objetivo = no.config["objetivo"].as_str().unwrap_or("{{entrada}}").replace("{{entrada}}", &ctx.entrada).replace("{{anterior}}", &ctx.anterior);
    let mut conversa: Vec<(String, String)> = Vec::new();
    for rodada in 1..=rodadas {
        let mut todos = true;
        for quem in &participantes {
            if let Ok(id) = v.lock().map(|e| e.id.clone()) { eventos::separar_participante(state, &id, &no.id, quem, rodada); }
            let prompt = fluxos::prompt_da_sala(&objetivo, &conversa, quem, rodada, rodadas);
            let fala = rodar_agente(state, v, &no.id, quem, &prompt, None, pasta, projeto, &Ajuste::limites_de(no))?;
            todos &= fluxos::concordou(&fala);
            conversa.push((quem.clone(), fala));
            let parcial = conversa.iter().map(|(q, t)| format!("### {q}\n{t}")).collect::<Vec<_>>().join("\n\n");
            mexer(state, v, |e| if let Some(n) = e.nos.get_mut(&no.id) { n.saida = parcial; n.voltas = rodada; });
        }
        if todos && rodada > 1 { break; }
    }
    let ata = rodar_agente(state, v, &no.id, &participantes[0], &fluxos::prompt_da_ata(&objetivo, &conversa), None, pasta, projeto, &Ajuste::limites_de(no))?;
    Ok(format!("## Ata\n\n{ata}\n\n---\n\n## Conversa\n\n{}", conversa.iter().map(|(q, t)| format!("### {q}\n{t}")).collect::<Vec<_>>().join("\n\n")))
}

fn abrir_pr(v: &Viva, no: &fluxos::No) -> Result<String> {
    let e = v.lock().map_err(|_| anyhow!("execucao"))?.clone();
    let branch = e.branch.clone().ok_or_else(|| anyhow!("Sem branch: o projeto não é git"))?;
    let titulo = no.config["titulo_pr"].as_str().filter(|t| !t.trim().is_empty()).map(str::to_owned)
        .unwrap_or_else(|| e.entrada.lines().next().unwrap_or("Mudanças do OMNI").chars().take(72).collect());
    git(&e.pasta, &["add", "-A"])?;
    if !git(&e.pasta, &["status", "--porcelain"])?.is_empty() {
        git(&e.pasta, &["commit", "-m", &titulo, "-m", &format!("Feito pelo fluxo “{}” do OMNI AGENTS.", e.fluxo.nome)])?;
    }
    git(&e.pasta, &["push", "-u", "origin", &branch])?;
    let resumo = e.fluxo.nos.iter().filter(|n| n.tipo == TipoDeNo::Agente)
        .filter_map(|n| e.nos.get(&n.id).filter(|s| s.status == "concluido").map(|s| format!("- **{}** ({}): {} voltas", fluxos::rotulo(n), n.agente, s.voltas.max(1))))
        .collect::<Vec<_>>().join("\n");
    let corpo = format!("## Tarefa\n\n{}\n\n## Etapas\n\n{resumo}\n\n---\nAberto pelo fluxo “{}” do OMNI AGENTS.", e.entrada, e.fluxo.nome);
    let mut c = Command::new("gh");
    c.args(["pr", "create", "--title", &titulo, "--body", &corpo, "--head", &branch]);
    if let Some(base) = &e.base { c.args(["--base", base]); }
    c.current_dir(&e.pasta).stdin(Stdio::null());
    sem_janela(&mut c);
    let saida = c.output().context("gh (GitHub CLI) não encontrado")?;
    if !saida.status.success() { return Err(anyhow!("gh pr create: {}", String::from_utf8_lossy(&saida.stderr).trim())); }
    Ok(String::from_utf8_lossy(&saida.stdout).lines().rev().find(|l| l.starts_with("http")).unwrap_or("").to_owned())
}

fn matar_processo(id: &str) {
    let prefixo = format!("{id}/");
    let pids: Vec<u32> = processos().lock().map(|p| p.iter().filter(|(k, _)| k.starts_with(&prefixo)).map(|(_, pid)| *pid).collect()).unwrap_or_default();
    for pid in pids {
        let mut c = if cfg!(windows) { let mut c = Command::new("taskkill"); c.args(["/F", "/T", "/PID", &pid.to_string()]); c }
            else { let mut c = Command::new("kill"); c.arg(pid.to_string()); c };
        sem_janela(&mut c);
        let _ = c.stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

pub fn pausar(state: &EngineState, id: &str) -> Result<()> {
    let v = viva(id)?;
    let rodava = mexer(state, &v, |e| { let r = e.status == "rodando"; if r { e.status = "pausado".into(); } r });
    if !rodava { return Err(anyhow!("Só dá para pausar uma execução rodando")); }
    matar_processo(id);
    Ok(())
}

/// Etapa parada que não está na fila: execução gravada por um engine antigo, ou que estava rodando
/// quando o engine caiu. Sem isto ela ficaria "rodando" para sempre e a execução nunca terminaria.
fn devolver_para_a_fila(e: &mut Execucao) {
    let paradas: Vec<String> = e.nos.iter().filter(|(id, n)| matches!(n.status.as_str(), "pausado" | "falhou") && !e.fila.iter().any(|p| p.no == **id)).map(|(id, _)| id.clone()).collect();
    for no in paradas {
        let de = fluxos::antecessores(&e.fluxo, &no).into_iter().find(|a| e.nos.get(*a).is_some_and(|n| n.status == "concluido")).map(str::to_owned);
        e.fila.insert(0, Passo { no, de });
    }
}

pub fn retomar(state: &Arc<EngineState>, id: &str) -> Result<()> {
    let v = viva(id)?;
    mexer(state, &v, |e| {
        if e.status != "pausado" && e.status != "falhou" { return Err(anyhow!("Só dá para retomar uma execução pausada ou que falhou")); }
        devolver_para_a_fila(e);
        for n in e.nos.values_mut().filter(|n| n.status == "pausado" || n.status == "falhou") { n.status = "pendente".into(); n.erro = None; }
        if std::mem::take(&mut e.parou_por_custo) { e.custo_aceito = e.custo_usd; }
        // Um ramo falhou enquanto outro esperava aprovação: a pergunta continua de pé, e a fila anda depois da resposta.
        e.status = if e.nos.values().any(|n| n.status == "aguardando") { "aguardando".into() } else { "rodando".into() };
        e.erro = None;
        Ok(())
    })?;
    soltar(state.clone(), id.to_owned());
    Ok(())
}

pub fn cancelar(state: &EngineState, id: &str) -> Result<()> {
    let v = viva(id)?;
    mexer(state, &v, |e| { e.status = "cancelado".into(); e.fila.clear(); for n in e.nos.values_mut().filter(|n| n.status == "rodando" || n.status == "aguardando") { n.status = "pausado".into(); }
        eventos::registrar(state, e, None, "aviso", "Execução cancelada");
    });
    matar_processo(id);
    Ok(())
}

pub fn aprovar(state: &Arc<EngineState>, id: &str, no: &str, sim: bool) -> Result<()> {
    let v = viva(id)?;
    let ok = mexer(state, &v, |e| {
        // O nó precisa esperar e a execução estar viva: com duas aprovações abertas ela já voltou a `rodando` quando a segunda
        // chega, mas `falhou`, `pausado` e `cancelado` só andam por `retomar`.
        let viva = matches!(e.status.as_str(), "rodando" | "aguardando");
        let aguardando = viva && e.nos.get(no).is_some_and(|n| n.status == "aguardando");
        if aguardando {
            if let Some(n) = e.nos.get_mut(no) { n.status = "concluido".into(); n.fim_ms = now_ms(); }
            if !e.nos.values().any(|n| n.status == "aguardando") { e.status = "rodando".into(); }
            eventos::registrar(state, e, Some(no), "aprovacao", if sim { "Aprovado" } else { "Recusado" });
        }
        aguardando
    });
    if !ok { return Err(anyhow!("Este nó não está esperando aprovação")); }
    seguir(state, &v, no, if sim { "sim" } else { "nao" });
    soltar(state.clone(), id.to_owned());
    Ok(())
}

/// O histórico de um nó, lido dos eventos que a CLI mandou: fala, ações e resultado — o mesmo
/// leitor das conversas.
pub fn historico(state: &EngineState, id: &str, no: &str) -> Result<Vec<Value>> {
    if !id_valido(id) || !id_valido(no) { return Err(anyhow!("Id inválido")); }
    let caminho = pasta_execucoes(state).join(id).join(format!("{no}.log"));
    let Ok(texto) = fs::read_to_string(caminho) else { return Ok(Vec::new()) };
    let mut itens = Vec::new();
    for linha in texto.lines() {
        let Ok(ev) = serde_json::from_str::<Value>(linha) else { continue };
        match ev["type"].as_str() {
            Some("assistant") | Some("user") => itens.extend(omni_core::feed::items(&ev, "claude").into_iter().map(|i| json!(i))),
            Some("volta") => itens.push(json!({"kind": "notice", "role": "assistant", "text": format!("Volta {}", ev["n"]), "separador": true})),
            Some("participante") => itens.push(json!({"kind": "notice", "role": "assistant", "text": format!("{} · rodada {}", ev["quem"].as_str().unwrap_or_default(), ev["rodada"]), "separador": true})),
            Some("comando") => itens.push(json!({"kind": "notice", "role": "assistant", "text": ev["text"]})),
            Some("saida") => itens.push(json!({"kind": "tool", "role": "assistant", "tool": "›", "text": ev["text"]})),
            Some("result") => itens.push(json!({"kind": "notice", "role": "assistant", "text": format!("Terminou em {} turnos · US$ {:.2}", ev["num_turns"], ev["total_cost_usd"].as_f64().unwrap_or(0.0))})),
            Some("item.completed") => {
                let item = &ev["item"];
                match item["type"].as_str() {
                    Some("agent_message") => itens.push(json!({"kind": "text", "role": "assistant", "text": item["text"]})),
                    Some("command_execution") => itens.push(json!({"kind": "tool", "role": "assistant", "tool": "Shell", "text": item["command"]})),
                    Some("file_change") => itens.push(json!({"kind": "tool", "role": "assistant", "tool": "Edit", "text": item["changes"].to_string().chars().take(160).collect::<String>()})),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    Ok(itens)
}

/// Abre o nó numa conversa de verdade (terminal com a CLI retomando a sessão dele), no worktree da
/// execução e na mesma conta — para pedir ajuste ou que ele desfaça o que fez.
pub fn abrir_conversa(state: &Arc<EngineState>, id: &str, no: &str) -> Result<TerminalSession> {
    let e = obter(id).ok_or_else(|| anyhow!("Execução não encontrada"))?;
    let n = e.nos.get(no).ok_or_else(|| anyhow!("Nó não encontrado"))?;
    let sid = n.session_id.clone().ok_or_else(|| anyhow!("Este nó ainda não tem sessão para abrir"))?;
    let cli = n.cli.clone().unwrap_or_else(|| "claude".into());
    let dir = state.state_file.parent().ok_or_else(|| anyhow!("engine"))?;
    let perfil = n.conta.as_ref().and_then(|c| omni_core::profiles(dir).into_iter().find(|p| &p.id == c));
    let base = mobile::comando_do_agente(state, &cli);
    let comando = if cli == "codex" { format!("{base} resume {sid}") } else { format!("{base} --resume {sid}") };
    let titulo = e.fluxo.nos.iter().find(|x| x.id == no).map(fluxos::rotulo).unwrap_or_else(|| no.to_owned());
    spawn_terminal_inner(state, state.next_id(), e.projeto_id.clone(), format!("{titulo} · {}", e.fluxo.nome), e.pasta.clone(), None,
        Some(comando), 30, 120, now_ms(), SessionOrigin {
            env: perfil.as_ref().map(omni_core::env_for).unwrap_or_default(), provider: Some(cli),
            profile_id: perfil.map(|p| p.id), conversation_id: None, external_session_id: Some(sid) })
}

#[path = "fluxos_eventos.rs"]
pub mod eventos;

#[cfg(test)]
#[path = "fluxos_testes.rs"]
mod limites;

#[cfg(test)]
mod tests {
    use super::*;

    fn fluxo_simples() -> Fluxo {
        serde_json::from_value(json!({"id": "t", "nome": "T", "nos": [
            {"id": "e", "tipo": "entrada"}, {"id": "a", "tipo": "aprovacao"}, {"id": "f", "tipo": "fim"}],
            "conexoes": [{"de": "e", "para": "a"}, {"de": "a", "para": "f", "saida": "sim"}]})).unwrap()
    }

    /// Conta da etapa: a escolhida se tem uso; esgotada ou sem login → a próxima; na troca por limite
    /// só serve conta com uso (sem nenhuma, a etapa falha em vez de girar).
    #[test]
    fn conta_da_etapa_pula_esgotada_e_troca_no_limite() {
        let p = |id: &str, builtin: bool, authenticated: bool| omni_core::Profile { id: id.into(), provider: "claude".into(), name: id.into(),
            config_dir: String::new(), builtin, created_at_ms: 0, last_used_at_ms: None, authenticated, ..serde_json::from_value(json!({
                "id": "", "provider": "", "name": "", "config_dir": "", "created_at_ms": 0})).unwrap() };
        let perfis = [p("principal", true, true), p("b", false, true), p("sem-login", false, false)];
        let nenhuma = |_: &omni_core::Profile| false;
        assert_eq!(escolher_conta(&perfis, "claude", "b", &[], false, nenhuma).unwrap().id, "b");
        assert_eq!(escolher_conta(&perfis, "claude", "", &[], false, nenhuma).unwrap().id, "principal");
        assert_eq!(escolher_conta(&perfis, "claude", "b", &[], false, |c| c.id == "b").unwrap().id, "principal");
        assert_eq!(escolher_conta(&perfis, "claude", "b", &["b".into()], true, nenhuma).unwrap().id, "principal");
        assert!(escolher_conta(&perfis, "claude", "b", &["b".into(), "principal".into()], true, nenhuma).is_none());
        assert!(eh_limite("Claude AI usage limit reached|1760000000") && eh_limite("You've hit your session limit · resets 8pm"));
        assert!(!eh_limite("claude saiu com erro: ENOENT"));
    }

    /// Comando de verdade no shell do sistema: passou → segue por `sim` com a saída gravada; falhou
    /// sem caminho ligado → a execução falha (não "conclui" com o build quebrado); falhou com `nao`
    /// ligado → segue por ele e conta a volta.
    #[test]
    fn comando_segue_pelo_codigo_de_saida() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(EngineState { persist_lock: Mutex::new(()), mobile: mobile::MobileRuntime::new(dir.path()),
            token: "t".into(), state_file: dir.path().join("sessions.json"), sessions: Mutex::new(HashMap::new()), id_sequence: AtomicU64::new(1) });
        let projeto = dir.path().join("proj");
        fs::create_dir_all(&projeto).unwrap();
        let fluxo = |comando: &str, com_nao: bool| -> Fluxo {
            let mut conexoes = vec![json!({"de": "e", "para": "c"}), json!({"de": "c", "para": "f", "saida": "sim"})];
            if com_nao { conexoes.push(json!({"de": "c", "para": "f", "saida": "nao"})); }
            serde_json::from_value(json!({"id": "c", "nome": "C", "na_pasta_do_projeto": true, "nos": [
                {"id": "e", "tipo": "entrada"}, {"id": "c", "tipo": "comando", "config": {"comando": comando}}, {"id": "f", "tipo": "fim"}],
                "conexoes": conexoes})).unwrap()
        };
        let ate_parar = |id: &str| { for _ in 0..250 { let e = obter(id).unwrap(); if e.status != "rodando" { return e; } std::thread::sleep(std::time::Duration::from_millis(20)); } panic!("não parou"); };
        let rodar = |f: Fluxo| ate_parar(&iniciar(&state, f, projeto.to_string_lossy().into(), "p".into(), "x".into()).unwrap().id);

        let ok = rodar(fluxo("echo ola-do-teste", false));
        assert_eq!((ok.status.as_str(), ok.nos["c"].caminho.as_deref(), ok.pasta.as_str()), ("concluido", Some("sim"), &*projeto.to_string_lossy()));
        assert!(ok.nos["c"].saida.contains("ola-do-teste"), "{}", ok.nos["c"].saida);
        assert!(historico(&state, &ok.id, "c").unwrap().iter().any(|i| i["text"] == "ola-do-teste"));

        let quebrou = rodar(fluxo("exit 3", false));
        assert_eq!(quebrou.status, "falhou");
        assert!(quebrou.erro.unwrap_or_default().contains("falhou"));

        let tratado = rodar(fluxo("exit 3", true));
        assert_eq!((tratado.status.as_str(), tratado.nos["c"].caminho.as_deref(), tratado.nos["c"].voltas), ("concluido", Some("nao"), 1));
    }

    /// Sem CLI nenhuma: entrada → aprovação para e espera; aprovar segue até o fim. Estado gravado
    /// em disco a cada passo e lido de volta como pausado/aguardando.
    #[test]
    fn executa_para_na_aprovacao_e_segue_ao_aprovar() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(EngineState { persist_lock: Mutex::new(()), mobile: mobile::MobileRuntime::new(dir.path()),
            token: "t".into(), state_file: dir.path().join("sessions.json"), sessions: Mutex::new(HashMap::new()), id_sequence: AtomicU64::new(1) });
        let projeto = dir.path().join("proj");
        fs::create_dir_all(&projeto).unwrap();
        let e = iniciar(&state, fluxo_simples(), projeto.to_string_lossy().into(), "p".into(), "fazer x".into()).unwrap();
        let espera = |status: &str| { for _ in 0..100 { if obter(&e.id).unwrap().status == status { return; } std::thread::sleep(std::time::Duration::from_millis(20)); } panic!("não chegou em {status}: {:?}", obter(&e.id).map(|x| x.status)); };
        espera("aguardando");
        assert_eq!(obter(&e.id).unwrap().nos["e"].saida, "fazer x");
        assert!(aprovar(&state, &e.id, "e", true).is_err(), "só o nó que espera pode ser aprovado");
        aprovar(&state, &e.id, "a", true).unwrap();
        espera("concluido");
        let gravada: Execucao = serde_json::from_slice(&fs::read(pasta_execucoes(&state).join(format!("{}.json", e.id))).unwrap()).unwrap();
        assert_eq!((gravada.status.as_str(), gravada.nos["f"].status.as_str(), gravada.nos["a"].caminho.as_deref()), ("concluido", "concluido", Some("sim")));
    }
    /// Entrada com duas saídas ligadas: os dois comandos rodam ao mesmo tempo (um começa antes de o
    /// outro acabar) e o Juntar só roda depois dos dois, com as duas saídas.
    #[test]
    fn ramos_paralelos_rodam_juntos_e_o_juntar_espera_os_dois() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(EngineState { persist_lock: Mutex::new(()), mobile: mobile::MobileRuntime::new(dir.path()),
            token: "t".into(), state_file: dir.path().join("sessions.json"), sessions: Mutex::new(HashMap::new()), id_sequence: AtomicU64::new(1) });
        let projeto = dir.path().join("proj");
        fs::create_dir_all(&projeto).unwrap();
        // ~1 s no laço do próprio cmd (sem depender de ping/timeout).
        let espera = |marca: &str| if cfg!(windows) { format!("(for /L %i in (1,1,20000) do @set x=%i) && echo {marca}") } else { format!("sleep 1 && echo {marca}") };
        let fluxo: Fluxo = serde_json::from_value(json!({"id": "p", "nome": "P", "na_pasta_do_projeto": true, "nos": [
            {"id": "e", "tipo": "entrada"},
            {"id": "a", "tipo": "comando", "titulo": "Ramo A", "config": {"comando": espera("saida-a")}},
            {"id": "b", "tipo": "comando", "titulo": "Ramo B", "config": {"comando": espera("saida-b")}},
            {"id": "j", "tipo": "juntar"}, {"id": "f", "tipo": "fim"}],
            "conexoes": [{"de": "e", "para": "a"}, {"de": "e", "para": "b"},
                {"de": "a", "para": "j", "saida": "sim"}, {"de": "b", "para": "j", "saida": "sim"}, {"de": "j", "para": "f"}]})).unwrap();
        let id = iniciar(&state, fluxo, projeto.to_string_lossy().into(), "p".into(), "x".into()).unwrap().id;
        let e = (0..400).find_map(|_| { let e = obter(&id).unwrap(); if e.status != "rodando" { Some(e) } else { std::thread::sleep(std::time::Duration::from_millis(20)); None } }).expect("não parou");
        assert_eq!(e.status, "concluido", "{:?}", e.erro);
        let (a, b) = (&e.nos["a"], &e.nos["b"]);
        assert!(a.inicio_ms < b.fim_ms && b.inicio_ms < a.fim_ms, "rodou em sequência: a {}–{}, b {}–{}", a.inicio_ms, a.fim_ms, b.inicio_ms, b.fim_ms);
        let junto = &e.nos["j"].saida;
        assert!(junto.contains("## Ramo A") && junto.contains("saida-a") && junto.contains("## Ramo B") && junto.contains("saida-b"), "{junto}");
        assert_eq!(e.nos["f"].saida, *junto, "o Juntar rodou uma vez só e levou as duas saídas adiante");
    }
}

