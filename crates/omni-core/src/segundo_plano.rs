//! O que roda em segundo plano num projeto: sessões esperando agente, subagentes do Claude e fluxos.
//! Sem I/O de processo: só lê os `.jsonl` que o Claude Code grava em `<conversa>/subagents/`.
use serde::Serialize;
use serde_json::Value;
use std::{fs, io::{Read, Seek, SeekFrom}, path::Path};

/// Subagente que escreveu no transcript há menos que isto conta como ativo.
pub const SUBAGENTE_ATIVO_MS: u64 = 2 * 60 * 1000;
/// Subagente que terminou há menos que isto continua no painel, na seção "Concluídos".
pub const CONCLUIDO_VISIVEL_MS: u64 = 60 * 60 * 1000;
/// Só o fim do transcript importa para a "última ação": não relê arquivo de megabytes a cada consulta.
const CAUDA_BYTES: u64 = 64 * 1024;
const ACAO_MAX: usize = 140;
const NOME_MAX: usize = 70;

/// Uma linha do painel. `tipo`: `sessao`, `subagente` ou `fluxo`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ItemDeFundo {
    pub tipo: String,
    pub titulo: String,
    pub atividade: String,
    /// `None` quando a fonte não sabe quando começou: a tela fica sem tempo em vez de mostrar um errado.
    pub desde_ms: Option<u64>,
    pub projeto_id: String,
    /// Conversa do OMNI a abrir/parar (`sessao` e `subagente`).
    pub sessao: Option<String>,
    /// Execução de fluxo (`fluxo`).
    pub execucao: Option<String>,
    /// Id do subagente (`subagente`): com `sessao`, abre o detalhe dele.
    pub subagente: Option<String>,
    /// Quando o subagente terminou. `Some` = vai para "Concluídos", não para a lista do que roda.
    pub concluido_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SubagenteAtivo { pub id: String, pub nome: String, pub ultima_acao: String, pub desde_ms: u64, pub atualizado_ms: u64,
    /// Terminou (`end_turn`, `SubagentHandback` ou `finished` no `.meta.json`).
    pub concluido: bool }

/// Subagentes de `pasta` (`<conversa>/subagents`): os que escreveram nos últimos 2 minutos sem ter
/// terminado e os que terminaram na última hora (`concluido`).
pub fn subagentes_ativos(pasta: &Path, agora_ms: u64) -> Vec<SubagenteAtivo> {
    let Ok(entradas) = fs::read_dir(pasta) else { return Vec::new() };
    let mut ativos: Vec<SubagenteAtivo> = entradas.flatten().filter_map(|e| ler_subagente(&e.path(), agora_ms)).collect();
    ativos.sort_by_key(|a| a.desde_ms);
    ativos
}

fn ler_subagente(caminho: &Path, agora_ms: u64) -> Option<SubagenteAtivo> {
    if caminho.extension()?.to_str()? != "jsonl" { return None; }
    let atualizado_ms = fs::metadata(caminho).ok()?.modified().ok()?
        .duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    let idade = agora_ms.saturating_sub(atualizado_ms);
    if idade > CONCLUIDO_VISIVEL_MS { return None; }
    let meta = fs::read_to_string(caminho.with_extension("meta.json")).ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok()).unwrap_or(Value::Null);
    let cauda = cauda(caminho);
    let concluido = meta["finished"] == true || terminou(&cauda);
    if !concluido && idade > SUBAGENTE_ATIVO_MS { return None; }
    let nome_do_arquivo = caminho.file_stem()?.to_str()?;
    let id = nome_do_arquivo.strip_prefix("agent-").unwrap_or(nome_do_arquivo).to_string();
    let primeira = primeira_linha(caminho);
    let descricao = meta["description"].as_str().map(str::to_string).filter(|d| !d.trim().is_empty());
    let nome = descricao.or_else(|| primeira.as_ref().and_then(texto_do_usuario)).map_or_else(|| id.clone(), |t| cortar(&t, NOME_MAX));
    let desde_ms = primeira.as_ref().and_then(|l| crate::conversations::timestamp(&l["timestamp"])).unwrap_or(atualizado_ms);
    // O comando cru pode ter chave ou senha, e a lista chega ao celular: passa por `sem_segredos`.
    let ultima_acao = crate::exportar::sem_segredos(&cauda.iter().rev().filter(|v| v["type"] == "assistant").find_map(acao_do_assistente).unwrap_or_default());
    Some(SubagenteAtivo { id, nome: crate::exportar::sem_segredos(&nome), ultima_acao, desde_ms, atualizado_ms, concluido })
}

