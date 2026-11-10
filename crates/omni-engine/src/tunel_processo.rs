//! O processo `cloudflared`: achar, baixar, rodar, reiniciar e garantir que ele não sobreviva ao
//! engine. Filho de `tunel`.

use super::*;
use std::ffi::OsString;
use std::io::{BufRead as _, BufReader as LeitorDeLinhas};
use std::process::{Command, Stdio};

/// Versão do cloudflared que o app sabe baixar. Fixa de propósito: o SHA-256 de cada arquivo vem das
/// notas da release oficial (github.com/cloudflare/cloudflared/releases/tag/2026.10.7) e é conferido
/// antes de o arquivo virar executável. Com o pacote do sistema (pacman), a versão é a dele.
pub(in crate::mobile) const VERSAO: &str = "2026.10.7";
const BAIXAR_DE: &str = "https://github.com/cloudflare/cloudflared/releases/download";

pub(in crate::mobile) struct Pacote { pub(in crate::mobile) arquivo: &'static str, pub(in crate::mobile) sha256: &'static str,
    pub(in crate::mobile) bytes: u64, pub(in crate::mobile) tgz: bool }

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

pub(in crate::mobile) fn pacote_do_sistema() -> Option<&'static Pacote> {
    PACOTES.iter().find(|(os, arch, _)| *os == std::env::consts::OS && *arch == std::env::consts::ARCH).map(|(_, _, p)| p)
}

/// Alternativa ao download, mostrada na tela: o pacote do sistema se atualiza junto com o resto.
pub(super) fn comando_do_sistema() -> &'static str {
    match std::env::consts::OS {
        "windows" => "winget install --id Cloudflare.cloudflared",
        "macos" => "brew install cloudflared",
        _ => "sudo pacman -S cloudflared",
    }
}

pub(super) fn pasta_bin(state: &EngineState) -> PathBuf { state.state_file.with_file_name("bin") }
fn arquivo_do_pid(state: &EngineState) -> PathBuf { state.state_file.with_file_name("cloudflared.pid") }
/// Config vazia de propósito: um `~/.cloudflared/config.yml` da pessoa (com `tunnel:`, `ingress:`…)
/// não pode mudar o que o túnel do celular publica.
fn config_vazia(state: &EngineState) -> PathBuf { state.state_file.with_file_name("cloudflared-vazio.yml") }

fn com_exe(nome: &str) -> String { if cfg!(windows) { format!("{nome}.exe") } else { nome.to_owned() } }
pub(in crate::mobile) fn nome_do_binario() -> String { com_exe("cloudflared") }

/// Programa no PATH, **só em pastas absolutas**: entrada vazia ou relativa (`.`, `bin`) faria o engine
/// rodar um `cloudflared`/`curl` plantado na pasta em que ele foi aberto.
pub(in crate::mobile) fn no_path(nome: &str, path: Option<OsString>) -> Option<PathBuf> {
    let nome = com_exe(nome);
    path.and_then(|p| std::env::split_paths(&p).filter(|d| d.is_absolute()).map(|d| d.join(&nome)).find(|c| c.is_file()))
}

/// Onde está o cloudflared: o do sistema (PATH) primeiro, senão o que o app baixou.
pub(in crate::mobile) fn localizar_em(path: Option<OsString>, pasta: &Path) -> Option<PathBuf> {
    no_path("cloudflared", path).or_else(|| Some(pasta.join(nome_do_binario())).filter(|c| c.is_file()))
}

pub(super) fn localizar(state: &EngineState) -> Option<PathBuf> { localizar_em(std::env::var_os("PATH"), &pasta_bin(state)) }

fn programa(nome: &str) -> Result<PathBuf> {
    no_path(nome, std::env::var_os("PATH")).ok_or_else(|| anyhow!("Não achei o {nome} neste PC."))
}

