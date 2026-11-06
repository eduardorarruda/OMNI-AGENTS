//! Kanban guardado no engine (`kanban.json` ao lado de `sessions.json`). Antes vivia só no
//! `localStorage` da tela, que o engine não alcança. Cada rota mexe em UM cartão: nada de substituir
//! o quadro inteiro, para dois pedidos seguidos (ou desktop + celular) não perderem cartão.

use super::*;
use axum::{extract::{DefaultBodyLimit, Path as RoutePath, State}, http::StatusCode, routing::{get, patch, post}, Json, Router};
use omni_core::kanban::{self as regras, Cartao, ErroKanban, ImportacaoExterna, Quadro, ResumoDaImportacao};
use serde_json::{json, Value};
use std::{fs, io, path::{Path, PathBuf}};

type ApiError = (StatusCode, Json<Value>);

/// Cartão e ajustes são JSON pequeno; a migração leva o quadro inteiro de uma vez.
const CARTAO_MAX_BYTES: usize = 256 * 1024;
const MIGRAR_MAX_BYTES: usize = 8 * 1024 * 1024;

/// Uma gravação por vez no processo: ler, mudar e gravar é uma só operação. Sem isso, dois pedidos
/// leem o mesmo quadro e o segundo apaga o cartão do primeiro.
static GRAVANDO: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn erro(status: StatusCode, mensagem: &str) -> ApiError {
    (status, Json(json!({"error": mensagem})))
}

fn arquivo_do_kanban(state: &EngineState) -> PathBuf {
    state.state_file.with_file_name("kanban.json")
}

/// Lê o quadro. Arquivo que não existe é quadro vazio. Arquivo que não abre como quadro NÃO é apagado
/// nem sobrescrito às cegas: vira `kanban.corrompido-<ms>.json` ao lado, e o quadro volta vazio.
/// Erro de leitura que não seja "não existe" (permissão, disco) sobe como erro, nunca como vazio.
pub(crate) fn ler(arquivo: &Path) -> Result<Quadro, String> {
    let bytes = match fs::read(arquivo) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Quadro::default()),
        Err(e) => return Err(format!("Não foi possível ler o Kanban: {e}")),
    };
    match serde_json::from_slice::<Quadro>(&bytes) {
        Ok(quadro) => Ok(quadro),
        Err(_) => {
            let agora = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
            let copia = arquivo.with_file_name(format!("kanban.corrompido-{agora}.json"));
            fs::rename(arquivo, &copia).map_err(|e| format!("Kanban corrompido e a cópia falhou: {e}"))?;
            Ok(Quadro::default())
        }
    }
}

/// Ler, aplicar `mudanca` e gravar de forma atômica (arquivo temporário + rename), tudo sob o trinco.
/// Se `mudanca` recusar, nada é gravado.
pub(crate) fn alterar<T>(
    arquivo: &Path,
    mudanca: impl FnOnce(&mut Quadro) -> Result<T, ErroKanban>,
) -> Result<T, ApiError> {
    let _trinco = GRAVANDO.lock().unwrap_or_else(|envenenado| envenenado.into_inner());
    let mut quadro = ler(arquivo).map_err(|m| erro(StatusCode::INTERNAL_SERVER_ERROR, &m))?;
    let resultado = mudanca(&mut quadro).map_err(converter)?;
    omni_protocol::atomic_write_json(arquivo, &quadro)
        .map_err(|_| erro(StatusCode::INTERNAL_SERVER_ERROR, "Não foi possível salvar o Kanban"))?;
    Ok(resultado)
}

fn converter(e: ErroKanban) -> ApiError {
    match e {
        ErroKanban::Invalido(m) => erro(StatusCode::BAD_REQUEST, &m),
        ErroKanban::Conflito(m) => erro(StatusCode::CONFLICT, &m),
        ErroKanban::NaoEncontrado(m) => erro(StatusCode::NOT_FOUND, &m),
    }
}

async fn kanban_ler(State(state): State<Arc<EngineState>>) -> Result<Json<Quadro>, ApiError> {
    let _trinco = GRAVANDO.lock().unwrap_or_else(|envenenado| envenenado.into_inner());
    ler(&arquivo_do_kanban(&state)).map(Json).map_err(|m| erro(StatusCode::INTERNAL_SERVER_ERROR, &m))
}

