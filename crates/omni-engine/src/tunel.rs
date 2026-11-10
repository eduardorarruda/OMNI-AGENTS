//! Túnel HTTPS da Cloudflare (quick tunnel: grátis, sem conta) para o celular abrir as conversas de
//! qualquer lugar, com microfone e "instalar como app" — os dois exigem HTTPS.
//!
//! Filho de `mobile`. O ponto de segurança: o túnel atende num listener **só dele**, em 127.0.0.1 numa
//! porta própria, e todo pedido que chega lá vem do `cloudflared` local — então "veio de loopback" não
//! prova nada e nada aqui confia nisso. Neste listener:
//! - `Host`/`Origin` têm de ser o endereço público do túnel (CSRF, DNS rebinding);
//! - o token é **outro**, só do túnel, e o QR não o leva: o aparelho novo só o recebe digitando o
//!   código atual do Authy (`/parear` deste listener). Quem tem o QR do modo Tailscale não entra;
//! - falhas de pareamento e de token travam o IP de origem por um tempo (`CF-Connecting-IP`, que o
//!   cloudflared põe; só é lido aqui, onde quem fala é ele);
//! - o resto é o mesmo roteador do celular, inclusive o `somente_leitura_no_http`.
//!
//! O túnel não liga sem o Authy confirmado e cai sozinho se o cadastro for refeito.

use super::*;
use axum::extract::ConnectInfo;
use std::io::{BufRead as _, BufReader as LeitorDeLinhas};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

/// Versão do cloudflared que o app sabe baixar. Fixa de propósito: o SHA-256 de cada arquivo vem das
/// notas da release oficial (github.com/cloudflare/cloudflared/releases/tag/2026.10.7) e é conferido
/// antes de o arquivo virar executável. Com o pacote do sistema (pacman), a versão é a dele.
pub(super) const VERSAO: &str = "2026.10.7";
const BAIXAR_DE: &str = "https://github.com/cloudflare/cloudflared/releases/download";

pub(super) struct Pacote { pub(super) arquivo: &'static str, sha256: &'static str, bytes: u64, tgz: bool }

const PACOTES: [(&str, &str, Pacote); 5] = [
    ("linux", "x86_64", Pacote { arquivo: "cloudflared-linux-amd64", bytes: 40_274_445, tgz: false,
        sha256: "1f99fb518b83dec2ba8d870a7cc991d82a77e6196295a3d2754ba17ab9f91872" }),
    ("linux", "aarch64", Pacote { arquivo: "cloudflared-linux-arm64", bytes: 37_667_293, tgz: false,
        sha256: "3df43aedc9ff291d1735b5cb576cb97e0b157488796db2e32b28aecabb445775" }),
    ("windows", "x86_64", Pacote { arquivo: "cloudflared-windows-amd64.exe", bytes: 55_390_512, tgz: false,
        sha256: "2493dbc168923bb3e59e5e7716f71deced92992eea9acd6e9d74f84ac7751343" }),
    ("macos", "x86_64", Pacote { arquivo: "cloudflared-darwin-amd64.tgz", bytes: 21_834_416, tgz: true,
        sha256: "5c55e77da7012cebca19170357e196176c48c9647cc2ead404702fa125ddbd86" }),
    ("macos", "aarch64", Pacote { arquivo: "cloudflared-darwin-arm64.tgz", bytes: 19_847_161, tgz: true,
        sha256: "2136431d5eb467bf447d2ee514b713e192719b143aec1d9e09bf7dc290d5bb12" }),
];

pub(super) fn pacote_do_sistema() -> Option<&'static Pacote> {
    PACOTES.iter().find(|(os, arch, _)| *os == std::env::consts::OS && *arch == std::env::consts::ARCH).map(|(_, _, p)| p)
}

