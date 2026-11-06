//! O que as buscas do MCP têm em comum: filtros, datas, repetidos, relevância e aviso de nota antiga.
//! `Base::buscar` continua como está para o terminal; aqui fica a versão que responde a um agente.
use crate::{memoria::{nota_da_linha, sem_acento, slug_de, so_letras, Nota, PALAVRAS_VAZIAS}, sem_dados_pessoais, Base};
use chrono::{DateTime, NaiveDate};
use rusqlite::params;
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};

/// Com menos que isto contendo todas as palavras, a busca completa com quem tem só parte delas.
const POUCOS_RESULTADOS: usize = 3;
/// Repetidos não gastam o `limite`, mas a varredura tem fim: uma palavra comum casa com a base inteira.
const CANDIDATOS: i64 = 2000;
const NOTAS_CANDIDATAS: i64 = 200;
const TERMOS_NA_BUSCA: usize = 12;
const PALAVRAS_CURTAS: [&str; 20] = ["de", "da", "do", "das", "dos", "em", "no", "na", "nos", "nas", "um", "uma", "ao", "as", "os", "por", "com", "que", "se", "ou"];
const VOLTA_DO_RESUMO: usize = 100;
const TAMANHO_DO_RESUMO: usize = 280;
const MENOR_TITULO_COMPARAVEL: usize = 3;
const SEMELHANCA_DE_TITULO: f64 = 0.7;
const TRECHOS_DE_UM_ARQUIVO: i64 = 200;

#[derive(Debug, Default)]
pub struct Filtros {
    pub fonte: String,
    pub projeto: String,
    pub desde_ms: Option<i64>,
    pub resumo: bool,
    pub sem_mascara: bool,
}

impl Filtros {
    pub fn de(argumentos: &Value) -> Result<Self, String> {
        let texto = |nome: &str| argumentos[nome].as_str().map(so_letras).unwrap_or_default();
        Ok(Self {
            fonte: texto("fonte"),
            projeto: texto("projeto"),
            desde_ms: argumentos["desde"].as_str().map(dia_em_ms).transpose()?,
            resumo: argumentos["resumo"] == true,
            sem_mascara: mascara_dispensada(argumentos),
        })
    }

    pub fn limpo(&self, texto: &str) -> String {
        if self.sem_mascara { texto.to_owned() } else { sem_dados_pessoais(texto) }
    }
}

/// Passada final sobre o texto pronto, que pega título e nome de fonte. Quem corta o texto antes disto
/// precisa mascarar antes de cortar: "Maria Si…" já não é reconhecido.
pub fn resposta_limpa(argumentos: &Value, resultado: Result<String, String>) -> Result<String, String> {
    resultado.map(|texto| if mascara_dispensada(argumentos) { texto } else { sem_dados_pessoais(&texto) })
}

/// O engine marca o processo da etapa de fluxo: ali não há pessoa para autorizar e o que o agente lê
/// pode mandar pedir `sem_mascara`, então o argumento é ignorado.
pub const MARCA_DE_ETAPA: &str = "OMNI_ETAPA_DE_FLUXO";

pub fn mascara_dispensada(argumentos: &Value) -> bool {
    dispensa_valida(argumentos, std::env::var_os(MARCA_DE_ETAPA).is_some())
}

fn dispensa_valida(argumentos: &Value, em_etapa_de_fluxo: bool) -> bool {
    argumentos["sem_mascara"] == true && !em_etapa_de_fluxo
}

#[cfg(test)]
mod testes_da_dispensa {
    use super::dispensa_valida;
    use serde_json::json;

    #[test]
    fn etapa_de_fluxo_nao_dispensa_a_mascara_mesmo_pedindo() {
        assert!(!dispensa_valida(&json!({"sem_mascara": true}), true));
    }

    #[test]
    fn conversa_dispensa_so_quando_pede() {
        assert!(dispensa_valida(&json!({"sem_mascara": true}), false));
        assert!(!dispensa_valida(&json!({}), false));
    }
}

/// Junta às propriedades da ferramenta os argumentos opcionais pedidos, com a mesma descrição em todas.
pub fn com_campos(base: Value, nomes: &[&str]) -> Value {
    let mut propriedades = base;
    for nome in nomes {
        propriedades[*nome] = match *nome {
            "fonte" => json!({"type": "string", "description": "Parte do nome da fonte"}),
            "origem" => json!({"type": "string", "description": "Parte da origem da nota: auto, compact ou importada"}),
            "projeto" => json!({"type": "string", "description": "Parte do nome do projeto"}),
            "desde" => json!({"type": "string", "description": "AAAA-MM-DD. O que não tem data fica de fora, e a resposta conta quantos"}),
            "resumo" => json!({"type": "boolean", "description": "Só título e uma linha curta"}),
            _ => json!({"type": "boolean", "description": "Mostra dados pessoais sem máscara. Use SOMENTE se o usuário autorizar explicitamente"}),
        };
    }
    propriedades
}

