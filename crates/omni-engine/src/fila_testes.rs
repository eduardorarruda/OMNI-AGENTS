//! Telas de mentira e pasta temporária: nenhuma CLI real. O `sleep` com nome de `claude` só faz o
//! `ps` achar a CLI viva.
use super::*;
use super::super::tests::{fixture, req};
use super::super::tests::actions::{CliDeMentira, fake_session, redesenhar};
use axum::body::to_bytes;
use tower::ServiceExt as _;

const PARADO: &str = "Claude Code\r\n❯ ";
const TRABALHANDO: &str = "\x1b[1;1H✻ Pensando… (esc to interrupt)\x1b[2;3H";
const LIVRE_DE_NOVO: &str = "\x1b[1;1H\x1b[2KClaude Code\x1b[2;3H";
const APROVACAO: &str = "\x1b[2J\x1b[1;1HClaude Code\r\nDo you want to proceed?\r\n❯ 1. Yes\r\n2. Yes, always\r\n3. No";
const ORIGEM: &str = "http://127.0.0.1:47322";

type Bytes = Arc<Mutex<Vec<u8>>>;

fn colado(bytes: &Bytes) -> String { String::from_utf8_lossy(&bytes.lock().unwrap()).into_owned() }
fn pedido(metodo: &str, uri: &str, corpo: &str, chave: &str) -> Request {
    let mut pedido = req(metodo,uri,ORIGEM,corpo);
    pedido.headers_mut().insert("idempotency-key",chave.parse().unwrap());
    pedido
}
async fn chamar(state: &Arc<EngineState>, pedido: Request) -> (StatusCode, Value) {
    let resposta = router(state.clone()).oneshot(pedido).await.unwrap();
    let status = resposta.status();
    let bytes = to_bytes(resposta.into_body(),1 << 20).await.unwrap();
    (status,serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}
async fn mandar(state: &Arc<EngineState>, texto: &str) -> (StatusCode, Value) {
    let corpo = json!({"texto":texto}).to_string();
    chamar(state,pedido("POST","/conversas/c/prompt",&corpo,&format!("chave-{}",texto.replace(' ',"-")))).await
}
async fn parar_agente(state: &Arc<EngineState>) -> StatusCode {
    chamar(state,req("POST","/conversas/c/parar",ORIGEM,"")).await.0
}
async fn fila_de(state: &Arc<EngineState>) -> Value {
    let (status,corpo) = chamar(state,req("GET","/conversas/c/fila",ORIGEM,"")).await;
    assert_eq!(status,StatusCode::OK);
    corpo
}
fn textos(fila: &Value) -> Vec<String> {
    fila["itens"].as_array().map(|itens| itens.iter().map(|i| i["texto"].as_str().unwrap().to_owned()).collect()).unwrap_or_default()
}
fn id_de(fila: &Value, posicao: usize) -> String { fila["itens"][posicao]["id"].as_str().unwrap().to_owned() }
async fn tirar_item(state: &Arc<EngineState>, item: &str) -> (StatusCode, Value) {
    chamar(state,pedido("DELETE",&format!("/conversas/c/fila/{item}"),"","d")).await
}
/// O que os laços de `mobile::start` fazem, sem o laço: a vez da fila e as ações que ela gerou.
fn um_ciclo(state: &Arc<EngineState>) {
    entregar_proximas(state);
    loop {
        let proxima = { let mut acoes = state.mobile.actions.lock().unwrap();
            acoes.iter_mut().find(|a| a.state == "queued").map(|a| { a.state = "executing".into(); a.clone() }) };
        let Some(acao) = proxima else { break };
        let resultado = execute(state,&acao);
        concluir_acao(state,&acao,resultado);
    }
}
/// O turno que a mensagem entregue abriu, do começo ao fim, como o leitor da PTY veria.
fn turno_inteiro(session: &Arc<LiveSession>) {
    redesenhar(session,TRABALHANDO);
    session.interaction.lock().unwrap().turnos_iniciados += 1;
    redesenhar(session,LIVRE_DE_NOVO);
}
fn com_agente_trabalhando(state: &Arc<EngineState>) -> (Arc<LiveSession>, Bytes, CliDeMentira) {
    let (session,bytes,_) = fake_session(state,PARADO);
    let cli = CliDeMentira::no_lugar_de(&session);
    redesenhar(&session,TRABALHANDO);
    (session,bytes,cli)
}

#[tokio::test] async fn mensagem_com_agente_trabalhando_fica_no_omni() {
    let (_dir,state) = fixture(); let (_session,bytes,_cli) = com_agente_trabalhando(&state);
    let (status,acao) = mandar(&state,"depois do turno").await;
    assert_eq!(status,StatusCode::ACCEPTED);
    assert_eq!(acao["state"],"guardada");
    um_ciclo(&state);
    assert!(bytes.lock().unwrap().is_empty(),"nada vai para a CLI enquanto o agente trabalha");
    let fila = fila_de(&state).await;
    assert_eq!(textos(&fila),["depois do turno"]);
    assert_eq!(fila["itens"][0]["id"],acao["id"],"a tela reconhece o item pela resposta do envio");
}