/// Alternativa ao download, mostrada na tela: o pacote do sistema se atualiza junto com o resto.
fn comando_do_sistema() -> &'static str {
    match std::env::consts::OS {
        "windows" => "winget install --id Cloudflare.cloudflared",
        "macos" => "brew install cloudflared",
        _ => "sudo pacman -S cloudflared",
    }
}

/// O que sobrevive a reiniciar o engine: a escolha de deixar ligado e o token do túnel.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct TunelConfig { pub(super) ligado: bool, pub(super) token: String }

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
struct Registro { parear: VecDeque<u64>, token: VecDeque<u64>, bloqueado_ate: u64 }

#[derive(Default)]
pub(super) struct Limites { por_ip: HashMap<String, Registro> }

impl Limites {
    pub(super) fn bloqueado(&self, ip: &str, agora_ms: u64) -> bool {
        self.por_ip.get(ip).is_some_and(|r| r.bloqueado_ate > agora_ms)
    }

    pub(super) fn falhou(&mut self, ip: &str, falha: Falha, agora_ms: u64) {
        if self.por_ip.len() >= IPS_MAX && !self.por_ip.contains_key(ip) {
            let recente = |fila: &VecDeque<u64>| fila.back().is_some_and(|t| agora_ms.saturating_sub(*t) < JANELA_MS);
            self.por_ip.retain(|_, r| r.bloqueado_ate > agora_ms || recente(&r.parear) || recente(&r.token));
            if self.por_ip.len() >= IPS_MAX { self.por_ip.retain(|_, r| r.bloqueado_ate > agora_ms); }
        }
        let registro = self.por_ip.entry(ip.to_owned()).or_default();
        let (fila, limite) = match falha {
            Falha::Parear => (&mut registro.parear, FALHAS_PAREAR_POR_IP),
            Falha::Token => (&mut registro.token, FALHAS_TOKEN_POR_IP),
        };
        fila.retain(|t| agora_ms.saturating_sub(*t) < JANELA_MS);
        fila.push_back(agora_ms);
        if fila.len() >= limite {
            fila.clear();
            registro.bloqueado_ate = agora_ms + BLOQUEIO_MS;
        }
    }
}

pub(super) struct Tunel {
    pub(super) config: Mutex<TunelConfig>,
    estado: Mutex<Estado>,
    mudou: Notify,
    /// Cada rodada do túnel tem um número; o supervisor do cloudflared para quando ele muda.
    geracao: AtomicU64,
    /// Trava global de pareamento **só do túnel**: um atacante na internet não tranca o pareamento
    /// pelo Tailscale do dono.
    pareamento: Mutex<Pareamento>,
    pub(super) limites: Mutex<Limites>,
    instalacao: Mutex<Instalacao>,
}

impl Tunel {
    pub(super) fn new(dir: &Path) -> Self {
        let mut config: TunelConfig = omni_protocol::read_json_or_default(&dir.join("tunel.json"));
        if config.token.is_empty() { config.token = random_token(); }
        Self { config: Mutex::new(config), estado: Mutex::new(Estado::default()), mudou: Notify::new(),
            geracao: AtomicU64::new(0), pareamento: Mutex::new(Pareamento::default()),
            limites: Mutex::new(Limites::default()), instalacao: Mutex::new(Instalacao::default()) }
    }
}

fn arquivo_de_config(state: &EngineState) -> PathBuf { state.state_file.with_file_name("tunel.json") }
fn pasta_bin(state: &EngineState) -> PathBuf { state.state_file.with_file_name("bin") }
fn nome_do_binario() -> &'static str { if cfg!(windows) { "cloudflared.exe" } else { "cloudflared" } }

fn gravar(state: &EngineState, config: &TunelConfig) -> Result<(), ApiError> {
    atomic_write_json(&arquivo_de_config(state), config)
        .map_err(|e| error(StatusCode::INTERNAL_SERVER_ERROR, &format!("Não consegui gravar o ajuste do túnel: {e}")))
}

