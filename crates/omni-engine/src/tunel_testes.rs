//! Testes do link seguro. Nenhum cloudflared de verdade roda aqui e nenhum túnel é aberto: o endereço
//! público é plantado com `definir_url_para_teste` e o roteador é chamado direto (`oneshot`).

use super::*;
use crate::mobile::tests::fixture;
use axum::body::{to_bytes, Body};
use tower::ServiceExt;

const URL: &str = "https://palavras-de-teste.trycloudflare.com";
const HOST: &str = "palavras-de-teste.trycloudflare.com";
const SEGREDO: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

fn com_tunel() -> (tempfile::TempDir, Arc<EngineState>) {
    let (dir, state) = fixture();
    {
        let mut config = state.mobile.config.lock().unwrap();
        config.token = "token-do-tailscale".into();
        config.totp_secret = SEGREDO.into();
        config.totp_confirmed = true;
    }
    state.mobile.tunel.config.lock().unwrap().token = "token-do-tunel".into();
    definir_url_para_teste(&state, URL, 41234);
    (dir, state)
}

fn pedido(metodo: &str, uri: &str, token: Option<&str>, ip: &str) -> axum::http::Request<Body> {
    let mut r = axum::http::Request::builder().method(metodo).uri(uri).header("host", HOST)
        .header("origin", URL).header("content-type", "application/json").header("cf-connecting-ip", ip);
    if let Some(token) = token { r = r.header("x-omni-token", token); }
    let mut r = r.body(Body::empty()).unwrap();
    // Como chega de verdade: tudo vem do cloudflared, em loopback.
    r.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50_000))));
    r
}

fn parear(codigo: &str, ip: &str) -> axum::http::Request<Body> {
    let mut r = pedido("POST", "/parear", None, ip);
    *r.body_mut() = Body::from(json!({"codigo": codigo}).to_string());
    r
}

fn codigo_agora() -> String { totp_de(SEGREDO, None).unwrap().generate(agora_segundos()) }

/// Saída real do `cloudflared tunnel --url` (2024–2026): a caixa com o endereço vem no stderr.
const SAIDA_REAL: &str = "\
2026-11-09T12:00:00Z INF Thank you for trying Cloudflare Tunnel. Doing so, without a Cloudflare account, is a quick way to experiment and try it out.
2026-11-09T12:00:00Z INF Requesting new quick Tunnel on trycloudflare.com...
2026-11-09T12:00:01Z INF +--------------------------------------------------------------------------------------------+
2026-11-09T12:00:01Z INF |  Your quick Tunnel has been created! Visit it at (it may take some time to be reachable):  |
2026-11-09T12:00:01Z INF |  https://seasonal-deck-organisms-sf.trycloudflare.com                                      |
2026-11-09T12:00:01Z INF +--------------------------------------------------------------------------------------------+
2026-11-09T12:00:01Z INF Cannot determine default configuration path. No file [config.yml config.yaml] in [~/.cloudflared ~/.cloudflare-warp ~/cloudflare-warp /etc/cloudflared /usr/local/etc/cloudflared]
2026-11-09T12:00:02Z INF Registered tunnel connection connIndex=0 connection=8f1c location=gru01 protocol=quic";

#[test]
fn acha_o_endereco_na_saida_real_do_cloudflared() {
    let achados: Vec<String> = SAIDA_REAL.lines().filter_map(url_da_saida).collect();
    assert_eq!(achados, ["https://seasonal-deck-organisms-sf.trycloudflare.com"]);
}

