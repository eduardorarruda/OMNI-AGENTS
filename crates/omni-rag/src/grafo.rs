//! Visão em grafo da memória: as ligações não são guardadas, saem da leitura das notas.
//! Citação, conversa e tag saem sempre da leitura (instantâneas). O texto parecido custa uma busca por nota
//! (medido: dezenas de segundos com 2 mil notas), então as vizinhas de texto ficam num cache por nota e só
//! as notas novas ou alteradas são buscadas de novo.
use crate::memoria::{cortar, nota_da_linha, slug_de, so_letras, Nota};
use crate::{sem_segredos, Base};
use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

pub const TAG_COMUM_DEMAIS: usize = 30;
pub const VIZINHOS_DE_TEXTO: usize = 3;
const CARACTERES_DO_RESUMO: usize = 180;
const PESO_WIKILINK: f32 = 1.0;
const PESO_CONVERSA: f32 = 0.7;
const PESO_TAG: f32 = 0.4;
const PESO_TEXTO: f32 = 0.2;

/// Uma passada de cada vez: a segunda chamada espera a primeira e já encontra o cache pronto.
static CALCULO: Mutex<()> = Mutex::new(());
static PROVISORIOS: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
thread_local! { static BUSCAS_DE_TEXTO: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(test)]
pub(crate) fn buscas_de_texto() -> usize { BUSCAS_DE_TEXTO.with(|c| c.get()) }
#[cfg(test)]
pub(crate) fn zerar_buscas_de_texto() { BUSCAS_DE_TEXTO.with(|c| c.set(0)) }

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

/// As vizinhas de texto de uma nota, válidas enquanto a nota não muda (data e impressão do texto).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct TextoDaNota {
    atualizado_ms: i64,
    impressao: u64,
    vizinhas: Vec<String>,
    /// A nota já pronta para a tela: a limpeza de segredos é cara e só roda para nota nova ou alterada.
    nota: NotaDoGrafo,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheDoGrafo {
    #[serde(default)]
    textos: HashMap<String, TextoDaNota>,
}

impl Base {
    fn todas_as_notas(&self) -> Vec<(Nota, i64)> {
        let Ok(mut pedido) = self.banco.prepare_cached("select slug, titulo, corpo, tags, projeto, origem, conversa, atualizado_ms from notas order by slug") else { return Vec::new() };
        let Ok(linhas) = pedido.query_map([], |l| Ok((nota_da_linha(l)?, l.get::<_, i64>(7)?))) else { return Vec::new() };
        linhas.flatten().collect()
    }

    fn arquivo_do_cache_do_grafo(&self) -> Option<PathBuf> {
        self.banco.path().and_then(|c| Path::new(c).parent()).map(|pasta| pasta.join("grafo-cache.json"))
    }
}

/// Notas e ligações, calculadas na leitura. `projeto` filtra as notas e, com elas, as ligações.
pub fn grafo_de_notas(base: &Base, projeto: Option<&str>) -> Grafo {
    filtrar_por_projeto(calcular(base, &mut HashMap::new()), projeto)
}

/// Igual a `grafo_de_notas`, mas guarda ao lado do banco as vizinhas de texto de cada nota.
pub fn grafo_com_cache(base: &Base, projeto: Option<&str>) -> Grafo {
    let _vez = CALCULO.lock().unwrap_or_else(|erro| erro.into_inner());
    let arquivo = base.arquivo_do_cache_do_grafo();
    let guardado = arquivo.as_ref().and_then(|a| std::fs::read(a).ok()).and_then(|b| serde_json::from_slice::<CacheDoGrafo>(&b).ok());
    let mut cache = guardado.unwrap_or_default();
    let antes = cache.textos.clone();
    let grafo = calcular(base, &mut cache.textos);
    if cache.textos != antes || !arquivo.as_ref().is_some_and(|a| a.exists()) {
        if let Some(arquivo) = &arquivo { guardar(arquivo, &cache); }
    }
    filtrar_por_projeto(grafo, projeto)
}

/// Escrita atômica com nome provisório único; falhar em guardar só custa buscar de novo na próxima vez.
fn guardar(arquivo: &Path, cache: &CacheDoGrafo) {
    let Ok(bytes) = serde_json::to_vec(cache) else { return };
    let numero = PROVISORIOS.fetch_add(1, Ordering::Relaxed);
    let provisorio = arquivo.with_extension(format!("{}.{numero}.tmp", std::process::id()));
    if std::fs::write(&provisorio, bytes).is_ok() && std::fs::rename(&provisorio, arquivo).is_err() { let _ = std::fs::remove_file(&provisorio); }
}