#[tokio::test] async fn tela_nao_reconhecida_guarda_na_fila() {
    let (_dir,state) = fixture(); let (session,bytes,_) = fake_session(&state,PARADO);
    let _cli = CliDeMentira::no_lugar_de(&session);
    redesenhar(&session,"\x1b[2;3Hmeu rascunho");
    let (status,_) = mandar(&state,"guardada").await;
    assert_eq!(status,StatusCode::ACCEPTED);
    um_ciclo(&state);
    assert!(bytes.lock().unwrap().is_empty(),"não cola em cima do rascunho");
    assert_eq!(textos(&fila_de(&state).await),["guardada"]);
}

#[tokio::test] async fn agente_parado_recebe_direto_sem_passar_pela_fila() {
    let (_dir,state) = fixture(); let (session,bytes,_) = fake_session(&state,PARADO);
    let _cli = CliDeMentira::no_lugar_de(&session);
    let (status,acao) = mandar(&state,"agora").await;
    assert_eq!(status,StatusCode::ACCEPTED);
    assert_eq!(acao["state"],"queued");
    um_ciclo(&state);
    assert!(colado(&bytes).contains("agora"));
    assert!(textos(&fila_de(&state).await).is_empty());
}

#[tokio::test] async fn mesma_chave_nao_guarda_duas_vezes() {
    let (_dir,state) = fixture(); let (_session,_,_cli) = com_agente_trabalhando(&state);
    let (_,primeira) = mandar(&state,"uma vez").await;
    let (status,segunda) = mandar(&state,"uma vez").await;
    assert_eq!(status,StatusCode::ACCEPTED);
    assert_eq!(primeira["id"],segunda["id"]);
    assert_eq!(textos(&fila_de(&state).await),["uma vez"]);
}

#[tokio::test] async fn entrega_em_ordem_quando_o_agente_termina() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"primeira").await;
    mandar(&state,"segunda").await;
    redesenhar(&session,LIVRE_DE_NOVO);
    um_ciclo(&state);
    assert!(colado(&bytes).contains("primeira") && !colado(&bytes).contains("segunda"));
    turno_inteiro(&session);
    um_ciclo(&state);
    let tudo = colado(&bytes);
    assert!(tudo.find("primeira").unwrap() < tudo.find("segunda").unwrap());
    assert!(textos(&fila_de(&state).await).is_empty());
}

#[tokio::test] async fn uma_por_vez_entre_turnos() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"primeira").await;
    mandar(&state,"segunda").await;
    redesenhar(&session,LIVRE_DE_NOVO);
    um_ciclo(&state);
    // A tela ainda parece livre: a CLI não desenhou o turno da primeira.
    um_ciclo(&state);
    um_ciclo(&state);
    assert!(colado(&bytes).contains("primeira"));
    assert!(!colado(&bytes).contains("segunda"),"a segunda espera o turno da primeira");
    assert_eq!(textos(&fila_de(&state).await),["segunda"]);
}

#[tokio::test] async fn mensagem_nova_nao_fura_a_fila() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"primeira").await;
    redesenhar(&session,LIVRE_DE_NOVO);
    let (_,acao) = mandar(&state,"segunda").await;
    assert_eq!(acao["state"],"guardada");
    um_ciclo(&state);
    assert!(colado(&bytes).contains("primeira") && !colado(&bytes).contains("segunda"));
}

#[tokio::test] async fn tirar_da_fila_nao_entrega() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"tira esta").await;
    mandar(&state,"fica esta").await;
    let item = id_de(&fila_de(&state).await,0);
    let (status,fila) = tirar_item(&state,&item).await;
    assert_eq!(status,StatusCode::OK);
    assert_eq!(textos(&fila),["fica esta"]);
    redesenhar(&session,LIVRE_DE_NOVO);
    um_ciclo(&state);
    assert!(!colado(&bytes).contains("tira esta"));
    assert!(colado(&bytes).contains("fica esta"));
}

#[tokio::test] async fn tirar_item_entregue_devolve_409() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"ja foi").await;
    let item = id_de(&fila_de(&state).await,0);
    redesenhar(&session,LIVRE_DE_NOVO);
    um_ciclo(&state);
    assert!(colado(&bytes).contains("ja foi"));
    let (status,corpo) = tirar_item(&state,&item).await;
    assert_eq!(status,StatusCode::CONFLICT);
    assert!(corpo["error"].as_str().unwrap().contains("já foi entregue"));
}

#[tokio::test] async fn dois_cliques_no_x_nao_quebram() {
    let (_dir,state) = fixture(); let (_session,_,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"tira").await;
    let item = id_de(&fila_de(&state).await,0);
    assert_eq!(tirar_item(&state,&item).await.0,StatusCode::OK);
    let (status,fila) = tirar_item(&state,&item).await;
    assert_eq!(status,StatusCode::OK);
    assert!(textos(&fila).is_empty());
}

