//! Túnel HTTPS da Cloudflare (quick tunnel: grátis, sem conta) para o celular abrir as conversas de
//! qualquer lugar, com microfone e "instalar como app" — os dois exigem HTTPS.
//!
//! Filho de `mobile`. O ponto de segurança: o túnel atende num listener **só dele**, em 127.0.0.1 numa
//! porta própria, e todo pedido que chega lá vem do `cloudflared` local — então "veio de loopback" não
//! prova nada e nada aqui confia nisso. Neste listener:
//! - `Host`/`Origin` têm de ser o endereço público do túnel (CSRF, DNS rebinding);
//! - o token é **outro**, só do túnel, e o QR não o leva: o aparelho novo só o recebe com o código
//!   atual do Authy **e** a aprovação do dono no PC (`tunel_pareamento.rs`);
//! - falhas de pareamento e de token travam o IP de origem por um tempo (`CF-Connecting-IP`, que o
//!   cloudflared põe; só é lido aqui, onde quem fala é ele);
//! - o resto é o mesmo roteador do celular, inclusive o `somente_leitura_no_http`.
//!
//! O túnel não liga sem o Authy confirmado e cai sozinho se o cadastro for refeito.

use super::*;
use axum::extract::ConnectInfo;
use std::sync::atomic::{AtomicU64, Ordering};

#[path = "tunel_processo.rs"] mod processo;
#[path = "tunel_pareamento.rs"] mod pareamento;
use processo::{estado_do_cloudflared, localizar, supervisionar};

/// O que sobrevive a reiniciar o engine: a escolha de deixar ligado, o token do túnel e se o
/// pareamento foi desligado por excesso de erros.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct TunelConfig { pub(super) ligado: bool, pub(super) token: String, pub(super) pareamento_desligado: bool }

#[derive(Clone, Default)]
struct Estado { url: Option<String>, erro: Option<String>, porta: Option<u16> }

#[derive(Clone, Default)]
struct Instalacao { ativa: bool, erro: Option<String> }

/// Janela em que as falhas de um IP são contadas, e quanto ele fica travado ao passar do limite.
const JANELA_MS: u64 = 5 * 60_000;
const BLOQUEIO_MS: u64 = 15 * 60_000;
pub(super) const FALHAS_PAREAR_POR_IP: usize = 5;
pub(super) const FALHAS_TOKEN_POR_IP: usize = 20;
/// Teto de IPs lembrados: a tabela não pode crescer sem fim com IPs forjados.
const IPS_MAX: usize = 4096;

#[derive(Clone, Copy)]
pub(super) enum Falha { Parear, Token }

#[derive(Default)]
struct Registro { parear: VecDeque<u64>, token: VecDeque<u64>, parear_ate: u64, token_ate: u64 }

/// Travas por IP. Separadas: errar o código só trava o **pareamento** daquele IP — um aparelho já
/// pareado atrás do mesmo IP (CGNAT do 4G) continua usando o token dele.
#[derive(Default)]
pub(super) struct Limites { por_ip: HashMap<String, Registro> }

impl Limites {
    pub(super) fn bloqueado(&self, ip: &str, falha: Falha, agora_ms: u64) -> bool {
        self.por_ip.get(ip).is_some_and(|r| match falha { Falha::Parear => r.parear_ate, Falha::Token => r.token_ate } > agora_ms)
    }

    pub(super) fn falhou(&mut self, ip: &str, falha: Falha, agora_ms: u64) {
        if self.por_ip.len() >= IPS_MAX && !self.por_ip.contains_key(ip) {
            let recente = |fila: &VecDeque<u64>| fila.back().is_some_and(|t| agora_ms.saturating_sub(*t) < JANELA_MS);
            let vivo = |r: &Registro| r.parear_ate > agora_ms || r.token_ate > agora_ms;
            self.por_ip.retain(|_, r| vivo(r) || recente(&r.parear) || recente(&r.token));
            if self.por_ip.len() >= IPS_MAX { self.por_ip.retain(|_, r| vivo(r)); }
        }
        let registro = self.por_ip.entry(ip.to_owned()).or_default();
        let (fila, ate, limite) = match falha {
            Falha::Parear => (&mut registro.parear, &mut registro.parear_ate, FALHAS_PAREAR_POR_IP),
            Falha::Token => (&mut registro.token, &mut registro.token_ate, FALHAS_TOKEN_POR_IP),
        };
        fila.retain(|t| agora_ms.saturating_sub(*t) < JANELA_MS);
        fila.push_back(agora_ms);
        if fila.len() >= limite {
            fila.clear();
            *ate = agora_ms + BLOQUEIO_MS;
        }
    }
}

