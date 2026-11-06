use super::grafo::*;
use super::memoria::*;
use super::*;

fn base(pasta: &Path) -> Base {
    Base::abrir(&pasta.join("rag/base.db")).unwrap()
}

fn nota(slug: &str, corpo: &str, projeto: &str, tags: &str, conversa: &str) -> Nota {
    Nota { slug: slug.into(), titulo: slug.into(), corpo: corpo.into(), projeto: projeto.into(), tags: tags.into(), conversa: conversa.into(), origem: "auto".into() }
}

fn gravar(base: &Base, notas: &[Nota]) {
    for n in notas { base.gravar_nota(n).unwrap(); }
}

fn liga(grafo: &Grafo, a: &str, b: &str, tipo: &str) -> Option<f32> {
    grafo.ligacoes.iter().find(|l| l.tipo == tipo && ((l.de == a && l.para == b) || (l.de == b && l.para == a))).map(|l| l.peso)
}

#[test]
fn wikilink_liga_e_pesa_mais() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("alfa", "veja [[beta]] para detalhes", "", "x", "c1"), nota("beta", "assunto distinto aqui", "", "x", "c1")]);
    let grafo = grafo_de_notas(&base, None);
    let wiki = liga(&grafo, "alfa", "beta", "wikilink").unwrap();
    let tag = liga(&grafo, "alfa", "beta", "tag").unwrap();
    let conversa = liga(&grafo, "alfa", "beta", "conversa").unwrap();
    assert!(wiki > conversa && conversa > tag);
}

#[test]
fn wikilink_para_nota_inexistente_nao_liga() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("alfa", "veja [[fantasma]] e [[Alfa]]", "", "", "")]);
    let grafo = grafo_de_notas(&base, None);
    assert!(grafo.ligacoes.is_empty());
    assert_eq!(grafo.notas.len(), 1);
}

#[test]
fn mesma_conversa_liga() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "zzz primeiro", "", "", "conv-1"), nota("b", "yyy segundo", "", "", "conv-1"), nota("c", "xxx terceiro", "", "", "conv-2")]);
    let grafo = grafo_de_notas(&base, None);
    assert!(liga(&grafo, "a", "b", "conversa").is_some());
    assert!(liga(&grafo, "a", "c", "conversa").is_none());
}

#[test]
fn conversa_vazia_nao_liga_tudo() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "zzz primeiro", "", "", ""), nota("b", "yyy segundo", "", "", "")]);
    assert!(grafo_de_notas(&base, None).ligacoes.iter().all(|l| l.tipo != "conversa"));
}

#[test]
fn tag_comum_demais_nao_liga() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    for i in 0..=TAG_COMUM_DEMAIS { gravar(&base, &[nota(&format!("n{i}"), &format!("corpo{i} unico{i}"), "", "geral", "")]); }
    assert!(grafo_de_notas(&base, None).ligacoes.iter().all(|l| l.tipo != "tag"));
    let pasta2 = tempfile::tempdir().unwrap();
    let base2 = self::base(pasta2.path());
    gravar(&base2, &[nota("a", "zzz", "", "Raro", ""), nota("b", "yyy", "", "raro, outra", "")]);
    assert!(liga(&grafo_de_notas(&base2, None), "a", "b", "tag").is_some());
}

#[test]
fn texto_parecido_limita_a_3_vizinhas() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    for i in 0..8 { gravar(&base, &[nota(&format!("p{i}"), "deploy produção esteira publicar servidor", "", "", "")]); }
    let grafo = grafo_de_notas(&base, None);
    for i in 0..8 {
        let nome = format!("p{i}");
        assert!(grafo.ligacoes.iter().filter(|l| l.tipo == "texto" && l.de == nome).count() <= VIZINHOS_DE_TEXTO);
    }
    assert!(grafo.ligacoes.iter().any(|l| l.tipo == "texto"));
}

#[test]
fn sem_ligacao_duplicada_nos_dois_sentidos() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "ver [[b]] deploy produção esteira", "", "", ""), nota("b", "ver [[a]] deploy produção esteira", "", "", "")]);
    let grafo = grafo_de_notas(&base, None);
    for tipo in ["wikilink", "texto"] {
        assert_eq!(grafo.ligacoes.iter().filter(|l| l.tipo == tipo).count(), 1, "{tipo}");
    }
}

#[test]
fn nota_nao_liga_a_si_mesma() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "ver [[a]] deploy produção esteira", "", "t", "c")]);
    assert!(grafo_de_notas(&base, None).ligacoes.is_empty());
}

#[test]
fn filtro_de_projeto() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "ver [[b]]", "Meu_Projeto", "", ""), nota("b", "zzz", "meu projeto", "", ""), nota("c", "ver [[a]]", "outro", "", "")]);
    let grafo = grafo_de_notas(&base, Some("meu-projeto"));
    let mut slugs: Vec<_> = grafo.notas.iter().map(|n| n.slug.as_str()).collect();
    slugs.sort();
    assert_eq!(slugs, ["a", "b"]);
    assert!(grafo.ligacoes.iter().all(|l| l.de != "c" && l.para != "c"));
    assert!(liga(&grafo, "a", "b", "wikilink").is_some());
}

#[test]
fn resumo_passa_por_sem_segredos() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "chave", "", "", "")]);
    let segredo = format!("sk-{}", "a".repeat(30));
    base.banco.execute("update notas set corpo = ?1", [format!("token {segredo} e mais texto {}", "x".repeat(400))]).unwrap();
    let grafo = grafo_de_notas(&base, None);
    assert!(!grafo.notas[0].resumo.contains(&segredo));
    assert!(grafo.notas[0].resumo.contains("[segredo removido]"));
    assert!(grafo.notas[0].resumo.chars().count() <= 200);
}