fn primeira_linha(caminho: &Path) -> Option<Value> {
    use std::io::BufRead;
    let leitor = std::io::BufReader::new(fs::File::open(caminho).ok()?);
    leitor.lines().map_while(Result::ok).find_map(|l| serde_json::from_str::<Value>(&l).ok())
}

fn texto_do_usuario(linha: &Value) -> Option<String> {
    let conteudo = &linha["message"]["content"];
    let texto = conteudo.as_str().map(str::to_string).or_else(||
        conteudo.as_array()?.iter().find_map(|p| p["text"].as_str().map(str::to_string)))?;
    let limpo = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    (!limpo.is_empty()).then_some(limpo)
}

fn cortar(texto: &str, max: usize) -> String {
    if texto.chars().count() <= max { return texto.to_string(); }
    format!("{}…", texto.chars().take(max - 1).collect::<String>().trim_end())
}

/// As linhas válidas do fim do arquivo; linha cortada ou corrompida é ignorada.
fn cauda(caminho: &Path) -> Vec<Value> {
    let ler = || -> Option<Vec<Value>> {
        let mut arquivo = fs::File::open(caminho).ok()?;
        let tamanho = arquivo.metadata().ok()?.len();
        let inicio = tamanho.saturating_sub(CAUDA_BYTES);
        arquivo.seek(SeekFrom::Start(inicio)).ok()?;
        let mut bytes = Vec::new();
        arquivo.read_to_end(&mut bytes).ok()?;
        let texto = String::from_utf8_lossy(&bytes);
        let mut linhas = texto.lines();
        if inicio > 0 { linhas.next(); } // começa no meio de uma linha
        Some(linhas.filter_map(|l| serde_json::from_str::<Value>(l).ok()).collect())
    };
    ler().unwrap_or_default()
}

/// Terminou: a última fala do assistente tem `end_turn`, ou ele entregou pelo `SubagentHandback`.
fn terminou(cauda: &[Value]) -> bool {
    let entregou = cauda.iter().any(|v| v["type"] == "assistant" && v["message"]["content"].as_array()
        .is_some_and(|partes| partes.iter().any(|p| p["type"] == "tool_use" && p["name"] == "SubagentHandback")));
    entregou || cauda.iter().rev().find(|v| v["type"] == "assistant").is_some_and(|v| v["message"]["stop_reason"] == "end_turn")
}

fn acao_do_assistente(linha: &Value) -> Option<String> {
    let partes = linha["message"]["content"].as_array()?;
    if let Some(ferramenta) = partes.iter().rev().find(|p| p["type"] == "tool_use") {
        let nome = ferramenta["name"].as_str().unwrap_or("ferramenta");
        let entrada = &ferramenta["input"];
        let alvo = ["command", "file_path", "pattern", "path", "description", "prompt", "url", "query"].iter()
            .find_map(|c| entrada[*c].as_str()).unwrap_or("");
        let alvo = alvo.split_whitespace().collect::<Vec<_>>().join(" ");
        return Some(cortar(format!("{nome} {alvo}").trim(), ACAO_MAX));
    }
    let fala = partes.iter().rev().find_map(|p| (p["type"] == "text").then(|| p["text"].as_str()).flatten())?;
    let fala = fala.split_whitespace().collect::<Vec<_>>().join(" ");
    (!fala.is_empty()).then(|| cortar(&fala, ACAO_MAX))
}

