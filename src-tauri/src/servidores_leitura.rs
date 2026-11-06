#![cfg_attr(windows, allow(dead_code))]
//! Leitura das portas em escuta fora do Windows: `ss` (Linux, com `/proc/net/tcp` de reserva) e
//! `lsof` (macOS). Os `parse_*` são puros e testados com saída gravada.

use serde::Deserialize;

#[derive(Debug, PartialEq, Clone, Copy, Deserialize)]
pub struct Escuta { pub p: u16, pub pid: u32 }

fn porta_do_endereco(endereco: &str) -> Option<u16> { endereco.rsplit(':').next()?.parse().ok() }

/// `ss -ltnpH`: o endereço local é a 4ª coluna e o dono vem em `users:(("nome",pid=N,fd=M),...)`.
/// Socket sem `users:` (de outro usuário) é ignorado.
pub fn parse_ss(saida: &str) -> Vec<Escuta> {
    let mut lista = Vec::new();
    for linha in saida.lines() {
        let Some(porta) = linha.split_whitespace().nth(3).and_then(porta_do_endereco) else { continue };
        let Some((_, donos)) = linha.split_once("users:(") else { continue };
        for dono in donos.split("pid=").skip(1) {
            let digitos: String = dono.chars().take_while(char::is_ascii_digit).collect();
            let Ok(pid) = digitos.parse() else { continue };
            let escuta = Escuta { p: porta, pid };
            if !lista.contains(&escuta) { lista.push(escuta); }
        }
    }
    lista
}

/// `lsof -nP -iTCP -sTCP:LISTEN`: PID na 2ª coluna, endereço logo antes de `(LISTEN)`.
pub fn parse_lsof(saida: &str) -> Vec<Escuta> {
    let mut lista = Vec::new();
    for linha in saida.lines() {
        let campos: Vec<&str> = linha.split_whitespace().collect();
        let Some(pid) = campos.get(1).and_then(|c| c.parse().ok()) else { continue };
        if campos.last() != Some(&"(LISTEN)") || campos.len() < 3 { continue }
        let Some(p) = porta_do_endereco(campos[campos.len() - 2]) else { continue };
        let escuta = Escuta { p, pid };
        if !lista.contains(&escuta) { lista.push(escuta); }
    }
    lista
}

/// `/proc/net/tcp` (e `tcp6`): linhas em LISTEN (`0A`) viram `(porta, inode)`.
pub fn parse_proc_net_tcp(saida: &str) -> Vec<(u16, u64)> {
    saida.lines().filter_map(|linha| {
        let campos: Vec<&str> = linha.split_whitespace().collect();
        if campos.get(3) != Some(&"0A") { return None; }
        let porta = u16::from_str_radix(campos.get(1)?.rsplit(':').next()?, 16).ok()?;
        Some((porta, campos.get(9)?.parse().ok()?))
    }).collect()
}

/// Nome, linha de comando e pasta de trabalho de um processo.
#[cfg(unix)]
pub struct DadosDoProcesso { pub nome: String, pub cmd: String, pub pasta: Option<String> }

#[cfg(unix)]
fn rodar(programa: &str, args: &[&str]) -> Result<String, String> {
    let saida = std::process::Command::new(programa).args(args).output().map_err(|erro| format!("{programa}: {erro}"))?;
    tratar_saida(saida.status.success(), &saida.stdout, &saida.stderr, programa)
}

/// Resultado de um comando de leitura. `lsof` sai com 1 e nada em stdout/stderr quando não há
/// escuta nenhuma: isso é lista vazia, não erro.
pub fn tratar_saida(sucesso: bool, stdout: &[u8], stderr: &[u8], programa: &str) -> Result<String, String> {
    if !sucesso && stdout.is_empty() && !stderr.iter().all(u8::is_ascii_whitespace) {
        return Err(format!("{programa}: {}", String::from_utf8_lossy(stderr).trim()));
    }
    Ok(String::from_utf8_lossy(stdout).into_owned())
}

/// Portas em escuta com o dono. macOS usa `lsof`; Linux usa `ss` e, se ele faltar, `/proc/net/tcp`.
#[cfg(unix)]
pub fn escutas() -> Result<Vec<Escuta>, String> {
    if cfg!(target_os = "macos") {
        return rodar("lsof", &["-nP", "-iTCP", "-sTCP:LISTEN"]).map(|s| parse_lsof(&s));
    }
    match std::process::Command::new("ss").args(["-ltnpH"]).output() {
        Ok(saida) if saida.status.success() => Ok(parse_ss(&String::from_utf8_lossy(&saida.stdout))),
        _ => escutas_do_proc(),
    }
}

