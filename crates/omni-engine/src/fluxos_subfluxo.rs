//! Fluxo chamando fluxo: o nó Subfluxo abre uma `Execucao` filha e espera por ela. A filha é uma
//! execução comum, com thread, linha do tempo e aprovações próprias, que trabalha na pasta do pai.
//!
//! Parar não depende de a filha perceber: `pausar`, `cancelar` e `retomar` andam pela cadeia aqui.
//! A ordem importa. Quem para muda o pai antes da filha, e a espera lê a filha antes do pai: quem viu
//! a filha cancelada vê também o pai parado e não confunde isso com falha da filha.
//!
//! Ciclo entre fluxos é permitido e sem teto de profundidade, então a cadeia pode ter milhares de níveis:
//! parar anda em laço, sem recursão, e nada aqui pode custar uma varredura do mapa por nível.

use super::*;
use std::collections::VecDeque;
use std::time::Duration;

const INTERVALO_DA_ESPERA: Duration = Duration::from_millis(500);

pub(super) fn esta_viva(e: &Execucao) -> bool { matches!(e.status.as_str(), "rodando" | "aguardando") }

/// Filhas diretas de uma execução, inclusive as de voltas anteriores do mesmo nó. Lê o mapa, não os
/// nós do pai: assim acha até a filha recém-aberta. Solta o mapa antes de travar cada execução, porque
/// `mexer` trava a execução e ninguém pode esperar o mapa com ela na mão.
pub fn filhas_de(id: &str) -> Vec<String> {
    let vivas: Vec<Viva> = execucoes().lock().map(|m| m.values().cloned().collect()).unwrap_or_default();
    vivas.iter().filter_map(|v| v.lock().ok().filter(|e| e.pai.as_ref().is_some_and(|p| p.execucao == id)).map(|e| e.id.clone())).collect()
}

/// Tudo o que está abaixo de `id`, pais antes das filhas. O mapa pai→filhas sai de uma passada só; como
/// em `filhas_de`, nenhuma execução é travada com o mapa na mão.
fn descendentes(id: &str) -> Vec<(String, Viva)> {
    let vivas: Vec<Viva> = execucoes().lock().map(|m| m.values().cloned().collect()).unwrap_or_default();
    let mut por_pai: HashMap<String, Vec<(String, Viva)>> = HashMap::new();
    for v in vivas {
        let Some((pai, filha)) = v.lock().ok().and_then(|e| Some((e.pai.as_ref()?.execucao.clone(), e.id.clone()))) else { continue };
        por_pai.entry(pai).or_default().push((filha, v));
    }
    let mut abaixo = Vec::new();
    let mut fila = VecDeque::from([id.to_owned()]);
    while let Some(atual) = fila.pop_front() {
        for (filha, v) in por_pai.remove(&atual).unwrap_or_default() {
            fila.push_back(filha.clone());
            abaixo.push((filha, v));
        }
    }
    abaixo
}

/// Para quem está abaixo de `id` e ainda `falta`. Repete porque a foto do mapa é anterior à parada: filha
/// aberta depois dela não foi vista, mas só existe enquanto o pai não parou, e a passada seguinte a acha.
fn parar_abaixo(id: &str, falta: impl Fn(&Execucao) -> bool, parar: impl Fn(&str, &Viva)) {
    loop {
        let pendentes: Vec<(String, Viva)> = descendentes(id).into_iter().filter(|(_, v)| v.lock().is_ok_and(|e| falta(&e))).collect();
        if pendentes.is_empty() { return; }
        for (filha, v) in &pendentes { parar(filha, v); }
    }
}

/// Só o status: a espera roda em cada nível, e clonar a execução inteira a cada volta pesa numa cadeia funda.
fn status_de(id: &str) -> Option<String> { viva(id).ok()?.lock().ok().map(|e| e.status.clone()) }

/// O fluxo pelo id. O do projeto vence o global de mesmo id, como nos modelos de agente.
pub fn fluxo_salvo(id: &str, global: Option<&Path>, projeto: &str) -> Result<Fluxo> {
    let lista = fluxos::listar(global, Some(Path::new(projeto)));
    lista.iter().find(|f| f.escopo == "projeto" && f.fluxo.id == id).or_else(|| lista.iter().find(|f| f.fluxo.id == id))
        .map(|f| f.fluxo.clone()).ok_or_else(|| anyhow!("O fluxo chamado “{id}” não existe mais"))
}

