//! O que restringe uma etapa de fluxo: teto de ramos, ferramentas proibidas, caminhos de escrita e
//! pastas extras. Tudo aqui só aperta o que a etapa já faria. Quem aplica é o engine; a regra de cada
//! texto que vira argumento de linha de comando mora aqui para ser testada sem abrir CLI.
//!
//! Medido no `claude -p` 2.1.318: `acceptEdits` ignora regra de caminho, então escrita limitada exige
//! `--permission-mode default` com `Edit(./caminho)` (`Write(caminho)` sozinho nega tudo). Uma regra
//! `allow` em settings alarga o limite, daí `--setting-sources ""`. Bash escreve onde quiser, daí o
//! bloqueio do shell: o sandbox da CLI não abre exceção dentro da pasta de trabalho.
use super::{No, TipoDeNo};
use serde_json::{json, Map, Value};
use std::path::{Component, Path, PathBuf};

/// Ausente no fluxo: o quanto a máquina aguenta sem o usuário ter pedido nada.
pub const MAX_PARALELO_PADRAO: u32 = 3;
pub const MAX_PARALELO_TETO: u32 = 10;
/// Ferramentas que gravam arquivo. O limite de caminho vale para elas.
pub const FERRAMENTAS_DE_ESCRITA: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];
/// Escrevem em qualquer lugar sem passar pelas regras de caminho.
pub const FERRAMENTAS_DE_SHELL: [&str; 2] = ["Bash", "PowerShell"];
/// Pastas que mudam o que a CLI ou o OMNI executam: liberar escrita nelas alarga o que a etapa alcança.
const PASTAS_PROTEGIDAS: [&str; 3] = [".git", ".claude", ".omni"];
const PASTAS_DO_SISTEMA: [&str; 16] = ["bin", "boot", "dev", "etc", "lib", "lib64", "opt", "proc", "root", "run", "sbin", "sys", "usr", "var", "Windows", "Program Files"];
const TAMANHO_MAXIMO: usize = 300;

pub fn limite_paralelo(f: &super::Fluxo) -> usize {
    f.max_paralelo.unwrap_or(MAX_PARALELO_PADRAO).max(1) as usize
}

/// Vira argumento de `--disallowedTools`: nome de ferramenta, com padrão opcional (`Bash(rm:*)`).
pub fn validar_proibida(regra: &str) -> Result<(), String> {
    let invalida = regra.trim().is_empty() || regra.starts_with('-') || regra.len() > TAMANHO_MAXIMO || regra.chars().any(char::is_control);
    if invalida { Err(format!("Ferramenta proibida inválida: “{regra}”")) } else { Ok(()) }
}

fn sem_ponto_inicial(caminho: &str) -> &str {
    let mut resto = caminho;
    while let Some(depois) = resto.strip_prefix("./") { resto = depois; }
    resto
}

/// Caminho de `escrita`: relativo à pasta da execução, sem sair dela. Glob é aceito (`src/**`).
pub fn validar_escrita(caminho: &str) -> Result<(), String> {
    let erro = |motivo: &str| Err(format!("Caminho de escrita “{caminho}”: {motivo}"));
    if caminho.trim().is_empty() || caminho.len() > TAMANHO_MAXIMO { return erro("vazio ou longo demais"); }
    if caminho.starts_with(['/', '~', '-']) || caminho.contains(':') { return erro("use um caminho relativo à pasta do projeto"); }
    if !caminho.chars().all(|c| c.is_alphanumeric() || "._-/*".contains(c)) { return erro("use só letras, números e . _ - / *"); }
    let partes: Vec<&str> = sem_ponto_inicial(caminho).split('/').collect();
    if partes.contains(&"..") { return erro("não pode sair da pasta do projeto"); }
    if partes.first().is_some_and(|p| PASTAS_PROTEGIDAS.contains(p)) { return erro("essa pasta não pode ser liberada"); }
    Ok(())
}