/// O Authy está cadastrado e confirmado: sem isso o túnel não liga (e cai, se já estava ligado).
pub(super) fn totp_pronto(state: &EngineState) -> bool {
    let config = state.mobile.config.lock().expect("config poisoned");
    config.totp_confirmed && !config.totp_secret.is_empty()
}

/// Onde está o cloudflared: o do sistema (PATH) primeiro, senão o que o app baixou.
pub(super) fn localizar_em(path: Option<std::ffi::OsString>, pasta: &Path) -> Option<PathBuf> {
    let nome = nome_do_binario();
    let no_path = path.and_then(|p| std::env::split_paths(&p).map(|d| d.join(nome)).find(|c| c.is_file()));
    no_path.or_else(|| Some(pasta.join(nome)).filter(|c| c.is_file()))
}

fn localizar(state: &EngineState) -> Option<PathBuf> { localizar_em(std::env::var_os("PATH"), &pasta_bin(state)) }

/// Tira o endereço público da saída do cloudflared. Ele escreve no stderr, numa caixa:
/// `INF |  https://palavras-aleatorias.trycloudflare.com  |`. Outras linhas também têm `https://`
/// do mesmo domínio (`api.trycloudflare.com`, o serviço que cria o túnel, aparece nos erros) — só
/// vale um subdomínio de um nível só, diferente de `api`.
pub(super) fn url_da_saida(linha: &str) -> Option<String> {
    const SUFIXO: &str = ".trycloudflare.com";
    linha.match_indices("https://").find_map(|(i, _)| {
        let resto = &linha[i + "https://".len()..];
        let fim = resto.find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '.')).unwrap_or(resto.len());
        let host = resto[..fim].trim_end_matches('.').to_ascii_lowercase();
        let sub = host.strip_suffix(SUFIXO)?;
        let valido = !sub.is_empty() && sub != "api" && !sub.contains('.') && !sub.starts_with('-') && !sub.ends_with('-');
        valido.then(|| format!("https://{host}"))
    })
}

fn argumentos(porta: u16) -> [String; 4] {
    ["tunnel".into(), "--url".into(), format!("http://127.0.0.1:{porta}"), "--no-autoupdate".into()]
}

/// O cloudflared morre junto com o engine, mesmo se o engine for morto à força. No Linux é o kernel
/// que garante (`PR_SET_PDEATHSIG`); por isso o processo é criado e esperado pela mesma thread, que
/// vive enquanto ele vive (o sinal vem quando a *thread* que criou o filho termina).
#[cfg(target_os = "linux")]
fn preparar(comando: &mut Command) {
    use std::os::unix::process::CommandExt;
    unsafe extern "C" { fn prctl(opcao: std::ffi::c_int, ...) -> std::ffi::c_int; }
    const PR_SET_PDEATHSIG: std::ffi::c_int = 1;
    const SIGTERM: std::ffi::c_ulong = 15;
    // SAFETY: entre o fork e o exec só roda `prctl`, que é async-signal-safe e não aloca.
    unsafe { comando.pre_exec(|| { prctl(PR_SET_PDEATHSIG, SIGTERM); Ok(()) }); }
}

#[cfg(not(target_os = "linux"))]
fn preparar(comando: &mut Command) {
    #[cfg(windows)] { use std::os::windows::process::CommandExt; comando.creation_flags(0x0800_0000); }
    let _ = comando;
}

fn definir_estado(state: &EngineState, geracao: u64, mudar: impl FnOnce(&mut Estado)) {
    let tunel = &state.mobile.tunel;
    let mut estado = tunel.estado.lock().expect("tunel poisoned");
    if tunel.geracao.load(Ordering::SeqCst) == geracao { mudar(&mut estado); }
}