/// Sem `ss`: cruza as portas de `/proc/net/tcp{,6}` com os sockets abertos de cada processo (`/proc/<pid>/fd`).
#[cfg(unix)]
fn escutas_do_proc() -> Result<Vec<Escuta>, String> {
    let mut portas = Vec::new();
    for arquivo in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(texto) = std::fs::read_to_string(arquivo) { portas.extend(parse_proc_net_tcp(&texto)); }
    }
    let pastas = std::fs::read_dir("/proc").map_err(|erro| erro.to_string())?;
    let mut lista = Vec::new();
    for pasta in pastas.flatten() {
        let Some(pid) = pasta.file_name().to_str().and_then(|n| n.parse::<u32>().ok()) else { continue };
        let Ok(fds) = std::fs::read_dir(pasta.path().join("fd")) else { continue };
        for fd in fds.flatten() {
            let Ok(alvo) = std::fs::read_link(fd.path()) else { continue };
            let Some(inode) = alvo.to_str().and_then(|a| a.strip_prefix("socket:[")).and_then(|a| a.strip_suffix(']')) else { continue };
            let Some((porta, _)) = portas.iter().find(|(_, i)| inode.parse() == Ok(*i)) else { continue };
            let escuta = Escuta { p: *porta, pid };
            if !lista.contains(&escuta) { lista.push(escuta); }
        }
    }
    Ok(lista)
}

/// `/proc/<pid>/{comm,cmdline,cwd}` no Linux; `ps` e `lsof -d cwd` no macOS (que não tem `/proc`).
#[cfg(unix)]
pub fn dados_do_processo(pid: u32) -> Option<DadosDoProcesso> {
    let proc = std::path::PathBuf::from(format!("/proc/{pid}"));
    if proc.join("comm").exists() {
        let nome = std::fs::read_to_string(proc.join("comm")).ok()?.trim().to_owned();
        let bruto = std::fs::read(proc.join("cmdline")).unwrap_or_default();
        let cmd = String::from_utf8_lossy(&bruto).split('\0').filter(|a| !a.is_empty()).collect::<Vec<_>>().join(" ");
        let pasta = std::fs::read_link(proc.join("cwd")).ok().map(|p| p.display().to_string());
        return Some(DadosDoProcesso { nome, cmd, pasta });
    }
    let p = pid.to_string();
    let executavel = rodar("ps", &["-ww", "-o", "comm=", "-p", &p]).ok()?;
    let nome = executavel.trim().rsplit('/').next()?.to_owned();
    let cmd = rodar("ps", &["-ww", "-o", "args=", "-p", &p]).ok()?.trim().to_owned();
    let pasta = rodar("lsof", &["-a", "-d", "cwd", "-Fn", "-p", &p]).ok().and_then(|s| parse_lsof_cwd(&s));
    Some(DadosDoProcesso { nome, cmd, pasta })
}

/// `lsof -Fn`: a linha do caminho começa com `n`.
#[allow(dead_code)]
pub fn parse_lsof_cwd(saida: &str) -> Option<String> {
    saida.lines().find_map(|l| l.strip_prefix('n')).map(str::to_owned)
}

/// Pares `(pid, ppid)` de todos os processos (`ps` existe no Linux e no macOS).
#[cfg(unix)]
fn pares_de_processos() -> Result<Vec<(u32, u32)>, String> {
    omni_core::processos::pares_do_sistema()
}

#[allow(unused_imports)]
pub use omni_core::processos::parse_ps_pares;

/// Vivo de verdade: zumbi (já morto, esperando o pai recolher) não conta.
#[cfg(unix)]
fn vivo(pid: u32) -> bool {
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        let estado = stat.rsplit(')').next().and_then(|r| r.trim_start().chars().next());
        return !matches!(estado, Some('Z' | 'X'));
    }
    std::process::Command::new("kill").args(["-0", &pid.to_string()]).output().is_ok_and(|s| s.status.success())
}

/// O `kill` falhar para um pid que já morreu é normal; quem confere o resultado é `confirmar_morte`.
/// Só falha de verdade (não conseguir nem rodar o `kill`) volta como erro.
#[cfg(unix)]
fn sinal(nome: &str, pids: &[u32]) -> Result<(), String> {
    if pids.is_empty() { return Ok(()); }
    std::process::Command::new("kill").arg(nome).args(pids.iter().map(u32::to_string)).output()
        .map(|_| ()).map_err(|erro| format!("Não deu para rodar o kill: {erro}"))
}

const MENSAGEM_PROPRIO: &str = "Derrubar este servidor fecharia os terminais do OMNI AGENTS (o app ou o engine está na mesma árvore). Nada foi encerrado.";