pub(super) struct Tunel {
    pub(super) config: Mutex<TunelConfig>,
    estado: Mutex<Estado>,
    mudou: Notify,
    /// Cada rodada do túnel tem um número; o supervisor do cloudflared para quando ele muda.
    geracao: AtomicU64,
    pareamento: Mutex<pareamento::Pareamento>,
    pub(super) limites: Mutex<Limites>,
    instalacao: Mutex<Instalacao>,
}

impl Tunel {
    pub(super) fn new(dir: &Path) -> Self {
        let mut config: TunelConfig = omni_protocol::read_json_or_default(&dir.join("tunel.json"));
        if config.token.is_empty() { config.token = random_token(); }
        Self { config: Mutex::new(config), estado: Mutex::new(Estado::default()), mudou: Notify::new(),
            geracao: AtomicU64::new(0), pareamento: Mutex::new(pareamento::Pareamento::default()),
            limites: Mutex::new(Limites::default()), instalacao: Mutex::new(Instalacao::default()) }
    }
}

fn arquivo_de_config(state: &EngineState) -> PathBuf { state.state_file.with_file_name("tunel.json") }

/// Muda e grava o `tunel.json` (0600: leva o token do túnel). Só troca na memória se gravou.
fn mudar_config(state: &EngineState, mudar: impl FnOnce(&mut TunelConfig)) -> Result<(), ApiError> {
    let mut config = state.mobile.tunel.config.lock().expect("tunel poisoned");
    let mut nova = config.clone();
    mudar(&mut nova);
    omni_protocol::atomic_write_secret_json(&arquivo_de_config(state), &nova)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Não consegui gravar o ajuste do túnel: {e}")))?;
    *config = nova;
    Ok(())
}

/// O Authy está cadastrado e confirmado: sem isso o túnel não liga (e cai, se já estava ligado).
pub(super) fn totp_pronto(state: &EngineState) -> bool {
    let config = state.mobile.config.lock().expect("config poisoned");
    config.totp_confirmed && !config.totp_secret.is_empty()
}

fn definir_estado(state: &EngineState, geracao: u64, mudar: impl FnOnce(&mut Estado)) {
    let tunel = &state.mobile.tunel;
    let mut estado = tunel.estado.lock().expect("tunel poisoned");
    if tunel.geracao.load(Ordering::SeqCst) == geracao { mudar(&mut estado); }
}

