//! Kanban guardado no engine (`kanban.json` ao lado de `sessions.json`). Antes vivia só no
//! `localStorage` da tela, que o engine não alcança. Cada rota mexe em UM cartão: nada de substituir
//! o quadro inteiro, para dois pedidos seguidos (ou desktop + celular) não perderem cartão.

use super::*;
use axum::{extract::{DefaultBodyLimit, Path as RoutePath, State}, http::StatusCode, routing::{get, patch, post}, Json, Router};
use omni_core::kanban::{self as regras, Cartao, ErroKanban, Quadro};
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
        .route("/kanban/ajustes", patch(ajustes_editar).layer(DefaultBodyLimit::max(CARTAO_MAX_BYTES)))
        .route("/kanban/migrar", post(migrar).layer(DefaultBodyLimit::max(MIGRAR_MAX_BYTES)))
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
        ] {
            let r = app.clone().oneshot(pedido(metodo, uri, &corpo, Some("segredo"))).await.unwrap();
            assert_eq!(r.status(), StatusCode::FORBIDDEN, "{metodo} {uri} com token de dispositivo");
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
