//! Linha do tempo de uma execução: `execucoes/<id>/eventos.jsonl`, só de acréscimo, uma linha por
//! evento. A tela lê por cursor (`seq`), sem precisar do arquivo inteiro. Tipos: `inicio`, `fim`,
//! `falha`, `atividade`, `aprovacao`, `subfluxo`, `aviso`.
use super::*;
use std::io::BufRead;
use std::collections::HashMap;

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

/// Por arquivo: (último `seq` entregue, byte logo depois da linha dele). Uma leitura com cursor igual
/// ou maior começa nesse byte, sem reler o que já passou. Cursor menor (ou engine religado, memória
/// vazia) lê do começo.
fn marcas() -> &'static Mutex<HashMap<PathBuf, (u64, u64)>> {
    static M: std::sync::OnceLock<Mutex<HashMap<PathBuf, (u64, u64)>>> = std::sync::OnceLock::new();
    M.get_or_init(Default::default)
}

/// Eventos com `seq` maior que `desde`, no máximo `MAXIMO_POR_LEITURA`. Sem arquivo (execução
/// antiga) devolve vazio. Linha quebrada é pulada, e a última linha sem `\n` (escrita em andamento)
/// fica para a próxima leitura.
pub fn ler_desde(state: &EngineState, id: &str, desde: u64) -> Result<Vec<Evento>> {
    if !id_valido(id) { return Err(anyhow!("Id inválido")); }
    let caminho = arquivo(state, id);
    let Ok(mut f) = fs::File::open(&caminho) else { return Ok(Vec::new()) };
    let marca = marcas().lock().ok().and_then(|m| m.get(&caminho).copied());
    let mut posicao = match marca {
        Some((seq, offset)) if desde >= seq && offset <= f.metadata().map_or(0, |m| m.len()) => offset,
        _ => 0,
    };
    std::io::Seek::seek(&mut f, std::io::SeekFrom::Start(posicao))?;
    let mut leitor = std::io::BufReader::new(f);
    let (mut eventos, mut ultima, mut linha) = (Vec::new(), None, String::new());
    while eventos.len() < MAXIMO_POR_LEITURA {
        linha.clear();
        let lidos = leitor.read_line(&mut linha)?;
        if lidos == 0 || !linha.ends_with('\n') { break; }
        posicao += lidos as u64;
        let Ok(e) = serde_json::from_str::<Evento>(&linha) else { continue };
        if e.seq > desde { ultima = Some((e.seq, posicao)); eventos.push(e); }
    }
    if let (Some(par), Ok(mut m)) = (ultima, marcas().lock()) { m.insert(caminho, par); }
    Ok(eventos)
}

/// Separador no log do nó quando um participante da Sala fala numa rodada.
pub fn separar_participante(state: &EngineState, id: &str, no: &str, quem: &str, rodada: u32) {
    escrever_no_log(state, id, no, &json!({"type": "participante", "quem": quem, "rodada": rodada}));
}

/// Separador no log do nó quando ele roda de novo: o histórico mostra onde começa cada volta.
pub fn separar_log(state: &EngineState, id: &str, no: &str, volta: u32) {
    escrever_no_log(state, id, no, &json!({"type": "volta", "n": volta}));
}

fn escrever_no_log(state: &EngineState, id: &str, no: &str, linha: &Value) {
    let pasta = pasta_execucoes(state).join(id);
    let gravou = fs::create_dir_all(&pasta).and_then(|()| {
        let mut f = fs::OpenOptions::new().create(true).append(true).open(pasta.join(format!("{no}.log")))?;
        writeln!(f, "{linha}")
    });
    if let Err(erro) = gravou { eprintln!("fluxos: não consegui separar o log do nó {no} ({id}): {erro}"); }
}
