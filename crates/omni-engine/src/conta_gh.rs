//! Conta do GitHub da sessão: troca o nome pedido (`OMNI_GH_CONTA`) pelo token do `gh`, só no
//! ambiente do processo que vai abrir. O token não passa por `meta.env`, que é gravado em
//! `sessions.json`, nem por log ou resposta de API.

use omni_core::conta_gh::{nome_valido, VARIAVEL_DA_CONTA};
use portable_pty::CommandBuilder;
use std::{io::Read, process::{Command, Stdio}, time::{Duration, Instant}};

/// O `gh` pode travar num chaveiro bloqueado; a sessão abre sem o token em vez de esperar.
const TEMPO_DO_GH: Duration = Duration::from_secs(5);
const ESPERA_DO_GH: Duration = Duration::from_millis(20);

fn conta_pedida(env: &[(String, String)]) -> Option<&str> {
    env.iter().rev().find(|(chave, _)| chave == VARIAVEL_DA_CONTA).map(|(_, conta)| conta.as_str()).filter(|conta| nome_valido(conta))
}

/// Define `GH_TOKEN` quando a sessão pede uma conta. Sem token, segue com o que o `gh` já tiver.
pub fn aplicar(command: &mut CommandBuilder, env: &[(String, String)], buscar_token: impl FnOnce(&str) -> Option<String>) {
    if let Some(token) = conta_pedida(env).and_then(buscar_token) {
        command.env("GH_TOKEN", token);
    }
}

fn token_lido(saida: &str) -> Option<String> {
    let token = saida.trim();
    (!token.is_empty() && !token.contains(char::is_whitespace)).then(|| token.to_owned())
}

/// `gh auth token` da conta. Stderr descartado de propósito: nada do `gh` sai daqui além do token.
pub fn token_da_conta(conta: &str) -> Option<String> {
    let mut comando = Command::new("gh");
    comando.args(["auth", "token", "--hostname", "github.com", "--user", conta])
        .env("GH_PROMPT_DISABLED", "1").env_remove("GH_TOKEN").env_remove("GITHUB_TOKEN")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)] { use std::os::windows::process::CommandExt; comando.creation_flags(0x0800_0000); }
    let mut filho = comando.spawn().ok()?;
    let limite = Instant::now() + TEMPO_DO_GH;
    let status = loop {
        match filho.try_wait().ok()? {
            Some(status) => break status,
            None if Instant::now() >= limite => { let _ = filho.kill(); let _ = filho.wait(); return None; }
            None => std::thread::sleep(ESPERA_DO_GH),
        }
    };
    if !status.success() { return None; }
    let mut saida = String::new();
    filho.stdout.take()?.read_to_string(&mut saida).ok()?;
    token_lido(&saida)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(conta: &str) -> Vec<(String, String)> {
        vec![("PATH".into(), "/bin".into()), (VARIAVEL_DA_CONTA.into(), conta.into())]
    }
    fn gh_token(command: &CommandBuilder) -> Option<String> {
        command.get_env("GH_TOKEN").and_then(|valor| valor.to_str()).map(str::to_owned)
    }

    #[test]
    fn conta_valida_vira_gh_token_so_no_comando() {
        let (mut comando, env) = (CommandBuilder::new("sh"), env("conta-a"));
        aplicar(&mut comando, &env, |conta| Some(format!("token-de-{conta}")));
        assert_eq!(gh_token(&comando).as_deref(), Some("token-de-conta-a"));
        assert!(env.iter().all(|(chave, valor)| chave != "GH_TOKEN" && !valor.contains("token-de")), "token foi parar no env gravado");
    }

    /// O CommandBuilder herda o ambiente: rodando dentro do OMNI, traria o token real, e `assert_eq!`
    /// o imprimiria na falha.
    fn comando_sem_token() -> CommandBuilder {
        let mut comando = CommandBuilder::new("sh");
        comando.env_remove("GH_TOKEN");
        comando
    }

    #[test]
    fn conta_invalida_nem_chama_o_gh() {
        let mut comando = comando_sem_token();
        aplicar(&mut comando, &env("--user=outra"), |_| panic!("chamou o gh"));
        assert!(gh_token(&comando).is_none());
    }

    #[test]
    fn sem_conta_ou_sem_token_segue_sem_gh_token() {
        let mut comando = comando_sem_token();
        aplicar(&mut comando, &[], |_| panic!("chamou o gh"));
        aplicar(&mut comando, &env("conta-b"), |_| None);
        assert!(gh_token(&comando).is_none());
    }

    #[test]
    fn saida_do_gh_so_vale_com_um_token() {
        assert_eq!(token_lido("abc123\n").as_deref(), Some("abc123"));
        assert_eq!(token_lido("  \n"), None);
        assert_eq!(token_lido("no oauth token found for conta-a"), None);
    }
}