async fn cartao_criar(State(state): State<Arc<EngineState>>, Json(cartao): Json<Cartao>) -> Result<(StatusCode, Json<Cartao>), ApiError> {
    alterar(&arquivo_do_kanban(&state), |q| regras::criar(q, cartao)).map(|c| (StatusCode::CREATED, Json(c)))
}

async fn cartao_editar(
    State(state): State<Arc<EngineState>>,
    RoutePath(id): RoutePath<String>,
    Json(mudancas): Json<Value>,
) -> Result<Json<Cartao>, ApiError> {
    alterar(&arquivo_do_kanban(&state), |q| regras::editar(q, &id, &mudancas)).map(Json)
}

async fn cartao_remover(State(state): State<Arc<EngineState>>, RoutePath(id): RoutePath<String>) -> Result<Json<Value>, ApiError> {
    let removeu: bool = alterar(&arquivo_do_kanban(&state), |q| Ok(regras::remover(q, &id)))?;
    Ok(Json(json!({"removido": removeu})))
}

async fn cartao_pegar(State(state): State<Arc<EngineState>>, RoutePath(id): RoutePath<String>) -> Result<Json<Cartao>, ApiError> {
    let agora = regras::instante_agora();
    alterar(&arquivo_do_kanban(&state), |q| regras::pegar(q, &id, &agora)).map(Json)
}

async fn cartao_iniciar(State(state): State<Arc<EngineState>>, RoutePath(id): RoutePath<String>) -> Result<Json<Cartao>, ApiError> {
    let agora = regras::instante_agora();
    alterar(&arquivo_do_kanban(&state), |q| regras::iniciar(q, &id, &agora)).map(Json)
}

async fn ajustes_editar(State(state): State<Arc<EngineState>>, Json(mudancas): Json<Value>) -> Result<Json<Value>, ApiError> {
    alterar(&arquivo_do_kanban(&state), |q| regras::ajustar(q, &mudancas).map(|()| q.ajustes.clone())).map(Json)
}

/// Migração única do `localStorage`. Só escreve com o quadro do engine vazio; senão devolve
/// `migrou:false` e o quadro atual, sem tocar em nada.
async fn migrar(State(state): State<Arc<EngineState>>, Json(origem): Json<Quadro>) -> Result<Json<Value>, ApiError> {
    let (migrou, quadro) = alterar(&arquivo_do_kanban(&state), |q| {
        let migrou = regras::migrar(q, origem);
        Ok((migrou, q.clone()))
    })?;
    Ok(Json(json!({"migrou": migrou, "quadro": quadro})))
}