/// Tira o endereço público da saída do cloudflared. Ele escreve no stderr, numa caixa:
/// `INF |  https://palavras-aleatorias.trycloudflare.com  |`. Outras linhas também têm `https://`
/// do mesmo domínio (`api.trycloudflare.com`, o serviço que cria o túnel, aparece nos erros) — só
/// vale um subdomínio de um nível só, diferente de `api`.
pub(in crate::mobile) fn url_da_saida(linha: &str) -> Option<String> {
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

/// Lê a saída de **um** processo: só a primeira URL vale. Uma segunda (log forjado, reconexão
/// estranha) não troca o endereço que o celular e o `Host` esperado usam.
#[derive(Default)]
pub(in crate::mobile) struct Leitura { url: Option<String> }
impl Leitura {
    pub(in crate::mobile) fn linha(&mut self, linha: &str) -> Option<String> {
        if self.url.is_some() { return None }
        self.url = url_da_saida(linha);
        self.url.clone()
    }
}

pub(in crate::mobile) fn argumentos(porta: u16, config: &Path) -> Vec<OsString> {
    vec!["tunnel".into(), "--config".into(), config.into(), "--url".into(),
        format!("http://127.0.0.1:{porta}").into(), "--no-autoupdate".into()]
}

/// Variáveis que o cloudflared lê como opção (`TUNNEL_TOKEN`, `TUNNEL_URL`, `TUNNEL_ORIGIN_CERT`…):
/// nenhuma do ambiente da pessoa entra no túnel do celular.
pub(in crate::mobile) fn variaveis_a_tirar(vars: impl Iterator<Item = OsString>) -> Vec<OsString> {
    vars.filter(|nome| nome.to_string_lossy().to_ascii_uppercase().starts_with("TUNNEL_")).collect()
}

/// No Linux o kernel mata o cloudflared quando a thread que o criou termina (`PR_SET_PDEATHSIG`); o
/// processo é criado e esperado pela mesma thread, que vive enquanto ele vive. Em todo Unix ele
/// nasce líder do próprio grupo, para o `matar_orfao` derrubar o grupo inteiro se o engine morrer
/// sem conseguir (macOS não tem PDEATHSIG).
#[cfg(unix)]
fn preparar(comando: &mut Command, grupo_proprio: bool) {
    use std::os::unix::process::CommandExt;
    if grupo_proprio { comando.process_group(0); }
    #[cfg(target_os = "linux")]
    {
        unsafe extern "C" { fn prctl(opcao: std::ffi::c_int, ...) -> std::ffi::c_int; }
        const PR_SET_PDEATHSIG: std::ffi::c_int = 1;
        const SIGTERM: std::ffi::c_ulong = 15;
        // SAFETY: entre o fork e o exec só roda `prctl`, que é async-signal-safe e não aloca.
        unsafe { comando.pre_exec(|| { prctl(PR_SET_PDEATHSIG, SIGTERM); Ok(()) }); }
    }
}

#[cfg(windows)]
fn preparar(comando: &mut Command, _grupo_proprio: bool) {
    use std::os::windows::process::CommandExt;
    comando.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
}

#[cfg(not(any(unix, windows)))]
fn preparar(_comando: &mut Command, _grupo_proprio: bool) {}

/// Windows: o cloudflared entra num Job Object com `KILL_ON_JOB_CLOSE`. O handle do job fica aberto
/// até o engine terminar — de qualquer jeito, até morto pelo Gerenciador de Tarefas —, e aí o
/// Windows fecha o handle e mata tudo que está no job.
#[cfg(windows)]
fn amarrar_ao_engine(filho: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE};
    static JOB: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    // SAFETY: chamadas Win32 com ponteiros válidos; o job vive até o fim do processo de propósito.
    let job = *JOB.get_or_init(|| unsafe {
        let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
        if job.is_null() { return 0; }
        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let ok = SetInformationJobObject(job, JobObjectExtendedLimitInformation,
            &info as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION as *const core::ffi::c_void,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32);
        if ok == 0 { return 0; }
        job as usize
    });
    if job == 0 { eprintln!("túnel: não consegui criar o Job Object; o cloudflared pode sobrar se o engine morrer"); return; }
    // SAFETY: o handle do processo é válido enquanto `filho` existir.
    if unsafe { AssignProcessToJobObject(job as _, filho.as_raw_handle() as _) } == 0 {
        eprintln!("túnel: não consegui amarrar o cloudflared ao engine");
    }
}