/// Regras `allow` do Claude para `escrita`. Sem barra nem glob não dá para saber se é arquivo ou
/// pasta, então vale para os dois.
pub fn regras_de_escrita(caminhos: &[String]) -> Vec<String> {
    caminhos.iter().flat_map(|caminho| {
        let limpo = sem_ponto_inicial(caminho);
        let base = limpo.trim_end_matches('/');
        if base.is_empty() || base == "." { vec!["Edit(./**)".to_owned()] }
        else if base.contains('*') { vec![format!("Edit(./{base})")] }
        else if limpo.ends_with('/') { vec![format!("Edit(./{base}/**)")] }
        else { vec![format!("Edit(./{base})"), format!("Edit(./{base}/**)")] }
    }).collect()
}

fn alcanca_arquivo(ferramenta: &str) -> bool {
    let nome = ferramenta.split('(').next().unwrap_or(ferramenta).trim();
    nome == "*" || FERRAMENTAS_DE_ESCRITA.contains(&nome) || FERRAMENTAS_DE_SHELL.contains(&nome)
}

/// `--allowedTools` de uma etapa com `escrita`: `Write` solto liberaria tudo, então só as regras de
/// caminho autorizam gravar.
pub fn ferramentas_com_escrita_limitada(ferramentas: &[String], caminhos: &[String]) -> Vec<String> {
    ferramentas.iter().filter(|f| !alcanca_arquivo(f)).cloned().chain(regras_de_escrita(caminhos)).collect()
}

/// Servidores MCP citados em `ferramentas`, tirados do `.claude.json`. Sem os settings a CLI também
/// não carrega os MCP do usuário (medido: 17 ferramentas do omni-rag viram 0), então a etapa com
/// `escrita` recebe por `--mcp-config` só o que o agente declarou.
pub fn mcp_das_ferramentas(ferramentas: &[String], configuracao: &Value) -> Option<Value> {
    let servidores: Map<String, Value> = configuracao["mcpServers"].as_object()?.iter()
        .filter(|(nome, _)| ferramentas.iter().any(|f| f.strip_prefix("mcp__").and_then(|resto| resto.split("__").next()) == Some(nome.as_str())))
        .map(|(nome, servidor)| (nome.clone(), servidor.clone())).collect();
    (!servidores.is_empty()).then(|| json!({"mcpServers": servidores}))
}

/// A etapa grava sem limite de caminho e por isso alcança as `pastas_extras`. `escrita` só aceita
/// caminho dentro da pasta da execução, e sem shell não sobra outro jeito de gravar nas extras.
pub fn escreve_sem_limite(no: &No) -> bool {
    matches!(no.tipo, TipoDeNo::Agente | TipoDeNo::Sala) && no.escrita.is_none()
        && !FERRAMENTAS_DE_ESCRITA.iter().all(|f| no.proibidas.iter().any(|p| p == f))
}

/// Confere o que dá para saber sem o projeto. O resto fica para `resolver_pasta_extra`.
pub fn validar_pasta_extra(pasta: &str) -> Result<(), String> {
    let erro = |motivo: &str| Err(format!("Pasta extra “{pasta}”: {motivo}"));
    if pasta.trim().is_empty() || pasta.len() > TAMANHO_MAXIMO || pasta.chars().any(char::is_control) { return erro("vazia ou inválida"); }
    if pasta.starts_with('~') || pasta.contains(['$', '%', '*', '?']) { return erro("escreva o caminho sem ~, variável ou curinga"); }
    let caminho = Path::new(pasta);
    if caminho.is_absolute() && caminho.components().all(|c| matches!(c, Component::RootDir | Component::Prefix(_))) { return erro("não pode ser a raiz do disco"); }
    Ok(())
}

fn real(caminho: &Path) -> Option<PathBuf> { caminho.canonicalize().ok() }