/// Liga e desliga o listener e o cloudflared conforme a escolha guardada. Roda desde a subida do
/// engine: ligado antes, volta ligado sozinho (com endereço novo — é o que o quick tunnel dá).
pub(super) fn iniciar(state: Arc<EngineState>) {
    tokio::spawn(async move {
        loop {
            let ligado = state.mobile.tunel.config.lock().expect("tunel poisoned").ligado;
            if !(ligado && totp_pronto(&state)) {
                let erro = ligado.then(|| "Configure o Authy para o túnel ligar.".to_owned());
                *state.mobile.tunel.estado.lock().expect("tunel poisoned") = Estado { erro, ..Estado::default() };
                state.mobile.tunel.mudou.notified().await;
                continue;
            }
            let listener = match TcpListener::bind(("127.0.0.1", 0)).await {
                Ok(listener) => listener,
                Err(e) => {
                    *state.mobile.tunel.estado.lock().expect("tunel poisoned") =
                        Estado { erro: Some(format!("Não consegui abrir a porta do túnel: {e}")), ..Estado::default() };
                    state.mobile.tunel.mudou.notified().await;
                    continue;
                }
            };
            let porta = listener.local_addr().map(|a| a.port()).unwrap_or(0);
            let geracao = {
                let mut estado = state.mobile.tunel.estado.lock().expect("tunel poisoned");
                *estado = Estado { porta: Some(porta), ..Estado::default() };
                state.mobile.tunel.geracao.fetch_add(1, Ordering::SeqCst) + 1
            };
            let supervisor = state.clone();
            std::thread::spawn(move || supervisionar(supervisor, porta, geracao));
            let parar = state.clone();
            let app = router_do_tunel(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
            let _ = axum::serve(listener, app).with_graceful_shutdown(async move {
                parar.mobile.tunel.mudou.notified().await;
                // Derruba o cloudflared já, sem esperar as conexões abertas terminarem.
                let _estado = parar.mobile.tunel.estado.lock().expect("tunel poisoned");
                parar.mobile.tunel.geracao.fetch_add(1, Ordering::SeqCst);
            }).await;
        }
    });
}

pub(super) fn router_do_tunel(state: Arc<EngineState>) -> Router {
    rotas()
        .fallback(get(asset)).layer(DefaultBodyLimit::max(32 * 1024))
        .layer(middleware::from_fn(crate::kanban::somente_leitura_no_http))
        .layer(middleware::from_fn_with_state(state.clone(), porteiro)).with_state(state)
}

/// IP de quem está do outro lado do túnel. O listener só escuta em loopback, e quem fala com ele é o
/// cloudflared, que põe o IP real em `CF-Connecting-IP`. Sem o cabeçalho, todos caem no mesmo balde.
pub(super) fn ip_de_origem(request: &Request) -> String {
    let par = request.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    if par.is_none_or(|ip| ip.is_loopback()) {
        let cabecalho = request.headers().get("cf-connecting-ip").and_then(|v| v.to_str().ok());
        if let Some(ip) = cabecalho.and_then(|v| v.trim().parse::<IpAddr>().ok()) { return ip.to_string(); }
    }
    par.map(|ip| ip.to_string()).unwrap_or_else(|| "sem-ip".into())
}

fn com_cabecalhos(mut response: Response) -> Response {
    cabecalhos_de_seguranca(response.headers_mut());
    response.headers_mut().insert("strict-transport-security", "max-age=31536000".parse().unwrap());
    response
}

fn muitas_tentativas() -> Response {
    error(StatusCode::TOO_MANY_REQUESTS, "Muitas tentativas erradas. Espere alguns minutos e tente de novo.").into_response()
}

async fn porteiro(State(state): State<Arc<EngineState>>, request: Request, next: Next) -> Response {
    let agora = now_ms();
    let ip = ip_de_origem(&request);
    let tunel = &state.mobile.tunel;
    let estado = tunel.estado.lock().expect("tunel poisoned").clone();
    let Some(publico) = estado.url.as_deref().and_then(|u| u.strip_prefix("https://")).map(str::to_owned) else {
        return error(StatusCode::SERVICE_UNAVAILABLE, "O link seguro ainda está ligando.").into_response();
    };
    // O cloudflared repassa o `Host` público; o literal da porta local fica aceito por garantia — um
    // site de fora nunca consegue mandar esse `Host`, então não abre DNS rebinding.
    let host = request.headers().get("host").and_then(|v| v.to_str().ok()).unwrap_or("");
    let local = estado.porta.map(|p| format!("127.0.0.1:{p}"));
    if host != publico && Some(host) != local.as_deref() {
        return error(StatusCode::FORBIDDEN, "Host não permitido").into_response();
    }
    if request.method() != axum::http::Method::GET {
        let origin = request.headers().get("origin").and_then(|v| v.to_str().ok()).unwrap_or("");
        if origin != format!("https://{publico}") {
            return error(StatusCode::FORBIDDEN, "Origem não permitida").into_response();
        }
    }
    let path = request.uri().path().to_owned();
    if path == "/parear" || path == "/parear/pedido" {
        if tunel.limites.lock().expect("limites poisoned").bloqueado(&ip, Falha::Parear, agora) { return com_cabecalhos(muitas_tentativas()); }
        let resposta = if path == "/parear" { pareamento::parear_pelo_tunel(&state, request, &ip, agora).await }
            else { pareamento::situacao_do_pedido(&state, request, agora).await };
        return com_cabecalhos(resposta);
    }
    if tunel.limites.lock().expect("limites poisoned").bloqueado(&ip, Falha::Token, agora) { return com_cabecalhos(muitas_tentativas()); }
    if !caminho_publico(&path) {
        let token = tunel.config.lock().expect("tunel poisoned").token.clone();
        let recebido = request.headers().get("x-omni-token").and_then(|v| v.to_str().ok()).unwrap_or("");
        if token.is_empty() || !token_confere(recebido, &token) {
            // Sem token nenhum é o celular ainda não pareado consultando (ou vários atrás do mesmo
            // IP): não é tentativa de adivinhar, então não conta para a trava.
            if !recebido.is_empty() { tunel.limites.lock().expect("limites poisoned").falhou(&ip, Falha::Token, agora); }
            return com_cabecalhos(error(StatusCode::UNAUTHORIZED, "Aparelho não autorizado; digite o código do Authy").into_response());
        }
    }
    com_cabecalhos(next.run(request).await)
}

/// Liga o túnel (e lembra a escolha). Recusa sem o Authy: exposto à internet, o código é obrigatório.
pub(super) fn ligar(state: &EngineState) -> Result<(), ApiError> {
    if !totp_pronto(state) {
        return Err(error(StatusCode::CONFLICT, "Configure o Authy primeiro: sem ele o link seguro não liga."));
    }
    if localizar(state).is_none() {
        return Err(error(StatusCode::CONFLICT, "Instale o cloudflared primeiro."));
    }
    mudar_ligado(state, true)
}

pub(super) fn desligar(state: &EngineState) -> Result<(), ApiError> { mudar_ligado(state, false) }

fn mudar_ligado(state: &EngineState, ligado: bool) -> Result<(), ApiError> {
    if state.mobile.tunel.config.lock().expect("tunel poisoned").ligado == ligado { return Ok(()); }
    mudar_config(state, |c| c.ligado = ligado)?;
    if !ligado { state.mobile.tunel.estado.lock().expect("tunel poisoned").url = None; }
    state.mobile.tunel.mudou.notify_one();
    Ok(())
}

/// "Trocar o código" e refazer o Authy: todo aparelho pareado pelo túnel precisa parear de novo.
pub(super) fn trocar_token(state: &EngineState) {
    if let Err((_, Json(erro))) = mudar_config(state, |c| c.token = random_token()) {
        eprintln!("túnel: não consegui trocar o token: {}", erro["error"]);
    }
}

/// O Authy mudou (refeito ou confirmado): o túnel reavalia se pode ficar de pé.
pub(super) fn authy_mudou(state: &EngineState) { state.mobile.tunel.mudou.notify_one(); }

pub(super) fn estado_json(state: &EngineState) -> Value {
    let config = state.mobile.tunel.config.lock().expect("tunel poisoned").clone();
    let estado = state.mobile.tunel.estado.lock().expect("tunel poisoned").clone();
    json!({
        "ligado": config.ligado,
        "url": estado.url,
        // Sem token no link: quem lê o QR cai no pareamento (código do Authy + aprovação no PC).
        "qr": estado.url.as_ref().map(|u| format!("{u}/")),
        "erro": estado.erro,
        "totp_confirmed": totp_pronto(state),
        "pareamento_desligado": config.pareamento_desligado,
        "cloudflared": estado_do_cloudflared(state),
    })
}

async fn estado_rota(State(state): State<Arc<EngineState>>) -> Json<Value> { Json(estado_json(&state)) }

async fn ligar_rota(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    ligar(&state)?;
    Ok(Json(estado_json(&state)))
}

async fn desligar_rota(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    desligar(&state)?;
    Ok(Json(estado_json(&state)))
}

async fn instalar_rota(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    processo::instalar(&state)?;
    Ok(Json(estado_json(&state)))
}

/// Rotas **só do desktop** (`mobile::local`): nunca entram no servidor HTTP do celular nem no do túnel.
pub(super) fn rotas_locais() -> Router<Arc<EngineState>> {
    Router::new()
        .route("/tunel", get(estado_rota))
        .route("/tunel/ligar", post(ligar_rota))
        .route("/tunel/desligar", post(desligar_rota))
        .route("/tunel/instalar", post(instalar_rota))
        .merge(pareamento::rotas_locais())
}

#[cfg(test)]
pub(super) fn definir_url_para_teste(state: &EngineState, url: &str, porta: u16) {
    *state.mobile.tunel.estado.lock().unwrap() = Estado { url: Some(url.into()), erro: None, porta: Some(porta) };
}

#[cfg(test)]
#[path = "tunel_testes.rs"]
mod testes;