#[test]
fn ignora_o_que_nao_e_o_endereco_do_tunel() {
    // Erro real quando a criação falha: cita a API da Cloudflare, que não é o túnel.
    let erro = r#"2026-11-09T12:00:00Z ERR Error unmarshaling QuickTunnel response: error="failed to request quick Tunnel: Post \"https://api.trycloudflare.com/tunnel\": dial tcp: lookup api.trycloudflare.com: no such host""#;
    assert_eq!(url_da_saida(erro), None);
    assert_eq!(url_da_saida("INF Requesting new quick Tunnel on trycloudflare.com..."), None);
    assert_eq!(url_da_saida("https://a.b.trycloudflare.com"), None, "um nível só de subdomínio");
    assert_eq!(url_da_saida("https://exemplo.com.trycloudflare.com.evil.io"), None);
    assert_eq!(url_da_saida("http://sem-https.trycloudflare.com"), None);
    assert_eq!(url_da_saida("|  https://Maiusculas-OK.trycloudflare.com  |").as_deref(), Some("https://maiusculas-ok.trycloudflare.com"));
}

#[test]
fn comando_do_cloudflared_aponta_so_para_loopback() {
    assert_eq!(argumentos(41234), ["tunnel", "--url", "http://127.0.0.1:41234", "--no-autoupdate"]);
}

#[test]
fn tunel_nao_liga_sem_authy() {
    let (dir, state) = fixture();
    let erro = ligar(&state).unwrap_err();
    assert_eq!(erro.0, StatusCode::CONFLICT);
    assert!(erro.1 .0["error"].as_str().unwrap().contains("Authy"));
    assert!(!state.mobile.tunel.config.lock().unwrap().ligado);
    assert!(!dir.path().join("tunel.json").exists(), "a escolha não pode ficar gravada");

    // Segredo gerado mas ainda não confirmado no PC também não basta.
    state.mobile.config.lock().unwrap().totp_secret = SEGREDO.into();
    assert_eq!(ligar(&state).unwrap_err().0, StatusCode::CONFLICT);
    assert!(!totp_pronto(&state));
}

#[test]
fn escolha_de_ligar_e_lembrada_no_disco() {
    let (dir, state) = com_tunel();
    mudar_ligado(&state, true).unwrap();
    let gravado: TunelConfig = omni_protocol::read_json_or_default(&dir.path().join("tunel.json"));
    assert!(gravado.ligado);
    assert_eq!(gravado.token, "token-do-tunel");
    let de_novo = Tunel::new(dir.path());
    assert!(de_novo.config.lock().unwrap().ligado, "o engine reaberto liga o túnel sozinho");
    desligar(&state).unwrap();
    let gravado: TunelConfig = omni_protocol::read_json_or_default(&dir.path().join("tunel.json"));
    assert!(!gravado.ligado);
}

