//! A exceção à autonomia da IA: tarefa vinda de fora nunca executa sem um humano. Execução marcada
//! para antes da primeira etapa que age, se nenhuma Aprovação vier antes, e espera pela mesma tela e
//! pela mesma rota da Aprovação. Aprovar solta a etapa; recusar cancela a execução, porque a etapa não
//! tem saída de recusa, e a `nao` de um Comando quer dizer que ele falhou. Sem a marca, nada daqui age.

use super::*;

fn esta_marcada(e: &Execucao, no: &str) -> bool {
    e.veio_de_fora && e.nos.get(no).is_some_and(|n| !n.liberada) && fluxos::primeiras_que_agem(&e.fluxo).contains(no)
}

/// Para a etapa antes de ela rodar. `true`: parou, e quem chamou não deve rodar a etapa.
pub(super) fn segurar(state: &EngineState, v: &Viva, no: &str, passo: &Passo) -> Result<bool> {
    if !v.lock().is_ok_and(|e| esta_marcada(&e, no)) { return Ok(false); }
    mexer(state, v, |e| {
        // Parada no meio: sem isto, `aguardando` passaria por cima de `pausado` ou `cancelado`.
        if !subfluxo::esta_viva(e) { return Err(anyhow!(PAUSADO)); }
        e.status = "aguardando".into();
        if let Some(n) = e.nos.get_mut(no) { n.status = "aguardando".into(); n.esperou_de.clone_from(&passo.de); }
        eventos::registrar(state, e, Some(no), "aprovacao", "Aguardando aprovação: a tarefa veio de fora e esta etapa ainda não rodou");
        Ok(true)
    })
}

/// O nó que espera resposta é uma etapa segurada aqui, e não um nó Aprovação do fluxo.
pub(super) fn esta_segurada(e: &Execucao, no: &str) -> bool { e.fluxo.nos.iter().any(|n| n.id == no && n.tipo != TipoDeNo::Aprovacao) }

/// Devolve a etapa à fila, sabendo de onde veio. Tudo numa operação só: com a etapa fora da fila e a
/// execução já `rodando`, outro executor daria a execução por concluída.
pub(super) fn liberar(e: &mut Execucao, no: &str) {
    let Some(n) = e.nos.get_mut(no) else { return };
    n.status = "pendente".into();
    n.liberada = true;
    let de = n.esperou_de.take();
    e.fila.push(Passo { no: no.to_owned(), de });
}

/// Há execução marcada, ainda de pé, trabalhando em `pasta` ou acima dela? É o que sobra quando o MCP
/// do agente perde a marca. Pausada conta: ela volta, e a conversa aberta nela segue viva.
pub(super) fn pasta_marcada(pasta: &str) -> bool {
    let real = |caminho: &Path| caminho.canonicalize().unwrap_or_else(|_| caminho.to_path_buf());
    let alvo = real(Path::new(pasta));
    let vivas: Vec<Viva> = execucoes().lock().map(|m| m.values().cloned().collect()).unwrap_or_default();
    let de_pe = |e: &Execucao| e.veio_de_fora && matches!(e.status.as_str(), "rodando" | "aguardando" | "pausado");
    let pastas: HashSet<String> = vivas.iter().filter_map(|v| v.lock().ok().filter(|e| de_pe(e)).map(|e| e.pasta.clone())).collect();
    pastas.iter().any(|marcada| alvo.starts_with(real(Path::new(marcada))))
}