fn filtrar_por_projeto(grafo: Grafo, projeto: Option<&str>) -> Grafo {
    let Some(alvo) = projeto.map(so_letras) else { return grafo };
    let notas: Vec<NotaDoGrafo> = grafo.notas.into_iter().filter(|n| so_letras(&n.projeto) == alvo).collect();
    let slugs: HashSet<&str> = notas.iter().map(|n| n.slug.as_str()).collect();
    let ligacoes = grafo.ligacoes.into_iter().filter(|l| slugs.contains(l.de.as_str()) && slugs.contains(l.para.as_str())).collect();
    Grafo { notas, ligacoes }
}

fn impressao_do_texto(nota: &Nota) -> u64 {
    let mut hasher = DefaultHasher::new();
    (&nota.titulo, &nota.corpo, &nota.projeto, &nota.tags).hash(&mut hasher);
    hasher.finish()
}

fn nota_para_a_tela(nota: &Nota) -> NotaDoGrafo {
    NotaDoGrafo {
        slug: nota.slug.clone(),
        titulo: sem_segredos(&nota.titulo),
        resumo: cortar(&sem_segredos(&nota.corpo), CARACTERES_DO_RESUMO),
        tags: tags_de(&sem_segredos(&nota.tags)).into_iter().collect(),
        projeto: nota.projeto.clone(),
    }
}

/// Busca as vizinhas de texto de uma nota. Pede uma a mais: a própria nota costuma voltar na lista.
fn buscar_vizinhas_de_texto(base: &Base, nota: &Nota) -> Vec<String> {
    #[cfg(test)]
    BUSCAS_DE_TEXTO.with(|c| c.set(c.get() + 1));
    let texto = format!("{} {}", nota.titulo, nota.corpo);
    base.notas_relacionadas(&texto, &nota.projeto, VIZINHOS_DE_TEXTO + 1)
        .into_iter().filter(|v| v.slug != nota.slug).take(VIZINHOS_DE_TEXTO).map(|v| v.slug).collect()
}

fn calcular(base: &Base, textos: &mut HashMap<String, TextoDaNota>) -> Grafo {
    let lidas = base.todas_as_notas();
    let slugs: HashSet<&str> = lidas.iter().map(|(n, _)| n.slug.as_str()).collect();
    let mut ligacoes = Vec::new();
    let mut vistas: HashSet<(String, String, &'static str)> = HashSet::new();
    let mut ligar = |de: &str, para: &str, tipo: &'static str, peso: f32| {
        if de == para { return; }
        let chave = if de < para { (de.to_owned(), para.to_owned(), tipo) } else { (para.to_owned(), de.to_owned(), tipo) };
        if vistas.insert(chave) { ligacoes.push(Ligacao { de: de.into(), para: para.into(), tipo: tipo.into(), peso }); }
    };
    for (nota, _) in &lidas {
        for alvo in wikilinks(&nota.corpo).into_iter().filter(|a| slugs.contains(a.as_str())) { ligar(&nota.slug, &alvo, "wikilink", PESO_WIKILINK); }
    }
    for grupo in grupos(lidas.iter().filter(|(n, _)| !n.conversa.trim().is_empty()).map(|(n, _)| (n.conversa.trim().to_owned(), n.slug.as_str()))) {
        ligar_grupo(&grupo, "conversa", PESO_CONVERSA, &mut ligar);
    }
    for grupo in grupos(lidas.iter().flat_map(|(n, _)| tags_de(&n.tags).into_iter().map(move |t| (t, n.slug.as_str())))) {
        ligar_grupo(&grupo, "tag", PESO_TAG, &mut ligar);
    }
    textos.retain(|slug, _| slugs.contains(slug.as_str()));
    for (nota, atualizado_ms) in &lidas {
        let impressao = impressao_do_texto(nota);
        let vale = textos.get(&nota.slug).is_some_and(|t| t.atualizado_ms == *atualizado_ms && t.impressao == impressao);
        if !vale {
            let pronta = TextoDaNota { atualizado_ms: *atualizado_ms, impressao, vizinhas: buscar_vizinhas_de_texto(base, nota), nota: nota_para_a_tela(nota) };
            textos.insert(nota.slug.clone(), pronta);
        }
        for vizinha in textos[&nota.slug].vizinhas.iter().filter(|v| slugs.contains(v.as_str())) { ligar(&nota.slug, vizinha, "texto", PESO_TEXTO); }
    }
    let notas = lidas.iter().map(|(n, _)| textos[&n.slug].nota.clone()).collect();
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
