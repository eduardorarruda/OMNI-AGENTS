//! Fluxo chamando fluxo: o nó Subfluxo abre uma `Execucao` filha e espera por ela. A filha é uma
//! execução comum, com thread, linha do tempo e aprovações próprias, que trabalha na pasta do pai.
//!
//! Parar não depende de a filha perceber: `pausar`, `cancelar` e `retomar` andam pela cadeia aqui.
//! A ordem importa. Quem para muda o pai antes da filha, e a espera lê a filha antes do pai: quem viu
//! a filha cancelada vê também o pai parado e não confunde isso com falha da filha.

use super::*;
use std::time::Duration;

const INTERVALO_DA_ESPERA: Duration = Duration::from_millis(500);

fn esta_viva(e: &Execucao) -> bool { matches!(e.status.as_str(), "rodando" | "aguardando") }

/// Filhas diretas de uma execução, inclusive as de voltas anteriores do mesmo nó. Lê o mapa, não os
/// nós do pai: assim acha até a filha recém-aberta. Solta o mapa antes de travar cada execução, porque
/// `mexer` trava a execução e ninguém pode esperar o mapa com ela na mão.
pub fn filhas_de(id: &str) -> Vec<String> {
    let vivas: Vec<Viva> = execucoes().lock().map(|m| m.values().cloned().collect()).unwrap_or_default();
    vivas.iter().filter_map(|v| v.lock().ok().filter(|e| e.pai.as_ref().is_some_and(|p| p.execucao == id)).map(|e| e.id.clone())).collect()
}

/// O fluxo pelo id. O do projeto vence o global de mesmo id, como nos modelos de agente.
pub fn fluxo_salvo(id: &str, global: Option<&Path>, projeto: &str) -> Result<Fluxo> {
    let lista = fluxos::listar(global, Some(Path::new(projeto)));
    lista.iter().find(|f| f.escopo == "projeto" && f.fluxo.id == id).or_else(|| lista.iter().find(|f| f.fluxo.id == id))
        .map(|f| f.fluxo.clone()).ok_or_else(|| anyhow!("O fluxo chamado “{id}” não existe mais"))
}

/// Roda o nó: abre a filha, ou acha a que já existe (engine religado, retomar), e devolve a saída do
/// `Fim` dela.
pub fn rodar_subfluxo(state: &Arc<EngineState>, v: &Viva, no: &fluxos::No, ctx: &Contexto) -> Result<String> {
    let existente = v.lock().map_err(|_| anyhow!("execucao"))?.nos.get(&no.id).and_then(|n| n.filha.clone()).filter(|id| obter(id).is_some());
    let filha = match existente { Some(id) => id, None => abrir_filha(state, v, no, ctx)? };
    esperar(state, v, &no.id, &filha)
}

/// Abre a filha. O id entra no pai antes de ela existir: cair no meio deixa o mesmo id para a próxima
/// tentativa, nunca duas filhas. Parar no meio também não escapa: a filha entra no mapa antes de o pai
/// ser conferido, então ou a parada a encontra, ou a conferência aqui vê a parada.
fn abrir_filha(state: &Arc<EngineState>, v: &Viva, no: &fluxos::No, ctx: &Contexto) -> Result<String> {
    let alvo = fluxos::fluxo_chamado(no).ok_or_else(|| anyhow!("O nó não diz qual fluxo chamar"))?;
    let projeto = v.lock().map_err(|_| anyhow!("execucao"))?.projeto.clone();
    let fluxo = fluxo_salvo(alvo, fluxos::pasta_global().as_deref(), &projeto)?;
    fluxos::validar(&fluxo).map_err(|erro| anyhow!("O fluxo chamado “{}” não pode rodar: {erro}", fluxo.nome))?;
    let pastas_extras = resolver_pastas(&fluxo, &projeto)?;
    let reservado = mexer(state, v, |e| if esta_viva(e) { e.nos.get_mut(&no.id).map(|n| n.filha.get_or_insert_with(novo_id).clone()) } else { None });
    let id = reservado.ok_or_else(|| anyhow!(PAUSADO))?;

    let pai = v.lock().map_err(|_| anyhow!("execucao"))?.clone();
    let nome = fluxo.nome.clone();
    let filha = nova_filha(&pai, &no.id, id.clone(), fluxo, ctx.anterior.clone(), pastas_extras)?;
    gravar(state, &filha);
    execucoes().lock().map_err(|_| anyhow!("execucao"))?.entry(id.clone()).or_insert_with(|| Arc::new(Mutex::new(filha)));

    let status_do_pai = mexer(state, v, |e| { eventos::registrar(state, e, Some(&no.id), "subfluxo", &format!("Chamou “{nome}”")); e.status.clone() });
    match status_do_pai.as_str() {
        "rodando" | "aguardando" => soltar(state.clone(), id.clone()),
        "cancelado" => cancelar(state, &id)?,
        // Fica pausada na fila: o `retomar` do pai solta.
        _ => mexer(state, &viva(&id)?, |e| e.status = "pausado".into()),
    }
    Ok(id)
}

