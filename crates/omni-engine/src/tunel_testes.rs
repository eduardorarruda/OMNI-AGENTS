//! Testes do link seguro. Nenhum cloudflared de verdade roda aqui e nenhum túnel é aberto: o endereço
//! público é plantado com `definir_url_para_teste` e o roteador é chamado direto (`oneshot`).

use super::pareamento::*;
use super::processo::*;
use super::*;
use crate::mobile::tests::fixture;
use std::ffi::OsString;
use axum::body::{to_bytes, Body};
use tower::ServiceExt;

const URL: &str = "https://palavras-de-teste.trycloudflare.com";
const HOST: &str = "palavras-de-teste.trycloudflare.com";
const SEGREDO: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
const IP: &str = "203.0.113.7";

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
        .header("origin", URL).header("content-type", "application/json").header("cf-connecting-ip", ip)
        .header("user-agent", "Mozilla/5.0 (Linux; Android 15; Pixel 9) Chrome/131.0 Mobile");
    if let Some(token) = token { r = r.header("x-omni-token", token); }
    let mut r = r.body(Body::empty()).unwrap();
    // Como chega de verdade: tudo vem do cloudflared, em loopback.
    r.extensions_mut().insert(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 50_000))));
    r
}

fn com_corpo(uri: &str, corpo: Value, ip: &str) -> axum::http::Request<Body> {
    let mut r = pedido("POST", uri, None, ip);
    *r.body_mut() = Body::from(corpo.to_string());
    r
}

fn parear(codigo: &str, ip: &str) -> axum::http::Request<Body> { com_corpo("/parear", json!({"codigo": codigo}), ip) }

/// Código de um passo TOTP relativo ao atual (a janela aceita ±1): dois pareamentos no mesmo teste
/// precisam de passos diferentes, porque o mesmo passo não vale duas vezes.
fn codigo(delta: i64) -> String {
    let passo = (agora_segundos() / 30) as i64 + delta;
    totp_de(SEGREDO, None).unwrap().generate(passo as u64 * 30)
}

async fn responder_json(r: Response) -> (StatusCode, Value) {
    let status = r.status();
    let corpo = to_bytes(r.into_body(), 64 * 1024).await.unwrap();
    (status, serde_json::from_slice(&corpo).unwrap_or(Value::Null))
}

async fn chamar(state: &Arc<EngineState>, r: axum::http::Request<Body>) -> (StatusCode, Value) {
    responder_json(router_do_tunel(state.clone()).oneshot(r).await.unwrap()).await
}

/// Celular manda o código certo e recebe o segredo do pedido.
async fn pedir(state: &Arc<EngineState>, delta: i64) -> String {
    let (status, corpo) = chamar(state, parear(&codigo(delta), IP)).await;
    assert_eq!(status, StatusCode::ACCEPTED, "{corpo}");
    assert!(corpo.get("token").is_none(), "o código sozinho não entrega token");
    corpo["pedido"].as_str().unwrap().to_owned()
}