pub fn dia_em_ms(dia: &str) -> Result<i64, String> {
    NaiveDate::parse_from_str(dia.trim(), "%Y-%m-%d").ok().and_then(|data| data.and_hms_opt(0, 0, 0))
        .map(|inicio| inicio.and_utc().timestamp_millis()).ok_or_else(|| format!("Data inválida: {dia}. Use AAAA-MM-DD."))
}

pub fn data_curta(ms: i64) -> String {
    DateTime::from_timestamp_millis(ms).map_or_else(|| "data inválida".to_owned(), |data| data.format("%Y-%m-%d").to_string())
}

pub fn instante_em_ms(texto: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(texto).ok().map(|data| data.timestamp_millis()).or_else(|| dia_em_ms(texto).ok())
}

/// A data que o próprio arquivo declara no frontmatter do Markdown.
pub fn data_do_texto(texto: &str) -> Option<i64> {
    let cabecalho = texto.strip_prefix("---\n")?.split_once("\n---")?.0;
    let valor = |chave: &str| cabecalho.lines().find_map(|linha| {
        let (nome, valor) = linha.split_once(':')?;
        if nome.trim() == chave { instante_em_ms(valor.trim().trim_matches('"')) } else { None }
    });
    valor("updated").or_else(|| valor("created"))
}

/// A mesma passagem com outra pontuação ou espaçamento continua sendo a mesma passagem.
pub fn chave_do_texto(texto: &str) -> String {
    so_letras(texto)
}

/// Palavra vazia fica de fora porque, no OR, "de" sozinho traz a base inteira. Só palavra vazia: fica tudo.
pub fn termos_da_busca(consulta: &str) -> Vec<String> {
    let todos: Vec<String> = consulta.split(|letra: char| !letra.is_alphanumeric()).filter(|termo| termo.chars().count() >= 2).map(str::to_lowercase).collect();
    let uteis: Vec<String> = todos.iter().filter(|termo| !PALAVRAS_CURTAS.contains(&termo.as_str()) && !PALAVRAS_VAZIAS.contains(&termo.as_str())).cloned().collect();
    let mut escolhidos = if uteis.is_empty() { todos } else { uteis };
    escolhidos.truncate(TERMOS_NA_BUSCA);
    escolhidos
}

fn expressao(termos: &[String], juntar: &str) -> String {
    termos.iter().map(|termo| format!("\"{termo}\"")).collect::<Vec<_>>().join(juntar)
}

/// Janela em volta da primeira palavra achada. Recebe o texto já mascarado, então cortar aqui é seguro.
pub fn resumo_em_volta(texto: &str, termos: &[String]) -> String {
    let dobrado: Vec<char> = texto.chars().map(sem_acento).collect();
    let posicao = termos.iter().find_map(|termo| {
        let procurado: Vec<char> = termo.chars().map(sem_acento).collect();
        dobrado.windows(procurado.len().max(1)).position(|janela| janela == procurado.as_slice())
    }).unwrap_or(0);
    let inicio = posicao.saturating_sub(VOLTA_DO_RESUMO);
    let janela: String = texto.chars().skip(inicio).take(TAMANHO_DO_RESUMO).collect();
    let compacto = janela.split_whitespace().collect::<Vec<_>>().join(" ");
    let antes = if inicio > 0 { "…" } else { "" };
    let depois = if inicio + TAMANHO_DO_RESUMO < dobrado.len() { "…" } else { "" };
    format!("{antes}{compacto}{depois}")
}

fn familia_do_slug(slug: &str) -> String {
    slug.strip_prefix("importada-").unwrap_or(slug).split('-').filter(|parte| !parte.chars().any(|letra| letra.is_ascii_digit())).collect::<Vec<_>>().join("-")
}

fn palavras_do_titulo(titulo: &str) -> HashSet<String> {
    slug_de(titulo).split('-').filter(|parte| parte.len() >= 4 && !parte.chars().any(|letra| letra.is_ascii_digit())).map(str::to_owned).collect()
}

