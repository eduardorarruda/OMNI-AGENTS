//! Fila do OMNI: mensagem que não pode entrar agora espera aqui, em disco, e entra sozinha quando o
//! agente fica livre, uma por vez. Filho de `mobile`: a entrega é uma `ActionKind::Prompt` comum, na
//! mesma fila de ações, para nunca colar ao mesmo tempo que outra ação.
//!
//! Ordem dos locks: `actions` antes de `filas`.

use super::*;
use axum::routing::delete;
use omni_core::fila::{anexos_do_texto, modo_de_fila, nome_do_arquivo, AguardoDoTurno, FilaDaConversa, ItemDaFila, ModoDeFila, Tirada, ITENS_MAX};

/// Quanto o engine demora, no pior caso, para notar que o agente ficou livre.
const RITMO_DA_ENTREGA: std::time::Duration = std::time::Duration::from_secs(1);
/// Chave da ação que entrega um item: é o que separa a entrega de uma mensagem direta.
const PREFIXO_DA_ENTREGA: &str = "fila:";
const SEM_SESSAO: &str = "A sessão foi encerrada. As mensagens continuam guardadas e entram quando a conversa voltar.";
const AGENTE_FORA: &str = "O agente não está rodando. As mensagens continuam guardadas até ele voltar.";

#[derive(Default)]
pub(super) struct Filas {
    por_conversa: HashMap<String, FilaDaConversa>,
    /// Em memória de propósito: depois de religar não há turno a esperar.
    aguardos: HashMap<String, AguardoDoTurno>,
}

/// A tela não aceita colar agora. Não é falha da mensagem: ela volta para a fila.
#[derive(Debug)]
pub(super) struct TelaNaoLivre(pub(super) &'static str);
impl std::fmt::Display for TelaNaoLivre {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str(self.0) }
}
impl std::error::Error for TelaNaoLivre {}

fn pasta(dir: &Path) -> PathBuf { dir.join("filas") }

/// Lê as filas gravadas. Entrega que estava no meio quando o engine caiu volta para a frente: repetir
/// uma mensagem é menos grave que perder.
pub(super) fn carregar(dir: &Path) -> Filas {
    let mut filas = Filas::default();
    let Ok(arquivos) = std::fs::read_dir(pasta(dir)) else { return filas };
    for arquivo in arquivos.flatten().map(|entrada| entrada.path()).filter(|p| p.extension().is_some_and(|e| e == "json")) {
        let lida = std::fs::read(&arquivo).map_err(|e| e.to_string())
            .and_then(|bytes| serde_json::from_slice::<FilaDaConversa>(&bytes).map_err(|e| e.to_string()));
        match lida {
            Ok(mut fila) => { fila.devolver(); filas.por_conversa.insert(fila.conversa.clone(),fila); }
            Err(motivo) => eprintln!("fila ilegível em {}: {motivo}",arquivo.display()),
        }
    }
    filas
}

fn gravar(state: &EngineState, fila: &FilaDaConversa) -> std::io::Result<()> {
    let dir = state.state_file.parent().expect("engine sem pasta");
    atomic_write_json(&pasta(dir).join(nome_do_arquivo(&fila.conversa)),fila)
}

/// Toda mudança passa aqui: muda uma cópia, grava, e só então vale. Disco cheio não deixa a tela
/// dizendo uma coisa e o arquivo outra.
fn mexer<R>(state: &EngineState, conversa: &str, muda: impl FnOnce(&mut FilaDaConversa) -> R) -> std::io::Result<R> {
    let mut filas = state.mobile.filas.lock().expect("filas poisoned");
    let atual = filas.por_conversa.get(conversa).cloned().unwrap_or_else(|| FilaDaConversa::nova(conversa));
    let mut nova = atual.clone();
    let resposta = muda(&mut nova);
    if nova != atual {
        gravar(state,&nova)?;
        filas.por_conversa.insert(conversa.to_owned(),nova);
    }
    Ok(resposta)
}

fn turnos_de(sessao: &LiveSession) -> u64 { sessao.interaction.lock().expect("screen poisoned").turnos_iniciados }

fn prompt_em_andamento(acoes: &VecDeque<Action>, conversa: &str) -> bool {
    acoes.iter().any(|a| a.conversation_id == conversa && matches!(a.kind,ActionKind::Prompt(_)) && (a.state == "queued" || a.state == "executing"))
}

pub(super) fn usa_fila_do_omni(sessao: &LiveSession) -> bool {
    let provider = sessao.meta.lock().expect("metadata poisoned").provider.clone().unwrap_or_default();
    modo_de_fila(&provider) == ModoDeFila::Omni
}

