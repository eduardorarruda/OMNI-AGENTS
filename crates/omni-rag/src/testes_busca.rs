//! Busca do MCP: repetidos, relevância, filtros e idade, numa base temporária.
use crate::{busca::{dia_em_ms, mesmo_assunto, resumo_em_volta, termos_da_busca, Filtros}, mcp, memoria::Nota, Base};
use serde_json::{json, Value};
use std::fs;
use tempfile::{tempdir, TempDir};

fn base_vazia() -> (TempDir, Base) {
    let pasta = tempdir().expect("pasta");
    let base = Base::abrir(&pasta.path().join("rag/base.db")).expect("base");
    (pasta, base)
}

fn com_arquivos(arquivos: &[(&str, &str)]) -> (TempDir, Base) {
    let (pasta, mut base) = base_vazia();
    for (nome, texto) in arquivos {
        let caminho = pasta.path().join(nome);
        fs::write(&caminho, texto).expect("arquivo");
        base.adicionar(&caminho).expect("adicionar");
    }
    (pasta, base)
}

fn ferramenta(base: &Base, nome: &str, argumentos: Value) -> String {
    let resposta = mcp::responder(base, &json!({"id": 1, "method": "tools/call", "params": {"name": nome, "arguments": argumentos}})).expect("resposta");
    resposta["result"]["content"][0]["text"].as_str().expect("texto").to_owned()
}

fn nota(base: &Base, slug: &str, titulo: &str, corpo: &str, projeto: &str) {
    base.gravar_nota(&Nota { slug: slug.into(), titulo: titulo.into(), corpo: corpo.into(), projeto: projeto.into(), origem: "auto".into(), ..Nota::default() }).expect("nota");
}

fn datar_nota(base: &Base, slug: &str, dia: &str) {
    base.banco.execute("update notas set atualizado_ms = ?2 where slug = ?1", rusqlite::params![slug, dia_em_ms(dia).expect("dia")]).expect("data");
}

fn sem_filtro() -> Filtros {
    Filtros::default()
}

#[test]
fn trecho_repetido_nao_gasta_o_limite_e_a_resposta_conta_quantos() {
    let igual = "A comissão do vendedor sai no fechamento do mês";
    let (_pasta, base) = com_arquivos(&[("a.txt", igual), ("b.txt", igual), ("c.txt", "  a COMISSÃO do vendedor sai no fechamento do mês!"), ("d.txt", "Regra diferente: comissão do vendedor com bônus")]);
    let achados = base.procurar_trechos("comissão vendedor", "", &sem_filtro(), 2).expect("busca");
    assert_eq!(achados.itens.len(), 2);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "comissão vendedor", "limite": 5}));
    assert_eq!(texto.matches("\n#").count() + 1, 2, "{texto}");
    assert!(texto.contains("2 repetidos unidos"), "{texto}");
}

#[test]
fn nota_repetida_some_e_conta_como_unida() {
    let (_pasta, base) = base_vazia();
    nota(&base, "uma", "Regra de deploy", "publicar pela esteira sempre", "p");
    nota(&base, "duas", "Regra de deploy copiada", "Publicar pela esteira, sempre.", "p");
    let achados = base.procurar_notas("publicar esteira", "p", &sem_filtro(), 5).expect("busca");
    assert_eq!((achados.itens.len(), achados.unidos), (1, 1));
}

#[test]
fn com_todas_as_palavras_o_or_nao_entra() {
    let (_pasta, base) = com_arquivos(&[
        ("a.txt", "planejamento de turmas do semestre"), ("b.txt", "turmas e planejamento anual"), ("c.txt", "planejamento das turmas novas"),
        ("d.txt", "planejamento financeiro sem relação"), ("e.txt", "o evento de confraternização"),
    ]);
    let achados = base.procurar_trechos("planejamento de turmas", "", &sem_filtro(), 5).expect("busca");
    assert_eq!(achados.itens.len(), 3);
    assert!(achados.itens.iter().all(|item| !item.parcial));
}

#[test]
fn com_poucos_resultados_completa_com_parciais_e_avisa() {
    let (_pasta, base) = com_arquivos(&[("a.txt", "planejamento de turmas"), ("b.txt", "planejamento financeiro")]);
    let achados = base.procurar_trechos("planejamento turmas", "", &sem_filtro(), 5).expect("busca");
    assert_eq!(achados.itens.iter().map(|item| item.parcial).collect::<Vec<_>>(), [false, true]);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "planejamento turmas"}));
    assert!(texto.contains("(parcial)") && texto.contains("só parte"), "{texto}");
}

#[test]
fn palavra_vazia_nao_entra_na_busca_a_nao_ser_que_so_tenha_ela() {
    assert_eq!(termos_da_busca("planejamento de turmas"), ["planejamento", "turmas"]);
    assert_eq!(termos_da_busca("de"), ["de"]);
    assert!(termos_da_busca("\"* (").is_empty());
}