/// Roda o nó: abre a filha, ou acha a que já existe (engine religado, retomar), e devolve a saída do
/// `Fim` dela. Filha cancelada conta como inexistente: morta, ela não duplica nada, e sem isto o pai
/// ficaria sem saída, com `retomar` falhando sempre. Só `retomar` chega aqui com ela: cancelar o pai
/// não deixa ele voltar, e o loop limpa `filha` em `seguir`.
pub fn rodar_subfluxo(state: &Arc<EngineState>, v: &Viva, no: &fluxos::No, ctx: &Contexto) -> Result<String> {
    let atual = v.lock().map_err(|_| anyhow!("execucao"))?.nos.get(&no.id).and_then(|n| n.filha.clone());
    let situacao = atual.as_deref().and_then(status_de);
    if situacao.as_deref() == Some("cancelado") { mexer(state, v, |e| if let Some(n) = e.nos.get_mut(&no.id) { n.filha = None; }); }
    let filha = match atual.filter(|_| situacao.as_deref().is_some_and(|s| s != "cancelado")) { Some(id) => id, None => abrir_filha(state, v, no, ctx)? };
    esperar(state, v, &no.id, &filha)
}

/// Abre a filha. O id entra no pai antes de ela existir: cair no meio deixa o mesmo id para a próxima
/// tentativa, nunca duas filhas. Parar no meio também não escapa: a filha entra no mapa antes de o pai
/// ser conferido, então ou a parada a encontra, ou a conferência aqui vê a parada. A raiz é conferida
/// antes de tudo: num ciclo, o fundo seguiria abrindo níveis até a parada chegar nele.
fn abrir_filha(state: &Arc<EngineState>, v: &Viva, no: &fluxos::No, ctx: &Contexto) -> Result<String> {
    let raiz = v.lock().map_err(|_| anyhow!("execucao"))?.raiz.clone();
    if raiz.as_deref().and_then(status_de).is_some_and(|s| matches!(s.as_str(), "pausado" | "cancelado")) { return Err(anyhow!(PAUSADO)); }
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
        _ => mexer(state, &viva(&id)?, |e| if esta_viva(e) { e.status = "pausado".into(); }),
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
        pastas_extras, origem: pai.origem.clone(), veio_de_fora: pai.veio_de_fora, aviso_seq: 0, aviso: None, evento_seq: 0,
        pai: Some(RefDoPai { execucao: pai.id.clone(), no: no.to_owned() }), pilha, raiz: Some(pai.raiz.clone().unwrap_or_else(|| pai.id.clone())) })
}

/// Espera a filha terminar ou o pai parar. Filha pausada ou aguardando não terminou: o pai segue
/// esperando e mostra a parada no nó. Só toca no pai quando o texto muda: `mexer` regrava o JSON, e
/// esta espera pode durar horas.
fn esperar(state: &EngineState, v: &Viva, no: &str, filha: &str) -> Result<String> {
    let mut mostrado = String::new();
    loop {
        let status = status_de(filha).ok_or_else(|| anyhow!("A execução chamada não existe mais"))?;
        if !v.lock().map(|e| esta_viva(&e)).unwrap_or(false) { return Err(anyhow!(PAUSADO)); }
        if matches!(status.as_str(), "concluido" | "falhou" | "cancelado") {
            let f = obter(filha).ok_or_else(|| anyhow!("A execução chamada não existe mais"))?;
            return terminou(state, v, no, &f, &status);
        }
        let texto = viva(filha).ok().and_then(|f| f.lock().ok().map(|f| andamento(&f))).unwrap_or_default();
        if texto != mostrado { definir_atividade(state, v, no, &texto); mostrado = texto; }
        std::thread::sleep(INTERVALO_DA_ESPERA);
    }
}

/// A filha acabou, com o `status` que a espera leu: devolve a saída dela ou o erro do nó.
fn terminou(state: &EngineState, v: &Viva, no: &str, f: &Execucao, status: &str) -> Result<String> {
    let nome = &f.fluxo.nome;
    let texto = match status {
        "concluido" => { marcar_fim(state, v, no, &format!("“{nome}” terminou")); return Ok(saida_do_fim(f)); }
        "falhou" => format!("“{nome}” falhou: {}", f.erro.as_deref().unwrap_or("sem detalhe")),
        _ => format!("“{nome}” foi cancelada"),
    };
    marcar_fim(state, v, no, &texto);
    Err(anyhow!(texto))
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
    parar_abaixo(id, esta_viva, |filha, v| {
        mexer(state, v, |e| if esta_viva(e) { e.status = "pausado".into(); });
        matar_processo(filha);
    });
}

/// Cancela o que ainda não acabou. Filha concluída fica como está: o que ela entregou continua valendo.
pub fn cancelar_filhas(state: &EngineState, id: &str) {
    parar_abaixo(id, |e| !matches!(e.status.as_str(), "concluido" | "cancelado"), |filha, v| cancelar_uma(state, v, filha));
}

/// Retoma as filhas paradas, antes do pai: a espera dele não pode ver a falha antiga como resposta nova.
pub fn retomar_filhas(state: &Arc<EngineState>, id: &str) {
    for filha in filhas_de(id) {
        if !obter(&filha).is_some_and(|e| matches!(e.status.as_str(), "pausado" | "falhou")) { continue; }
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
