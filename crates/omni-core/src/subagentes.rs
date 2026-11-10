//! Subagentes do Claude Code. Cada um grava `<sessão>/subagents/agent-<id>.jsonl` ao lado do transcript
//! da conversa, com um `.meta.json` (tipo, descrição e o `toolUseId` da chamada que o abriu).
use serde::Serialize;
use serde_json::Value;
use std::{io::{BufRead, BufReader}, path::{Path, PathBuf}};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Subagente {
    pub id: String,
    pub tipo: String,
    pub descricao: String,
    /// `rodando`, `concluido` ou `parado`.
    pub estado: &'static str,
    /// Última ação dele (ferramenta e alvo), para a lista mostrar o que está fazendo agora.
    pub ultima_acao: Option<String>,
    pub atualizado_ms: u64,
}

pub fn pasta(transcript: &Path) -> PathBuf {
    transcript.with_extension("").join("subagents")
}

/// Arquivo do subagente `id` desta conversa. `None` para id que não é de subagente (sem `..` nem barra).
pub fn arquivo(transcript: &Path, id: &str) -> Option<PathBuf> {
    let valido = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric());
    valido.then(|| pasta(transcript).join(format!("agent-{id}.jsonl"))).filter(|p| p.is_file())
}

/// Tudo de um subagente para a tela de detalhe. Texto passa por `sem_segredos`: chega ao celular.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detalhe {
    #[serde(flatten)]
    pub resumo: Subagente,
    pub modelo: Option<String>,
    /// Hora da primeira linha (o pedido).
    pub desde_ms: Option<u64>,
    /// O pedido que ele recebeu, inteiro.
    pub prompt: String,
    /// O que ele entregou ao terminar (`SubagentHandback` ou a última fala).
    pub resultado: Option<String>,
}

/// Subagentes da conversa, os mais recentes primeiro. `viva = false` (sessão fechada): quem não
/// terminou não está mais rodando.
pub fn listar(transcript: &Path, viva: bool) -> Vec<Subagente> {
    let Ok(entradas) = std::fs::read_dir(pasta(transcript)) else { return Vec::new() };
    let pai = CaudaDoPai::new(transcript);
    let mut lista: Vec<_> = entradas.flatten().filter_map(|entrada| {
        let id = entrada.file_name().to_str()?.strip_prefix("agent-")?.strip_suffix(".jsonl")?.to_owned();
        Some(ler(&entrada.path(), id, viva, &pai)?.resumo)
    }).collect();
    lista.sort_by_key(|s| std::cmp::Reverse(s.atualizado_ms));
    lista
}

/// Detalhe do subagente `id`; `None` para id inválido ou que não existe.
pub fn detalhe(transcript: &Path, id: &str, viva: bool) -> Option<Detalhe> {
    let caminho = arquivo(transcript, id)?;
    ler(&caminho, id.to_owned(), viva, &CaudaDoPai::new(transcript))
}

/// Subagente que não terminou mas foi parado (`killed`/`stopped` no transcript da conversa).
pub fn parado(transcript: &Path, id: &str) -> bool {
    let Some(caminho) = arquivo(transcript, id) else { return false };
    let meta = ler_meta(&caminho);
    let tool_use_id = meta["toolUseId"].as_str().unwrap_or("");
    !tool_use_id.is_empty() && CaudaDoPai::new(transcript).parou(tool_use_id)
}