/// Mantém só os itens do projeto pedido (sem filtro, todos).
pub fn filtrar_por_projeto(itens: Vec<ItemDeFundo>, projeto: Option<&str>) -> Vec<ItemDeFundo> {
    match projeto {
        Some(id) => itens.into_iter().filter(|i| i.projeto_id == id).collect(),
        None => itens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AGORA: u64 = 1_800_000_000_000;

    fn linha_usuario(ts: &str, texto: &str) -> String {
        serde_json::json!({"type":"user","timestamp":ts,"message":{"role":"user","content":texto}}).to_string()
    }
    fn linha_ferramenta(ts: &str, nome: &str, entrada: Value) -> String {
        serde_json::json!({"type":"assistant","timestamp":ts,"message":{"role":"assistant","content":[
            {"type":"text","text":"vou ver"},{"type":"tool_use","name":nome,"input":entrada}]}}).to_string()
    }
    fn escrever(pasta: &Path, id: &str, linhas: &[String], idade_ms: u64) {
        fs::create_dir_all(pasta).unwrap();
        let arquivo = pasta.join(format!("agent-{id}.jsonl"));
        fs::write(&arquivo, linhas.join("\n")).unwrap();
        let quando = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(AGORA - idade_ms);
        fs::OpenOptions::new().write(true).open(&arquivo).unwrap().set_modified(quando).unwrap();
    }

    #[test]
    fn transcript_recente_e_ativo() {
        let dir = tempfile::tempdir().unwrap();
        escrever(dir.path(), "a1", &[linha_usuario("2026-11-06T18:00:00.000Z", "faça X")], 10_000);
        let ativos = subagentes_ativos(dir.path(), AGORA);
        assert_eq!(ativos.len(), 1);
        assert_eq!(ativos[0].id, "a1");
        assert_eq!(ativos[0].atualizado_ms, AGORA - 10_000);
    }

    #[test]
    fn transcript_velho_nao_entra() {
        let dir = tempfile::tempdir().unwrap();
        escrever(dir.path(), "a1", &[linha_usuario("2026-11-06T18:00:00.000Z", "faça X")], SUBAGENTE_ATIVO_MS + 1_000);
        assert!(subagentes_ativos(dir.path(), AGORA).is_empty());
        assert!(subagentes_ativos(&dir.path().join("nao-existe"), AGORA).is_empty());
    }

    #[test]
    fn linha_corrompida_nao_derruba() {
        let dir = tempfile::tempdir().unwrap();
        escrever(dir.path(), "a1", &[linha_usuario("2026-11-06T18:00:00.000Z", "faça X"), "{isto nao e json".into(),
            linha_ferramenta("2026-11-06T18:00:05.000Z", "Bash", serde_json::json!({"command":"cargo test"})), "{\"type\":\"assistant\",\"mess".into()], 5_000);
        let ativos = subagentes_ativos(dir.path(), AGORA);
        assert_eq!(ativos.len(), 1);
        assert!(ativos[0].ultima_acao.contains("cargo test"), "{:?}", ativos[0].ultima_acao);
    }

    #[test]
    fn nome_e_ultima_acao_vem_do_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        escrever(dir.path(), "a1", &[linha_usuario("2026-11-06T18:00:00.000Z", "Revise o módulo de fluxos e liste os problemas"),
            linha_ferramenta("2026-11-06T18:00:05.000Z", "Read", serde_json::json!({"file_path":"/p/src/fluxos.rs"}))], 1_000);
        let a = &subagentes_ativos(dir.path(), AGORA)[0];
        assert_eq!(a.nome, "Revise o módulo de fluxos e liste os problemas");
        assert_eq!(a.ultima_acao, "Read /p/src/fluxos.rs");
        assert_eq!(a.desde_ms, 1_793_988_000_000, "início = timestamp da primeira linha");
        // O .meta.json, quando existe, dá o nome que o Claude Code deu à tarefa.
        fs::write(dir.path().join("agent-a1.meta.json"), r#"{"description":"Frente B"}"#).unwrap();
        assert_eq!(subagentes_ativos(dir.path(), AGORA)[0].nome, "Frente B");
    }

    #[test]
    fn comando_com_segredo_nao_chega_a_atividade() {
        let dir = tempfile::tempdir().unwrap();
        escrever(dir.path(), "a1", &[linha_usuario("2026-11-06T18:00:00.000Z", "x"),
            linha_ferramenta("2026-11-06T18:00:05.000Z", "Bash", serde_json::json!({"command":"curl -H sk-ant-abc123DEF456ghi789JKL012 https://x"}))], 1_000);
        let a = &subagentes_ativos(dir.path(), AGORA)[0];
        assert!(!a.ultima_acao.contains("sk-ant-abc123DEF456ghi789JKL012"), "{:?}", a.ultima_acao);
    }

    #[test]
    fn filtro_por_projeto() {
        let item = |p: &str| ItemDeFundo { tipo: "subagente".into(), titulo: "t".into(), atividade: "a".into(), desde_ms: Some(1),
            projeto_id: p.into(), sessao: None, execucao: None, subagente: None, concluido_ms: None };
        let lista = vec![item("a"), item("b")];
        assert_eq!(filtrar_por_projeto(lista.clone(), Some("a")).len(), 1);
        assert_eq!(filtrar_por_projeto(lista, None).len(), 2);
    }

    fn linha_entrega(ts: &str, texto: &str) -> String {
        serde_json::json!({"type":"assistant","isSidechain":true,"timestamp":ts,"message":{"role":"assistant","stop_reason":"tool_use","content":[
            {"type":"tool_use","name":"SubagentHandback","input":{"message":texto}}]}}).to_string()
    }

    #[test]
    fn concluido_continua_visivel_e_nao_conta_como_ativo() {
        let dir = tempfile::tempdir().unwrap();
        // Entregou o resultado há 10 min: some dos ativos, aparece nos concluídos.
        escrever(dir.path(), "a1", &[linha_usuario("2026-11-06T18:00:00.000Z", "x"), linha_entrega("2026-11-06T18:01:00.000Z", "feito")], 10 * 60_000);
        // Terminou com `end_turn` há 20 s: também concluído, mesmo dentro dos 2 min.
        escrever(dir.path(), "a2", &[linha_usuario("2026-11-06T18:00:00.000Z", "y"),
            serde_json::json!({"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"ok"}]}}).to_string()], 20_000);
        // Parado há 10 min sem terminar: fora das duas listas.
        escrever(dir.path(), "a3", &[linha_usuario("2026-11-06T18:00:00.000Z", "z")], 10 * 60_000);
        // Concluído há mais de uma hora: fora.
        escrever(dir.path(), "a4", &[linha_usuario("2026-11-06T18:00:00.000Z", "w"), linha_entrega("2026-11-06T18:01:00.000Z", "feito")], CONCLUIDO_VISIVEL_MS + 1_000);
        let lista = subagentes_ativos(dir.path(), AGORA);
        let concluidos: Vec<_> = lista.iter().filter(|a| a.concluido).map(|a| a.id.as_str()).collect();
        let ativos: Vec<_> = lista.iter().filter(|a| !a.concluido).map(|a| a.id.as_str()).collect();
        assert_eq!((ativos, concluidos.len()), (vec![], 2), "{lista:?}");
        assert!(concluidos.contains(&"a1") && concluidos.contains(&"a2"));
        // `finished` no .meta.json basta.
        fs::write(dir.path().join("agent-a3.meta.json"), r#"{"description":"Z","finished":true}"#).unwrap();
        assert!(subagentes_ativos(dir.path(), AGORA).iter().any(|a| a.id == "a3" && a.concluido));
    }
}
