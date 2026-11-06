//! Visão em grafo da memória: as ligações não são guardadas, saem da leitura das notas.
use crate::memoria::{cortar, nota_da_linha, slug_de, so_letras, Nota};
use crate::{sem_segredos, Base};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const TAG_COMUM_DEMAIS: usize = 30;
pub const VIZINHOS_DE_TEXTO: usize = 3;
const CARACTERES_DO_RESUMO: usize = 180;
const PESO_WIKILINK: f32 = 1.0;
const PESO_CONVERSA: f32 = 0.7;
const PESO_TAG: f32 = 0.4;
const PESO_TEXTO: f32 = 0.2;
/// Quantas candidatas pedir ao índice antes de tirar a própria nota e as de fora do filtro.
const CANDIDATAS_DE_TEXTO: usize = 12;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct NotaDoGrafo {
    pub slug: String,
    pub titulo: String,
    pub resumo: String,
    pub tags: Vec<String>,
    pub projeto: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Ligacao {
    pub de: String,
    pub para: String,
    pub tipo: String,
    pub peso: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Grafo {
    pub notas: Vec<NotaDoGrafo>,
    pub ligacoes: Vec<Ligacao>,
}

impl Base {
    fn todas_as_notas(&self) -> Vec<Nota> {
        let Ok(mut pedido) = self.banco.prepare_cached("select slug, titulo, corpo, tags, projeto, origem, conversa from notas order by slug") else { return Vec::new() };
        let Ok(linhas) = pedido.query_map([], nota_da_linha) else { return Vec::new() };
        linhas.flatten().collect()
    }

    fn assinatura_das_notas(&self) -> (i64, i64) {
        self.banco.query_row("select count(*), coalesce(max(atualizado_ms), 0) from notas", [], |l| Ok((l.get(0)?, l.get(1)?))).unwrap_or((0, 0))
    }

    fn arquivo_do_cache_do_grafo(&self) -> Option<PathBuf> {
        self.banco.path().and_then(|c| Path::new(c).parent()).map(|pasta| pasta.join("grafo-cache.json"))
    }
}

/// Notas e ligações, calculadas na leitura. `projeto` filtra as notas e, com elas, as ligações.
pub fn grafo_de_notas(base: &Base, projeto: Option<&str>) -> Grafo {
    filtrar_por_projeto(calcular(base), projeto)
}

/// Igual a `grafo_de_notas`, mas guarda o grafo inteiro ao lado do banco enquanto as notas não mudam.
/// Medido numa cópia de 2 mil notas: o cálculo leva dezenas de segundos (uma busca de texto por nota), muito
/// acima de 500 ms; a assinatura é o total de notas mais o maior `atualizado_ms`.
pub fn grafo_com_cache(base: &Base, projeto: Option<&str>) -> Grafo {
    let assinatura = base.assinatura_das_notas();
    let arquivo = base.arquivo_do_cache_do_grafo();
    let guardado = arquivo.as_ref().and_then(|a| std::fs::read(a).ok()).and_then(|b| serde_json::from_slice::<CacheDoGrafo>(&b).ok());
    let inteiro = match guardado {
        Some(cache) if cache.assinatura == assinatura => cache.grafo,
        _ => {
            let grafo = calcular(base);
            if let Some(arquivo) = &arquivo { guardar(arquivo, &CacheDoGrafo { assinatura, grafo: grafo.clone() }); }
            grafo
        }
    };
    filtrar_por_projeto(inteiro, projeto)
}

#[derive(Serialize, Deserialize)]
struct CacheDoGrafo {
    assinatura: (i64, i64),
    grafo: Grafo,
}

/// Escrita atômica; falhar em guardar só custa recalcular na próxima vez.
fn guardar(arquivo: &Path, cache: &CacheDoGrafo) {
    let Ok(bytes) = serde_json::to_vec(cache) else { return };
    let provisorio = arquivo.with_extension("tmp");
    if std::fs::write(&provisorio, bytes).is_ok() && std::fs::rename(&provisorio, arquivo).is_err() { let _ = std::fs::remove_file(&provisorio); }
}

fn filtrar_por_projeto(grafo: Grafo, projeto: Option<&str>) -> Grafo {
    let Some(alvo) = projeto.map(so_letras) else { return grafo };
    let notas: Vec<NotaDoGrafo> = grafo.notas.into_iter().filter(|n| so_letras(&n.projeto) == alvo).collect();
    let slugs: HashSet<&str> = notas.iter().map(|n| n.slug.as_str()).collect();
    let ligacoes = grafo.ligacoes.into_iter().filter(|l| slugs.contains(l.de.as_str()) && slugs.contains(l.para.as_str())).collect();
    Grafo { notas, ligacoes }
}

fn calcular(base: &Base) -> Grafo {
    let notas: Vec<Nota> = base.todas_as_notas();
    let slugs: HashSet<&str> = notas.iter().map(|n| n.slug.as_str()).collect();
    let mut ligacoes = Vec::new();
    let mut vistas: HashSet<(String, String, &'static str)> = HashSet::new();
    let mut ligar = |de: &str, para: &str, tipo: &'static str, peso: f32| {
        if de == para { return; }
        let chave = if de < para { (de.to_owned(), para.to_owned(), tipo) } else { (para.to_owned(), de.to_owned(), tipo) };
        if vistas.insert(chave) { ligacoes.push(Ligacao { de: de.into(), para: para.into(), tipo: tipo.into(), peso }); }
    };
    for nota in &notas {
        for alvo in wikilinks(&nota.corpo).into_iter().filter(|a| slugs.contains(a.as_str())) { ligar(&nota.slug, &alvo, "wikilink", PESO_WIKILINK); }
    }
    for grupo in grupos(notas.iter().filter(|n| !n.conversa.trim().is_empty()).map(|n| (n.conversa.trim().to_owned(), n.slug.as_str()))) {
        ligar_grupo(&grupo, "conversa", PESO_CONVERSA, &mut ligar);
    }
    for grupo in grupos(notas.iter().flat_map(|n| tags_de(&n.tags).into_iter().map(move |t| (t, n.slug.as_str())))) {
        ligar_grupo(&grupo, "tag", PESO_TAG, &mut ligar);
    }
    for nota in &notas {
        let texto = format!("{} {}", nota.titulo, nota.corpo);
        let parecidas = base.notas_relacionadas(&texto, &nota.projeto, CANDIDATAS_DE_TEXTO);
        for vizinha in parecidas.iter().filter(|v| v.slug != nota.slug && slugs.contains(v.slug.as_str())).take(VIZINHOS_DE_TEXTO) {
            ligar(&nota.slug, &vizinha.slug, "texto", PESO_TEXTO);
        }
    }
    let notas = notas.into_iter().map(|n| NotaDoGrafo {
        resumo: cortar(&sem_segredos(&n.corpo), CARACTERES_DO_RESUMO),
        tags: tags_de(&n.tags).into_iter().collect(),
        slug: n.slug, titulo: sem_segredos(&n.titulo), projeto: n.projeto,
    }).collect();
    Grafo { notas, ligacoes }
}

fn tags_de(tags: &str) -> BTreeSet<String> {
    tags.split(',').map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty()).collect()
}

/// Agrupa por chave e descarta o grupo comum demais: ele ligaria tudo com tudo e viraria um novelo.
fn grupos<'a>(pares: impl Iterator<Item = (String, &'a str)>) -> Vec<Vec<&'a str>> {
    let mut por_chave: HashMap<String, BTreeSet<&str>> = HashMap::new();
    for (chave, slug) in pares { por_chave.entry(chave).or_default().insert(slug); }
    por_chave.into_values().filter(|g| g.len() > 1 && g.len() <= TAG_COMUM_DEMAIS).map(|g| g.into_iter().collect()).collect()
}

fn ligar_grupo(grupo: &[&str], tipo: &'static str, peso: f32, ligar: &mut impl FnMut(&str, &str, &'static str, f32)) {
    for (i, a) in grupo.iter().enumerate() {
        for b in &grupo[i + 1..] { ligar(a, b, tipo, peso); }
    }
}

/// Os `[[alvo]]` do corpo, já no formato de slug (aceita o título, com ou sem `|apelido`).
fn wikilinks(corpo: &str) -> Vec<String> {
    let mut achados = Vec::new();
    let mut resto = corpo;
    while let Some(inicio) = resto.find("[[") {
        let depois = &resto[inicio + 2..];
        let Some(fim) = depois.find("]]") else { break };
        let slug = slug_de(depois[..fim].split('|').next().unwrap_or_default().trim());
        if !slug.is_empty() { achados.push(slug); }
        resto = &depois[fim + 2..];
    }
    achados
}