/// Mesma família de slug (só muda número ou data) ou títulos com quase as mesmas palavras.
pub fn mesmo_assunto(slug_a: &str, titulo_a: &str, slug_b: &str, titulo_b: &str) -> bool {
    let familia = familia_do_slug(slug_a);
    if familia.contains('-') && familia == familia_do_slug(slug_b) { return true; }
    let (a, b) = (palavras_do_titulo(titulo_a), palavras_do_titulo(titulo_b));
    if a.len().min(b.len()) < MENOR_TITULO_COMPARAVEL { return false; }
    a.intersection(&b).count() as f64 / a.union(&b).count() as f64 >= SEMELHANCA_DE_TITULO
}

#[derive(Debug)]
pub struct Trecho {
    pub id: i64,
    pub fonte: String,
    pub titulo: String,
    pub texto: String,
    /// Só a data que o conteúdo declara. A importação da fonte não diz de quando o texto é.
    pub data_ms: Option<i64>,
    pub importado_ms: i64,
    pub parcial: bool,
    pub mais_novo: Option<i64>,
}

#[derive(Debug)]
pub struct NotaAchada {
    pub nota: Nota,
    pub data_ms: Option<i64>,
    pub gravada_ms: i64,
    pub parcial: bool,
    pub mais_nova: Option<(String, i64)>,
}

#[derive(Debug)]
pub struct Achados<T> {
    pub itens: Vec<T>,
    pub unidos: usize,
    pub sem_data: usize,
}

impl<T> Default for Achados<T> {
    fn default() -> Self {
        Self { itens: Vec::new(), unidos: 0, sem_data: 0 }
    }
}

/// O que explica a resposta sem repetir em cada linha. Vazio quando não há nada a dizer.
pub fn rodape<T>(achados: &Achados<T>, parcial: bool) -> String {
    let mut partes = Vec::new();
    if achados.unidos > 0 { partes.push(format!("{} repetidos unidos.", achados.unidos)); }
    if parcial { partes.push("Poucos resultados com todas as palavras: os marcados com (parcial) têm só parte delas.".to_owned()); }
    if achados.sem_data > 0 { partes.push(format!("{} sem data ficaram de fora por causa de `desde`.", achados.sem_data)); }
    partes.join(" ")
}

#[derive(Default)]
struct Varredura {
    ids: HashSet<i64>,
    textos: HashSet<String>,
    datas: HashMap<String, Option<i64>>,
    achados: Achados<Trecho>,
}

struct LinhaDeTrecho {
    id: i64,
    fonte: String,
    titulo: String,
    texto: String,
    importado_ms: i64,
}

fn linha_de_trecho(linha: &rusqlite::Row) -> rusqlite::Result<LinhaDeTrecho> {
    Ok(LinhaDeTrecho { id: linha.get(0)?, fonte: linha.get(1)?, titulo: linha.get(2)?, texto: linha.get(3)?, importado_ms: linha.get(4)? })
}

impl Base {
    /// O pedaço de SQL que limita às fontes com esse nome. Vazio é "sem filtro"; `None`, nenhuma fonte.
    pub fn fontes_com_nome(&self, nome: &str) -> Result<Option<String>, String> {
        if nome.is_empty() { return Ok(Some(String::new())); }
        let ids: Vec<String> = self.listar()?.iter().filter(|fonte| so_letras(&fonte.nome).contains(nome)).map(|fonte| fonte.id.to_string()).collect();
        Ok((!ids.is_empty()).then(|| format!("and fonte_id in ({})", ids.join(","))))
    }

    /// O frontmatter fica só no primeiro trecho do arquivo, e os outros herdam a data dele.
    fn data_do_arquivo(&self, titulo: &str) -> Option<i64> {
        if !titulo.ends_with(".md") { return None; }
        let frase = titulo.split(|letra: char| !letra.is_alphanumeric()).filter(|parte| !parte.is_empty()).collect::<Vec<_>>().join(" ");
        let mut pedido = self.banco.prepare_cached("select titulo, texto from trechos where trechos match ?1 order by rowid limit ?2").ok()?;
        let linhas = pedido.query_map(params![format!("titulo : \"{frase}\""), TRECHOS_DE_UM_ARQUIVO], |linha| Ok((linha.get::<_, String>(0)?, linha.get::<_, String>(1)?))).ok()?;
        let primeiro = linhas.flatten().find(|(atual, _)| atual == titulo);
        primeiro.and_then(|(_, texto)| data_do_texto(&texto))
    }

    fn data_do_trecho(&self, linha: &LinhaDeTrecho, datas: &mut HashMap<String, Option<i64>>) -> Option<i64> {
        if let Some(data) = datas.get(&linha.titulo) { return *data; }
        let data = data_do_texto(&linha.texto).or_else(|| self.data_do_arquivo(&linha.titulo));
        datas.insert(linha.titulo.clone(), data);
        data
    }

