//! Pareamento pelo link seguro: código do Authy **e** aprovação no PC. Filho de `tunel`.
//!
//! O código sozinho não basta porque o link fica na internet: força bruta lenta, com muitos IPs,
//! acaba acertando um código de 6 dígitos. Com a aprovação, acertar o código só faz aparecer um
//! pedido na tela do dono, que recusa. Código errado ainda conta nas travas — por IP e global, com
//! espera que dobra a cada pausa e um teto diário que desliga o pareamento até o dono reativar.

use super::*;

/// Quanto tempo o PC tem para aprovar; depois disso o celular digita um código novo.
pub(in crate::mobile) const PEDIDO_VALE_MS: u64 = 2 * 60_000;
/// Pedido resolvido fica guardado este tempo para o celular ler a resposta.
const PEDIDO_GUARDA_MS: u64 = 10 * 60_000;
const PEDIDOS_MAX: usize = 5;
const APARELHO_MAX: usize = 300;

/// Erros globais tolerados na janela antes de pausar o pareamento pelo túnel.
pub(in crate::mobile) const GLOBAL_FALHAS: usize = 5;
const GLOBAL_JANELA_MS: u64 = 5 * 60_000;
/// Primeira pausa; cada pausa seguida dobra (5 min, 10, 20…), até um dia.
pub(in crate::mobile) const ESPERA_BASE_MS: u64 = 5 * 60_000;
const ESPERA_MAX_MS: u64 = DIA_MS;
/// Erros num dia que desligam o pareamento pelo túnel até o dono reativar no PC.
pub(in crate::mobile) const TETO_DIA: usize = 20;
const DIA_MS: u64 = 24 * 60 * 60_000;

#[derive(Debug, PartialEq)]
pub(in crate::mobile) enum Efeito { Nada, Pausou(u64), Desligou }

#[derive(Default)]
pub(in crate::mobile) struct TravaGlobal { falhas: VecDeque<u64>, no_dia: VecDeque<u64>, pausas: u32, ate: u64 }

impl TravaGlobal {
    #[cfg(test)]
    pub(in crate::mobile) fn sem_pausa_para_teste(&mut self) { self.ate = 0; }

    pub(in crate::mobile) fn pausado_ate(&self, agora_ms: u64) -> Option<u64> { (self.ate > agora_ms).then_some(self.ate) }