/// Decisão pura: a árvore a derrubar não pode conter o próprio processo, um ancestral dele, nem
/// um processo `omni-engine`/`omni-agents`. O engine ser ANCESTRAL do servidor é normal e não barra.
pub fn verificar_arvore(alvos: &[u32], pares: &[(u32, u32)], eu: u32, nomes: &[(u32, String)]) -> Result<(), String> {
    let mut meus_ancestrais = vec![eu];
    let mut atual = eu;
    while let Some((_, pai)) = pares.iter().find(|(pid, _)| *pid == atual) {
        if *pai == 0 || meus_ancestrais.contains(pai) { break; }
        meus_ancestrais.push(*pai);
        atual = *pai;
    }
    let do_omni = |pid: &u32| nomes.iter().any(|(p, n)| p == pid && matches!(n.as_str(), "omni-engine" | "omni-agents"));
    if alvos.iter().any(|pid| meus_ancestrais.contains(pid) || do_omni(pid)) { return Err(MENSAGEM_PROPRIO.to_owned()); }
    Ok(())
}

/// `Err` com a causa se algum dos `pids` ainda vive.
pub fn confirmar_morte(pids: &[u32], vivo: impl Fn(u32) -> bool) -> Result<(), String> {
    match pids.iter().find(|p| vivo(**p)) {
        Some(pid) => Err(format!("Não deu para encerrar o processo {pid}: ele continuou vivo mesmo depois do KILL.")),
        None => Ok(()),
    }
}