    fn registrar_trecho(&self, linha: LinhaDeTrecho, parcial: bool, filtros: &Filtros, estado: &mut Varredura) {
        if !estado.ids.insert(linha.id) { return; }
        if !estado.textos.insert(chave_do_texto(&linha.texto)) { estado.achados.unidos += 1; return; }
        let data_ms = self.data_do_trecho(&linha, &mut estado.datas);
        if let Some(desde) = filtros.desde_ms {
            match data_ms {
                None => { estado.achados.sem_data += 1; return; }
                Some(data) if data < desde => return,
                Some(_) => {}
            }
        }
        estado.achados.itens.push(Trecho { id: linha.id, fonte: linha.fonte, titulo: linha.titulo, texto: linha.texto, data_ms, importado_ms: linha.importado_ms, parcial, mais_novo: None });
    }

    fn varrer_trechos(&self, busca: &str, restricao: &str, parcial: bool, filtros: &Filtros, limite: usize, estado: &mut Varredura) -> Result<(), String> {
        let pedido = format!("select rowid, (select nome from fontes where id = fonte_id), titulo, texto, (select criado_em_ms from fontes where id = fonte_id)
            from trechos where trechos match ?1 {restricao} order by bm25(trechos, 1.0, 2.0) limit ?2");
        let mut consulta = self.banco.prepare_cached(&pedido).map_err(|erro| erro.to_string())?;
        let mut linhas = consulta.query(params![busca, CANDIDATOS]).map_err(|erro| erro.to_string())?;
        while estado.achados.itens.len() < limite {
            let Some(linha) = linhas.next().map_err(|erro| erro.to_string())? else { break };
            if let Ok(lida) = linha_de_trecho(linha) { self.registrar_trecho(lida, parcial, filtros, estado); }
        }
        Ok(())
    }

    /// Exige todas as palavras. Com poucos resultados, completa com quem tem parte delas, sem esconder isso.
    pub fn procurar_trechos(&self, consulta: &str, restricao: &str, filtros: &Filtros, limite: usize) -> Result<Achados<Trecho>, String> {
        let termos = termos_da_busca(consulta);
        if termos.is_empty() { return Ok(Achados::default()); }
        let mut estado = Varredura::default();
        self.varrer_trechos(&expressao(&termos, " "), restricao, false, filtros, limite, &mut estado)?;
        if estado.achados.itens.len() < POUCOS_RESULTADOS.min(limite) && termos.len() > 1 {
            self.varrer_trechos(&expressao(&termos, " OR "), restricao, true, filtros, limite, &mut estado)?;
        }
        let mut achados = estado.achados;
        marcar_trechos_antigos(&mut achados.itens);
        Ok(achados)
    }

    /// Nota importada: `atualizado_ms` é o dia da importação. A data real vem do arquivo, se ele também
    /// entrou como fonte. Sem isso a nota fica sem data, e não com uma inventada.
    fn data_da_nota(&self, nota: &Nota, gravada_ms: i64) -> Option<i64> {
        if nota.origem != "importada" { return Some(gravada_ms); }
        self.data_do_arquivo(&format!("{}.md", nota.slug.strip_prefix("importada-").unwrap_or(&nota.slug)))
    }

    fn notas_que_casam(&self, busca: &str, parcial: bool) -> Result<Vec<NotaAchada>, String> {
        let mut pedido = self.banco.prepare_cached("select n.slug, n.titulo, n.corpo, n.tags, n.projeto, n.origem, n.conversa, n.atualizado_ms
            from notas_fts join notas n on n.id = notas_fts.rowid where notas_fts match ?1 order by bm25(notas_fts, 3.0, 1.0, 2.0) limit ?2").map_err(|erro| erro.to_string())?;
        let linhas = pedido.query_map(params![busca, NOTAS_CANDIDATAS], |linha| Ok((nota_da_linha(linha)?, linha.get::<_, i64>(7)?))).map_err(|erro| erro.to_string())?;
        Ok(linhas.flatten().map(|(nota, gravada_ms)| {
            let data_ms = self.data_da_nota(&nota, gravada_ms);
            NotaAchada { nota, data_ms, gravada_ms, parcial, mais_nova: None }
        }).collect())
    }

    /// Todas as palavras primeiro; com poucas notas depois dos filtros, completa com as parciais.
    fn notas_candidatas(&self, termos: &[String], filtros: &Filtros, limite: usize) -> Result<(Vec<NotaAchada>, usize), String> {
        let com_todas = self.notas_que_casam(&expressao(termos, " "), false)?;
        // Antes de unir: a cópia descartada volta no OR e não pode ser contada duas vezes.
        let vistas: HashSet<String> = com_todas.iter().map(|achada| achada.nota.slug.clone()).collect();
        let (notas, unidas) = sem_notas_repetidas(com_todas);
        let aceitas = notas.iter().filter(|nota| passa_nos_filtros(nota, filtros)).count();
        if aceitas >= POUCOS_RESULTADOS.min(limite) || termos.len() < 2 { return Ok((notas, unidas)); }
        let parciais = self.notas_que_casam(&expressao(termos, " OR "), true)?.into_iter().filter(|achada| !vistas.contains(&achada.nota.slug));
        let (todas, novas) = sem_notas_repetidas(notas.into_iter().chain(parciais).collect());
        Ok((todas, unidas + novas))
    }

    /// Notas ligadas à consulta, sem repetidas, com as do projeto atual na frente.
    pub fn procurar_notas(&self, consulta: &str, projeto_atual: &str, filtros: &Filtros, limite: usize) -> Result<Achados<NotaAchada>, String> {
        let termos = termos_da_busca(consulta);
        if termos.is_empty() { return Ok(Achados::default()); }
        let (mut notas, unidos) = self.notas_candidatas(&termos, filtros, limite)?;
        marcar_notas_antigas(&mut notas);
        let sem_data = if filtros.desde_ms.is_some() { notas.iter().filter(|achada| achada.data_ms.is_none()).count() } else { 0 };
        let daqui = so_letras(projeto_atual);
        let mut itens: Vec<NotaAchada> = notas.into_iter().filter(|achada| passa_nos_filtros(achada, filtros)).collect();
        itens.sort_by_key(|achada| (achada.parcial, so_letras(&achada.nota.projeto) != daqui));
        itens.truncate(limite);
        Ok(Achados { itens, unidos, sem_data })
    }
}

fn passa_nos_filtros(achada: &NotaAchada, filtros: &Filtros) -> bool {
    so_letras(&achada.nota.projeto).contains(&filtros.projeto) && so_letras(&achada.nota.origem).contains(&filtros.fonte)
        && filtros.desde_ms.is_none_or(|desde| achada.data_ms.is_some_and(|data| data >= desde))
}

/// A cópia mantida é a mais bem colocada, com a data mais nova do grupo.
fn sem_notas_repetidas(notas: Vec<NotaAchada>) -> (Vec<NotaAchada>, usize) {
    let mut posicoes: HashMap<String, usize> = HashMap::new();
    let mut unicas: Vec<NotaAchada> = Vec::with_capacity(notas.len());
    let mut unidas = 0;
    for achada in notas {
        let chave = chave_do_texto(&achada.nota.corpo);
        if let Some(&posicao) = posicoes.get(&chave) {
            unicas[posicao].data_ms = unicas[posicao].data_ms.max(achada.data_ms);
            unidas += 1;
        } else {
            posicoes.insert(chave, unicas.len());
            unicas.push(achada);
        }
    }
    (unicas, unidas)
}

fn marcar_notas_antigas(notas: &mut [NotaAchada]) {
    let marcas: Vec<Option<(String, i64)>> = notas.iter().map(|achada| {
        let data = achada.data_ms?;
        notas.iter().filter(|outra| so_letras(&outra.nota.projeto) == so_letras(&achada.nota.projeto))
            .filter(|outra| outra.data_ms.is_some_and(|nova| nova > data))
            .filter(|outra| mesmo_assunto(&achada.nota.slug, &achada.nota.titulo, &outra.nota.slug, &outra.nota.titulo))
            .max_by_key(|outra| outra.data_ms)
            .and_then(|outra| Some((outra.nota.titulo.clone(), outra.data_ms?)))
    }).collect();
    notas.iter_mut().zip(marcas).for_each(|(achada, marca)| achada.mais_nova = marca);
}

fn marcar_trechos_antigos(trechos: &mut [Trecho]) {
    let nome = |trecho: &Trecho| trecho.titulo.trim_end_matches(".md").replace('-', " ");
    let marcas: Vec<Option<i64>> = trechos.iter().map(|trecho| {
        let data = trecho.data_ms?;
        trechos.iter().filter(|outro| outro.titulo != trecho.titulo && outro.data_ms.is_some_and(|nova| nova > data))
            .find(|outro| mesmo_assunto(&slug_de(&nome(trecho)), &nome(trecho), &slug_de(&nome(outro)), &nome(outro)))
            .map(|outro| outro.id)
    }).collect();
    trechos.iter_mut().zip(marcas).for_each(|(trecho, marca)| trecho.mais_novo = marca);
}