    pub(in crate::mobile) fn falhou(&mut self, agora_ms: u64) -> Efeito {
        self.no_dia.retain(|t| agora_ms.saturating_sub(*t) < DIA_MS);
        // Um dia inteiro sem erro: a espera volta a ser a curta.
        if self.no_dia.is_empty() { self.pausas = 0; }
        self.no_dia.push_back(agora_ms);
        if self.no_dia.len() >= TETO_DIA {
            *self = Self::default();
            return Efeito::Desligou;
        }
        self.falhas.retain(|t| agora_ms.saturating_sub(*t) < GLOBAL_JANELA_MS);
        self.falhas.push_back(agora_ms);
        if self.falhas.len() < GLOBAL_FALHAS { return Efeito::Nada; }
        self.falhas.clear();
        let espera = ESPERA_BASE_MS.saturating_mul(1 << self.pausas.min(20)).min(ESPERA_MAX_MS);
        self.pausas += 1;
        self.ate = agora_ms + espera;
        Efeito::Pausou(self.ate)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::mobile) enum Situacao { Aguardando, Aprovado, Recusado }

#[derive(Clone)]
pub(in crate::mobile) struct Pedido {
    /// Id curto, para o desktop. Não dá acesso a nada.
    pub(in crate::mobile) id: String,
    /// Segredo do celular para perguntar a resposta. Só ele recebe o token.
    segredo: String,
    aparelho: String,
    ip: String,
    pub(in crate::mobile) criado_ms: u64,
    pub(in crate::mobile) situacao: Situacao,
}

impl Pedido {
    fn expirou(&self, agora_ms: u64) -> bool {
        self.situacao == Situacao::Aguardando && agora_ms.saturating_sub(self.criado_ms) >= PEDIDO_VALE_MS
    }
}

#[derive(Default)]
pub(in crate::mobile) struct Pareamento {
    pub(in crate::mobile) trava: TravaGlobal,
    pub(in crate::mobile) pedidos: Vec<Pedido>,
    /// O que avisar no desktop: pausa ou desligamento por excesso de erros. `(quando, texto)`.
    alerta: Option<(u64, String)>,
}

impl Pareamento {
    fn limpar(&mut self, agora_ms: u64) {
        self.pedidos.retain(|p| agora_ms.saturating_sub(p.criado_ms) < PEDIDO_GUARDA_MS);
    }
}

/// Texto do `User-Agent` para o dono reconhecer o aparelho. Só caracteres visíveis, tamanho limitado.
fn aparelho_de(headers: &HeaderMap) -> String {
    let bruto = headers.get("user-agent").and_then(|v| v.to_str().ok()).unwrap_or("");
    let limpo: String = bruto.chars().filter(|c| !c.is_control()).take(APARELHO_MAX).collect();
    if limpo.trim().is_empty() { "aparelho sem identificação".into() } else { limpo }
}

async fn corpo<T: serde::de::DeserializeOwned>(request: Request) -> Option<T> {
    let bytes = axum::body::to_bytes(request.into_body(), 1024).await.ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// `POST /parear` pelo túnel: código certo vira um pedido esperando o dono aprovar no PC.
pub(super) async fn parear_pelo_tunel(state: &Arc<EngineState>, request: Request, ip: &str, agora_ms: u64) -> Response {
    if request.method() != axum::http::Method::POST { return StatusCode::METHOD_NOT_ALLOWED.into_response(); }
    let aparelho = aparelho_de(request.headers());
    let Some(body) = corpo::<PareamentoBody>(request).await else {
        return error(StatusCode::BAD_REQUEST, "Pedido inválido").into_response();
    };
    let tunel = &state.mobile.tunel;
    if tunel.config.lock().expect("tunel poisoned").pareamento_desligado {
        return error(StatusCode::LOCKED, "O pareamento pelo link seguro foi desligado depois de muitos códigos errados. Reative no PC, em Configurações → Celular.").into_response();
    }
    let config = state.mobile.config.lock().expect("config poisoned").clone();
    if !config.totp_confirmed || config.totp_secret.is_empty() {
        return error(StatusCode::CONFLICT, "Configure o Authy na aba Celular do PC primeiro").into_response();
    }
    if tunel.pareamento.lock().expect("pareamento poisoned").trava.pausado_ate(agora_ms).is_some() {
        return error(StatusCode::TOO_MANY_REQUESTS, "Muitos códigos errados. O pareamento está pausado; tente mais tarde.").into_response();
    }
    if !codigo_novo(state, &config, &body.codigo, agora_ms / 1000) {
        registrar_erro(state, ip, agora_ms);
        return error(StatusCode::UNAUTHORIZED, "Código do Authy não confere. Use o código que está aparecendo agora.").into_response();
    }
    let mut pareamento = tunel.pareamento.lock().expect("pareamento poisoned");
    pareamento.limpar(agora_ms);
    let esperando = pareamento.pedidos.iter().filter(|p| p.situacao == Situacao::Aguardando && !p.expirou(agora_ms)).count();
    if esperando >= PEDIDOS_MAX {
        return error(StatusCode::TOO_MANY_REQUESTS, "Já há pedidos esperando aprovação no PC.").into_response();
    }
    let segredo = random_token();
    pareamento.pedidos.push(Pedido { id: random_token()[..12].to_owned(), segredo: segredo.clone(), aparelho,
        ip: ip.to_owned(), criado_ms: agora_ms, situacao: Situacao::Aguardando });
    drop(pareamento);
    (StatusCode::ACCEPTED, Json(json!({"pedido": segredo, "expira_em_s": PEDIDO_VALE_MS / 1000}))).into_response()
}

fn registrar_erro(state: &EngineState, ip: &str, agora_ms: u64) {
    let tunel = &state.mobile.tunel;
    tunel.limites.lock().expect("limites poisoned").falhou(ip, Falha::Parear, agora_ms);
    let mut pareamento = tunel.pareamento.lock().expect("pareamento poisoned");
    match pareamento.trava.falhou(agora_ms) {
        Efeito::Nada => {}
        Efeito::Pausou(ate) => {
            let minutos = ate.saturating_sub(agora_ms).div_ceil(60_000);
            pareamento.alerta = Some((agora_ms, format!("Muitos códigos do Authy errados pelo link seguro. Novos aparelhos ficam bloqueados por {minutos} min.")));
        }
        Efeito::Desligou => {
            pareamento.alerta = Some((agora_ms, "Códigos do Authy errados demais hoje pelo link seguro: o pareamento de aparelhos novos foi desligado. Os aparelhos já conectados continuam funcionando.".into()));
            drop(pareamento);
            let _ = mudar_config(state, |c| c.pareamento_desligado = true);
        }
    }
}

#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct PedidoBody { pedido: String }

/// `POST /parear/pedido` pelo túnel: o celular pergunta se o dono já respondeu.
pub(super) async fn situacao_do_pedido(state: &Arc<EngineState>, request: Request, agora_ms: u64) -> Response {
    if request.method() != axum::http::Method::POST { return StatusCode::METHOD_NOT_ALLOWED.into_response(); }
    let Some(body) = corpo::<PedidoBody>(request).await else {
        return error(StatusCode::BAD_REQUEST, "Pedido inválido").into_response();
    };
    let tunel = &state.mobile.tunel;
    let mut pareamento = tunel.pareamento.lock().expect("pareamento poisoned");
    pareamento.limpar(agora_ms);
    let Some(i) = pareamento.pedidos.iter().position(|p| token_confere(&body.pedido, &p.segredo)) else {
        return error(StatusCode::NOT_FOUND, "Pedido não encontrado. Digite um código novo.").into_response();
    };
    let pedido = pareamento.pedidos[i].clone();
    if pedido.expirou(agora_ms) {
        pareamento.pedidos.remove(i);
        return error(StatusCode::GONE, "O pedido expirou sem aprovação. Digite um código novo.").into_response();
    }
    match pedido.situacao {
        Situacao::Aguardando => (StatusCode::ACCEPTED, Json(json!({"estado": "aguardando"}))).into_response(),
        Situacao::Recusado => {
            pareamento.pedidos.remove(i);
            error(StatusCode::FORBIDDEN, "O pedido foi recusado no computador.").into_response()
        }
        Situacao::Aprovado => {
            pareamento.pedidos.remove(i);
            drop(pareamento);
            Json(json!({"token": tunel.config.lock().expect("tunel poisoned").token.clone()})).into_response()
        }
    }
}

/// Lista para o desktop: o que espera resposta e o alerta de excesso de erros.
pub(super) fn pedidos_json(state: &EngineState, agora_ms: u64) -> Value {
    let desligado = state.mobile.tunel.config.lock().expect("tunel poisoned").pareamento_desligado;
    let mut pareamento = state.mobile.tunel.pareamento.lock().expect("pareamento poisoned");
    pareamento.limpar(agora_ms);
    let pedidos: Vec<Value> = pareamento.pedidos.iter()
        .filter(|p| p.situacao == Situacao::Aguardando && !p.expirou(agora_ms))
        .map(|p| json!({"id": p.id, "aparelho": p.aparelho, "ip": p.ip, "em_ms": p.criado_ms, "expira_em_ms": p.criado_ms + PEDIDO_VALE_MS}))
        .collect();
    json!({
        "pedidos": pedidos,
        "pareamento_desligado": desligado,
        "pausado_ate_ms": pareamento.trava.pausado_ate(agora_ms),
        "alerta": pareamento.alerta.as_ref().map(|(em, texto)| json!({"em_ms": em, "texto": texto})),
    })
}

/// O dono respondeu no PC. Só pedido ainda esperando e dentro do prazo.
pub(in crate::mobile) fn responder(state: &EngineState, id: &str, aprovar: bool, agora_ms: u64) -> Result<(), ApiError> {
    let mut pareamento = state.mobile.tunel.pareamento.lock().expect("pareamento poisoned");
    pareamento.limpar(agora_ms);
    let Some(pedido) = pareamento.pedidos.iter_mut().find(|p| p.id == id && p.situacao == Situacao::Aguardando) else {
        return Err(error(StatusCode::NOT_FOUND, "Esse pedido não está mais esperando."));
    };
    if pedido.expirou(agora_ms) { return Err(error(StatusCode::GONE, "O pedido expirou; peça para digitar um código novo.")); }
    pedido.situacao = if aprovar { Situacao::Aprovado } else { Situacao::Recusado };
    // Aparelho aprovado pelo dono: a espera crescente volta ao começo.
    if aprovar { pareamento.trava = TravaGlobal::default(); }
    Ok(())
}

pub(in crate::mobile) fn reativar(state: &EngineState) -> Result<(), ApiError> {
    {
        let mut pareamento = state.mobile.tunel.pareamento.lock().expect("pareamento poisoned");
        pareamento.trava = TravaGlobal::default();
        pareamento.alerta = None;
    }
    mudar_config(state, |c| c.pareamento_desligado = false)
}

async fn pedidos_rota(State(state): State<Arc<EngineState>>) -> Json<Value> { Json(pedidos_json(&state, now_ms())) }

async fn responder_rota(State(state): State<Arc<EngineState>>, RoutePath((id, acao)): RoutePath<(String, String)>) -> Result<Json<Value>, ApiError> {
    let aprovar = match acao.as_str() {
        "permitir" => true,
        "recusar" => false,
        _ => return Err(error(StatusCode::NOT_FOUND, "Ação desconhecida")),
    };
    responder(&state, &id, aprovar, now_ms())?;
    Ok(Json(pedidos_json(&state, now_ms())))
}

async fn reativar_rota(State(state): State<Arc<EngineState>>) -> Result<Json<Value>, ApiError> {
    reativar(&state)?;
    Ok(Json(pedidos_json(&state, now_ms())))
}

async fn alerta_visto(State(state): State<Arc<EngineState>>) -> Json<Value> {
    state.mobile.tunel.pareamento.lock().expect("pareamento poisoned").alerta = None;
    Json(pedidos_json(&state, now_ms()))
}

/// Rotas só do desktop (entram em `tunel::rotas_locais`).
pub(super) fn rotas_locais() -> Router<Arc<EngineState>> {
    Router::new()
        .route("/tunel/pedidos", get(pedidos_rota))
        .route("/tunel/pedidos/{id}/{acao}", post(responder_rota))
        .route("/tunel/pareamento/reativar", post(reativar_rota))
        .route("/tunel/alerta/visto", post(alerta_visto))
}