/// Derruba `raiz` e tudo que desceu dela: TERM em cada um, espera até `espera` o que ainda vive
/// sair, e KILL no que sobrou. Recusa tudo, sem matar nada, se a árvore atingir o próprio OMNI.
/// No fim confere quem sobreviveu e devolve erro se sobrar alguém.
#[cfg(unix)]
pub fn derrubar_arvore(raiz: u32, espera: std::time::Duration) -> Result<(), String> {
    let pares = pares_de_processos()?;
    let mut alvos = omni_core::processos::descendentes(raiz, &pares);
    alvos.push(raiz);
    let nomes: Vec<(u32, String)> = alvos.iter().filter_map(|p| Some((*p, dados_do_processo(*p)?.nome))).collect();
    verificar_arvore(&alvos, &pares, std::process::id(), &nomes)?;
    sinal("-TERM", &alvos)?;
    let limite = std::time::Instant::now() + espera;
    while std::time::Instant::now() < limite && alvos.iter().any(|p| vivo(*p)) {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let sobrou: Vec<u32> = alvos.into_iter().filter(|p| vivo(*p)).collect();
    sinal("-KILL", &sobrou)?;
    std::thread::sleep(std::time::Duration::from_millis(200));
    confirmar_morte(&sobrou, vivo)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ss_le_porta_e_pid() {
        let linha = r#"LISTEN 0 511 *:5173 *:* users:(("node",pid=4321,fd=23))"#;
        assert_eq!(parse_ss(linha), vec![Escuta { p: 5173, pid: 4321 }]);
    }

    #[test]
    fn ss_le_ipv6_e_varios_donos() {
        let saida = "LISTEN 0 4096 [::1]:3000 [::]:* users:((\"next-server (v1\",pid=77,fd=19))\n\
LISTEN 0 128 127.0.0.1:8000 0.0.0.0:* users:((\"python3\",pid=5,fd=3),(\"python3\",pid=6,fd=3))\n";
        assert_eq!(parse_ss(saida), vec![
            Escuta { p: 3000, pid: 77 }, Escuta { p: 8000, pid: 5 }, Escuta { p: 8000, pid: 6 },
        ]);
    }

    #[test]
    fn ss_ignora_linha_sem_dono() {
        let saida = "LISTEN 0 128 0.0.0.0:22 0.0.0.0:*\nLISTEN 0 511 *:5173 *:* users:((\"node\",pid=4321,fd=23))\n";
        assert_eq!(parse_ss(saida), vec![Escuta { p: 5173, pid: 4321 }]);
    }

    #[test]
    fn lsof_le_porta_e_pid_e_pula_cabecalho() {
        let saida = "COMMAND   PID USER   FD   TYPE DEVICE SIZE/OFF NODE NAME\n\
node    12345    e   23u  IPv6 0x1234      0t0  TCP *:5173 (LISTEN)\n\
node    12345    e   24u  IPv4 0x1235      0t0  TCP 127.0.0.1:5174 (LISTEN)\n\
rapportd  600    e    8u  IPv4 0x1236      0t0  TCP *:49152 (LISTEN)\n";
        assert_eq!(parse_lsof(saida), vec![
            Escuta { p: 5173, pid: 12345 }, Escuta { p: 5174, pid: 12345 }, Escuta { p: 49152, pid: 600 },
        ]);
    }

    #[test]
    fn proc_net_tcp_pega_so_listen() {
        let saida = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 00000000:1435 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 98765 1 0000 100 0 0 10 0\n\
   1: 0100007F:E2A8 0100007F:1435 01 00000000:00000000 00:00000000 00000000  1000        0 11111 1 0000 100 0 0 10 0\n";
        assert_eq!(parse_proc_net_tcp(saida), vec![(5173, 98765)]);
    }

    #[test]
    fn lsof_cwd_le_a_linha_do_caminho() {
        assert_eq!(parse_lsof_cwd("p123\nfcwd\nn/Users/e/p\n"), Some("/Users/e/p".to_owned()));
    }

    #[test]
    fn ps_le_pares() {
        assert_eq!(parse_ps_pares("    1     0\n  200   1\nlixo\n"), vec![(1, 0), (200, 1)]);
    }

    /// Só age em processo que o próprio teste criou: um `sh` com um neto `sleep`.
    #[cfg(unix)]
    #[test]
    fn derrubar_arvore_leva_filho_e_neto() {
        use std::process::{Command, Stdio};
        let mut filho = Command::new("sh").args(["-c", "sleep 300 & wait"]).stdout(Stdio::null()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        let neto = omni_core::processos::descendentes(filho.id(), &pares_de_processos().unwrap());
        assert_eq!(neto.len(), 1, "o sleep deveria descender do sh");
        derrubar_arvore(filho.id(), std::time::Duration::from_secs(2)).unwrap();
        filho.wait().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!vivo(neto[0]), "o neto sobreviveu");
    }

    #[cfg(unix)]
    fn nomes(l: &[(u32, &str)]) -> Vec<(u32, String)> { l.iter().map(|(p, n)| (*p, (*n).to_owned())).collect() }

    #[cfg(unix)]
    #[test]
    fn arvore_com_o_proprio_processo_e_recusada() {
        let pares = [(10, 1), (20, 10), (30, 20)];
        let e = verificar_arvore(&[30, 20, 99], &pares, 99, &nomes(&[(30, "node"), (20, "sh"), (99, "omni-agents")])).unwrap_err();
        assert!(e.contains("terminais do OMNI"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn arvore_com_ancestral_do_omni_e_recusada() {
        // eu (30) desce de 10; derrubar 10 levaria o OMNI junto.
        let pares = [(10, 1), (20, 10), (30, 20)];
        assert!(verificar_arvore(&[10], &pares, 30, &nomes(&[(10, "bash")])).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn arvore_com_engine_ou_app_pelo_nome_e_recusada() {
        let pares = [(5, 1), (6, 5)];
        assert!(verificar_arvore(&[6, 5], &pares, 999, &nomes(&[(5, "omni-engine"), (6, "node")])).is_err());
        assert!(verificar_arvore(&[5], &pares, 999, &nomes(&[(5, "omni-agents")])).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn servidor_filho_de_terminal_do_omni_pode_cair() {
        // engine(5) -> pty/sh(6) -> npm(7) -> node(8). Cair o 7 não atinge o engine, que é só ancestral.
        let pares = [(5, 1), (6, 5), (7, 6), (8, 7), (999, 1)];
        let n = nomes(&[(5, "omni-engine"), (6, "sh"), (7, "npm"), (8, "node")]);
        assert_eq!(verificar_arvore(&[8, 7], &pares, 999, &n), Ok(()));
    }

    #[cfg(unix)]
    #[test]
    fn confirmar_morte_acusa_quem_sobrou() {
        assert_eq!(confirmar_morte(&[1, 2], |_| false), Ok(()));
        let e = confirmar_morte(&[1, 2], |p| p == 2).unwrap_err();
        assert!(e.contains('2') && e.contains("KILL"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn derrubar_arvore_mata_ate_quem_ignora_term() {
        use std::process::{Command, Stdio};
        let mut filho = Command::new("sh").args(["-c", "trap '' TERM; while :; do sleep 1; done"]).stdout(Stdio::null()).spawn().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(300));
        derrubar_arvore(filho.id(), std::time::Duration::from_millis(300)).unwrap();
        assert!(!vivo(filho.id()));
        filho.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn saida_1_sem_nada_e_lista_vazia() {
        assert_eq!(tratar_saida(false, b"", b"", "lsof"), Ok(String::new()));
        assert!(tratar_saida(false, b"", b"lsof: boom\n", "lsof").unwrap_err().contains("boom"));
        assert_eq!(tratar_saida(true, b"x", b"", "ps"), Ok("x".to_owned()));
    }
}