/// O desktop manda o resultado de uma busca que nasceu de um clique. O engine nunca busca sozinho.
async fn externas_importar(State(state): State<Arc<EngineState>>, Json(importacao): Json<ImportacaoExterna>) -> Result<Json<ResumoDaImportacao>, ApiError> {
    alterar(&arquivo_do_kanban(&state), |q| regras::importar_externas(q, importacao)).map(Json)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct EscolhaDeProjeto {
    project_id: String,
    #[serde(default)]
    origem: Option<String>,
}

async fn externa_escolher_projeto(
    State(state): State<Arc<EngineState>>,
    RoutePath(id): RoutePath<String>,
    Json(escolha): Json<EscolhaDeProjeto>,
) -> Result<Json<Cartao>, ApiError> {
    let agora = regras::instante_agora();
    alterar(&arquivo_do_kanban(&state), |q| regras::escolher_projeto(q, &id, escolha.origem.as_deref(), &escolha.project_id, &agora)).map(Json)
}

/// Só entra no servidor HTTP (celular e web), nunca no `local` do desktop: o token de dispositivo lê o
/// Kanban, mas quem escreve é o desktop. Web e celular não usam o Kanban.
pub(crate) async fn somente_leitura_no_http(request: axum::extract::Request, next: axum::middleware::Next) -> axum::response::Response {
    use axum::response::IntoResponse;
    let caminho = request.uri().path();
    let do_kanban = caminho == "/kanban" || caminho.starts_with("/kanban/");
    if do_kanban && request.method() != axum::http::Method::GET {
        return erro(StatusCode::FORBIDDEN, "O Kanban só é alterado pelo app do computador").into_response();
    }
    next.run(request).await
}

pub(crate) fn rotas() -> Router<Arc<EngineState>> {
    Router::new()
        .route("/kanban", get(kanban_ler))
        .route("/kanban/cartoes", post(cartao_criar).layer(DefaultBodyLimit::max(CARTAO_MAX_BYTES)))
        .route(
            "/kanban/cartoes/{id}",
            patch(cartao_editar).delete(cartao_remover).layer(DefaultBodyLimit::max(CARTAO_MAX_BYTES)),
        )
        .route("/kanban/cartoes/{id}/pegar", post(cartao_pegar))
        .route("/kanban/cartoes/{id}/iniciar", post(cartao_iniciar))
        .route("/kanban/ajustes", patch(ajustes_editar).layer(DefaultBodyLimit::max(CARTAO_MAX_BYTES)))
        .route("/kanban/migrar", post(migrar).layer(DefaultBodyLimit::max(MIGRAR_MAX_BYTES)))
        .route("/kanban/externas", post(externas_importar).layer(DefaultBodyLimit::max(MIGRAR_MAX_BYTES)))
        .route("/kanban/externas/{id}/projeto", post(externa_escolher_projeto).layer(DefaultBodyLimit::max(CARTAO_MAX_BYTES)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mobile::{rotas, router};
    use axum::{body::{to_bytes, Body}, http::Request};
    use tower::ServiceExt;

    const ORIGEM: &str = "http://127.0.0.1:47322";

    /// Estado de mentira numa pasta temporária: nunca toca nos dados reais do usuário.
    fn fixture() -> (tempfile::TempDir, Arc<EngineState>) {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(EngineState {
            persist_lock: Mutex::new(()),
            mobile: mobile::MobileRuntime::new(dir.path()),
            token: "test-only".into(),
            state_file: dir.path().join("sessions.json"),
            sessions: Mutex::new(HashMap::new()),
            id_sequence: AtomicU64::new(1),
        });
        (dir, state)
    }

    /// As rotas como o desktop as chama (`mobile::local`): sem token de dispositivo, com o limite global de 32 KB.
    fn app_local(state: Arc<EngineState>) -> Router {
        rotas().layer(DefaultBodyLimit::max(32 * 1024)).with_state(state)
    }

    fn pedido(metodo: &str, uri: &str, corpo: &str, token: Option<&str>) -> Request<Body> {
        let mut r = Request::builder().method(metodo).uri(uri).header("host", "127.0.0.1:47322")
            .header("origin", ORIGEM).header("content-type", "application/json");
        if let Some(t) = token {
            r = r.header("x-omni-token", t);
        }
        r.body(Body::from(corpo.to_owned())).unwrap()
    }

    async fn chamar(app: &Router, metodo: &str, uri: &str, corpo: &str) -> (StatusCode, Value) {
        let resposta = app.clone().oneshot(pedido(metodo, uri, corpo, None)).await.unwrap();
        let status = resposta.status();
        let bytes = to_bytes(resposta.into_body(), 4 * 1024 * 1024).await.unwrap();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    fn cartao(id: &str) -> Value {
        json!({"id":id,"projectId":"p","title":format!("Tarefa {id}"),"description":"","status":"backlog","position":1,
            "loopTotal":null,"loopRemaining":null,"resultNote":null,
            "createdAt":"2026-11-06T00:00:00Z","updatedAt":"2026-11-06T00:00:00Z"})
    }

    fn cartao_tipado(id: &str) -> Cartao {
        serde_json::from_value(cartao(id)).unwrap()
    }

    #[tokio::test]
    async fn criar_e_ler_cartao() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let (status, criado) = chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(criado["id"], "a");
        let (status, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quadro["versao"], 1);
        assert_eq!(quadro["tarefas"].as_array().unwrap().len(), 1);
        assert_eq!(quadro["tarefas"][0]["title"], "Tarefa a");
        // id repetido não duplica
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn editar_um_cartao_nao_afeta_outro() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        chamar(&app, "POST", "/kanban/cartoes", &cartao("b").to_string()).await;
        let (status, editado) = chamar(&app, "PATCH", "/kanban/cartoes/a", r#"{"status":"ready","title":"Nova"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(editado["status"], "ready");
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["tarefas"][0]["title"], "Nova");
        assert_eq!(quadro["tarefas"][1], cartao("b"));
        let (status, _) = chamar(&app, "PATCH", "/kanban/cartoes/nao-existe", r#"{"title":"x"}"#).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = chamar(&app, "DELETE", "/kanban/cartoes/a", "").await;
        assert_eq!(status, StatusCode::OK);
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["tarefas"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn dois_pedidos_seguidos_nao_perdem_cartao() {
        let dir = tempfile::tempdir().unwrap();
        let arquivo = dir.path().join("kanban.json");
        let fios: Vec<_> = (0..24)
            .map(|n| {
                let arquivo = arquivo.clone();
                std::thread::spawn(move || {
                    alterar(&arquivo, |q| regras::criar(q, cartao_tipado(&format!("c{n}")))).unwrap();
                })
            })
            .collect();
        for fio in fios {
            fio.join().unwrap();
        }
        assert_eq!(ler(&arquivo).unwrap().tarefas.len(), 24);
    }

    #[test]
    fn gravacao_e_atomica() {
        let dir = tempfile::tempdir().unwrap();
        let arquivo = dir.path().join("kanban.json");
        alterar(&arquivo, |q| regras::criar(q, cartao_tipado("a"))).unwrap();
        // um .tmp sobrando de uma queda anterior não atrapalha nem fica depois de gravar
        fs::write(arquivo.with_extension("json.tmp"), b"{ meio escrito").unwrap();
        alterar(&arquivo, |q| regras::criar(q, cartao_tipado("b"))).unwrap();
        assert!(!arquivo.with_extension("json.tmp").exists(), "o temporário vira o arquivo por rename");
        assert_eq!(ler(&arquivo).unwrap().tarefas.len(), 2);
        // operação recusada não grava nada
        let antes = fs::read(&arquivo).unwrap();
        assert!(alterar(&arquivo, |q| regras::criar(q, cartao_tipado("a"))).is_err());
        assert_eq!(fs::read(&arquivo).unwrap(), antes);
    }

    #[tokio::test]
    async fn migrar_so_vale_com_quadro_vazio() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let primeiro = json!({"versao":1,"tarefas":[cartao("a"),cartao("b")],"ajustes":{"dispatcherEnabled":true}});
        let (status, r) = chamar(&app, "POST", "/kanban/migrar", &primeiro.to_string()).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(r["migrou"], true);
        let segundo = json!({"versao":1,"tarefas":[cartao("z")],"ajustes":{}});
        let (_, r) = chamar(&app, "POST", "/kanban/migrar", &segundo.to_string()).await;
        assert_eq!(r["migrou"], false);
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        let ids: Vec<_> = quadro["tarefas"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap().to_owned()).collect();
        assert_eq!(ids, ["a", "b"]);
        assert_eq!(quadro["ajustes"]["dispatcherEnabled"], true);
    }

    #[tokio::test]
    async fn arquivo_corrompido_vira_copia_e_nao_apaga() {
        let (dir, state) = fixture();
        fs::write(dir.path().join("kanban.json"), b"{ isto nao e json").unwrap();
        let app = app_local(state);
        let (status, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(quadro["tarefas"].as_array().unwrap().len(), 0);
        let copias: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("kanban.corrompido-")).collect();
        assert_eq!(copias.len(), 1, "o arquivo ruim fica guardado");
        assert_eq!(fs::read(copias[0].path()).unwrap(), b"{ isto nao e json");
        // e o Kanban volta a funcionar
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn quadro_grande_passa_pelo_limite_global_de_32kb() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let cartoes: Vec<Value> = (0..60)
            .map(|n| {
                let mut c = cartao(&format!("c{n}"));
                c["description"] = json!("ç".repeat(1000));
                c
            })
            .collect();
        let corpo = json!({"versao":1,"tarefas":cartoes,"ajustes":{}}).to_string();
        assert!(corpo.len() > 100 * 1024, "o corpo precisa passar bem de 32 KB");
        let (status, r) = chamar(&app, "POST", "/kanban/migrar", &corpo).await;
        assert_eq!(status, StatusCode::OK, "{r}");
        assert_eq!(r["migrou"], true);
        let mut grande = cartao("g");
        grande["description"] = json!("ç".repeat(15_000));
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes", &grande.to_string()).await;
        assert_eq!(status, StatusCode::CREATED);
    }

    #[tokio::test]
    async fn apagar_tudo_depois_de_migrar_nao_ressuscita_a_copia_antiga() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let origem = json!({"versao":1,"tarefas":[cartao("a")],"ajustes":{}}).to_string();
        let (_, r) = chamar(&app, "POST", "/kanban/migrar", &origem).await;
        assert_eq!(r["migrou"], true);
        chamar(&app, "DELETE", "/kanban/cartoes/a", "").await;
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["migrado"], true);
        let (_, r) = chamar(&app, "POST", "/kanban/migrar", &origem).await;
        assert_eq!(r["migrou"], false);
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["tarefas"].as_array().unwrap().len(), 0);
    }

    fn importacao(tarefas: Value) -> String {
        json!({"origem":"central.exemplo","tarefas":tarefas}).to_string()
    }

    async fn atualizar(app: &Router, tarefas: Value) -> Value {
        let (status, resumo) = chamar(app, "POST", "/kanban/externas", &importacao(tarefas)).await;
        assert_eq!(status, StatusCode::OK, "{resumo}");
        resumo
    }

    #[tokio::test]
    async fn atualizar_nao_duplica() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let lista = json!([{"id":"7","titulo":"Corrigir","descricao":"Detalhe"},{"id":"7","titulo":"Repetida na resposta"},{"id":"8","titulo":"Outra"}]);
        let resumo = atualizar(&app, lista.clone()).await;
        assert_eq!(resumo["novas"], 2);
        let resumo = atualizar(&app, json!([{"id":"7","titulo":"Título novo"},{"id":"8","titulo":"Outra"}])).await;
        assert_eq!((&resumo["novas"], &resumo["atualizadas"]), (&json!(0), &json!(2)));
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        let triagem = quadro["triagem"].as_array().unwrap();
        assert_eq!(triagem.len(), 2);
        assert_eq!(triagem[0]["titulo"], "Título novo");
        assert_eq!(triagem[0]["origem"], "central.exemplo");
        // mesmo id em outra origem é outra tarefa
        let corpo = json!({"origem":"outra.exemplo","tarefas":[{"id":"7","titulo":"De outra API"}]}).to_string();
        chamar(&app, "POST", "/kanban/externas", &corpo).await;
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["triagem"].as_array().unwrap().len(), 3);
        assert_eq!(quadro["tarefas"].as_array().unwrap().len(), 0, "triagem não é cartão de projeto");
    }

    #[tokio::test]
    async fn tarefa_que_sumiu_fica_marcada() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        atualizar(&app, json!([{"id":"7","titulo":"Fica"},{"id":"8","titulo":"Vai sumir"},{"id":"9","titulo":"No projeto"}])).await;
        chamar(&app, "POST", "/kanban/externas/9/projeto", r#"{"projectId":"p"}"#).await;
        // outra origem não é afetada por esta lista
        let outra = json!({"origem":"outra.exemplo","tarefas":[{"id":"1","titulo":"De outra API"}]}).to_string();
        chamar(&app, "POST", "/kanban/externas", &outra).await;

        let resumo = atualizar(&app, json!([{"id":"7","titulo":"Fica"}])).await;
        assert_eq!(resumo["sumiram"], 2);
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        let sumiu = |id: &str| quadro["triagem"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap()["sumiu"].clone();
        assert_eq!((sumiu("7"), sumiu("8"), sumiu("1")), (json!(false), json!(true), json!(false)));
        assert_eq!(quadro["tarefas"][0]["externa"]["sumiu"], true, "o cartão fica no quadro, marcado");

        // a lista vazia também é uma resposta: tudo o que era desta origem saiu
        atualizar(&app, json!([])).await;
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["triagem"][0]["sumiu"], true);

        // e quem volta perde a marca
        let resumo = atualizar(&app, json!([{"id":"8","titulo":"Voltou"},{"id":"9","titulo":"No projeto"}])).await;
        assert_eq!(resumo["sumiram"], 0);
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["triagem"][1]["sumiu"], false);
        assert_eq!(quadro["tarefas"][0]["externa"]["sumiu"], false);
    }

    #[tokio::test]
    async fn escolher_projeto_vira_backlog() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        atualizar(&app, json!([{"id":"7/x","titulo":"Corrigir","descricao":"Detalhe"},{"id":"8","titulo":""}])).await;
        let (status, novo) = chamar(&app, "POST", "/kanban/externas/7%2Fx/projeto", r#"{"projectId":"proj-1","origem":"central.exemplo"}"#).await;
        assert_eq!(status, StatusCode::OK, "{novo}");
        assert_eq!((&novo["projectId"], &novo["status"], &novo["title"]), (&json!("proj-1"), &json!("backlog"), &json!("Corrigir")));
        assert_eq!(novo["description"], "Detalhe");
        assert_eq!(novo["externa"], json!({"origem":"central.exemplo","id":"7/x","sumiu":false}));
        assert!(novo["position"].as_i64().unwrap() > 1, "vai para o fim da coluna");
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["triagem"].as_array().unwrap().len(), 1);
        assert_eq!(quadro["tarefas"].as_array().unwrap().len(), 2);
        // tarefa sem título ainda vira cartão válido
        let (status, novo) = chamar(&app, "POST", "/kanban/externas/8/projeto", r#"{"projectId":"proj-1"}"#).await;
        assert_eq!(status, StatusCode::OK, "{novo}");
        assert_eq!(novo["title"], "Tarefa 8");
        let (status, _) = chamar(&app, "POST", "/kanban/externas/nao-existe/projeto", r#"{"projectId":"p"}"#).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn escolher_projeto_com_id_igual_em_duas_origens_pede_a_origem() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        atualizar(&app, json!([{"id":"7","titulo":"Daqui"}])).await;
        let outra = json!({"origem":"outra.exemplo","tarefas":[{"id":"7","titulo":"De lá"}]}).to_string();
        chamar(&app, "POST", "/kanban/externas", &outra).await;
        let (status, _) = chamar(&app, "POST", "/kanban/externas/7/projeto", r#"{"projectId":"p"}"#).await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, novo) = chamar(&app, "POST", "/kanban/externas/7/projeto", r#"{"projectId":"p","origem":"outra.exemplo"}"#).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(novo["title"], "De lá");
    }

    #[tokio::test]
    async fn cartao_ja_em_projeto_nao_volta_para_triagem() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let lista = json!([{"id":"7","titulo":"Original"}]);
        atualizar(&app, lista.clone()).await;
        let (_, novo) = chamar(&app, "POST", "/kanban/externas/7/projeto", r#"{"projectId":"p"}"#).await;
        let id = novo["id"].as_str().unwrap().to_owned();
        chamar(&app, "PATCH", &format!("/kanban/cartoes/{id}"), r#"{"title":"Editado por mim","status":"ready"}"#).await;
        let resumo = atualizar(&app, json!([{"id":"7","titulo":"Mudou na origem"}])).await;
        assert_eq!((&resumo["novas"], &resumo["jaNoQuadro"]), (&json!(0), &json!(1)));
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["triagem"].as_array().unwrap().len(), 0);
        assert_eq!(quadro["tarefas"][0]["title"], "Editado por mim", "o que o usuário editou não é sobrescrito");
        assert_eq!(quadro["tarefas"][0]["status"], "ready");
    }

    #[tokio::test]
    async fn lista_externa_grande_passa_pelo_limite_global_de_32kb() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let lista: Vec<Value> = (0..60).map(|n| json!({"id":n.to_string(),"titulo":"T","descricao":"ç".repeat(1000)})).collect();
        let corpo = importacao(json!(lista));
        assert!(corpo.len() > 100 * 1024);
        let (status, r) = chamar(&app, "POST", "/kanban/externas", &corpo).await;
        assert_eq!(status, StatusCode::OK, "{r}");
    }

    #[tokio::test]
    async fn triagem_cheia_recusa_sem_gravar_pela_metade() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let lista: Vec<Value> = (0..=regras::TRIAGEM_MAX).map(|n| json!({"id":n.to_string(),"titulo":"T"})).collect();
        let (status, r) = chamar(&app, "POST", "/kanban/externas", &importacao(json!(lista))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(r["error"].as_str().unwrap().contains("Filtre"), "{r}");
        let (_, quadro) = chamar(&app, "GET", "/kanban", "").await;
        assert_eq!(quadro["triagem"].as_array().unwrap().len(), 0);
    }

    /// A mensagem também: um 403 de outra camada (Host, origem) passaria sem provar nada sobre esta regra.
    async fn assert_recusa_do_kanban(resposta: axum::response::Response) {
        let corpo = to_bytes(resposta.into_body(), 1 << 20).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&corpo).unwrap()["error"], "O Kanban só é alterado pelo app do computador");
    }

    #[tokio::test]
    async fn pegar_so_vale_uma_vez_e_so_antes_de_rodar() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        let (status, pego) = chamar(&app, "POST", "/kanban/cartoes/a/pegar", "").await;
        assert_eq!(status, StatusCode::OK, "{pego}");
        assert_eq!((&pego["status"], &pego["pegoPor"]), (&json!("doing"), &json!("mcp")));
        assert_ne!(pego["updatedAt"], cartao("a")["updatedAt"]);
        // dois agentes no mesmo cartão: o segundo perde
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/a/pegar", "").await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (_, concluido) = chamar(&app, "PATCH", "/kanban/cartoes/a", r#"{"status":"done"}"#).await;
        assert!(concluido.get("pegoPor").is_none(), "sair de Rodando limpa quem pegou: {concluido}");
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/a/pegar", "").await;
        assert_eq!(status, StatusCode::CONFLICT, "cartão concluído não volta a rodar por aqui");
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/nao-existe/pegar", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn iniciar_so_aceita_pronta_e_uma_vez() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        chamar(&app, "POST", "/kanban/cartoes", &cartao("a").to_string()).await;
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/a/iniciar", "").await;
        assert_eq!(status, StatusCode::CONFLICT, "cartão do Backlog não é da fila do dispatcher");
        chamar(&app, "PATCH", "/kanban/cartoes/a", r#"{"status":"ready"}"#).await;
        let (status, iniciado) = chamar(&app, "POST", "/kanban/cartoes/a/iniciar", "").await;
        assert_eq!(status, StatusCode::OK, "{iniciado}");
        assert_eq!(iniciado["status"], "doing");
        assert!(iniciado.get("pegoPor").is_none(), "quem olha o cartão é o dispatcher: {iniciado}");
        assert_ne!(iniciado["updatedAt"], cartao("a")["updatedAt"]);
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/a/iniciar", "").await;
        assert_eq!(status, StatusCode::CONFLICT);
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/nao-existe/iniciar", "").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn agente_e_dispatcher_nao_levam_o_mesmo_cartao() {
        let (_dir, state) = fixture();
        let app = app_local(state);
        let mut pronto = cartao("a");
        pronto["status"] = json!("ready");
        chamar(&app, "POST", "/kanban/cartoes", &pronto.to_string()).await;
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/a/pegar", "").await;
        assert_eq!(status, StatusCode::OK);
        let (status, _) = chamar(&app, "POST", "/kanban/cartoes/a/iniciar", "").await;
        assert_eq!(status, StatusCode::CONFLICT);
    }

    /// O caminho do MCP: `EngineRequest::Api`, com o token do engine, cai em `mobile::local`, que monta
    /// as rotas sem o `somente_leitura_no_http`. Se isto passar a dar 403, o MCP deixou de escrever.
    #[tokio::test]
    async fn credencial_local_do_engine_escreve_no_kanban() {
        let (_dir, state) = fixture();
        let criado = mobile::local(state.clone(), "POST", "/kanban/cartoes", Some(cartao("a").to_string()), None, vec![]).await;
        assert!(matches!(criado, EngineResponse::Api { status: 201, .. }), "{criado:?}");
        let pego = mobile::local(state.clone(), "POST", "/kanban/cartoes/a/pegar", None, None, vec![]).await;
        assert!(matches!(pego, EngineResponse::Api { status: 200, .. }), "{pego:?}");
        let movido = mobile::local(state, "PATCH", "/kanban/cartoes/a", Some(r#"{"status":"done","pegoPor":null}"#.into()), None, vec![]).await;
        assert!(matches!(movido, EngineResponse::Api { status: 200, .. }), "{movido:?}");
    }

    #[tokio::test]
    async fn token_do_celular_nao_pega_nem_move_cartao() {
        let (_dir, state) = fixture();
        state.mobile.config.lock().unwrap().token = "segredo".into();
        let criado = mobile::local(state.clone(), "POST", "/kanban/cartoes", Some(cartao("a").to_string()), None, vec![]).await;
        assert!(matches!(criado, EngineResponse::Api { status: 201, .. }));
        let app = router(state.clone());
        for (metodo, uri, corpo) in [
            ("POST", "/kanban/cartoes/a/pegar", ""),
            ("POST", "/kanban/cartoes/a/iniciar", ""),
            ("PATCH", "/kanban/cartoes/a", r#"{"status":"done"}"#),
        ] {
            let r = app.clone().oneshot(pedido(metodo, uri, corpo, Some("segredo"))).await.unwrap();
            assert_eq!(r.status(), StatusCode::FORBIDDEN, "{metodo} {uri} com token de dispositivo");
            assert_recusa_do_kanban(r).await;
        }
        let quadro = ler(&arquivo_do_kanban(&state)).unwrap();
        assert_eq!((quadro.tarefas[0].status.as_str(), quadro.tarefas[0].pego_por.as_deref()), ("backlog", None));
    }

    #[tokio::test]
    async fn token_do_celular_so_le_o_kanban() {
        let (_dir, state) = fixture();
        state.mobile.config.lock().unwrap().token = "segredo".into();
        let app = router(state);
        let r = app.clone().oneshot(pedido("GET", "/kanban", "", Some("segredo"))).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        for (metodo, uri, corpo) in [
            ("POST", "/kanban/cartoes", cartao("x").to_string()),
            ("PATCH", "/kanban/cartoes/x", r#"{"title":"y"}"#.to_string()),
            ("DELETE", "/kanban/cartoes/x", String::new()),
            ("PATCH", "/kanban/ajustes", r#"{"dispatcherEnabled":true}"#.to_string()),
            ("POST", "/kanban/migrar", r#"{"versao":1}"#.to_string()),
            ("POST", "/kanban/externas", importacao(json!([{"id":"7","titulo":"x"}]))),
            ("POST", "/kanban/externas/7/projeto", r#"{"projectId":"p"}"#.to_string()),
            ("POST", "/kanban/cartoes/x/pegar", String::new()),
            ("POST", "/kanban/cartoes/x/iniciar", String::new()),
        ] {
            let r = app.clone().oneshot(pedido(metodo, uri, &corpo, Some("segredo"))).await.unwrap();
            assert_eq!(r.status(), StatusCode::FORBIDDEN, "{metodo} {uri} com token de dispositivo");
            assert_recusa_do_kanban(r).await;
        }
        let r = app.oneshot(pedido("GET", "/kanban", "", Some("segredo"))).await.unwrap();
        let corpo = to_bytes(r.into_body(), 1 << 20).await.unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&corpo).unwrap()["tarefas"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn rota_exige_token() {
        let (_dir, state) = fixture();
        state.mobile.config.lock().unwrap().token = "segredo".into();
        let app = router(state);
        for (metodo, uri, corpo) in [
            ("GET", "/kanban", ""),
            ("POST", "/kanban/cartoes", r#"{}"#),
            ("PATCH", "/kanban/cartoes/a", r#"{}"#),
            ("DELETE", "/kanban/cartoes/a", ""),
            ("PATCH", "/kanban/ajustes", r#"{}"#),
            ("POST", "/kanban/migrar", r#"{"versao":1}"#),
            ("POST", "/kanban/externas", r#"{}"#),
            ("POST", "/kanban/externas/7/projeto", r#"{}"#),
            ("POST", "/kanban/cartoes/a/pegar", ""),
            ("POST", "/kanban/cartoes/a/iniciar", ""),
        ] {
            let r = app.clone().oneshot(pedido(metodo, uri, corpo, None)).await.unwrap();
            assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{metodo} {uri} sem token");
            let r = app.clone().oneshot(pedido(metodo, uri, corpo, Some("errado"))).await.unwrap();
            assert_eq!(r.status(), StatusCode::UNAUTHORIZED, "{metodo} {uri} token errado");
        }
        let r = app.oneshot(pedido("GET", "/kanban", "", Some("segredo"))).await.unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }
}