#[cfg(not(windows))]
fn amarrar_ao_engine(_filho: &std::process::Child) {}

/// O processo do pid é o cloudflared **deste app** (a linha de comando cita a config vazia dele)?
/// Um cloudflared da pessoa, ou qualquer outro programa que herdou o pid, nunca é tocado.
pub(in crate::mobile) fn e_o_nosso(linha_de_comando: &str, config: &Path) -> bool {
    let programa = linha_de_comando.split_whitespace().next().map(Path::new).and_then(Path::file_stem);
    programa.is_some_and(|nome| nome == "cloudflared") && linha_de_comando.contains(&*config.to_string_lossy())
}

/// macOS e outros Unix sem PDEATHSIG: se o engine anterior morreu à força, o cloudflared dele ficou
/// rodando. Ao subir de novo, derruba o grupo dele (pid guardado em arquivo).
#[cfg(unix)]
fn matar_orfao(state: &EngineState) {
    let arquivo = arquivo_do_pid(state);
    let Some(pid) = fs::read_to_string(&arquivo).ok().and_then(|t| t.trim().parse::<i32>().ok()).filter(|p| *p > 1) else { return };
    let _ = fs::remove_file(&arquivo);
    let Ok(ps) = programa("ps") else { return };
    let Ok(saida) = Command::new(ps).args(["-p", &pid.to_string(), "-o", "args="]).stdin(Stdio::null()).output() else { return };
    if !e_o_nosso(&String::from_utf8_lossy(&saida.stdout), &config_vazia(state)) { return; }
    unsafe extern "C" { fn kill(pid: i32, sinal: i32) -> i32; }
    // SAFETY: `kill(2)` só recebe inteiros. Negativo = o grupo todo; ele nasceu líder (`process_group(0)`).
    unsafe { kill(-pid, 15); }
}

#[cfg(not(unix))]
fn matar_orfao(_state: &EngineState) {}

fn abrir(state: &EngineState, binario: &Path, porta: u16) -> std::io::Result<std::process::Child> {
    let config = config_vazia(state);
    if !config.is_file() { fs::write(&config, "")?; }
    let mut comando = Command::new(binario);
    comando.args(argumentos(porta, &config)).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    for nome in variaveis_a_tirar(std::env::vars_os().map(|(nome, _)| nome)) { comando.env_remove(nome); }
    preparar(&mut comando, true);
    let filho = comando.spawn()?;
    amarrar_ao_engine(&filho);
    if let Err(erro) = fs::write(arquivo_do_pid(state), filho.id().to_string()) {
        eprintln!("túnel: não consegui guardar o pid do cloudflared: {erro}");
    }
    Ok(filho)
}