/// Mantém o cloudflared de pé enquanto esta rodada do túnel valer: reinicia se ele cair, com espera
/// crescente (1 s, 2 s, 4 s… até 1 min). A saída dele não vai para log nenhum — só o endereço e a
/// última linha de erro, para a tela; o token nunca passa por ele.
fn supervisionar(state: Arc<EngineState>, porta: u16, geracao: u64) {
    let vivo = |state: &EngineState| state.mobile.tunel.geracao.load(Ordering::SeqCst) == geracao;
    let mut espera_s = 1_u64;
    while vivo(&state) {
        let Some(binario) = localizar(&state) else {
            definir_estado(&state, geracao, |e| e.erro = Some("O cloudflared não está instalado neste PC.".into()));
            return;
        };
        let mut comando = Command::new(binario);
        comando.args(argumentos(porta)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        preparar(&mut comando);
        let inicio = now_ms();
        let ultimo_erro = Arc::new(Mutex::new(String::new()));
        match comando.spawn() {
            Err(e) => definir_estado(&state, geracao, |s| s.erro = Some(format!("Não consegui abrir o cloudflared: {e}"))),
            Ok(mut filho) => {
                if let Some(saida) = filho.stderr.take() {
                    let (leitor, ultimo) = (state.clone(), ultimo_erro.clone());
                    std::thread::spawn(move || for linha in LeitorDeLinhas::new(saida).lines().map_while(Result::ok) {
                        if let Some(url) = url_da_saida(&linha) {
                            definir_estado(&leitor, geracao, |e| { e.url = Some(url); e.erro = None; });
                        } else if linha.contains(" ERR ") {
                            *ultimo.lock().expect("erro poisoned") = linha.chars().take(300).collect();
                        }
                    });
                }
                loop {
                    if !vivo(&state) { let _ = filho.kill(); let _ = filho.wait(); return; }
                    match filho.try_wait() {
                        Ok(None) => std::thread::sleep(std::time::Duration::from_millis(300)),
                        _ => break,
                    }
                }
                let detalhe = ultimo_erro.lock().expect("erro poisoned").clone();
                definir_estado(&state, geracao, |e| {
                    e.url = None;
                    e.erro = Some(if detalhe.is_empty() { "O túnel caiu; tentando de novo…".into() }
                        else { format!("O túnel caiu; tentando de novo… ({detalhe})") });
                });
            }
        }
        if now_ms().saturating_sub(inicio) > 60_000 { espera_s = 1; }
        for _ in 0..espera_s * 10 {
            if !vivo(&state) { return; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        espera_s = (espera_s * 2).min(60);
    }
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

async fn porteiro(State(state): State<Arc<EngineState>>, request: Request, next: Next) -> Response {
    let agora = now_ms();
    let ip = ip_de_origem(&request);
    let tunel = &state.mobile.tunel;
    if tunel.limites.lock().expect("limites poisoned").bloqueado(&ip, agora) {
        return error(StatusCode::TOO_MANY_REQUESTS, "Muitas tentativas erradas. Espere alguns minutos e tente de novo.").into_response();
    }
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
    if path == "/parear" { return com_cabecalhos(parear_pelo_tunel(&state, request, &ip, agora).await); }
    if !caminho_publico(&path) {
        let token = tunel.config.lock().expect("tunel poisoned").token.clone();
        let recebido = request.headers().get("x-omni-token").and_then(|v| v.to_str().ok()).unwrap_or("");
        if token.is_empty() || !token_confere(recebido, &token) {
            tunel.limites.lock().expect("limites poisoned").falhou(&ip, Falha::Token, agora);
            return com_cabecalhos(error(StatusCode::UNAUTHORIZED, "Aparelho não autorizado; digite o código do Authy").into_response());
        }
    }
    com_cabecalhos(next.run(request).await)
}

/// O pareamento do túnel: o código atual do Authy vira o token **do túnel** (nunca o do Tailscale).
async fn parear_pelo_tunel(state: &Arc<EngineState>, request: Request, ip: &str, agora_ms: u64) -> Response {
    if request.method() != axum::http::Method::POST { return StatusCode::METHOD_NOT_ALLOWED.into_response(); }
    let Ok(corpo) = axum::body::to_bytes(request.into_body(), 1024).await else {
        return error(StatusCode::BAD_REQUEST, "Pedido inválido").into_response();
    };
    let Ok(body) = serde_json::from_slice::<PareamentoBody>(&corpo) else {
        return error(StatusCode::BAD_REQUEST, "Pedido inválido").into_response();
    };
    let config = state.mobile.config.lock().expect("config poisoned").clone();
    let tunel = &state.mobile.tunel;
    match conferir_pareamento(&config, &tunel.pareamento, &body.codigo, agora_ms / 1000) {
        Ok(()) => Json(json!({"token": tunel.config.lock().expect("tunel poisoned").token.clone()})).into_response(),
        Err(erro) => {
            if erro.0 == StatusCode::UNAUTHORIZED {
                tunel.limites.lock().expect("limites poisoned").falhou(ip, Falha::Parear, agora_ms);
            }
            erro.into_response()
        }
    }
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
    let mut config = state.mobile.tunel.config.lock().expect("tunel poisoned");
    if config.ligado == ligado { return Ok(()); }
    let mut nova = config.clone();
    nova.ligado = ligado;
    gravar(state, &nova)?;
    *config = nova;
    drop(config);
    if !ligado { state.mobile.tunel.estado.lock().expect("tunel poisoned").url = None; }
    state.mobile.tunel.mudou.notify_one();
    Ok(())
}

/// "Trocar o código" e refazer o Authy: todo aparelho pareado pelo túnel precisa parear de novo.
pub(super) fn trocar_token(state: &EngineState) {
    let mut config = state.mobile.tunel.config.lock().expect("tunel poisoned");
    let mut nova = config.clone();
    nova.token = random_token();
    if gravar(state, &nova).is_ok() { *config = nova; }
}

/// O Authy mudou (refeito ou confirmado): o túnel reavalia se pode ficar de pé.
pub(super) fn authy_mudou(state: &EngineState) { state.mobile.tunel.mudou.notify_one(); }

fn progresso(state: &EngineState) -> Option<u8> {
    let pacote = pacote_do_sistema()?;
    let parcial = fs::metadata(pasta_bin(state).join(format!("{}.part", pacote.arquivo))).ok()?;
    Some(((parcial.len().min(pacote.bytes) * 100) / pacote.bytes) as u8)
}

pub(super) fn estado_json(state: &EngineState) -> Value {
    let ligado = state.mobile.tunel.config.lock().expect("tunel poisoned").ligado;
    let estado = state.mobile.tunel.estado.lock().expect("tunel poisoned").clone();
    let instalacao = state.mobile.tunel.instalacao.lock().expect("tunel poisoned").clone();
    let binario = localizar(state);
    json!({
        "ligado": ligado,
        "url": estado.url,
        // Sem token no link: quem lê o QR cai no pareamento e precisa do código do Authy.
        "qr": estado.url.as_ref().map(|u| format!("{u}/")),
        "erro": estado.erro,
        "totp_confirmed": totp_pronto(state),
        "cloudflared": {
            "instalado": binario.is_some(),
            "caminho": binario.map(|c| c.to_string_lossy().into_owned()),
            "pode_baixar": pacote_do_sistema().is_some(),
            "instalando": instalacao.ativa,
            "progresso": instalacao.ativa.then(|| progresso(state)).flatten(),
            "erro": instalacao.erro,
            "comando": comando_do_sistema(),
            "versao": VERSAO,
        },
    })
}

fn sha256_de(caminho: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    std::io::copy(&mut fs::File::open(caminho)?, &mut hash)?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Confere tamanho e SHA-256 publicados pela Cloudflare. Só depois disso o arquivo vira executável.
pub(super) fn confere(caminho: &Path, pacote: &Pacote) -> bool {
    fs::metadata(caminho).is_ok_and(|m| m.len() == pacote.bytes) && sha256_de(caminho).is_ok_and(|h| h == pacote.sha256)
}

fn baixar(state: &EngineState, pacote: &Pacote) -> Result<()> {
    let pasta = pasta_bin(state);
    fs::create_dir_all(&pasta)?;
    let parcial = pasta.join(format!("{}.part", pacote.arquivo));
    let url = format!("{BAIXAR_DE}/{VERSAO}/{}", pacote.arquivo);
    let mut curl = Command::new("curl");
    // `--proto =https`: nem o redirecionamento do GitHub para o CDN pode descer para http.
    curl.args(["-L", "--fail", "-sS", "--retry", "3", "--proto", "=https", "--proto-redir", "=https", "-o"])
        .arg(&parcial).arg(&url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    preparar(&mut curl);
    let saida = curl.output().context("não consegui abrir o curl")?;
    if !saida.status.success() {
        let _ = fs::remove_file(&parcial);
        let detalhe = String::from_utf8_lossy(&saida.stderr).trim().lines().last().unwrap_or("").to_owned();
        return Err(anyhow!("O download falhou. Confira a internet e tente de novo. {detalhe}"));
    }
    if !confere(&parcial, pacote) {
        let _ = fs::remove_file(&parcial);
        return Err(anyhow!("O arquivo baixado não bate com o publicado pela Cloudflare; nada foi instalado."));
    }
    let destino = pasta.join(nome_do_binario());
    let pronto = if pacote.tgz {
        let extraido = pasta.join("cloudflared-extraido");
        let _ = fs::remove_dir_all(&extraido);
        fs::create_dir_all(&extraido)?;
        let ok = Command::new("tar").arg("-xzf").arg(&parcial).arg("-C").arg(&extraido).status()?.success();
        let _ = fs::remove_file(&parcial);
        if !ok { return Err(anyhow!("Não consegui abrir o pacote do cloudflared")); }
        extraido.join("cloudflared")
    } else { parcial };
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&pronto, fs::Permissions::from_mode(0o755))?; }
    fs::rename(&pronto, &destino)?;
    let _ = fs::remove_dir_all(pasta.join("cloudflared-extraido"));
    Ok(())
}

/// Baixa o cloudflared oficial para a pasta do app, em segundo plano. A tela acompanha por `estado_json`.
pub(super) fn instalar(state: &Arc<EngineState>) -> Result<(), ApiError> {
    let Some(pacote) = pacote_do_sistema() else {
        return Err(error(StatusCode::CONFLICT, "Não há download pronto para este sistema; use o comando ao lado."));
    };
    {
        let mut instalacao = state.mobile.tunel.instalacao.lock().expect("tunel poisoned");
        if instalacao.ativa || localizar(state).is_some() { return Ok(()); }
        *instalacao = Instalacao { ativa: true, erro: None };
    }
    let state = state.clone();
    std::thread::spawn(move || {
        let erro = baixar(&state, pacote).err().map(|e| e.to_string());
        *state.mobile.tunel.instalacao.lock().expect("tunel poisoned") = Instalacao { ativa: false, erro };
    });
    Ok(())
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
    instalar(&state)?;
    Ok(Json(estado_json(&state)))
}

/// Rotas **só do desktop** (`mobile::local`): nunca entram no servidor HTTP do celular nem no do túnel.
pub(super) fn rotas_locais() -> Router<Arc<EngineState>> {
    Router::new()
        .route("/tunel", get(estado_rota))
        .route("/tunel/ligar", post(ligar_rota))
        .route("/tunel/desligar", post(desligar_rota))
        .route("/tunel/instalar", post(instalar_rota))
}

#[cfg(test)]
pub(super) fn definir_url_para_teste(state: &EngineState, url: &str, porta: u16) {
    *state.mobile.tunel.estado.lock().unwrap() = Estado { url: Some(url.into()), erro: None, porta: Some(porta) };
}

#[cfg(test)]
#[path = "tunel_testes.rs"]
mod testes;