/// Mensagem nova não fura a fila nem cola em cima da anterior, que ainda pode não ter aberto o turno.
pub(super) fn deve_guardar(state: &EngineState, conversa: &str, sessao: &LiveSession, capacidade: &interaction::Capabilities, acoes: &VecDeque<Action>) -> bool {
    if !usa_fila_do_omni(sessao) { return false; }
    if !capacidade.prompt || prompt_em_andamento(acoes,conversa) { return true; }
    let turnos = turnos_de(sessao);
    let filas = state.mobile.filas.lock().expect("filas poisoned");
    filas.por_conversa.get(conversa).is_some_and(|fila| !fila.vazia())
        || filas.aguardos.get(conversa).is_some_and(|aguardo| !aguardo.liberado(now_ms(),turnos))
}

fn item_de(id: &str, texto: &str, em_ms: u64) -> ItemDaFila {
    ItemDaFila { id: id.to_owned(), texto: texto.to_owned(), anexos: anexos_do_texto(texto), em_ms }
}

/// A resposta do envio leva o id do item: é por ele que a tela se acha na fila.
pub(super) fn guardar_pedido(state: &Arc<EngineState>, conversa: &str, chave: &str, sessao: &LiveSession, revisao: String, texto: &str, acoes: &mut VecDeque<Action>) -> Result<(StatusCode,Json<Value>),ApiError> {
    let id = state.next_id();
    let item = item_de(&id,texto,now_ms());
    let coube = mexer(state,conversa,|fila| {
        if fila.itens.len() >= ITENS_MAX { return false; }
        fila.guardar(item);
        true
    }).map_err(|e| { eprintln!("fila de {conversa} não gravada: {e}"); error(StatusCode::INTERNAL_SERVER_ERROR,"Não consegui guardar a mensagem na fila") })?;
    if !coube { return Err(error(StatusCode::TOO_MANY_REQUESTS,"Fila cheia; tire alguma mensagem ou aguarde")); }
    let session_id = sessao.meta.lock().expect("metadata poisoned").id.clone();
    let action = Action { id,conversation_id:conversa.to_owned(),session_id,state:"guardada".into(),created_at_ms:now_ms(),error:None,
        key:chave.to_owned(),revision:revisao,kind:ActionKind::Prompt(texto.to_owned()) };
    let resposta = json!(action);
    acoes.push_back(action);
    Ok((StatusCode::ACCEPTED,Json(resposta)))
}

/// A vez da fila: só com o agente livre, sem outra mensagem a caminho e depois do turno da anterior.
fn entregar(state: &Arc<EngineState>, conversa: &str) -> std::io::Result<()> {
    let Ok((_,Some(sessao))) = current(state,conversa) else { return Ok(()) };
    let capacidade = interaction::capabilities(&sessao);
    if !capacidade.prompt { return Ok(()); }
    let turnos = turnos_de(&sessao);
    let mut acoes = state.mobile.actions.lock().expect("actions poisoned");
    if prompt_em_andamento(&acoes,conversa) { return Ok(()); }
    let agora = now_ms();
    if state.mobile.filas.lock().expect("filas poisoned").aguardos.get(conversa).is_some_and(|a| !a.liberado(agora,turnos)) { return Ok(()); }
    let Some(item) = mexer(state,conversa,FilaDaConversa::proxima)? else { return Ok(()) };
    state.mobile.filas.lock().expect("filas poisoned").aguardos.insert(conversa.to_owned(),AguardoDoTurno { desde_ms: agora, turnos });
    let session_id = sessao.meta.lock().expect("metadata poisoned").id.clone();
    acoes.push_back(Action { id:state.next_id(),conversation_id:conversa.to_owned(),session_id,state:"queued".into(),created_at_ms:agora,error:None,
        key:format!("{PREFIXO_DA_ENTREGA}{}",item.id),revision:capacidade.revision,kind:ActionKind::Prompt(item.texto) });
    drop(acoes);
    state.mobile.pending.notify_one();
    Ok(())
}

pub(super) fn entregar_proximas(state: &Arc<EngineState>) {
    let pendentes: Vec<String> = state.mobile.filas.lock().expect("filas poisoned").por_conversa.values()
        .filter(|fila| fila.pode_entregar()).map(|fila| fila.conversa.clone()).collect();
    for conversa in pendentes {
        if let Err(motivo) = entregar(state,&conversa) { eprintln!("fila de {conversa} não entregue: {motivo}"); }
    }
}

pub(super) fn iniciar(state: Arc<EngineState>) {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(RITMO_DA_ENTREGA).await;
            let ocioso = !state.mobile.filas.lock().expect("filas poisoned").por_conversa.values().any(FilaDaConversa::pode_entregar);
            if ocioso { continue; }
            let engine = state.clone();
            let _ = tokio::task::spawn_blocking(move || entregar_proximas(&engine)).await;
        }
    });
}