/// Medição manual em cópia do banco real: defina OMNI_GRAFO_COPIA e rode com `--ignored --nocapture`.
#[test]
#[ignore]
fn medir_grafo_na_copia() {
    let Some(caminho) = std::env::var_os("OMNI_GRAFO_COPIA") else { return };
    let base = Base::abrir(Path::new(&caminho)).unwrap();
    let medir = |rotulo: &str| {
        let inicio = std::time::Instant::now();
        let grafo = grafo_com_cache(&base, None);
        println!("{rotulo}: notas={} ligacoes={} tempo={:?}", grafo.notas.len(), grafo.ligacoes.len(), inicio.elapsed());
    };
    medir("frio");
    medir("quente");
    base.banco.execute("update notas set atualizado_ms = atualizado_ms + 1 where slug = (select slug from notas limit 1)", []).unwrap();
    medir("depois de mudar uma nota");
}

#[test]
fn cache_vale_enquanto_as_notas_nao_mudam_e_cai_quando_mudam() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "ver [[b]]", "", "", ""), nota("b", "zzz", "", "", "")]);
    assert_eq!(grafo_com_cache(&base, None).ligacoes.len(), 1);
    let arquivo = pasta.path().join("rag/grafo-cache.json");
    assert!(arquivo.exists());
    let antes = std::fs::read(&arquivo).unwrap();
    assert_eq!(grafo_com_cache(&base, None).ligacoes.len(), 1);
    assert_eq!(std::fs::read(&arquivo).unwrap(), antes);
    base.gravar_nota(&nota("c", "ver [[a]]", "", "", "")).unwrap();
    assert_eq!(grafo_com_cache(&base, None).notas.len(), 3);
    assert_eq!(grafo_com_cache(&base, Some("nenhum")).notas.len(), 0);
}

fn parecidas(slug: &str) -> Nota {
    nota(slug, "deploy produção esteira publicar servidor", "", "", "")
}

#[test]
fn mudar_uma_nota_nao_refaz_a_busca_das_outras() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[parecidas("a"), parecidas("b"), parecidas("c")]);
    zerar_buscas_de_texto();
    grafo_com_cache(&base, None);
    assert_eq!(buscas_de_texto(), 3);
    grafo_com_cache(&base, None);
    assert_eq!(buscas_de_texto(), 3, "sem mudança não busca");
    let mut mudada = parecidas("b");
    mudada.corpo = "outro assunto totalmente diferente sobre bolo".into();
    base.gravar_nota(&mudada).unwrap();
    grafo_com_cache(&base, None);
    assert_eq!(buscas_de_texto(), 4, "só a nota alterada");
    gravar(&base, &[parecidas("d")]);
    grafo_com_cache(&base, None);
    assert_eq!(buscas_de_texto(), 5, "só a nota nova");
}

#[test]
fn apagar_uma_nota_tira_a_ligacao_de_quem_apontava() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[parecidas("a"), parecidas("b")]);
    assert!(grafo_com_cache(&base, None).ligacoes.iter().any(|l| l.tipo == "texto"));
    base.banco.execute("delete from notas where slug = 'b'", []).unwrap();
    let depois = grafo_com_cache(&base, None);
    assert!(depois.ligacoes.is_empty());
    assert_eq!(depois.notas.len(), 1);
}

#[test]
fn cache_corrompido_recalcula() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[parecidas("a"), parecidas("b")]);
    std::fs::write(pasta.path().join("rag/grafo-cache.json"), "{ lixo").unwrap();
    assert!(grafo_com_cache(&base, None).ligacoes.iter().any(|l| l.tipo == "texto"));
    assert!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(pasta.path().join("rag/grafo-cache.json")).unwrap()).is_ok());
}

#[test]
fn chamadas_ao_mesmo_tempo_dao_o_mesmo_grafo() {
    let pasta = tempfile::tempdir().unwrap();
    let caminho = pasta.path().to_path_buf();
    gravar(&self::base(&caminho), &[parecidas("a"), parecidas("b"), parecidas("c")]);
    let fios: Vec<_> = (0..4).map(|_| { let c = caminho.clone(); std::thread::spawn(move || grafo_com_cache(&self::base(&c), None)) }).collect();
    let grafos: Vec<Grafo> = fios.into_iter().map(|f| f.join().unwrap()).collect();
    assert!(grafos.windows(2).all(|par| par[0] == par[1]));
    let provisorios: Vec<_> = std::fs::read_dir(caminho.join("rag")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.ends_with(".tmp")).collect();
    assert!(provisorios.is_empty(), "{provisorios:?}");
}

#[test]
fn tags_passam_por_sem_segredos() {
    let pasta = tempfile::tempdir().unwrap();
    let base = base(pasta.path());
    gravar(&base, &[nota("a", "corpo da nota", "", "x", "")]);
    let segredo = format!("sk-{}", "b".repeat(30));
    base.banco.execute("update notas set tags = ?1", [format!("ok,{segredo}")]).unwrap();
    let grafo = grafo_de_notas(&base, None);
    assert!(grafo.notas[0].tags.iter().all(|t| !t.contains(&segredo)));
}

#[test]
fn banco_vazio_devolve_grafo_vazio() {
    let pasta = tempfile::tempdir().unwrap();
    let grafo = grafo_de_notas(&base(pasta.path()), None);
    assert!(grafo.notas.is_empty() && grafo.ligacoes.is_empty());
}