/// Sem o `\\?\` do Windows, que a CLI não entende.
pub fn texto_do_caminho(caminho: &Path) -> String {
    let texto = caminho.to_string_lossy();
    texto.strip_prefix(r"\\?\").unwrap_or(&texto).to_owned()
}

fn pasta_do_sistema(caminho: &Path) -> bool {
    caminho.components().find_map(|c| match c { Component::Normal(nome) => Some(nome), _ => None })
        .is_some_and(|nome| PASTAS_DO_SISTEMA.iter().any(|sistema| nome == *sistema))
}

/// O caminho real de uma pasta extra, relativo ao projeto. Resolve symlink antes de julgar: um link
/// para a home não passa. Recusa o que contém o projeto ou a home e as pastas de configuração.
pub fn resolver_pasta_extra(projeto: &Path, pasta: &str, home: Option<&Path>) -> Result<PathBuf, String> {
    validar_pasta_extra(pasta)?;
    let erro = |motivo: &str| Err(format!("Pasta extra “{pasta}”: {motivo}"));
    let Some(alvo) = real(&projeto.join(pasta)).filter(|alvo| alvo.is_dir()) else { return erro("não existe neste PC") };
    let Some(raiz_do_projeto) = real(projeto) else { return erro("o projeto não existe neste PC") };
    if alvo.parent().is_none() { return erro("não pode ser a raiz do disco"); }
    if raiz_do_projeto.starts_with(&alvo) { return erro("não pode conter o próprio projeto"); }
    let home = home.and_then(real);
    if let Some(home) = &home {
        if home.starts_with(&alvo) { return erro("não pode ser a pasta do usuário nem conter ela"); }
        if let Ok(dentro) = alvo.strip_prefix(home) {
            let oculta = dentro.components().next().is_some_and(|c| c.as_os_str().to_string_lossy().starts_with('.'));
            return if oculta { erro("pasta de configuração do usuário não pode ser liberada") } else { Ok(alvo) };
        }
    }
    if pasta_do_sistema(&alvo) { return erro("pasta do sistema não pode ser liberada"); }
    Ok(alvo)
}

pub fn pasta_do_usuario() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from)
}

