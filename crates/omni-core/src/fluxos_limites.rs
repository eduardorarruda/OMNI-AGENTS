//! O que o autor de um fluxo pode pedir para restringir: ramos ao mesmo tempo, voltas, ferramentas
//! proibidas e caminhos de escrita. Nada aqui é imposto: ausente é sem limite. Quem aplica é o engine;
//! a regra de cada texto que vira argumento de linha de comando mora aqui para ser testada sem abrir CLI.
//!
//! Medido no `claude -p` 2.1.318: `acceptEdits` ignora regra de caminho, então escrita limitada exige
//! `--permission-mode default` com `Edit(./caminho)` (`Write(caminho)` sozinho nega tudo). Uma regra
//! `allow` em settings alarga o limite, daí `--setting-sources ""`. Bash escreve onde quiser, daí o
//! bloqueio do shell: o sandbox da CLI não abre exceção dentro da pasta de trabalho.
use super::No;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

/// Ferramentas que gravam arquivo. O limite de caminho vale para elas.
pub const FERRAMENTAS_DE_ESCRITA: [&str; 4] = ["Write", "Edit", "MultiEdit", "NotebookEdit"];
/// Escrevem em qualquer lugar sem passar pelas regras de caminho.
pub const FERRAMENTAS_DE_SHELL: [&str; 2] = ["Bash", "PowerShell"];
const TAMANHO_MAXIMO: usize = 300;

/// Quantas etapas rodam ao mesmo tempo: o que o autor definiu; sem isso, todas.
pub fn limite_paralelo(f: &super::Fluxo) -> usize {
    f.max_paralelo.map_or(usize::MAX, |n| n.max(1) as usize)
}

/// Quantas reprovações ou falhas o nó aguenta antes de sair por `esgotado`. Ausente: sem limite, e a
/// parada é o botão Parar.
pub fn max_voltas(no: &No) -> u32 {
    no.config["max_voltas"].as_u64().map_or(u32::MAX, |n| u32::try_from(n.max(1)).unwrap_or(u32::MAX))
}

/// Rodadas da Sala. Ausente: 3.
pub fn rodadas_da_sala(no: &No) -> u32 {
    u32::try_from(no.config["rodadas"].as_u64().unwrap_or(3).max(1)).unwrap_or(u32::MAX)
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

/// Só o que impediria achar a pasta: `~` e variável não são expandidos. Onde ela fica é com o autor.
pub fn validar_pasta_extra(pasta: &str) -> Result<(), String> {
    let erro = |motivo: &str| Err(format!("Pasta extra “{pasta}”: {motivo}"));
    if pasta.trim().is_empty() || pasta.len() > TAMANHO_MAXIMO || pasta.chars().any(char::is_control) { return erro("vazia ou inválida"); }
    if pasta.starts_with('~') || pasta.contains(['$', '%', '*', '?']) { return erro("escreva o caminho sem ~, variável ou curinga"); }
    Ok(())
}

/// Sem o `\\?\` do Windows, que a CLI não entende.
pub fn texto_do_caminho(caminho: &Path) -> String {
    let texto = caminho.to_string_lossy();
    texto.strip_prefix(r"\\?\").unwrap_or(&texto).to_owned()
}

/// O caminho real de uma pasta extra, relativo ao projeto. Precisa existir: `--add-dir` de pasta que
/// não existe só falharia lá na frente.
pub fn resolver_pasta_extra(projeto: &Path, pasta: &str) -> Result<PathBuf, String> {
    validar_pasta_extra(pasta)?;
    projeto.join(pasta).canonicalize().ok().filter(|alvo| alvo.is_dir()).ok_or_else(|| format!("Pasta extra “{pasta}”: não existe neste PC"))
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
    use super::super::TipoDeNo;

    fn lista(itens: &[&str]) -> Vec<String> { itens.iter().map(|i| (*i).to_owned()).collect() }

    #[test]
    fn caminho_de_escrita_nao_sai_da_pasta() {
        for bom in ["src", "src/", "./docs/a.md", "src/**", "*.md", ".", "ação/x"] { validar_escrita(bom).unwrap_or_else(|e| panic!("{e}")); }
        for ruim in ["", "/etc", "~/x", "../fora", "a/../../b", "C:/x", "a b", "a,b", "a)", "-x", "a\\b"] {
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

    fn com_config(config: Value) -> No { No { config, ..agente(None, &[]) } }

    #[test]
    fn sala_aceita_20_rodadas() {
        assert_eq!(rodadas_da_sala(&com_config(json!({"rodadas": 20}))), 20);
        assert_eq!(rodadas_da_sala(&com_config(json!({}))), 3, "ausente segue 3: a Sala para no consenso ou nas rodadas");
        assert_eq!(rodadas_da_sala(&com_config(json!({"rodadas": 0}))), 1);
    }

    #[test]
    fn max_voltas_ausente_nao_tem_limite() {
        assert_eq!(max_voltas(&com_config(json!({}))), u32::MAX);
        assert_eq!(max_voltas(&com_config(json!({"max_voltas": 50}))), 50);
        assert_eq!(max_voltas(&com_config(json!({"max_voltas": 0}))), 1);
        assert_eq!(max_voltas(&com_config(json!({"max_voltas": u64::MAX}))), u32::MAX, "número grande não dá a volta");
    }

    #[test]
    fn escrita_aceita_git_e_claude() {
        for caminho in [".git/hooks", ".claude/settings.json", "./.omni/fluxos/x.json", ".git"] { validar_escrita(caminho).unwrap_or_else(|e| panic!("{e}")); }
    }

    #[test]
    fn paralelo_ausente_nao_tem_teto() {
        let mut f: super::super::Fluxo = serde_json::from_value(json!({"id": "a", "nome": "A", "nos": [], "conexoes": []})).unwrap();
        assert_eq!(limite_paralelo(&f), usize::MAX);
        f.max_paralelo = Some(50);
        assert_eq!(limite_paralelo(&f), 50);
    }

    #[cfg(unix)]
    #[test]
    fn pasta_extra_aceita_pasta_do_sistema() {
        let raiz = tempfile::tempdir().unwrap();
        let projeto = raiz.path().join("casa/trabalho/proj");
        std::fs::create_dir_all(raiz.path().join("casa/.ssh")).unwrap();
        std::fs::create_dir_all(&projeto).unwrap();
        for pasta in ["/etc", "/usr/share", "/", "..", "../..", "../../.ssh"] { resolver_pasta_extra(&projeto, pasta).unwrap_or_else(|e| panic!("{e}")); }
        validar_pasta_extra("/").unwrap();
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
    fn pasta_extra_e_resolvida_e_precisa_existir() {
        let raiz = tempfile::tempdir().unwrap();
        let projeto = raiz.path().join("trabalho/proj");
        for pasta in [projeto.join("sub"), raiz.path().join("trabalho/irmao")] { std::fs::create_dir_all(pasta).unwrap(); }
        assert_eq!(resolver_pasta_extra(&projeto, "../irmao").unwrap(), raiz.path().join("trabalho/irmao").canonicalize().unwrap());
        assert_eq!(resolver_pasta_extra(&projeto, "sub").unwrap(), projeto.join("sub").canonicalize().unwrap());
        for ruim in ["nao-existe", "~", "~/x", "$HOME/x", "sub/*"] { assert!(resolver_pasta_extra(&projeto, ruim).is_err(), "{ruim}"); }
    }
}