#[test]
fn memoria_prefere_todas_as_palavras_e_marca_o_resto() {
    let (_pasta, base) = base_vazia();
    nota(&base, "a", "Comissão", "regra da comissão no histórico do aluno", "p");
    nota(&base, "b", "Outra", "histórico de chamadas", "p");
    let achados = base.procurar_notas("comissão histórico eduq", "p", &sem_filtro(), 5).expect("busca");
    assert!(achados.itens.iter().all(|item| item.parcial));
    let (_pasta, base) = base_vazia();
    for numero in 0..3 { nota(&base, &format!("n{numero}"), "Comissão", &format!("regra {} da comissão no histórico", "x".repeat(numero + 1)), "p"); }
    nota(&base, "b", "Outra", "histórico de chamadas", "p");
    let achados = base.procurar_notas("comissão histórico", "p", &sem_filtro(), 5).expect("busca");
    assert_eq!(achados.itens.len(), 3);
}

#[test]
fn filtro_de_fonte_e_parcial_e_avisa_quando_nao_existe() {
    let (_pasta, base) = com_arquivos(&[("crm-ebpos.txt", "regra de boleto do crm"), ("central.txt", "regra de boleto da central")]);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "fonte": "CRM"}));
    assert!(texto.contains("crm-ebpos") && !texto.contains("central.txt"), "{texto}");
    let vazio = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "fonte": "inexistente"}));
    assert!(vazio.contains("Nenhuma fonte") && vazio.contains("central.txt"), "{vazio}");
}

#[test]
fn projeto_nao_existe_nos_trechos_e_a_resposta_diz_isso() {
    let (_pasta, base) = com_arquivos(&[("a.txt", "regra de boleto")]);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "projeto": "x"}));
    assert!(texto.contains('#') && texto.contains("não têm projeto"), "{texto}");
}

const COM_DATA: &str = "---\ntitle: \"Boleto\"\nupdated: \"2026-03-02T10:00:00.000Z\"\n---\nregra do boleto";

#[test]
fn trecho_mostra_a_data_do_conteudo_e_sem_ela_a_da_importacao_com_esse_nome() {
    let (_pasta, base) = com_arquivos(&[("datado.md", COM_DATA), ("solto.txt", "regra do boleto solta")]);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto"}));
    assert!(texto.contains("2026-03-02"), "{texto}");
    assert!(texto.contains("sem data do conteúdo, importado em"), "{texto}");
}

#[test]
fn trecho_sem_frontmatter_herda_a_data_do_primeiro_trecho_do_arquivo() {
    let longo = format!("{COM_DATA}\n{}\nzebra no fim", "palavra de enchimento\n".repeat(200));
    let (_pasta, base) = com_arquivos(&[("longo.md", &longo)]);
    let achados = base.procurar_trechos("zebra", "", &sem_filtro(), 5).expect("busca");
    assert_eq!(achados.itens[0].data_ms, Some(dia_em_ms("2026-03-02").expect("dia") + 36_000_000));
}

#[test]
fn desde_tira_o_antigo_e_conta_o_que_nao_tem_data() {
    let (_pasta, base) = com_arquivos(&[("datado.md", COM_DATA), ("solto.txt", "regra do boleto solta")]);
    let depois = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "desde": "2026-06-01"}));
    assert!(depois.contains("Nada encontrado") && depois.contains("1 sem data"), "{depois}");
    let antes = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "desde": "2026-01-01"}));
    assert!(antes.contains("datado.md") && !antes.contains("solto.txt") && antes.contains("1 sem data"), "{antes}");
    let erro = mcp::responder(&base, &json!({"id": 1, "method": "tools/call", "params": {"name": "buscar_conhecimento", "arguments": {"consulta": "boleto", "desde": "02/03/2026"}}})).expect("resposta");
    assert_eq!(erro["result"]["isError"], true);
}

#[test]
fn resumo_traz_titulo_e_linha_curta() {
    let (_pasta, base) = com_arquivos(&[("a.txt", &format!("boleto {}", "palavra ".repeat(300)))]);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "resumo": true}));
    let linha = texto.lines().next().expect("linha");
    assert!(linha.contains("a.txt") && linha.chars().count() < 200, "{linha}");
}

#[test]
fn nota_importada_usa_a_data_do_arquivo_e_sem_arquivo_nao_inventa() {
    let (pasta, base) = com_arquivos(&[("guardada.md", COM_DATA)]);
    let cofre = pasta.path().join("cofre");
    fs::create_dir_all(&cofre).expect("cofre");
    fs::write(cofre.join("guardada.md"), COM_DATA).expect("nota");
    fs::write(cofre.join("solta.md"), "---\ntitle: \"Solta\"\n---\nregra do boleto sem fonte").expect("nota");
    base.importar_notas(&cofre).expect("importar");
    let achados = base.procurar_notas("boleto", "", &sem_filtro(), 5).expect("busca");
    let data_de = |titulo: &str| achados.itens.iter().find(|item| item.nota.titulo == titulo).expect("nota").data_ms;
    assert!(data_de("Boleto").is_some());
    assert_eq!(data_de("Solta"), None);
}