/// Mantém o cloudflared de pé enquanto esta rodada do túnel valer: reinicia se ele cair, com espera
/// crescente (1 s, 2 s, 4 s… até 1 min). A saída dele não vai para log nenhum — só o endereço e a
/// última linha de erro, para a tela; o token nunca passa por ele.
pub(super) fn supervisionar(state: Arc<EngineState>, porta: u16, geracao: u64) {
    let vivo = |state: &EngineState| state.mobile.tunel.geracao.load(Ordering::SeqCst) == geracao;
    matar_orfao(&state);
    let mut espera_s = 1_u64;
    while vivo(&state) {
        let Some(binario) = localizar(&state) else {
            definir_estado(&state, geracao, |e| e.erro = Some("O cloudflared não está instalado neste PC.".into()));
            return;
        };
        let inicio = now_ms();
        let ultimo_erro = Arc::new(Mutex::new(String::new()));
        match abrir(&state, &binario, porta) {
            Err(e) => definir_estado(&state, geracao, |s| s.erro = Some(format!("Não consegui abrir o cloudflared: {e}"))),
            Ok(mut filho) => {
                if let Some(saida) = filho.stderr.take() {
                    let (leitor, ultimo) = (state.clone(), ultimo_erro.clone());
                    std::thread::spawn(move || {
                        let mut leitura = Leitura::default();
                        for linha in LeitorDeLinhas::new(saida).lines().map_while(Result::ok) {
                            if let Some(url) = leitura.linha(&linha) {
                                definir_estado(&leitor, geracao, |e| { e.url = Some(url); e.erro = None; });
                            } else if linha.contains(" ERR ") {
                                *ultimo.lock().expect("erro poisoned") = linha.chars().take(300).collect();
                            }
                        }
                    });
                }
                loop {
                    if !vivo(&state) {
                        let _ = filho.kill();
                        let _ = filho.wait();
                        let _ = fs::remove_file(arquivo_do_pid(&state));
                        return;
                    }
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

fn progresso(state: &EngineState) -> Option<u8> {
    let pacote = pacote_do_sistema()?;
    let parcial = fs::metadata(pasta_bin(state).join(format!("{}.part", pacote.arquivo))).ok()?;
    Some(((parcial.len().min(pacote.bytes) * 100) / pacote.bytes) as u8)
}

fn sha256_de(caminho: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    std::io::copy(&mut fs::File::open(caminho)?, &mut hash)?;
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Confere tamanho e SHA-256 publicados pela Cloudflare. Só depois disso o arquivo vira executável.
pub(in crate::mobile) fn confere(caminho: &Path, pacote: &Pacote) -> bool {
    fs::metadata(caminho).is_ok_and(|m| m.len() == pacote.bytes) && sha256_de(caminho).is_ok_and(|h| h == pacote.sha256)
}

fn baixar(state: &EngineState, pacote: &Pacote) -> Result<()> {
    let pasta = pasta_bin(state);
    fs::create_dir_all(&pasta)?;
    let parcial = pasta.join(format!("{}.part", pacote.arquivo));
    let url = format!("{BAIXAR_DE}/{VERSAO}/{}", pacote.arquivo);
    let mut curl = Command::new(programa("curl")?);
    // `--proto =https`: nem o redirecionamento do GitHub para o CDN pode descer para http.
    curl.args(["-L", "--fail", "-sS", "--retry", "3", "--proto", "=https", "--proto-redir", "=https", "-o"])
        .arg(&parcial).arg(&url).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
    preparar(&mut curl, false);
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
    let extraido = pasta.join("cloudflared-extraido");
    let pronto = if pacote.tgz {
        let _ = fs::remove_dir_all(&extraido);
        fs::create_dir_all(&extraido)?;
        let mut tar = Command::new(programa("tar")?);
        tar.arg("-xzf").arg(&parcial).arg("-C").arg(&extraido).stdin(Stdio::null());
        preparar(&mut tar, false);
        let ok = tar.status()?.success();
        let _ = fs::remove_file(&parcial);
        if !ok { return Err(anyhow!("Não consegui abrir o pacote do cloudflared")); }
        extraido.join("cloudflared")
    } else { parcial };
    #[cfg(unix)] { use std::os::unix::fs::PermissionsExt; fs::set_permissions(&pronto, fs::Permissions::from_mode(0o755))?; }
    fs::rename(&pronto, &destino)?;
    let _ = fs::remove_dir_all(&extraido);
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

pub(super) fn estado_do_cloudflared(state: &EngineState) -> Value {
    let instalacao = state.mobile.tunel.instalacao.lock().expect("tunel poisoned").clone();
    let binario = localizar(state);
    json!({
        "instalado": binario.is_some(),
        "caminho": binario.map(|c| c.to_string_lossy().into_owned()),
        "pode_baixar": pacote_do_sistema().is_some(),
        "instalando": instalacao.ativa,
        "progresso": instalacao.ativa.then(|| progresso(state)).flatten(),
        "erro": instalacao.erro,
        "comando": comando_do_sistema(),
        "versao": VERSAO,
    })
}