/// O aviso `<task-notification>` de parada fica no fim do transcript da conversa: só a cauda é lida,
/// e só na primeira vez que alguém pergunta (subagente que terminou nem pergunta).
const CAUDA_DO_PAI: u64 = 256 * 1024;
struct CaudaDoPai<'a> { transcript: &'a Path, texto: std::cell::OnceCell<String> }
impl<'a> CaudaDoPai<'a> {
    fn new(transcript: &'a Path) -> Self { Self { transcript, texto: std::cell::OnceCell::new() } }
    fn parou(&self, tool_use_id: &str) -> bool {
        parado_no_pai(self.texto.get_or_init(|| ler_cauda(self.transcript, CAUDA_DO_PAI)), tool_use_id)
    }
}

fn ler_cauda(caminho: &Path, bytes: u64) -> String {
    use std::io::{Read, Seek, SeekFrom};
    let Ok(mut arquivo) = std::fs::File::open(caminho) else { return String::new() };
    let tamanho = arquivo.metadata().map_or(0, |m| m.len());
    if arquivo.seek(SeekFrom::Start(tamanho.saturating_sub(bytes))).is_err() { return String::new(); }
    let mut lido = Vec::new();
    let _ = arquivo.read_to_end(&mut lido);
    String::from_utf8_lossy(&lido).into_owned()
}

fn ler_meta(caminho: &Path) -> Value {
    std::fs::read(caminho.with_extension("meta.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
}

fn ler(caminho: &Path, id: String, viva: bool, pai: &CaudaDoPai) -> Option<Detalhe> {
    let meta = ler_meta(caminho);
    let atualizado_ms = std::fs::metadata(caminho).ok()?.modified().ok()?
        .duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64;
    let leitura = percorrer(caminho);
    let terminou = leitura.terminou || meta["finished"] == true;
    let tool_use_id = meta["toolUseId"].as_str().unwrap_or("");
    let estado = if terminou { "concluido" }
        else if !viva || (!tool_use_id.is_empty() && pai.parou(tool_use_id)) { "parado" }
        else { "rodando" };
    let mascarar = |t: String| crate::exportar::sem_segredos(&t);
    Some(Detalhe {
        resumo: Subagente {
            id,
            tipo: meta["agentType"].as_str().unwrap_or("Agente").to_owned(),
            descricao: mascarar(meta["description"].as_str().unwrap_or_default().to_owned()),
            estado, ultima_acao: leitura.ultima_acao.map(mascarar), atualizado_ms,
        },
        modelo: leitura.modelo.or_else(|| meta["model"].as_str().map(str::to_owned)),
        desde_ms: leitura.desde_ms,
        prompt: mascarar(leitura.prompt.unwrap_or_default()),
        resultado: terminou.then_some(leitura.resultado).flatten().map(mascarar),
    })
}

#[derive(Default, Clone)]
struct Leitura { terminou: bool, entregou: bool, ultima_acao: Option<String>, prompt: Option<String>, desde_ms: Option<u64>, modelo: Option<String>, resultado: Option<String> }

/// Até onde cada `.jsonl` de subagente já foi lido: o detalhe é relido a cada 3 s, e só o que a CLI
/// acrescentou desde a última vez é processado.
#[derive(Clone)]
struct Lido { offset: u64, tamanho: u64, modificado: Option<std::time::SystemTime>, leitura: Leitura }
const CACHE_MAX: usize = 256;
fn lidos() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, Lido>> {
    static M: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<PathBuf, Lido>>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Pedido (1ª fala do usuário), modelo, última ação e o fim, lendo só o que é novo no arquivo.
fn percorrer(caminho: &Path) -> Leitura {
    use std::io::{Seek, SeekFrom};
    let Ok(mut arquivo) = std::fs::File::open(caminho) else { return Leitura::default() };
    let metadados = arquivo.metadata().ok();
    let tamanho = metadados.as_ref().map_or(0, |m| m.len());
    let modificado = metadados.and_then(|m| m.modified().ok());
    let anterior = lidos().lock().ok().and_then(|m| m.get(caminho).cloned());
    let mut lido = match anterior {
        Some(l) if l.tamanho == tamanho && l.modificado == modificado => return l.leitura,
        // Cresceu: continua de onde parou. Menor: foi trocado, recomeça.
        Some(l) if tamanho >= l.offset => l,
        _ => Lido { offset: 0, tamanho: 0, modificado: None, leitura: Leitura::default() },
    };
    if arquivo.seek(SeekFrom::Start(lido.offset)).is_err() { return lido.leitura; }
    let mut leitor = BufReader::new(arquivo);
    let mut linha = Vec::new();
    let mut provisoria = None;
    loop {
        linha.clear();
        let Ok(n) = leitor.read_until(b'\n', &mut linha) else { break };
        if n == 0 { break; }
        // Sem `\n` no fim: a linha pode estar no meio da escrita. Se já é JSON inteiro, vale para esta
        // resposta, mas não entra no que fica guardado — a próxima leitura recomeça dela.
        if linha.last() != Some(&b'\n') { provisoria = serde_json::from_slice::<Value>(&linha).ok(); break; }
        lido.offset += n as u64;
        if let Ok(valor) = serde_json::from_slice::<Value>(&linha) { acumular(&mut lido.leitura, &valor); }
    }
    lido.tamanho = tamanho;
    lido.modificado = modificado;
    let mut leitura = lido.leitura.clone();
    if let Some(valor) = provisoria {
        acumular(&mut leitura, &valor);
        // Guardado sem a linha provisória: tamanho diferente força a releitura dela.
        lido.tamanho = lido.offset;
    }
    if let Ok(mut mapa) = lidos().lock() {
        if mapa.len() >= CACHE_MAX && !mapa.contains_key(caminho) { mapa.clear(); }
        mapa.insert(caminho.to_path_buf(), lido);
    }
    leitura
}

fn acumular(l: &mut Leitura, valor: &Value) {
    let mensagem = &valor["message"];
    if l.prompt.is_none() && valor["type"] == "user" {
        l.prompt = texto_do_pedido(&mensagem["content"]);
        l.desde_ms = crate::conversations::timestamp(&valor["timestamp"]);
    }
    if valor["type"] == "assistant" {
        l.terminou = l.entregou || mensagem["stop_reason"] == "end_turn";
        if let Some(m) = mensagem["model"].as_str().filter(|m| !m.starts_with('<')) { l.modelo = Some(m.to_owned()); }
        for bloco in mensagem["content"].as_array().into_iter().flatten() {
            if bloco["type"] == "tool_use" && bloco["name"] == "SubagentHandback" {
                l.entregou = true;
                l.terminou = true;
                l.resultado = bloco["input"]["message"].as_str().map(str::to_owned);
            } else if bloco["type"] == "text" && !l.entregou {
                l.resultado = bloco["text"].as_str().filter(|t| !t.trim().is_empty()).map(str::to_owned).or(l.resultado.take());
            }
        }
    }
    for item in crate::feed::items_do_subagente(valor) {
        if item.kind == crate::feed::Kind::Tool {
            l.ultima_acao = Some(format!("{} {}", item.tool.unwrap_or_default(), item.text).trim().to_owned());
        }
    }
}

fn texto_do_pedido(conteudo: &Value) -> Option<String> {
    let texto = conteudo.as_str().map(str::to_owned).or_else(|| {
        let partes: Vec<&str> = conteudo.as_array()?.iter().filter_map(|p| p["text"].as_str()).collect();
        (!partes.is_empty()).then(|| partes.join("\n\n"))
    })?;
    (!texto.trim().is_empty()).then_some(texto)
}

/// Parado pela CLI ou pelo usuário: o aviso `<task-notification>` daquela chamada diz `killed`/`stopped`.
fn parado_no_pai(pai: &str, tool_use_id: &str) -> bool {
    if tool_use_id.is_empty() { return false; }
    let marca = format!("<tool-use-id>{tool_use_id}</tool-use-id>");
    pai.match_indices(&marca).any(|(i, _)| {
        let resto = &pai[i..pai.len().min(i + 600)];
        resto.contains("<status>killed</status>") || resto.contains("<status>stopped</status>")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn grava(pasta: &Path, id: &str, meta: Value, linhas: &[Value]) {
        std::fs::write(pasta.join(format!("agent-{id}.meta.json")), meta.to_string()).unwrap();
        std::fs::write(pasta.join(format!("agent-{id}.jsonl")), linhas.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
    }

    #[test]
    fn le_estado_e_ultima_acao_de_cada_subagente() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("sessao.jsonl");
        let sub = pasta(&transcript);
        std::fs::create_dir_all(&sub).unwrap();
        let bash = json!({"type":"assistant","message":{"role":"assistant","stop_reason":"tool_use","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"npm test"}}]}});
        let fim = json!({"type":"assistant","message":{"role":"assistant","stop_reason":"end_turn","content":[{"type":"text","text":"Pronto"}]}});
        grava(&sub, "a1", json!({"agentType":"Explore","description":"Mapear rotas","toolUseId":"tu1"}), &[bash.clone()]);
        grava(&sub, "a2", json!({"agentType":"general-purpose","description":"Rodar testes","toolUseId":"tu2"}), &[bash.clone(), fim]);
        grava(&sub, "a3", json!({"agentType":"Explore","description":"Parado","toolUseId":"tu3"}), &[bash]);
        std::fs::write(&transcript, "<task-notification>\\n<tool-use-id>tu3</tool-use-id>\\n<status>killed</status>").unwrap();

        let lista = listar(&transcript, true);
        let estado = |id: &str| lista.iter().find(|s| s.id == id).unwrap().estado;
        assert_eq!((estado("a1"), estado("a2"), estado("a3")), ("rodando", "concluido", "parado"));
        let a1 = lista.iter().find(|s| s.id == "a1").unwrap();
        assert_eq!((a1.tipo.as_str(), a1.descricao.as_str()), ("Explore", "Mapear rotas"));
        assert!(a1.ultima_acao.as_deref().is_some_and(|a| a.contains("npm test")), "{:?}", a1.ultima_acao);

        assert_eq!(listar(&transcript, false).iter().find(|s| s.id == "a1").unwrap().estado, "parado", "sessão fechada não roda nada");
        assert!(arquivo(&transcript, "a1").is_some());
        assert!(arquivo(&transcript, "../x").is_none());
    }

    /// Linhas reais de subagente têm `isSidechain: true`: sem isso a última ação e o histórico saíam vazios.
    /// O fim pode ser `end_turn` ou a entrega pelo `SubagentHandback` (com `finished` no `.meta.json`).
    #[test]
    fn detalhe_traz_pedido_modelo_resultado_e_mascara_segredos() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("sessao.jsonl");
        let sub = pasta(&transcript);
        std::fs::create_dir_all(&sub).unwrap();
        let pedido = json!({"type":"user","isSidechain":true,"timestamp":"2026-11-06T18:00:00.000Z",
            "message":{"role":"user","content":"Revise X.\nUse a chave sk-ant-abc123DEF456ghi789JKL012 no teste."}});
        let bash = json!({"type":"assistant","isSidechain":true,"message":{"role":"assistant","model":"claude-opus-5-5","stop_reason":"tool_use",
            "content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"curl -H ghp_abcdefghijklmnopqrstuvwxyz0123456789 x"}}]}});
        let resultado = json!({"type":"user","isSidechain":true,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"ok"}]}});
        let entrega = json!({"type":"assistant","isSidechain":true,"message":{"role":"assistant","model":"claude-opus-5-5","stop_reason":"tool_use",
            "content":[{"type":"tool_use","id":"t2","name":"SubagentHandback","input":{"message":"Tudo certo: 3 problemas."}}]}});
        grava(&sub, "a1", json!({"agentType":"Explore","description":"Revisar","toolUseId":"tu1","model":"opus"}), &[pedido.clone(), bash.clone(), resultado.clone()]);
        grava(&sub, "a2", json!({"agentType":"Explore","description":"Revisar 2","toolUseId":"tu2","model":"opus","finished":true}),
            &[pedido, bash, resultado.clone(), entrega, resultado]);

        let a1 = detalhe(&transcript, "a1", true).unwrap();
        assert_eq!(a1.resumo.estado, "rodando");
        assert!(a1.resumo.ultima_acao.as_deref().is_some_and(|a| a.starts_with("Bash curl")), "{:?}", a1.resumo.ultima_acao);
        assert!(!a1.resumo.ultima_acao.unwrap().contains("ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
        assert!(a1.prompt.starts_with("Revise X.\nUse a chave"), "prompt inteiro, com as quebras: {:?}", a1.prompt);
        assert!(!a1.prompt.contains("sk-ant-abc123DEF456ghi789JKL012"));
        assert_eq!(a1.modelo.as_deref(), Some("claude-opus-5-5"));
        assert_eq!(a1.desde_ms, Some(1_793_988_000_000));
        assert_eq!(a1.resultado, None);

        let a2 = detalhe(&transcript, "a2", true).unwrap();
        assert_eq!(a2.resumo.estado, "concluido");
        assert_eq!(a2.resultado.as_deref(), Some("Tudo certo: 3 problemas."));
        assert_eq!(listar(&transcript, true).iter().find(|s| s.id == "a2").unwrap().estado, "concluido");
        assert!(detalhe(&transcript, "../a1", true).is_none());
        assert!(detalhe(&transcript, "zz", true).is_none());
    }

    /// Relido a cada 3 s: só o que foi acrescentado é processado, e a linha ainda sem `\n` aparece
    /// sem ser contada duas vezes.
    #[test]
    fn detalhe_le_so_o_que_foi_acrescentado() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("sessao.jsonl");
        let sub = pasta(&transcript);
        std::fs::create_dir_all(&sub).unwrap();
        let acao = |cmd: &str| json!({"type":"assistant","isSidechain":true,"message":{"role":"assistant","stop_reason":"tool_use",
            "content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":cmd}}]}}).to_string();
        let pedido = json!({"type":"user","isSidechain":true,"message":{"role":"user","content":"faça"}}).to_string();
        std::fs::write(sub.join("agent-b1.jsonl"), format!("{pedido}\n{}", acao("um"))).unwrap();
        assert_eq!(detalhe(&transcript, "b1", true).unwrap().resumo.ultima_acao.as_deref(), Some("Bash um"), "linha sem \\n já aparece");
        let mut f = std::fs::OpenOptions::new().append(true).open(sub.join("agent-b1.jsonl")).unwrap();
        write!(f, "\n{}\n", acao("dois")).unwrap();
        let d = detalhe(&transcript, "b1", true).unwrap();
        assert_eq!((d.resumo.ultima_acao.as_deref(), d.prompt.as_str()), (Some("Bash dois"), "faça"));
        // Parado: o aviso está na cauda do transcript da conversa.
        std::fs::write(sub.join("agent-b1.meta.json"), r#"{"toolUseId":"tu9"}"#).unwrap();
        assert!(!parado(&transcript, "b1"));
        std::fs::write(&transcript, format!("{}<tool-use-id>tu9</tool-use-id>\n<status>killed</status>", "x".repeat(300 * 1024))).unwrap();
        assert!(parado(&transcript, "b1"));
        assert_eq!(detalhe(&transcript, "b1", true).unwrap().resumo.estado, "parado");
    }
}