#[test]
fn nota_mais_velha_do_mesmo_assunto_fica_marcada() {
    let (_pasta, base) = base_vazia();
    nota(&base, "regra-comissao-vendedor-2026-01", "Regra comissão vendedor", "comissão antiga de cinco por cento", "p");
    nota(&base, "regra-comissao-vendedor-2026-03", "Regra comissão vendedor nova", "comissão nova de sete por cento", "p");
    nota(&base, "ferias-do-time", "Férias do time", "comissão não entra nas férias", "p");
    datar_nota(&base, "regra-comissao-vendedor-2026-01", "2026-01-10");
    datar_nota(&base, "regra-comissao-vendedor-2026-03", "2026-03-10");
    datar_nota(&base, "ferias-do-time", "2026-06-01");
    let achados = base.procurar_notas("comissão", "p", &sem_filtro(), 5).expect("busca");
    let marca = |slug: &str| achados.itens.iter().find(|item| item.nota.slug == slug).expect("nota").mais_nova.is_some();
    assert!(marca("regra-comissao-vendedor-2026-01"));
    assert!(!marca("regra-comissao-vendedor-2026-03"));
    assert!(!marca("ferias-do-time"));
}

#[test]
fn assunto_igual_por_slug_ou_titulo_e_diferente_quando_so_lembra() {
    assert!(mesmo_assunto("deploy-central-2026-01", "x", "deploy-central-2026-02", "y"));
    assert!(mesmo_assunto("a", "Regra de comissão do vendedor externo", "b", "Regra comissão vendedor externo"));
    assert!(!mesmo_assunto("a", "Regra de comissão do vendedor", "b", "Regra de férias do time"));
    assert!(!mesmo_assunto("reuniao-01", "Reunião", "reuniao-02", "Reunião"));
}

#[test]
fn resumo_acha_a_palavra_sem_acento_e_nao_corta_a_mascara() {
    let texto = format!("{} comissão [nome] {}", "antes ".repeat(100), "depois ".repeat(100));
    let resumo = resumo_em_volta(&texto, &["comissao".to_owned()]);
    assert!(resumo.contains("comissão [nome]") && resumo.starts_with('…') && resumo.ends_with('…'), "{resumo}");
}

#[test]
fn busca_mascara_antes_de_cortar_e_sem_mascara_mostra_tudo() {
    let longe = "enchimento ".repeat(24);
    let (_pasta, base) = com_arquivos(&[("a.txt", &format!("{longe}Maria Silva pagou R$ 1.500,00 de boleto"))]);
    let mascarado = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto"}));
    assert!(!mascarado.contains("Silva") && !mascarado.contains("1.500") && mascarado.contains("[valor]"), "{mascarado}");
    let aberto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "boleto", "sem_mascara": true}));
    assert!(aberto.contains("Maria Silva") && aberto.contains("R$ 1.500,00"), "{aberto}");
}

#[test]
fn procurar_pelo_nome_acha_mesmo_com_o_nome_mascarado() {
    let (_pasta, base) = com_arquivos(&[("a.txt", "Maria Silva pediu reembolso")]);
    let texto = ferramenta(&base, "buscar_conhecimento", json!({"consulta": "Maria Silva"}));
    assert!(texto.contains("[nome] pediu reembolso"), "{texto}");
}

#[test]
fn ler_trecho_sai_mascarado() {
    let (_pasta, base) = com_arquivos(&[("a.txt", "boleto de maria@empresa.com.br")]);
    let id = base.procurar_trechos("boleto", "", &sem_filtro(), 1).expect("busca").itens[0].id;
    assert!(ferramenta(&base, "ler_trecho", json!({"id": id})).contains("[email]"));
    assert!(ferramenta(&base, "ler_trecho", json!({"id": id, "sem_mascara": true})).contains("maria@empresa"));
}

#[test]
fn toda_ferramenta_com_dados_pessoais_oferece_sem_mascara_com_o_aviso() {
    let (_pasta, base) = base_vazia();
    let lista = mcp::responder(&base, &json!({"id": 1, "method": "tools/list"})).expect("lista");
    for nome in ["buscar_conhecimento", "ler_trecho", "buscar_memoria", "buscar_no_historico", "ler_conversa", "listar_conversas", "listar_tarefas", "ler_tarefa"] {
        let ferramenta = lista["result"]["tools"].as_array().expect("tools").iter().find(|item| item["name"] == nome).expect("ferramenta");
        let descricao = ferramenta["inputSchema"]["properties"]["sem_mascara"]["description"].as_str().unwrap_or_default();
        assert!(descricao.contains("autorizar explicitamente"), "{nome}");
    }
}