#[tokio::test]
async fn tunel_exige_o_token_dele_e_nao_confia_em_loopback() {
    let (_dir, state) = com_tunel();
    let app = router_do_tunel(state.clone());
    // Sem token, vindo de 127.0.0.1 (como tudo que o cloudflared entrega): recusa.
    let r = app.clone().oneshot(pedido("GET", "/conversas", None, "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    // Sem cabeçalho do cloudflared nenhum, ainda em loopback: recusa igual.
    let mut r = pedido("GET", "/conversas", None, "x");
    r.headers_mut().remove("cf-connecting-ip");
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), StatusCode::UNAUTHORIZED);
    // O token do QR do Tailscale não vale aqui: só o do túnel, que sai do pareamento com o Authy.
    let r = app.clone().oneshot(pedido("GET", "/conversas", Some("token-do-tailscale"), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = app.clone().oneshot(pedido("GET", "/conversas", Some("token-do-tunel"), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["strict-transport-security"], "max-age=31536000");
    assert!(r.headers().contains_key("content-security-policy"));
}

#[tokio::test]
async fn tunel_confere_host_e_origem() {
    let (_dir, state) = com_tunel();
    let app = router_do_tunel(state.clone());
    let mut r = pedido("GET", "/conversas", Some("token-do-tunel"), "203.0.113.7");
    r.headers_mut().insert("host", "outro.trycloudflare.com".parse().unwrap());
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), StatusCode::FORBIDDEN);
    let mut r = pedido("POST", "/conversas/c/nome", Some("token-do-tunel"), "203.0.113.7");
    r.headers_mut().insert("origin", "https://site-malicioso.example".parse().unwrap());
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), StatusCode::FORBIDDEN);
    let mut r = pedido("POST", "/conversas/c/nome", Some("token-do-tunel"), "203.0.113.7");
    r.headers_mut().remove("origin");
    assert_eq!(app.clone().oneshot(r).await.unwrap().status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tunel_sem_endereco_publico_nao_atende() {
    let (_dir, state) = com_tunel();
    *state.mobile.tunel.estado.lock().unwrap() = Estado::default();
    let r = router_do_tunel(state).oneshot(pedido("GET", "/conversas", Some("token-do-tunel"), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn kanban_continua_so_leitura_pelo_tunel() {
    let (_dir, state) = com_tunel();
    let r = router_do_tunel(state).oneshot(pedido("POST", "/kanban/cartoes/a/pegar", Some("token-do-tunel"), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn pareamento_pelo_tunel_entrega_o_token_do_tunel() {
    let (_dir, state) = com_tunel();
    let app = router_do_tunel(state.clone());
    let r = app.clone().oneshot(parear(&codigo_agora(), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let corpo: Value = serde_json::from_slice(&to_bytes(r.into_body(), 4096).await.unwrap()).unwrap();
    assert_eq!(corpo["token"], "token-do-tunel", "nunca o token do Tailscale");
}

#[tokio::test]
async fn pareamento_pelo_tunel_sem_authy_confirmado_recusa() {
    let (_dir, state) = com_tunel();
    state.mobile.config.lock().unwrap().totp_confirmed = false;
    let r = router_do_tunel(state).oneshot(parear(&codigo_agora(), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn codigo_errado_trava_o_ip_de_origem() {
    let (_dir, state) = com_tunel();
    let app = router_do_tunel(state.clone());
    for _ in 0..FALHAS_PAREAR_POR_IP {
        let r = app.clone().oneshot(parear("000000", "203.0.113.7")).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }
    // Travado: nem o código certo, nem o token certo passam deste IP.
    let r = app.clone().oneshot(parear(&codigo_agora(), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    let r = app.clone().oneshot(pedido("GET", "/conversas", Some("token-do-tunel"), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn token_errado_trava_so_aquele_ip() {
    let (_dir, state) = com_tunel();
    let app = router_do_tunel(state.clone());
    for _ in 0..FALHAS_TOKEN_POR_IP {
        let r = app.clone().oneshot(pedido("GET", "/conversas", Some("chute"), "198.51.100.9")).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }
    let r = app.clone().oneshot(pedido("GET", "/conversas", Some("token-do-tunel"), "198.51.100.9")).await.unwrap();
    assert_eq!(r.status(), StatusCode::TOO_MANY_REQUESTS);
    let r = app.clone().oneshot(pedido("GET", "/conversas", Some("token-do-tunel"), "203.0.113.7")).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK, "outro IP segue entrando");
}

#[test]
fn trava_por_ip_acaba_sozinha() {
    let mut limites = Limites::default();
    for _ in 0..FALHAS_PAREAR_POR_IP { limites.falhou("1.2.3.4", Falha::Parear, 1_000); }
    assert!(limites.bloqueado("1.2.3.4", 1_000));
    assert!(!limites.bloqueado("5.6.7.8", 1_000));
    assert!(!limites.bloqueado("1.2.3.4", 1_000 + BLOQUEIO_MS));
    // Falhas espalhadas além da janela não somam.
    let mut limites = Limites::default();
    for i in 0..FALHAS_PAREAR_POR_IP as u64 { limites.falhou("1.2.3.4", Falha::Parear, i * JANELA_MS); }
    assert!(!limites.bloqueado("1.2.3.4", FALHAS_PAREAR_POR_IP as u64 * JANELA_MS));
}

#[test]
fn ip_de_origem_so_le_o_cabecalho_vindo_de_loopback() {
    let mut r = pedido("GET", "/", None, "203.0.113.7");
    assert_eq!(ip_de_origem(&r), "203.0.113.7");
    r.extensions_mut().insert(ConnectInfo(SocketAddr::from(([192, 168, 0, 9], 1))));
    assert_eq!(ip_de_origem(&r), "192.168.0.9", "de fora do loopback o cabeçalho é ignorado");
    let mut r = pedido("GET", "/", None, "não-é-ip");
    assert_eq!(ip_de_origem(&r), "127.0.0.1");
    r.extensions_mut().remove::<ConnectInfo<SocketAddr>>();
    assert_eq!(ip_de_origem(&r), "sem-ip");
}

#[tokio::test]
async fn rotas_do_tunel_so_existem_para_o_desktop() {
    let (_dir, state) = com_tunel();
    let lan = |metodo: &str, uri: &str| axum::http::Request::builder().method(metodo).uri(uri)
        .header("host", "127.0.0.1:47322").header("origin", "http://127.0.0.1:47322")
        .header("x-omni-token", "token-do-tailscale").body(Body::empty()).unwrap();
    for uri in ["/tunel/ligar", "/tunel/desligar", "/tunel/instalar"] {
        let r = router(state.clone()).oneshot(lan("POST", uri)).await.unwrap();
        assert!(!r.status().is_success(), "{uri} pelo celular: {}", r.status());
        let r = router_do_tunel(state.clone()).oneshot(pedido("POST", uri, Some("token-do-tunel"), "203.0.113.7")).await.unwrap();
        assert!(!r.status().is_success(), "{uri} pelo túnel: {}", r.status());
    }
    let EngineResponse::Api { status, body } = local(state.clone(), "GET", "/tunel", None, None, vec![]).await else { panic!() };
    assert_eq!(status, 200);
    let estado: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(estado["qr"], format!("{URL}/"), "o QR do túnel não leva token");
    assert!(!body.contains("token-do-tunel") && !body.contains("token-do-tailscale"));
    assert_eq!(estado["totp_confirmed"], true);
}

#[test]
fn trocar_o_codigo_derruba_quem_pareou_pelo_tunel() {
    let (_dir, state) = com_tunel();
    settings(&state, Some(MobileConfig::default()), true).unwrap();
    let token = state.mobile.tunel.config.lock().unwrap().token.clone();
    assert_ne!(token, "token-do-tunel");
    assert_eq!(token.len(), 64);
}

#[test]
fn refazer_o_authy_troca_o_token_do_tunel() {
    let (_dir, state) = com_tunel();
    totp(&state, TotpAction::Reset).unwrap();
    assert_ne!(state.mobile.tunel.config.lock().unwrap().token, "token-do-tunel");
    assert!(!totp_pronto(&state));
}

#[test]
fn download_confere_tamanho_e_sha256() {
    let dir = tempfile::tempdir().unwrap();
    let arquivo = dir.path().join("x");
    std::fs::write(&arquivo, b"abc").unwrap();
    let certo = Pacote { arquivo: "x", bytes: 3, tgz: false,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" };
    assert!(confere(&arquivo, &certo));
    let errado = Pacote { sha256: "00", ..certo };
    assert!(!confere(&arquivo, &errado));
    assert!(!confere(&arquivo, &Pacote { bytes: 4, ..certo }));
}

#[test]
fn localiza_primeiro_no_path_depois_na_pasta_do_app() {
    let path = tempfile::tempdir().unwrap();
    let app = tempfile::tempdir().unwrap();
    assert_eq!(localizar_em(Some(path.path().into()), app.path()), None);
    std::fs::write(app.path().join(nome_do_binario()), b"").unwrap();
    assert_eq!(localizar_em(Some(path.path().into()), app.path()), Some(app.path().join(nome_do_binario())));
    std::fs::write(path.path().join(nome_do_binario()), b"").unwrap();
    assert_eq!(localizar_em(Some(path.path().into()), app.path()), Some(path.path().join(nome_do_binario())));
}

#[test]
fn ha_download_para_este_sistema() {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(pacote_do_sistema().map(|p| p.arquivo), Some("cloudflared-linux-amd64"));
    }
}