fn nova_filha(pai: &Execucao, no: &str, id: String, fluxo: Fluxo, entrada: String, pastas_extras: Vec<String>) -> Result<Execucao> {
    let inicio = fluxos::entrada(&fluxo).ok_or_else(|| anyhow!("sem entrada"))?.id.clone();
    let agora = now_ms();
    let mut pilha = if pai.pilha.is_empty() { vec![pai.fluxo.nome.clone()] } else { pai.pilha.clone() };
    pilha.push(fluxo.nome.clone());
    Ok(Execucao { id, nos: estados_iniciais(&fluxo), fluxo, projeto: pai.projeto.clone(), projeto_id: pai.projeto_id.clone(), entrada,
        pasta: pai.pasta.clone(), branch: pai.branch.clone(), base: pai.base.clone(), status: "rodando".into(), erro: None,
        criado_ms: agora, atualizado_ms: agora, fila: vec![Passo { no: inicio, de: None }], pr_url: None, custo_usd: 0.0,
        pastas_extras, custo_aceito: 0.0, parou_por_custo: false, aviso_seq: 0, aviso: None, evento_seq: 0,
        pai: Some(RefDoPai { execucao: pai.id.clone(), no: no.to_owned() }), pilha })
}

/// Espera a filha terminar ou o pai parar. Filha pausada ou aguardando não terminou: o pai segue
/// esperando e mostra a parada no nó.
fn esperar(state: &EngineState, v: &Viva, no: &str, filha: &str) -> Result<String> {
    loop {
        let f = obter(filha).ok_or_else(|| anyhow!("A execução chamada não existe mais"))?;
        if !v.lock().map(|e| esta_viva(&e)).unwrap_or(false) { return Err(anyhow!(PAUSADO)); }
        let nome = &f.fluxo.nome;
        match f.status.as_str() {
            "concluido" => { marcar_fim(state, v, no, &format!("“{nome}” terminou")); return Ok(saida_do_fim(&f)); }
            "falhou" => {
                let texto = format!("“{nome}” falhou: {}", f.erro.as_deref().unwrap_or("sem detalhe"));
                marcar_fim(state, v, no, &texto);
                return Err(anyhow!(texto));
            }
            "cancelado" => { let texto = format!("“{nome}” foi cancelada"); marcar_fim(state, v, no, &texto); return Err(anyhow!(texto)); }
            _ => definir_atividade(state, v, no, &andamento(&f)),
        }
        std::thread::sleep(INTERVALO_DA_ESPERA);
    }
}

fn marcar_fim(state: &EngineState, v: &Viva, no: &str, texto: &str) {
    mexer(state, v, |e| eventos::registrar(state, e, Some(no), "subfluxo", texto));
}

/// O que o nó do pai mostra enquanto espera. A filha que espera a neta repassa o que a neta mostra,
/// para a parada de Z chegar em X.
fn andamento(f: &Execucao) -> String {
    match f.status.as_str() {
        "aguardando" => format!("“{}” espera aprovação", f.fluxo.nome),
        "pausado" => format!("“{}” está pausada", f.fluxo.nome),
        _ => f.nos.values().find(|n| n.status == "rodando" && n.filha.is_some() && !n.atividade.is_empty()).map_or_else(|| format!("Rodando “{}”", f.fluxo.nome), |n| n.atividade.clone()),
    }
}

/// A saída do `Fim` que a filha alcançou por último: um fluxo pode ter mais de um.
fn saida_do_fim(f: &Execucao) -> String {
    f.fluxo.nos.iter().filter(|n| n.tipo == TipoDeNo::Fim).filter_map(|n| f.nos.get(&n.id)).filter(|n| n.status == "concluido")
        .max_by_key(|n| n.fim_ms).map(|n| n.saida.clone()).unwrap_or_default()
}

/// Pausa as filhas e tudo abaixo delas. Filha `aguardando` também para: aprovar não pode fazê-la andar
/// com a cadeia pausada, e `retomar` devolve a pergunta.
pub fn pausar_filhas(state: &EngineState, id: &str) {
    for filha in filhas_de(id) {
        if let Ok(v) = viva(&filha) { mexer(state, &v, |e| if esta_viva(e) { e.status = "pausado".into(); }); }
        matar_processo(&filha);
        pausar_filhas(state, &filha);
    }
}

/// Cancela o que ainda não acabou. Filha concluída fica como está: o que ela entregou continua valendo.
pub fn cancelar_filhas(state: &EngineState, id: &str) {
    for filha in filhas_de(id) {
        if obter(&filha).is_some_and(|e| matches!(e.status.as_str(), "concluido" | "cancelado")) { continue; }
        if let Err(erro) = cancelar(state, &filha) { eprintln!("fluxos: não consegui cancelar a filha {filha}: {erro:#}"); }
    }
}

/// Retoma as filhas paradas, antes do pai: a espera dele não pode ver a falha antiga como resposta nova.
/// Filha parada pelo custo fica para o usuário: o MCP pode retomar o pai, e esse gasto ele não libera.
pub fn retomar_filhas(state: &Arc<EngineState>, id: &str) {
    for filha in filhas_de(id) {
        if !obter(&filha).is_some_and(|e| matches!(e.status.as_str(), "pausado" | "falhou") && !e.parou_por_custo) { continue; }
        if let Err(erro) = retomar(state, &filha) { eprintln!("fluxos: não consegui retomar a filha {filha}: {erro:#}"); }
    }
}

/// Ao subir o engine: filha `aguardando` de pai pausado também pausa, senão uma aprovação faria a
/// cadeia andar pela metade. Repete porque a pausa desce um nível por passada.
pub fn pausar_cadeias_carregadas(lidas: &mut [Execucao]) {
    let mut pausadas: HashSet<String> = lidas.iter().filter(|e| e.status == "pausado").map(|e| e.id.clone()).collect();
    loop {
        let mut mudou = false;
        for e in lidas.iter_mut().filter(|e| e.status == "aguardando") {
            if e.pai.as_ref().is_some_and(|p| pausadas.contains(&p.execucao)) {
                e.status = "pausado".into();
                pausadas.insert(e.id.clone());
                mudou = true;
            }
        }
        if !mudou { return; }
    }
}