fn id_no_desktop(state: &EngineState) -> String {
    let lista = pedidos_json(state, now_ms());
    lista["pedidos"][0]["id"].as_str().expect("pedido aparece no PC").to_owned()
}

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
fn so_a_primeira_url_de_cada_processo_vale() {
    let mut leitura = Leitura::default();
    let achadas: Vec<String> = SAIDA_REAL.lines().chain(["INF |  https://outra-coisa.trycloudflare.com  |"])
        .filter_map(|l| leitura.linha(l)).collect();
    assert_eq!(achadas, ["https://seasonal-deck-organisms-sf.trycloudflare.com"]);
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
fn cloudflared_roda_com_config_vazia_e_so_aponta_para_loopback() {
    let config = Path::new("/dados/cloudflared-vazio.yml");
    let args: Vec<String> = argumentos(41234, config).into_iter().map(|a| a.to_string_lossy().into_owned()).collect();
    assert_eq!(args, ["tunnel", "--config", "/dados/cloudflared-vazio.yml", "--url", "http://127.0.0.1:41234", "--no-autoupdate"]);
}

#[test]
fn ambiente_do_cloudflared_sai_sem_tunnel() {
    let vars = ["PATH", "TUNNEL_TOKEN", "tunnel_url", "HOME", "TUNNEL_ORIGIN_CERT"].map(OsString::from);
    assert_eq!(variaveis_a_tirar(vars.into_iter()), ["TUNNEL_TOKEN", "tunnel_url", "TUNNEL_ORIGIN_CERT"].map(OsString::from));
}

#[test]
fn so_mata_o_cloudflared_deste_app() {
    let config = Path::new("/home/x/.local/share/omni/engine/cloudflared-vazio.yml");
    assert!(e_o_nosso("/usr/bin/cloudflared tunnel --config /home/x/.local/share/omni/engine/cloudflared-vazio.yml --url http://127.0.0.1:1", config));
    assert!(!e_o_nosso("/usr/bin/cloudflared tunnel run meu-tunel", config), "o túnel da pessoa não é tocado");
    assert!(!e_o_nosso("vim /home/x/.local/share/omni/engine/cloudflared-vazio.yml", config));
    assert!(!e_o_nosso("", config));
}

#[test]
fn path_relativo_ou_vazio_e_ignorado() {
    let pasta = tempfile::tempdir().unwrap();
    let nome = nome_do_binario();
    std::fs::write(pasta.path().join(&nome), b"").unwrap();
    // Entrada relativa que existe de verdade a partir da pasta atual do processo de teste.
    let atual = std::env::current_dir().unwrap();
    let relativa = pathdiff(pasta.path(), &atual);
    let path = std::env::join_paths([PathBuf::new(), relativa.clone()]).unwrap();
    assert!(atual.join(&relativa).join(&nome).is_file(), "o arquivo existe pelo caminho relativo");
    assert_eq!(no_path("cloudflared", Some(path)), None);
    let absoluto = std::env::join_paths([PathBuf::from("rel"), pasta.path().to_path_buf()]).unwrap();
    assert_eq!(no_path("cloudflared", Some(absoluto)), Some(pasta.path().join(&nome)));
}

/// Caminho relativo de `alvo` a partir de `base` (`../../tmp/x`), sem dependência nova.
fn pathdiff(alvo: &Path, base: &Path) -> PathBuf {
    let comum = alvo.components().zip(base.components()).take_while(|(a, b)| a == b).count();
    let subir = base.components().count() - comum;
    std::iter::repeat_n(std::path::Component::ParentDir.as_os_str(), subir)
        .chain(alvo.components().skip(comum).map(|c| c.as_os_str())).collect()
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
fn download_confere_tamanho_e_sha256() {
    let dir = tempfile::tempdir().unwrap();
    let arquivo = dir.path().join("x");
    std::fs::write(&arquivo, b"abc").unwrap();
    let certo = Pacote { arquivo: "x", bytes: 3, tgz: false,
        sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad" };
    assert!(confere(&arquivo, &certo));
    assert!(!confere(&arquivo, &Pacote { sha256: "00", ..certo }));
    assert!(!confere(&arquivo, &Pacote { bytes: 4, ..certo }));
}

#[test]
fn ha_download_para_este_sistema() {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        assert_eq!(pacote_do_sistema().map(|p| p.arquivo), Some("cloudflared-linux-amd64"));
    }
}

#[test]
fn tunel_nao_liga_sem_authy() {
    let (dir, state) = fixture();
    let erro = ligar(&state).unwrap_err();
    assert_eq!(erro.0, StatusCode::CONFLICT);
    assert!(erro.1 .0["error"].as_str().unwrap().contains("Authy"));
    assert!(!state.mobile.tunel.config.lock().unwrap().ligado);
    assert!(!dir.path().join("tunel.json").exists(), "a escolha não pode ficar gravada");
    state.mobile.config.lock().unwrap().totp_secret = SEGREDO.into();
    assert_eq!(ligar(&state).unwrap_err().0, StatusCode::CONFLICT, "segredo sem confirmar não basta");
    assert!(!totp_pronto(&state));
}

#[test]
fn escolha_de_ligar_e_lembrada_no_disco_so_para_o_dono() {
    let (dir, state) = com_tunel();
    mudar_ligado(&state, true).unwrap();
    let gravado: TunelConfig = omni_protocol::read_json_or_default(&dir.path().join("tunel.json"));
    assert!(gravado.ligado);
    assert_eq!(gravado.token, "token-do-tunel");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(dir.path().join("tunel.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
    assert!(Tunel::new(dir.path()).config.lock().unwrap().ligado, "o engine reaberto liga o túnel sozinho");
    desligar(&state).unwrap();
    assert!(!omni_protocol::read_json_or_default::<TunelConfig>(&dir.path().join("tunel.json")).ligado);
}

#[tokio::test]
async fn tunel_exige_o_token_dele_e_nao_confia_em_loopback() {
    let (_dir, state) = com_tunel();
    assert_eq!(chamar(&state, pedido("GET", "/conversas", None, IP)).await.0, StatusCode::UNAUTHORIZED);
    let mut r = pedido("GET", "/conversas", None, "x");
    r.headers_mut().remove("cf-connecting-ip");
    assert_eq!(chamar(&state, r).await.0, StatusCode::UNAUTHORIZED, "loopback sem cabeçalho também não");
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tailscale"), IP)).await.0, StatusCode::UNAUTHORIZED,
        "o token do QR do Tailscale não vale aqui");
    let r = router_do_tunel(state.clone()).oneshot(pedido("GET", "/conversas", Some("token-do-tunel"), IP)).await.unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["strict-transport-security"], "max-age=31536000");
    assert!(r.headers().contains_key("content-security-policy"));
}

#[tokio::test]
async fn tunel_confere_host_e_origem() {
    let (_dir, state) = com_tunel();
    let mut r = pedido("GET", "/conversas", Some("token-do-tunel"), IP);
    r.headers_mut().insert("host", "outro.trycloudflare.com".parse().unwrap());
    assert_eq!(chamar(&state, r).await.0, StatusCode::FORBIDDEN);
    let mut r = pedido("POST", "/conversas/c/nome", Some("token-do-tunel"), IP);
    r.headers_mut().insert("origin", "https://site-malicioso.example".parse().unwrap());
    assert_eq!(chamar(&state, r).await.0, StatusCode::FORBIDDEN);
    let mut r = pedido("POST", "/conversas/c/nome", Some("token-do-tunel"), IP);
    r.headers_mut().remove("origin");
    assert_eq!(chamar(&state, r).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn tunel_sem_endereco_publico_nao_atende() {
    let (_dir, state) = com_tunel();
    *state.mobile.tunel.estado.lock().unwrap() = Estado::default();
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tunel"), IP)).await.0, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn kanban_continua_so_leitura_pelo_tunel() {
    let (_dir, state) = com_tunel();
    assert_eq!(chamar(&state, pedido("POST", "/kanban/cartoes/a/pegar", Some("token-do-tunel"), IP)).await.0, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn sem_aprovar_no_pc_nao_sai_token() {
    let (_dir, state) = com_tunel();
    let segredo = pedir(&state, 0).await;
    let (status, corpo) = chamar(&state, com_corpo("/parear/pedido", json!({"pedido": segredo}), IP)).await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert_eq!(corpo["estado"], "aguardando");
    assert!(corpo.get("token").is_none());
    let lista = pedidos_json(&state, now_ms());
    assert_eq!(lista["pedidos"][0]["ip"], IP);
    assert!(lista["pedidos"][0]["aparelho"].as_str().unwrap().contains("Android"));
    assert!(!lista.to_string().contains(&segredo), "o segredo do celular não vai para a tela");
    assert!(!lista.to_string().contains("token-do-tunel"));
}

#[tokio::test]
async fn aprovado_no_pc_sai_o_token_do_tunel_uma_vez() {
    let (_dir, state) = com_tunel();
    let segredo = pedir(&state, 0).await;
    responder(&state, &id_no_desktop(&state), true, now_ms()).unwrap();
    let (status, corpo) = chamar(&state, com_corpo("/parear/pedido", json!({"pedido": segredo}), IP)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(corpo["token"], "token-do-tunel", "nunca o token do Tailscale");
    assert_eq!(chamar(&state, com_corpo("/parear/pedido", json!({"pedido": segredo}), IP)).await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn recusado_ou_expirado_nao_sai_token() {
    let (_dir, state) = com_tunel();
    let segredo = pedir(&state, 0).await;
    responder(&state, &id_no_desktop(&state), false, now_ms()).unwrap();
    let (status, corpo) = chamar(&state, com_corpo("/parear/pedido", json!({"pedido": segredo}), IP)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(corpo.get("token").is_none());

    let segredo = pedir(&state, 1).await;
    let id = id_no_desktop(&state);
    state.mobile.tunel.pareamento.lock().unwrap().pedidos[0].criado_ms -= PEDIDO_VALE_MS;
    assert_eq!(responder(&state, &id, true, now_ms()).unwrap_err().0, StatusCode::GONE);
    let (status, corpo) = chamar(&state, com_corpo("/parear/pedido", json!({"pedido": segredo}), IP)).await;
    assert_eq!(status, StatusCode::GONE);
    assert!(corpo.get("token").is_none());
    assert!(pedidos_json(&state, now_ms())["pedidos"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn pareamento_pelo_tunel_sem_authy_confirmado_recusa() {
    let (_dir, state) = com_tunel();
    state.mobile.config.lock().unwrap().totp_confirmed = false;
    assert_eq!(chamar(&state, parear(&codigo(0), IP)).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn codigo_usado_pelo_tailscale_nao_vale_no_tunel() {
    let (dir, state) = com_tunel();
    let atual = codigo(0);
    let _ = parear_em(&state, &atual, agora_segundos()).expect("vale pelo Tailscale");
    assert_eq!(chamar(&state, parear(&atual, IP)).await.0, StatusCode::UNAUTHORIZED);
    // E o passo usado fica em disco: reiniciar o engine não reabre o código.
    let gravado: Value = omni_protocol::read_json_or_default(&dir.path().join("totp-passo.json"));
    assert_eq!(gravado["ultimo_passo"], agora_segundos() / 30);
    let reaberto = MobileRuntime::new(dir.path());
    assert_eq!(*reaberto.passo_totp.lock().unwrap(), agora_segundos() / 30);
}

#[tokio::test]
async fn codigo_errado_trava_o_pareamento_do_ip_mas_nao_o_aparelho_ja_pareado() {
    let (_dir, state) = com_tunel();
    for _ in 0..FALHAS_PAREAR_POR_IP {
        assert_eq!(chamar(&state, parear("000000", IP)).await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(chamar(&state, parear(&codigo(0), IP)).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tunel"), IP)).await.0, StatusCode::OK,
        "quem já tem o token segue entrando do mesmo IP");
}

#[tokio::test]
async fn token_errado_trava_so_aquele_ip_e_sem_token_nao_conta() {
    let (_dir, state) = com_tunel();
    // Celular ainda não pareado consultando: muitas vezes, sem token, nunca trava.
    for _ in 0..FALHAS_TOKEN_POR_IP * 2 {
        assert_eq!(chamar(&state, pedido("GET", "/conversas", None, "198.51.100.9")).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(chamar(&state, pedido("GET", "/conversas", Some(""), "198.51.100.9")).await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tunel"), "198.51.100.9")).await.0, StatusCode::OK);
    for _ in 0..FALHAS_TOKEN_POR_IP {
        assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("chute"), "198.51.100.9")).await.0, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tunel"), "198.51.100.9")).await.0, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tunel"), IP)).await.0, StatusCode::OK, "outro IP segue entrando");
}

#[test]
fn trava_por_ip_acaba_sozinha() {
    let mut limites = Limites::default();
    for _ in 0..FALHAS_PAREAR_POR_IP { limites.falhou("1.2.3.4", Falha::Parear, 1_000); }
    assert!(limites.bloqueado("1.2.3.4", Falha::Parear, 1_000));
    assert!(!limites.bloqueado("1.2.3.4", Falha::Token, 1_000));
    assert!(!limites.bloqueado("5.6.7.8", Falha::Parear, 1_000));
    assert!(!limites.bloqueado("1.2.3.4", Falha::Parear, 1_000 + BLOQUEIO_MS));
    let mut limites = Limites::default();
    for i in 0..FALHAS_PAREAR_POR_IP as u64 { limites.falhou("1.2.3.4", Falha::Parear, i * JANELA_MS); }
    assert!(!limites.bloqueado("1.2.3.4", Falha::Parear, FALHAS_PAREAR_POR_IP as u64 * JANELA_MS), "falhas espalhadas não somam");
}

#[test]
fn trava_global_dobra_a_espera_e_desliga_no_teto_do_dia() {
    let mut trava = TravaGlobal::default();
    let mut agora = 10_000_u64;
    let mut esperas = Vec::new();
    let mut desligou = false;
    for _ in 0..TETO_DIA {
        match trava.falhou(agora) {
            Efeito::Pausou(ate) => { esperas.push(ate - agora); agora = ate; }
            Efeito::Desligou => { desligou = true; break; }
            Efeito::Nada => agora += 1_000,
        }
    }
    assert_eq!(esperas, [ESPERA_BASE_MS, ESPERA_BASE_MS * 2, ESPERA_BASE_MS * 4]);
    assert!(desligou, "o 20º erro do dia desliga");
    assert_eq!(trava.pausado_ate(agora), None);
}

#[tokio::test]
async fn teto_do_dia_desliga_o_pareamento_ate_reativar_no_pc() {
    let (dir, state) = com_tunel();
    // Pelo caminho real: errar até o teto, de IPs diferentes e esperando as pausas acabarem.
    let mut desligado = false;
    for i in 0..TETO_DIA * 2 {
        let sem_pausa = desfazer_pausa(&state);
        state.mobile.tunel.pareamento.lock().unwrap().trava = sem_pausa;
        let (status, _) = chamar(&state, parear("000000", &format!("198.51.100.{i}"))).await;
        if status == StatusCode::LOCKED { desligado = true; break; }
    }
    assert!(desligado, "o pareamento pelo túnel desliga");
    assert!(omni_protocol::read_json_or_default::<TunelConfig>(&dir.path().join("tunel.json")).pareamento_desligado);
    let lista = pedidos_json(&state, now_ms());
    assert_eq!(lista["pareamento_desligado"], true);
    assert!(lista["alerta"]["texto"].as_str().unwrap().contains("desligado"));
    assert_eq!(chamar(&state, parear(&codigo(0), IP)).await.0, StatusCode::LOCKED, "nem o código certo passa");
    assert_eq!(chamar(&state, pedido("GET", "/conversas", Some("token-do-tunel"), IP)).await.0, StatusCode::OK, "pareado segue");
    reativar(&state).unwrap();
    assert_eq!(chamar(&state, parear(&codigo(0), IP)).await.0, StatusCode::ACCEPTED);
}

/// Mantém a contagem do dia e tira só a pausa ativa — para o teste chegar ao teto sem esperar horas.
fn desfazer_pausa(state: &EngineState) -> TravaGlobal {
    let mut atual = std::mem::take(&mut state.mobile.tunel.pareamento.lock().unwrap().trava);
    atual.sem_pausa_para_teste();
    atual
}

#[test]
fn ip_de_origem_so_le_o_cabecalho_vindo_de_loopback() {
    let mut r = pedido("GET", "/", None, IP);
    assert_eq!(ip_de_origem(&r), IP);
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
    let lan = |uri: &str| axum::http::Request::builder().method("POST").uri(uri)
        .header("host", "127.0.0.1:47322").header("origin", "http://127.0.0.1:47322")
        .header("x-omni-token", "token-do-tailscale").body(Body::empty()).unwrap();
    for uri in ["/tunel/ligar", "/tunel/desligar", "/tunel/instalar", "/tunel/pedidos/x/permitir", "/tunel/pareamento/reativar"] {
        let r = router(state.clone()).oneshot(lan(uri)).await.unwrap();
        assert!(!r.status().is_success(), "{uri} pelo celular: {}", r.status());
        let r = router_do_tunel(state.clone()).oneshot(pedido("POST", uri, Some("token-do-tunel"), IP)).await.unwrap();
        assert!(!r.status().is_success(), "{uri} pelo túnel: {}", r.status());
    }
    let EngineResponse::Api { status, body } = local(state.clone(), "GET", "/tunel", None, None, vec![]).await else { panic!() };
    assert_eq!(status, 200);
    let estado: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(estado["qr"], format!("{URL}/"), "o QR do túnel não leva token");
    assert!(!body.contains("token-do-tunel") && !body.contains("token-do-tailscale"));
    let EngineResponse::Api { status, .. } = local(state.clone(), "GET", "/tunel/pedidos", None, None, vec![]).await else { panic!() };
    assert_eq!(status, 200);
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
