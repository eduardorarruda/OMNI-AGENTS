//! O ensaio de um fluxo: tudo que dá para conferir sem abrir agente. Aqui fica a parte pura; pasta,
//! CLI e MCP dependem da máquina e quem olha é a ferramenta `ensaiar_fluxo`.
use super::{antecessores, entrada, participantes, proximos, rotulo, saidas, validar, Fluxo, No, TipoDeNo};
use crate::agentes::ModeloDeAgente;
use std::collections::{BTreeSet, HashSet};

#[derive(Debug, Default, PartialEq)]
pub struct Ensaio {
    /// Impedem a execução, ou fazem uma etapa rodar sem o que precisa.
    pub problemas: Vec<String>,
    /// Roda, mas talvez não como você imagina.
    pub avisos: Vec<String>,
    /// Etapas de agente mais participantes de sala.
    pub agentes: usize,
    pub aprovacoes: usize,
    /// Maior número de etapas com agente ou comando ao mesmo tempo, já com `max_paralelo`.
    pub maior_lote: usize,
}

/// Ids dos `{{no:id}}` de um texto.
pub fn marcadores_de_no(texto: &str) -> Vec<&str> {
    let mut lista = Vec::new();
    let mut resto = texto;
    while let Some(inicio) = resto.find("{{no:") {
        resto = &resto[inicio + 5..];
        let Some(fim) = resto.find("}}") else { break };
        lista.push(resto[..fim].trim());
        resto = &resto[fim + 2..];
    }
    lista
}

pub fn marcadores_do_no(no: &No) -> Vec<&str> {
    no.config["prompt"].as_str().map(marcadores_de_no).unwrap_or_default()
}

/// Dá para chegar de `de` em `ate` sem pisar em `evitando`.
fn alcanca(f: &Fluxo, de: &str, ate: &str, evitando: Option<&str>) -> bool {
    let mut fila = vec![de];
    let mut vistos = HashSet::new();
    while let Some(atual) = fila.pop() {
        if !vistos.insert(atual) { continue; }
        for conexao in f.conexoes.iter().filter(|c| c.de == atual && Some(c.para.as_str()) != evitando) {
            if conexao.para == ate { return true; }
            fila.push(&conexao.para);
        }
    }
    false
}

fn conferir_marcadores(f: &Fluxo, ensaio: &mut Ensaio) {
    let inicio = entrada(f).map(|n| n.id.as_str()).unwrap_or_default();
    for no in &f.nos {
        for alvo in marcadores_do_no(no) {
            if alvo == no.id || !alcanca(f, alvo, &no.id, None) {
                ensaio.problemas.push(format!("“{}” usa {{{{no:{alvo}}}}}, mas essa etapa não roda antes dela", rotulo(no)));
            } else if alvo != inicio && !alcanca(f, inicio, alvo, Some(&no.id)) {
                ensaio.avisos.push(format!("“{}” usa {{{{no:{alvo}}}}}: na primeira passada ainda não há saída e o marcador fica vazio", rotulo(no)));
            }
        }
    }
}

fn nomes_de_agentes(no: &No) -> Vec<String> {
    match no.tipo {
        TipoDeNo::Agente => vec![no.agente.clone()],
        TipoDeNo::Sala => participantes(no),
        _ => Vec::new(),
    }
}

/// Modelos que o fluxo usa, sem repetir. Os que não existem ficam de fora.
pub fn agentes_usados<'a>(f: &Fluxo, disponiveis: &'a [ModeloDeAgente]) -> Vec<&'a ModeloDeAgente> {
    let nomes: BTreeSet<String> = f.nos.iter().flat_map(nomes_de_agentes).collect();
    disponiveis.iter().filter(|a| nomes.contains(&a.name)).collect()
}

fn conferir_agentes(f: &Fluxo, disponiveis: &[ModeloDeAgente], ensaio: &mut Ensaio) {
    for no in &f.nos {
        for nome in nomes_de_agentes(no) {
            let Some(agente) = disponiveis.iter().find(|a| a.name == nome) else {
                ensaio.problemas.push(format!("“{}” usa o agente “{nome}”, que não existe", rotulo(no)));
                continue;
            };
            let tem_shell = agente.ferramentas.iter().any(|f| super::FERRAMENTAS_DE_SHELL.iter().any(|s| f.starts_with(s)));
            if no.escrita.is_some() && tem_shell {
                ensaio.avisos.push(format!("“{}” tem `escrita`: o Bash de “{nome}” fica bloqueado nessa etapa", rotulo(no)));
            }
            if agente.cli == "claude" { continue; }
            if !no.proibidas.is_empty() { ensaio.problemas.push(format!("“{}”: `proibidas` só vale para o Claude, e “{nome}” usa {}", rotulo(no), agente.cli)); }
            if no.escrita.as_ref().is_some_and(|lista| !lista.is_empty()) {
                ensaio.problemas.push(format!("“{}”: `escrita` com caminhos só vale para o Claude, e “{nome}” usa {}", rotulo(no), agente.cli));
            }
        }
    }
}

