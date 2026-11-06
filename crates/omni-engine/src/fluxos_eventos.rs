//! Linha do tempo de uma execução: `execucoes/<id>/eventos.jsonl`, só de acréscimo, uma linha por
//! evento. A tela lê por cursor (`seq`), sem precisar do arquivo inteiro. Tipos: `inicio`, `fim`,
//! `falha`, `atividade`, `aprovacao`, `subfluxo`, `aviso`.
use super::*;
use std::io::BufRead;

/// Quantos eventos uma leitura devolve no máximo.
pub const MAXIMO_POR_LEITURA: usize = 500;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Evento { pub seq: u64, pub ms: u64, pub no: Option<String>, pub tipo: String, pub texto: String }

fn arquivo(state: &EngineState, id: &str) -> PathBuf { pasta_execucoes(state).join(id).join("eventos.jsonl") }

/// Anexa um evento e sobe o `evento_seq` da execução. Quem chama já tem a execução travada. Se não
/// der para gravar, a execução segue (o arquivo é só um espelho para a tela), mas o erro vai para o
/// log do engine em vez de sumir.
pub fn registrar(state: &EngineState, e: &mut Execucao, no: Option<&str>, tipo: &str, texto: &str) {
    e.evento_seq += 1;
    let evento = Evento { seq: e.evento_seq, ms: now_ms(), no: no.map(str::to_owned), tipo: tipo.to_owned(), texto: texto.to_owned() };
    if let Err(erro) = anexar(state, &e.id, &evento) {
        eprintln!("fluxos: não consegui gravar o evento {} da execução {}: {erro:#}", evento.seq, e.id);
    }
}

fn anexar(state: &EngineState, id: &str, evento: &Evento) -> Result<()> {
    let caminho = arquivo(state, id);
    if let Some(pasta) = caminho.parent() { fs::create_dir_all(pasta)?; }
    let mut linha = serde_json::to_vec(evento)?;
    linha.push(b'\n');
    // Uma escrita só por linha: duas execuções nunca misturam bytes no meio de um evento.
    fs::OpenOptions::new().create(true).append(true).open(caminho)?.write_all(&linha)?;
    Ok(())
}

/// Eventos com `seq` maior que `desde`, no máximo `MAXIMO_POR_LEITURA`. Lê em fluxo e para ao
/// juntar o suficiente. Sem arquivo (execução antiga) devolve vazio. Linha quebrada (queda no meio
/// da escrita) é pulada.
pub fn ler_desde(state: &EngineState, id: &str, desde: u64) -> Result<Vec<Evento>> {
    if !id_valido(id) { return Err(anyhow!("Id inválido")); }
    let Ok(f) = fs::File::open(arquivo(state, id)) else { return Ok(Vec::new()) };
    Ok(std::io::BufReader::new(f).lines().map_while(std::result::Result::ok)
        .filter_map(|l| serde_json::from_str::<Evento>(&l).ok())
        .filter(|e| e.seq > desde).take(MAXIMO_POR_LEITURA).collect())
}

/// Separador no log do nó quando ele roda de novo: o histórico mostra onde começa cada volta.
pub fn separar_log(state: &EngineState, id: &str, no: &str, volta: u32) {
    let pasta = pasta_execucoes(state).join(id);
    let gravou = fs::create_dir_all(&pasta).and_then(|()| {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(pasta.join(format!("{no}.log")))?;
        writeln!(f, "{}", json!({"type": "volta", "n": volta}))
    });
    if let Err(erro) = gravou { eprintln!("fluxos: não consegui separar a volta {volta} do nó {no} ({id}): {erro}"); }
}