/// O executável `nome` no PATH.
pub fn executavel(nome: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let nomes: Vec<String> = if cfg!(windows) { vec![format!("{nome}.exe"), format!("{nome}.cmd")] } else { vec![nome.to_owned()] };
    std::env::split_paths(&path).find_map(|d| nomes.iter().map(|n| d.join(n)).find(|p| p.is_file()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lista(itens: &[&str]) -> Vec<String> { itens.iter().map(|i| (*i).to_owned()).collect() }

    #[test]
    fn caminho_de_escrita_nao_sai_da_pasta_nem_libera_pasta_protegida() {
        for bom in ["src", "src/", "./docs/a.md", "src/**", "*.md", ".", "ação/x"] { validar_escrita(bom).unwrap_or_else(|e| panic!("{e}")); }
        for ruim in ["", "/etc", "~/x", "../fora", "a/../../b", "C:/x", "a b", "a,b", "a)", "-x", ".git/hooks", "./.claude/settings.json", ".omni/fluxos/x.json", "a\\b"] {
            assert!(validar_escrita(ruim).is_err(), "{ruim}");
        }
    }

    #[test]
    fn escrita_vira_regra_de_edit_relativa_a_pasta() {
        assert_eq!(regras_de_escrita(&lista(&["src/", "*.md", "./docs/**", "."])), ["Edit(./src/**)", "Edit(./*.md)", "Edit(./docs/**)", "Edit(./**)"]);
        assert_eq!(regras_de_escrita(&lista(&["README.md"])), ["Edit(./README.md)", "Edit(./README.md/**)"]);
    }

    /// `Write` solto e `Bash` passariam por cima das regras de caminho.
    #[test]
    fn limite_de_escrita_tira_o_que_liberaria_tudo() {
        let ferramentas = lista(&["Read", "Write", "Edit(./**)", "MultiEdit", "NotebookEdit", "Bash", "Bash(npm test)", "PowerShell", "*", "mcp__omni-rag__ler_trecho"]);
        assert_eq!(ferramentas_com_escrita_limitada(&ferramentas, &lista(&["src/"])), ["Read", "mcp__omni-rag__ler_trecho", "Edit(./src/**)"]);
        assert_eq!(ferramentas_com_escrita_limitada(&lista(&["Read"]), &[]), ["Read"], "lista vazia é só leitura");
    }

    #[test]
    fn etapa_recebe_so_o_mcp_que_o_agente_declarou() {
        let config = serde_json::json!({"mcpServers": {"omni-rag": {"command": "a"}, "outro": {"command": "b"}}});
        let lista = mcp_das_ferramentas(&lista(&["Read", "mcp__omni-rag__ler_trecho", "mcp__omni-rag__buscar_conhecimento"]), &config).unwrap();
        assert_eq!(lista, serde_json::json!({"mcpServers": {"omni-rag": {"command": "a"}}}));
        assert!(mcp_das_ferramentas(&["Read".to_owned()], &config).is_none());
        assert!(mcp_das_ferramentas(&["mcp__sumido__x".to_owned()], &config).is_none());
        assert!(mcp_das_ferramentas(&["mcp__omni-rag__x".to_owned()], &serde_json::json!({})).is_none());
    }

    #[test]
    fn proibida_nao_vira_flag_da_cli() {
        for boa in ["Bash", "Bash(rm:*)", "mcp__x__y", "WebFetch"] { validar_proibida(boa).unwrap(); }
        for ruim in ["", "  ", "--dangerously-skip-permissions", "a\nb"] { assert!(validar_proibida(ruim).is_err(), "{ruim}"); }
    }

    fn agente(escrita: Option<&[&str]>, proibidas: &[&str]) -> No {
        No { id: "a".into(), tipo: TipoDeNo::Agente, titulo: String::new(), agente: "dev".into(), config: serde_json::json!({}), x: 0.0, y: 0.0,
            proibidas: lista(proibidas), escrita: escrita.map(lista) }
    }

    #[test]
    fn so_etapa_sem_limite_alcanca_as_pastas_extras() {
        assert!(escreve_sem_limite(&agente(None, &[])));
        assert!(escreve_sem_limite(&agente(None, &["Write"])), "bloquear uma ferramenta só não tira a escrita");
        assert!(!escreve_sem_limite(&agente(Some(&[]), &[])));
        assert!(!escreve_sem_limite(&agente(Some(&["src/"]), &[])));
        assert!(!escreve_sem_limite(&agente(None, &FERRAMENTAS_DE_ESCRITA)));
    }

    #[test]
    fn pasta_extra_e_resolvida_e_nao_alarga_para_home_nem_raiz() {
        let raiz = tempfile::tempdir().unwrap();
        let home = raiz.path().join("casa");
        let projeto = home.join("trabalho/proj");
        for pasta in [projeto.join("sub"), home.join("trabalho/irmao"), home.join(".ssh")] { std::fs::create_dir_all(pasta).unwrap(); }
        let resolver = |pasta: &str| resolver_pasta_extra(&projeto, pasta, Some(&home));
        assert_eq!(resolver("../irmao").unwrap(), home.join("trabalho/irmao").canonicalize().unwrap());
        assert_eq!(resolver("sub").unwrap(), projeto.join("sub").canonicalize().unwrap());
        for ruim in [".", "..", "../..", "../../..", "/", "~", "~/x", "$HOME/x", "../../.ssh", "nao-existe", "/etc", "/usr/share"] {
            assert!(resolver(ruim).is_err(), "{ruim}");
        }
        assert!(resolver(&home.to_string_lossy()).is_err(), "home pelo caminho absoluto");
    }

    #[cfg(unix)]
    #[test]
    fn link_para_a_home_nao_passa_como_pasta_extra() {
        let raiz = tempfile::tempdir().unwrap();
        let home = raiz.path().join("casa");
        let projeto = home.join("proj");
        std::fs::create_dir_all(&projeto).unwrap();
        std::os::unix::fs::symlink(&home, projeto.join("atalho")).unwrap();
        assert!(resolver_pasta_extra(&projeto, "atalho", Some(&home)).is_err());
    }
}