/// Sem `esgotado` ligado, a última reprovação não tem para onde ir e a execução acaba como concluída.
fn conferir_saidas(f: &Fluxo, ensaio: &mut Ensaio) {
    for no in f.nos.iter().filter(|n| n.tipo == TipoDeNo::Condicao) {
        for saida in saidas(&no.tipo) {
            if !f.conexoes.iter().any(|c| c.de == no.id && c.saida == *saida) {
                ensaio.avisos.push(format!("A condição “{}” não tem a saída `{saida}` ligada: se ela for escolhida, o caminho termina ali", rotulo(no)));
            }
        }
    }
}

fn trabalha(no: &No) -> bool {
    matches!(no.tipo, TipoDeNo::Agente | TipoDeNo::Sala | TipoDeNo::Comando | TipoDeNo::Pr)
}

/// Anda de nível em nível, sem repetir nó. É estimativa: ramos de tamanhos diferentes se desencontram.
/// Das saídas de um nó só uma é escolhida por vez, então o nível conta a mais larga.
fn maior_lote(f: &Fluxo) -> usize {
    let mut nivel: Vec<&str> = entrada(f).map(|n| n.id.as_str()).into_iter().collect();
    let mut vistos: HashSet<&str> = nivel.iter().copied().collect();
    let mut maior = 0;
    while !nivel.is_empty() {
        let mut proximo: Vec<&str> = Vec::new();
        let mut juntos = 0;
        for no in nivel.iter().filter_map(|id| f.nos.iter().find(|n| n.id == *id)) {
            let por_saida: Vec<Vec<&No>> = saidas(&no.tipo).iter().map(|saida| proximos(f, &no.id, saida).into_iter().filter(|n| !vistos.contains(n.id.as_str())).collect()).collect();
            juntos += por_saida.iter().map(|alvos| alvos.iter().filter(|n| trabalha(n)).count()).max().unwrap_or(0);
            proximo.extend(por_saida.iter().flatten().map(|n| n.id.as_str()));
        }
        proximo.sort_unstable();
        proximo.dedup();
        vistos.extend(proximo.iter().copied());
        maior = maior.max(juntos);
        nivel = proximo;
    }
    maior.min(super::limite_paralelo(f))
}