#[tokio::test] async fn parar_nao_solta_a_fila() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"espera").await;
    assert_eq!(parar_agente(&state).await,StatusCode::OK);
    redesenhar(&session,LIVRE_DE_NOVO);
    bytes.lock().unwrap().clear();
    um_ciclo(&state);
    assert!(!colado(&bytes).contains("espera"),"Parar não pode recomeçar o trabalho sozinho");
    let fila = fila_de(&state).await;
    assert_eq!(fila["pausada"],true);
    assert_eq!(textos(&fila),["espera"]);
}

#[tokio::test] async fn mensagem_nova_tira_a_pausa_e_respeita_a_ordem() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"antiga").await;
    parar_agente(&state).await;
    redesenhar(&session,LIVRE_DE_NOVO);
    bytes.lock().unwrap().clear();
    let (_,acao) = mandar(&state,"nova").await;
    assert_eq!(acao["state"],"guardada");
    assert_eq!(fila_de(&state).await["pausada"],false);
    um_ciclo(&state);
    assert!(colado(&bytes).contains("antiga") && !colado(&bytes).contains("nova"));
}

#[tokio::test] async fn fila_sobrevive_ao_engine_religado() {
    let (dir,state) = fixture(); let (_session,_,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"sobrevive").await;
    let item = id_de(&fila_de(&state).await,0);
    let religado = Arc::new(EngineState { persist_lock:Mutex::new(()),mobile:MobileRuntime::new(dir.path()),
        token:"test-only".into(),state_file:dir.path().join("sessions.json"),sessions:Mutex::new(HashMap::new()),id_sequence:AtomicU64::new(1) });
    let fila = fila_de(&religado).await;
    assert_eq!(textos(&fila),["sobrevive"]);
    assert_eq!(id_de(&fila,0),item);
}

#[tokio::test] async fn entrega_interrompida_pelo_desligamento_volta_para_a_fila() {
    let (dir,state) = fixture(); let (session,_,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"no meio").await;
    redesenhar(&session,LIVRE_DE_NOVO);
    entregar_proximas(&state);
    let religado = carregar(dir.path());
    let fila = religado.por_conversa.get("c").unwrap();
    assert!(fila.entregando.is_none());
    assert_eq!(fila.itens[0].texto,"no meio");
}

#[tokio::test] async fn aprovacao_pendente_segura_a_entrega() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"espera a resposta").await;
    redesenhar(&session,APROVACAO);
    um_ciclo(&state);
    assert!(bytes.lock().unwrap().is_empty());
    assert_eq!(textos(&fila_de(&state).await),["espera a resposta"]);
}

#[tokio::test] async fn sessao_que_morreu_mantem_a_fila_e_avisa() {
    let (_dir,state) = fixture(); let (_session,_,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"guardada").await;
    state.sessions.lock().unwrap().clear();
    um_ciclo(&state);
    let fila = fila_de(&state).await;
    assert_eq!(textos(&fila),["guardada"]);
    assert!(fila["aviso"].as_str().unwrap().contains("guardada"));
}

#[tokio::test] async fn tela_muda_antes_de_colar_a_entrega_volta_para_a_frente() {
    let (_dir,state) = fixture(); let (session,bytes,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"primeira").await;
    mandar(&state,"segunda").await;
    redesenhar(&session,LIVRE_DE_NOVO);
    entregar_proximas(&state);
    redesenhar(&session,TRABALHANDO);
    let acao = state.mobile.actions.lock().unwrap().iter().find(|a| a.state == "queued").cloned().unwrap();
    let resultado = execute(&state,&acao);
    concluir_acao(&state,&acao,resultado);
    assert!(bytes.lock().unwrap().is_empty());
    assert_eq!(textos(&fila_de(&state).await),["primeira","segunda"]);
}

#[tokio::test] async fn mensagem_automatica_segue_na_fila_da_cli() {
    let (_dir,state) = fixture(); let (_session,bytes,_cli) = com_agente_trabalhando(&state);
    enfileirar_sozinho(&state,"c","auto","/compact".into()).unwrap();
    um_ciclo(&state);
    assert!(colado(&bytes).contains("/compact"),"o que o OMNI manda sozinho não ganha X");
    assert!(textos(&fila_de(&state).await).is_empty());
}

#[tokio::test] async fn timeline_traz_a_fila_junto() {
    let (_dir,state) = fixture(); let (_session,_,_cli) = com_agente_trabalhando(&state);
    mandar(&state,"na tela").await;
    let (_,corpo) = chamar(&state,req("GET","/conversas/c/timeline",ORIGEM,"")).await;
    assert_eq!(textos(&corpo["fila"]),["na tela"]);
}

#[tokio::test] async fn rota_exige_token() {
    let (_dir,state) = fixture();
    state.mobile.config.lock().unwrap().token = "segredo".into();
    let (ler,_) = chamar(&state,req("GET","/conversas/c/fila",ORIGEM,"")).await;
    let (tirar,_) = chamar(&state,pedido("DELETE","/conversas/c/fila/x","","d")).await;
    assert_eq!(ler,StatusCode::UNAUTHORIZED);
    assert_eq!(tirar,StatusCode::UNAUTHORIZED);
}