/// O fim de uma mensagem que passou pela fila de ações: `(estado, erro)`.
fn desfecho(state: &Arc<EngineState>, action: &Action, resultado: Result<()>) -> (&'static str, Option<String>) {
    let ActionKind::Prompt(texto) = &action.kind else {
        return match resultado { Ok(()) => ("sent",None), Err(e) => ("rejected",Some(e.to_string())) };
    };
    let conversa = &action.conversation_id;
    let entrega = action.key.starts_with(PREFIXO_DA_ENTREGA);
    match resultado {
        Ok(()) if entrega => {
            if let Err(motivo) = mexer(state,conversa,FilaDaConversa::entregue) { eprintln!("fila de {conversa} não gravada: {motivo}"); }
            ("sent",None)
        }
        Ok(()) => {
            // Mensagem direta também abre turno: a seguinte não pode colar em cima dela.
            let turnos = current(state,conversa).ok().and_then(|(_,sessao)| sessao).map_or(0,|sessao| turnos_de(&sessao));
            state.mobile.filas.lock().expect("filas poisoned").aguardos.insert(conversa.clone(),AguardoDoTurno { desde_ms: now_ms(), turnos });
            ("sent",None)
        }
        Err(erro) if entrega => {
            state.mobile.filas.lock().expect("filas poisoned").aguardos.remove(conversa);
            if let Err(motivo) = mexer(state,conversa,FilaDaConversa::devolver) { eprintln!("fila de {conversa} não gravada: {motivo}"); }
            ("guardada",Some(erro.to_string()).filter(|_| !erro.is::<TelaNaoLivre>()))
        }
        // O agente começou a trabalhar entre a chegada e a colagem.
        Err(erro) if erro.is::<TelaNaoLivre>() => match mexer(state,conversa,|fila| fila.guardar_na_frente(item_de(&action.id,texto,action.created_at_ms))) {
            Ok(()) => ("guardada",None),
            Err(motivo) => { eprintln!("fila de {conversa} não gravada: {motivo}"); ("rejected",Some(erro.to_string())) }
        },
        Err(erro) => ("rejected",Some(erro.to_string())),
    }
}

pub(super) fn concluir_acao(state: &Arc<EngineState>, action: &Action, resultado: Result<()>) {
    let (estado,error) = desfecho(state,action,resultado);
    if let Some(record) = state.mobile.actions.lock().expect("actions poisoned").iter_mut().find(|a| a.id == action.id) {
        record.state = estado.into(); record.error = error;
    }
}

/// Depois de Parar nada entra sozinho. Falha ao gravar não desfaz o Parar: o agente já parou.
pub(super) fn pausar(state: &EngineState, conversa: &str) {
    if let Err(motivo) = mexer(state,conversa,FilaDaConversa::pausar) { eprintln!("fila de {conversa} não pausada: {motivo}"); }
}

/// O que a tela mostra: `GET /fila` e a timeline devolvem isto.
pub(super) fn para_a_tela(state: &EngineState, conversa: &str, sessao: Option<&Arc<LiveSession>>) -> Value {
    let fila = state.mobile.filas.lock().expect("filas poisoned").por_conversa.get(conversa).cloned().unwrap_or_default();
    let aviso = (!fila.vazia()).then(|| match sessao {
        None => Some(SEM_SESSAO),
        Some(viva) => interaction::capabilities(viva).iniciando.then_some(AGENTE_FORA),
    }).flatten();
    json!({"itens":fila.itens,"entregando":fila.entregando,"pausada":fila.pausada,"aviso":aviso})
}

pub(super) fn rotas() -> Router<Arc<EngineState>> {
    Router::new().route("/conversas/{id}/fila",get(ler)).route("/conversas/{id}/fila/{item}",delete(tirar))
}

async fn ler(State(state): State<Arc<EngineState>>, RoutePath(id): RoutePath<String>) -> Result<Json<Value>,ApiError> {
    tokio::task::spawn_blocking(move || {
        let (_,sessao) = current(&state,&id)?;
        Ok(Json(para_a_tela(&state,&id,sessao.as_ref())))
    }).await.map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR,"Falha ao ler a fila"))?
}

/// Item que não existe mais responde como tirado: o segundo clique no X não é erro.
async fn tirar(State(state): State<Arc<EngineState>>, RoutePath((id,item)): RoutePath<(String,String)>) -> Result<Json<Value>,ApiError> {
    tokio::task::spawn_blocking(move || {
        let (_,sessao) = current(&state,&id)?;
        let tirada = mexer(&state,&id,|fila| fila.tirar(&item))
            .map_err(|e| { eprintln!("fila de {id} não gravada: {e}"); error(StatusCode::INTERNAL_SERVER_ERROR,"Não consegui tirar a mensagem da fila") })?;
        if tirada == Tirada::JaEntregue { return Err(error(StatusCode::CONFLICT,"Essa mensagem já foi entregue ao agente")); }
        Ok(Json(para_a_tela(&state,&id,sessao.as_ref())))
    }).await.map_err(|_| error(StatusCode::INTERNAL_SERVER_ERROR,"Falha ao tirar da fila"))?
}

#[cfg(all(test, unix))]
#[path = "fila_testes.rs"]
mod testes;