/// O que dá para conferir só com o fluxo e a lista de agentes.
pub fn ensaiar(f: &Fluxo, disponiveis: &[ModeloDeAgente]) -> Ensaio {
    let mut ensaio = Ensaio {
        agentes: f.nos.iter().map(|n| nomes_de_agentes(n).len()).sum(),
        aprovacoes: f.nos.iter().filter(|n| n.tipo == TipoDeNo::Aprovacao).count(),
        maior_lote: maior_lote(f),
        ..Default::default()
    };
    if let Err(erro) = validar(f) { ensaio.problemas.push(erro); }
    conferir_agentes(f, disponiveis, &mut ensaio);
    conferir_marcadores(f, &mut ensaio);
    conferir_saidas(f, &mut ensaio);
    for juntar in f.nos.iter().filter(|n| n.tipo == TipoDeNo::Juntar) {
        for ramo in antecessores(f, &juntar.id).into_iter().filter_map(|id| f.nos.iter().find(|n| n.id == id)).filter(|n| n.tipo == TipoDeNo::Comando) {
            ensaio.avisos.push(format!("“{}” chega no Juntar “{}”: se o comando seguir por outra saída, o Juntar não roda", rotulo(ramo), rotulo(juntar)));
        }
    }
    ensaio
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn fluxo(valor: Value) -> Fluxo { serde_json::from_value(valor).unwrap() }

    fn agente(nome: &str, cli: &str, ferramentas: &[&str]) -> ModeloDeAgente {
        serde_json::from_value(json!({"name": nome, "description": "d", "cli": cli, "instrucoes": "x", "ferramentas": ferramentas, "modelo": "", "esforco": "", "conta": "", "saida": "texto"})).unwrap()
    }

    /// entrada → dev → qa → condição, com `nao` voltando para dev.
    fn com_loop(prompt_dev: &str) -> Fluxo {
        fluxo(json!({"id": "l", "nome": "L", "nos": [
            {"id": "e", "tipo": "entrada"}, {"id": "dev", "tipo": "agente", "agente": "dev", "config": {"prompt": prompt_dev}},
            {"id": "qa", "tipo": "agente", "agente": "dev"}, {"id": "c", "tipo": "condicao"}, {"id": "f", "tipo": "fim"}],
            "conexoes": [{"de": "e", "para": "dev"}, {"de": "dev", "para": "qa"}, {"de": "qa", "para": "c"},
                {"de": "c", "para": "dev", "saida": "nao"}, {"de": "c", "para": "f", "saida": "sim"}, {"de": "c", "para": "f", "saida": "esgotado"}]}))
    }

    #[test]
    fn acha_os_marcadores_do_texto() {
        assert_eq!(marcadores_de_no("{{no:a}} e {{ no:b }} {{no:c}} {{no:quebrado"), ["a", "c"]);
    }

    #[test]
    fn marcador_de_etapa_que_nao_roda_antes_e_problema_e_de_loop_e_aviso() {
        let disponiveis = [agente("dev", "claude", &[])];
        assert!(ensaiar(&com_loop("{{no:e}}"), &disponiveis).problemas.is_empty());
        let loop_ = ensaiar(&com_loop("{{no:qa}}"), &disponiveis);
        assert!(loop_.problemas.is_empty(), "{:?}", loop_.problemas);
        assert!(loop_.avisos.iter().any(|a| a.contains("primeira passada")), "{:?}", loop_.avisos);
        assert!(ensaiar(&com_loop("{{no:dev}}"), &disponiveis).problemas.iter().any(|p| p.contains("não roda antes")), "a própria etapa não tem saída ainda");
    }

    #[test]
    fn ramo_paralelo_nao_roda_antes_do_irmao() {
        let f = fluxo(json!({"id": "p", "nome": "P", "nos": [
            {"id": "e", "tipo": "entrada"}, {"id": "a", "tipo": "agente", "agente": "dev", "config": {"prompt": "{{no:b}}"}},
            {"id": "b", "tipo": "agente", "agente": "dev"}, {"id": "j", "tipo": "juntar"}, {"id": "f", "tipo": "fim"}],
            "conexoes": [{"de": "e", "para": "a"}, {"de": "e", "para": "b"}, {"de": "a", "para": "j"}, {"de": "b", "para": "j"}, {"de": "j", "para": "f"}]}));
        let ensaio = ensaiar(&f, &[agente("dev", "claude", &[])]);
        assert_eq!(ensaio.problemas.len(), 1, "{:?}", ensaio.problemas);
        assert_eq!((ensaio.agentes, ensaio.maior_lote), (2, 2));
    }

    #[test]
    fn conta_agentes_aprovacoes_e_maior_lote_com_o_teto() {
        let ramos: Vec<Value> = (0..5).map(|i| json!({"id": format!("r{i}"), "tipo": "agente", "agente": "dev"})).collect();
        let mut nos = vec![json!({"id": "e", "tipo": "entrada"}), json!({"id": "ok", "tipo": "aprovacao"}), json!({"id": "f", "tipo": "fim"})];
        nos.extend(ramos);
        let mut conexoes = vec![json!({"de": "e", "para": "ok"}), json!({"de": "ok", "para": "f", "saida": "nao"})];
        conexoes.extend((0..5).map(|i| json!({"de": "ok", "para": format!("r{i}"), "saida": "sim"})));
        conexoes.extend((0..5).map(|i| json!({"de": format!("r{i}"), "para": "f"})));
        let mut f = fluxo(json!({"id": "c", "nome": "C", "nos": nos, "conexoes": conexoes}));
        let padrao = ensaiar(&f, &[agente("dev", "claude", &[])]);
        assert_eq!((padrao.agentes, padrao.aprovacoes, padrao.maior_lote), (5, 1, 5), "sem max_paralelo, todos juntos");
        f.max_paralelo = Some(4);
        assert_eq!(ensaiar(&f, &[agente("dev", "claude", &[])]).maior_lote, 4);
    }

    #[test]
    fn agente_que_nao_existe_e_limite_em_outra_cli_sao_problemas() {
        let mut f = com_loop("x");
        assert!(ensaiar(&f, &[]).problemas.iter().any(|p| p.contains("não existe")));
        f.nos[1].proibidas = vec!["Bash".into()];
        f.nos[1].escrita = Some(vec!["src/".into()]);
        let ensaio = ensaiar(&f, &[agente("dev", "codex", &[])]);
        assert!(ensaio.problemas.iter().any(|p| p.contains("`proibidas`")) && ensaio.problemas.iter().any(|p| p.contains("`escrita`")), "{:?}", ensaio.problemas);
        f.nos[1].proibidas.clear();
        f.nos[1].escrita = Some(Vec::new());
        assert!(ensaiar(&f, &[agente("dev", "codex", &[])]).problemas.is_empty(), "só leitura o Codex faz");
    }

    #[test]
    fn avisa_que_bash_fica_bloqueado_e_que_falta_saida_da_condicao() {
        let mut f = com_loop("x");
        f.nos[1].escrita = Some(vec!["src/".into()]);
        f.conexoes.retain(|c| c.saida != "esgotado");
        let ensaio = ensaiar(&f, &[agente("dev", "claude", &["Bash"])]);
        assert!(ensaio.avisos.iter().any(|a| a.contains("Bash")), "{:?}", ensaio.avisos);
        assert!(ensaio.avisos.iter().any(|a| a.contains("`esgotado`")), "{:?}", ensaio.avisos);
    }
}
