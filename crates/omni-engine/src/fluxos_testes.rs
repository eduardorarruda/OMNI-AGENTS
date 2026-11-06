//! O executor com comandos de verdade no shell: retomada, ramos em paralelo e volta seletiva.
use super::*;
use std::time::Duration;

struct Ambiente { _dir: tempfile::TempDir, state: Arc<EngineState>, projeto: PathBuf }

fn ambiente() -> Ambiente {
    let dir = tempfile::tempdir().unwrap();
    let state = Arc::new(EngineState { persist_lock: Mutex::new(()), mobile: mobile::MobileRuntime::new(dir.path()),
        token: "t".into(), state_file: dir.path().join("sessions.json"), sessions: Mutex::new(HashMap::new()), id_sequence: AtomicU64::new(1) });
    let projeto = dir.path().join("proj");
    fs::create_dir_all(&projeto).unwrap();
    Ambiente { _dir: dir, state, projeto }
}

impl Ambiente {
    fn iniciar(&self, fluxo: Value) -> String {
        let fluxo: Fluxo = serde_json::from_value(fluxo).unwrap();
        iniciar(&self.state, fluxo, self.projeto.to_string_lossy().into(), "p".into(), "x".into()).unwrap().id
    }
    fn linhas(&self, arquivo: &str) -> usize {
        fs::read_to_string(self.projeto.join(arquivo)).map_or(0, |t| t.lines().count())
    }
}

/// Espera sair de `rodando`; ficar preso nele é falha do teste, não timeout silencioso.
fn ate_parar(id: &str) -> Execucao {
    for _ in 0..600 {
        let e = obter(id).unwrap();
        if e.status != "rodando" { return e; }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("ficou rodando: {:?}", obter(id).map(|e| (e.nos.iter().map(|(k, n)| (k.clone(), n.status.clone())).collect::<Vec<_>>(), e.fila)));
}

/// Conta cada passada num arquivo e só passa depois que `libera` existir.
fn conta_e_exige(marca: &str, libera: &str) -> String { format!("echo x >> {marca}.log; test -f {libera}") }
fn conta(marca: &str) -> String { format!("echo x >> {marca}.log") }

fn comando(id: &str, texto: &str) -> Value { json!({"id": id, "tipo": "comando", "titulo": id, "config": {"comando": texto}}) }

/// entrada → (a, b, …) → juntar → depois → fim
fn paralelo(ramos: &[(&str, String)]) -> Value {
    let mut nos = vec![json!({"id": "e", "tipo": "entrada"})];
    let mut conexoes = Vec::new();
    for (id, texto) in ramos {
        nos.push(comando(id, texto));
        conexoes.push(json!({"de": "e", "para": id}));
        conexoes.push(json!({"de": id, "para": "j", "saida": "sim"}));
    }
    nos.extend([json!({"id": "j", "tipo": "juntar"}), comando("depois", &conta("depois")), json!({"id": "f", "tipo": "fim"})]);
    conexoes.extend([json!({"de": "j", "para": "depois"}), json!({"de": "depois", "para": "f", "saida": "sim"})]);
    json!({"id": "p", "nome": "P", "na_pasta_do_projeto": true, "nos": nos, "conexoes": conexoes})
}

#[test]
fn retomar_refaz_so_a_etapa_que_falhou_no_lote_paralelo() {
    let amb = ambiente();
    let id = amb.iniciar(paralelo(&[("a", conta("a")), ("b", conta_e_exige("b", "libera"))]));
    assert_eq!(ate_parar(&id).status, "falhou");
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("depois.log")), (1, 1, 0));
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("depois.log")), (1, 2, 1), "o ramo que terminou não roda de novo");
    assert!(e.nos["j"].saida.contains("## a") && e.nos["j"].saida.contains("## b"), "{}", e.nos["j"].saida);
}

#[test]
fn retomar_refaz_as_duas_etapas_quando_o_lote_falha_inteiro() {
    let amb = ambiente();
    let id = amb.iniciar(paralelo(&[("a", conta_e_exige("a", "libera")), ("b", conta_e_exige("b", "libera"))]));
    assert_eq!(ate_parar(&id).status, "falhou");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("depois.log")), (2, 2, 1));
}

/// O retry tem que saber de onde veio: sem isso o `{{anterior}}` da etapa chega vazio.
#[test]
fn etapa_que_falhou_volta_para_a_fila_sabendo_de_onde_veio() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "s", "nome": "S", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, comando("c", "exit 1"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    let e = ate_parar(&id);
    assert_eq!(e.status, "falhou");
    assert_eq!(e.fila.iter().map(|p| (p.no.as_str(), p.de.as_deref())).collect::<Vec<_>>(), vec![("c", Some("e"))]);
}

/// O engine caiu com a etapa rodando: ela volta como pausada e retomar precisa rodá-la de novo.
#[test]
fn retomar_depois_de_reabrir_o_engine_refaz_a_etapa_interrompida() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "r", "nome": "R", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "a", "tipo": "aprovacao"}, comando("c", &conta("c")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "a"}, {"de": "a", "para": "c", "saida": "sim"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    assert_eq!(ate_parar(&id).status, "aguardando");
    // Como `carregar` deixa uma execução que estava no meio de `c` quando o processo morreu.
    let v = viva(&id).unwrap();
    mexer(&amb.state, &v, |e| {
        e.status = "pausado".into();
        e.nos.get_mut("a").unwrap().status = "concluido".into();
        e.nos.get_mut("c").unwrap().status = "pausado".into();
    });
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(amb.linhas("c.log"), 1);
}

/// Grava o instante de início e fim de cada ramo: o teto vale se nunca há mais de `teto` abertos.
#[test]
fn max_paralelo_limita_quantos_ramos_rodam_juntos() {
    let amb = ambiente();
    let ramo = |id: &str| format!("echo + >> ordem.log; sleep 0.3; echo - >> ordem.log; echo {id}");
    let ramos: Vec<(String, String)> = ["a", "b", "c", "d", "e2"].iter().map(|id| ((*id).to_owned(), ramo(id))).collect();
    let pares: Vec<(&str, String)> = ramos.iter().map(|(id, texto)| (id.as_str(), texto.clone())).collect();
    let abertos_no_pico = |id: &str, amb: &Ambiente| {
        let e = ate_parar(id);
        assert_eq!(e.status, "concluido", "{:?}", e.erro);
        let ordem = fs::read_to_string(amb.projeto.join("ordem.log")).unwrap();
        let (pico, _) = ordem.lines().fold((0, 0), |(pico, agora), l| { let agora = if l == "+" { agora + 1 } else { agora - 1 }; (pico.max(agora), agora) });
        assert_eq!(e.nos["depois"].status, "concluido");
        assert!(["a", "b", "c", "d", "e2"].iter().all(|r| e.nos["j"].saida.contains(&format!("## {r}"))), "o Juntar espera até o último lote");
        pico
    };
    let mut fluxo = paralelo(&pares);
    fluxo["max_paralelo"] = json!(2);
    let id = amb.iniciar(fluxo);
    assert_eq!(abertos_no_pico(&id, &amb), 2);

    let amb = ambiente();
    let id = amb.iniciar(paralelo(&pares));
    assert_eq!(abertos_no_pico(&id, &amb), 5, "sem o campo, não há teto");
}

/// O `{{anterior}}` volta para a etapa pendente que ficou fora do primeiro lote.
#[test]
fn etapa_que_esperou_vaga_roda_depois_com_a_origem_certa() {
    let mut e: Execucao = serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "rodando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": paralelo(&[("a", conta("a")), ("b", conta("b"))]), "nos": {"a": {"status": "pendente"}, "b": {"status": "pendente"}},
        "fila": [{"no": "a", "de": "e"}, {"no": "b", "de": "e"}]})).unwrap();
    e.fluxo.max_paralelo = Some(1);
    assert_eq!(proximo_lote(&mut e).unwrap().iter().map(|p| p.no.as_str()).collect::<Vec<_>>(), ["a"]);
    assert!(proximo_lote(&mut e).is_none(), "sem vaga enquanto `a` roda, e sem dar por concluída");
    assert_eq!((e.status.as_str(), e.fila[0].de.as_deref()), ("rodando", Some("e")));
    e.nos.get_mut("a").unwrap().status = "concluido".into();
    assert_eq!(proximo_lote(&mut e).unwrap()[0].no, "b");
}

#[test]
fn aviso_sobe_ao_pedir_aprovacao_falhar_e_terminar() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "n", "nome": "N", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ok", "tipo": "aprovacao"}, comando("c", "test -f libera"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ok"}, {"de": "ok", "para": "c", "saida": "sim"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    let e = ate_parar(&id);
    assert_eq!((e.aviso.as_deref(), e.aviso_seq), (Some("aprovacao"), 1));
    aprovar(&amb.state, &id, "ok", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!((e.status.as_str(), e.aviso.as_deref(), e.aviso_seq), ("falhou", Some("falhou"), 2));
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!((e.status.as_str(), e.aviso.as_deref(), e.aviso_seq), ("concluido", Some("concluido"), 3));
    assert!(listar().iter().any(|item| item["id"] == *id && item["aviso_seq"] == 3 && item["aviso"] == "concluido"));
}

/// Ramo falhou enquanto o outro esperava aprovação: retomar não some com a pergunta.
#[test]
fn retomar_nao_passa_por_cima_de_aprovacao_pendente() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "a", "nome": "A", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ok", "tipo": "aprovacao"}, comando("c", "sleep 0.2; test -f libera"), {"id": "f", "tipo": "fim"}, comando("perigo", &conta("perigo"))],
        "conexoes": [{"de": "e", "para": "ok"}, {"de": "e", "para": "c"}, {"de": "ok", "para": "perigo", "saida": "sim"}, {"de": "perigo", "para": "f", "saida": "sim"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    for _ in 0..200 { if obter(&id).unwrap().status == "falhou" { break; } std::thread::sleep(Duration::from_millis(20)); }
    assert_eq!(obter(&id).unwrap().status, "falhou");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    assert_eq!(ate_parar(&id).status, "aguardando");
    assert_eq!(amb.linhas("perigo.log"), 0, "sem aprovação a etapa não anda");
    aprovar(&amb.state, &id, "ok", true).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");
    assert_eq!(amb.linhas("perigo.log"), 1);
}

/// Dois ramos pedem aprovação ao mesmo tempo: responder a primeira não pode prender a segunda.
#[test]
fn duas_aprovacoes_seguidas_soltam_as_duas() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "a", "nome": "A", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ap1", "tipo": "aprovacao"}, {"id": "ap2", "tipo": "aprovacao"},
        comando("c1", &conta("c1")), comando("c2", &conta("c2")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap1"}, {"de": "e", "para": "ap2"}, {"de": "ap1", "para": "c1", "saida": "sim"}, {"de": "ap2", "para": "c2", "saida": "sim"},
            {"de": "c1", "para": "f", "saida": "sim"}, {"de": "c2", "para": "f", "saida": "sim"}]}));
    for _ in 0..300 {
        let e = obter(&id).unwrap();
        if e.nos.get("ap1").is_some_and(|n| n.status == "aguardando") && e.nos.get("ap2").is_some_and(|n| n.status == "aguardando") { break; }
        std::thread::sleep(Duration::from_millis(20));
    }
    aprovar(&amb.state, &id, "ap1", true).unwrap();
    aprovar(&amb.state, &id, "ap2", true).expect("a segunda aprovação não pode ficar presa");
    assert_eq!(ate_parar(&id).status, "concluido");
    assert_eq!((amb.linhas("c1.log"), amb.linhas("c2.log")), (1, 1));
}

fn duas_aprovacoes() -> Value {
    json!({"id": "a", "nome": "A", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ap1", "tipo": "aprovacao"}, {"id": "ap2", "tipo": "aprovacao"},
        comando("c1", &conta("c1")), comando("c2", &conta("c2")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap1"}, {"de": "e", "para": "ap2"}, {"de": "ap1", "para": "c1", "saida": "sim"}, {"de": "ap2", "para": "c2", "saida": "sim"},
            {"de": "c1", "para": "f", "saida": "sim"}, {"de": "c2", "para": "f", "saida": "sim"}]})
}

fn esperar_as_duas(id: &str) {
    for _ in 0..300 {
        let e = obter(id).unwrap();
        if e.nos.get("ap1").is_some_and(|n| n.status == "aguardando") && e.nos.get("ap2").is_some_and(|n| n.status == "aguardando") { return; }
        std::thread::sleep(Duration::from_millis(20));
    }
    panic!("as duas aprovações não chegaram a aguardar");
}

/// Execução que já falhou, foi pausada ou cancelada não volta a andar por uma aprovação sobrando.
#[test]
fn aprovar_nao_ressuscita_execucao_que_nao_esta_viva() {
    for parada in ["falhou", "pausado", "cancelado"] {
        let amb = ambiente();
        let id = amb.iniciar(duas_aprovacoes());
        esperar_as_duas(&id);
        let v = viva(&id).unwrap();
        mexer(&amb.state, &v, |e| { e.status = parada.into(); e.erro = Some("falha do outro ramo".into()); });
        assert!(aprovar(&amb.state, &id, "ap1", true).is_err(), "{parada}: devia recusar");
        let e = obter(&id).unwrap();
        assert_eq!((e.status.as_str(), e.nos["ap1"].status.as_str()), (parada, "aguardando"), "{parada}: nada muda");
        assert_eq!(amb.linhas("c1.log"), 0);
    }
}

/// Recusar a primeira (caminho `nao`) e aprovar a segunda soltam as duas.
#[test]
fn recusar_a_primeira_e_aprovar_a_segunda_soltam_as_duas() {
    let amb = ambiente();
    let id = amb.iniciar(duas_aprovacoes());
    esperar_as_duas(&id);
    aprovar(&amb.state, &id, "ap1", false).unwrap();
    aprovar(&amb.state, &id, "ap2", true).expect("a segunda não pode ficar presa");
    assert_eq!(ate_parar(&id).status, "concluido");
    assert_eq!((amb.linhas("c1.log"), amb.linhas("c2.log")), (0, 1));
}

/// entrada → (a, b, c) → juntar → revisor → condição, com `nao` ligado nos três ramos. O revisor é um
/// comando que imprime o veredito guardado em `veredito.txt`.
fn com_revisor(ramos: Value, nos_extra: Vec<Value>, conexoes_extra: Vec<Value>) -> Value {
    let ramos: Vec<String> = ramos.as_array().unwrap().iter().map(|r| r.as_str().unwrap().to_owned()).collect();
    let mut nos = vec![json!({"id": "e", "tipo": "entrada"}), json!({"id": "j", "tipo": "juntar"}),
        comando("rev", "echo x >> rev.log; cat veredito-$(wc -l < rev.log | tr -d ' ').txt"), json!({"id": "cond", "tipo": "condicao", "config": {"max_voltas": 3}}),
        json!({"id": "f", "tipo": "fim"}), json!({"id": "desistiu", "tipo": "fim"})];
    nos.extend(nos_extra);
    let mut conexoes = vec![json!({"de": "j", "para": "rev"}), json!({"de": "rev", "para": "cond", "saida": "sim"}),
        json!({"de": "cond", "para": "f", "saida": "sim"}), json!({"de": "cond", "para": "desistiu", "saida": "esgotado"})];
    for id in &ramos { conexoes.extend([json!({"de": "e", "para": id}), json!({"de": "cond", "para": id, "saida": "nao"})]); }
    conexoes.extend(conexoes_extra);
    json!({"id": "v", "nome": "V", "na_pasta_do_projeto": true, "nos": nos, "conexoes": conexoes})
}

fn ramos_simples(ids: &[&str]) -> (Vec<Value>, Vec<Value>) {
    (ids.iter().map(|id| comando(id, &format!("echo x >> {id}.log; echo saida-{id}-$(wc -l < {id}.log | tr -d ' ')"))).collect(),
     ids.iter().map(|id| json!({"de": id, "para": "j", "saida": "sim"})).collect())
}

fn escrever_vereditos(amb: &Ambiente, vereditos: &[&str]) {
    // Em bloco, como os agentes entregam: o comando ainda ganha uma linha de status depois da saída.
    for (i, v) in vereditos.iter().enumerate() { fs::write(amb.projeto.join(format!("veredito-{}.txt", i + 1)), format!("```json\n{v}\n```")).unwrap(); }
}

#[test]
fn reprovado_com_refazer_roda_so_os_ramos_pedidos_e_o_juntar_reusa_os_outros() {
    let amb = ambiente();
    escrever_vereditos(&amb, &[r#"{"veredito":"reprovado","refazer":["a","c","inventado"]}"#, r#"{"veredito":"aprovado"}"#]);
    let (nos, conexoes) = ramos_simples(&["a", "b", "c"]);
    let e = ate_parar(&amb.iniciar(com_revisor(json!(["a", "b", "c"]), nos, conexoes)));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("c.log"), amb.linhas("rev.log")), (2, 1, 2, 2));
    let junto = &e.nos["j"].saida;
    assert!(junto.contains("saida-a-2") && junto.contains("saida-b-1") && junto.contains("saida-c-2"), "{junto}");
}

/// Duas reprovações seguidas: a segunda passada do revisor não pode reabrir o ramo que ficou de fora.
#[test]
fn segunda_reprovacao_seletiva_reusa_o_ramo_que_nao_foi_pedido() {
    let amb = ambiente();
    escrever_vereditos(&amb, &[r#"{"veredito":"reprovado","refazer":["a"]}"#, r#"{"veredito":"reprovado","refazer":["c"]}"#, r#"{"veredito":"aprovado"}"#]);
    let (nos, conexoes) = ramos_simples(&["a", "b", "c"]);
    let e = ate_parar(&amb.iniciar(com_revisor(json!(["a", "b", "c"]), nos, conexoes)));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("c.log"), amb.linhas("rev.log")), (2, 1, 2, 3));
    assert_eq!(e.nos["cond"].caminho.as_deref(), Some("sim"));
}

#[test]
fn reprovado_sem_refazer_ou_com_ids_invalidos_roda_todos_os_ramos() {
    for veredito in [r#"{"veredito":"reprovado"}"#, r#"{"veredito":"reprovado","refazer":["j","nada"]}"#] {
        let amb = ambiente();
        escrever_vereditos(&amb, &[veredito, r#"{"veredito":"aprovado"}"#]);
        let (nos, conexoes) = ramos_simples(&["a", "b"]);
        let e = ate_parar(&amb.iniciar(com_revisor(json!(["a", "b"]), nos, conexoes)));
        assert_eq!(e.status, "concluido", "{:?}", e.erro);
        assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("rev.log")), (2, 2, 2), "{veredito}");
    }
}

/// O id que o revisor manda só escolhe entre as conexões do `nao`: `j` existe mas não está ligado.
#[test]
fn refazer_nao_abre_caminho_que_nao_existe() {
    let (nos, conexoes) = ramos_simples(&["a", "b"]);
    let e: Execucao = serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "rodando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": com_revisor(json!(["a", "b"]), nos, conexoes), "nos": {"cond": {"status": "concluido", "saida": r#"{"veredito":"reprovado","refazer":["j"]}"#}}, "fila": []})).unwrap();
    assert_eq!(proximos_da_saida(&e, "cond", "nao"), ["a", "b"]);
    assert_eq!(proximos_da_saida(&e, "cond", "sim"), ["f"]);
}

/// Ramo de duas etapas e outro de uma: o fim do ramo longo que vai refazer não pode contar como
/// pronto, senão o Juntar sai duas vezes e o revisor julga com a saída velha.
#[test]
fn volta_por_ramo_longo_espera_o_fim_dele_no_juntar() {
    let amb = ambiente();
    escrever_vereditos(&amb, &[r#"{"veredito":"reprovado"}"#, r#"{"veredito":"aprovado"}"#]);
    let nos = vec![comando("a1", "echo x >> a1.log; sleep 0.2"), comando("a2", "echo x >> a2.log; echo fim-a-$(wc -l < a2.log | tr -d ' ')"), comando("b", "echo x >> b.log")];
    let conexoes = vec![json!({"de": "a1", "para": "a2", "saida": "sim"}), json!({"de": "a2", "para": "j", "saida": "sim"}), json!({"de": "b", "para": "j", "saida": "sim"})];
    let e = ate_parar(&amb.iniciar(com_revisor(json!(["a1", "b"]), nos, conexoes)));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a2.log"), amb.linhas("b.log"), amb.linhas("rev.log")), (2, 2, 2), "o revisor roda uma vez por volta");
    assert!(e.nos["j"].saida.contains("fim-a-2"), "{}", e.nos["j"].saida);
}

#[test]
fn custo_alto_nao_pausa() {
    let mut e: Execucao = serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "rodando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": {"id": "a", "nome": "A", "custo_maximo": 1.0, "nos": [comando("a", "true")], "conexoes": []},
        "nos": {"a": {"status": "pendente"}}, "fila": [{"no": "a", "de": "e"}], "custo_usd": 500.0})).unwrap();
    assert!(proximo_lote(&mut e).is_some());
    assert_eq!(e.status, "rodando");
}

/// Execução que parou pelo custo antes da mudança: o disco ainda tem a marca, e ela não pode prender nada.
#[test]
fn execucao_antiga_pausada_por_custo_retoma() {
    let amb = ambiente();
    let id = format!("antiga-{}", novo_id());
    let e: Execucao = serde_json::from_value(json!({"id": id, "projeto": amb.projeto, "projeto_id": "p", "entrada": "x", "pasta": amb.projeto, "status": "pausado", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": {"id": "a", "nome": "A", "custo_maximo": 0.5, "nos": [{"id": "e", "tipo": "entrada"}, comando("c", &conta("c")), {"id": "f", "tipo": "fim"}],
            "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "f", "saida": "sim"}]},
        "nos": {"e": {"status": "concluido"}, "c": {"status": "pendente"}, "f": {"status": "pendente"}}, "fila": [{"no": "c", "de": "e"}],
        "custo_usd": 9.0, "custo_aceito": 0.0, "parou_por_custo": true, "erro": "Pausado pelo limite de custo"})).unwrap();
    assert!(!serde_json::to_string(&e).unwrap().contains("parou_por_custo"), "a marca morre na leitura");
    execucoes().lock().unwrap().insert(id.clone(), Arc::new(Mutex::new(e)));
    retomar(&amb.state, &id).unwrap();
    let fim = ate_parar(&id);
    assert_eq!((fim.status.as_str(), fim.erro), ("concluido", None));
    assert_eq!(amb.linhas("c.log"), 1);
}

/// Cada ramo só sai quando os 20 chegaram: se algum ficasse esperando vaga, ninguém terminaria.
#[test]
fn vinte_ramos_rodam_juntos() {
    let amb = ambiente();
    let barreira = "echo x >> chegou.log; for i in $(seq 1 200); do [ $(wc -l < chegou.log) -ge 20 ] && exit 0; sleep 0.05; done; exit 1";
    let ramos: Vec<String> = (0..20).map(|i| format!("r{i}")).collect();
    let pares: Vec<(&str, String)> = ramos.iter().map(|id| (id.as_str(), barreira.to_owned())).collect();
    let e = ate_parar(&amb.iniciar(paralelo(&pares)));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
}

#[test]
fn max_paralelo_definido_ainda_vale() {
    let mut e: Execucao = serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "rodando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": paralelo(&[("a", conta("a")), ("b", conta("b")), ("c", conta("c"))]), "nos": {}, "fila": [{"no": "a", "de": "e"}, {"no": "b", "de": "e"}, {"no": "c", "de": "e"}]})).unwrap();
    e.fluxo.max_paralelo = Some(2);
    assert_eq!(proximo_lote(&mut e).unwrap().len(), 2);
}

/// entrada → rev → cond, com `nao` voltando para `rev`. Aprova na `passada`.
fn laco_de_condicao(amb: &Ambiente, config: Value, passada: usize) -> Value {
    let reprovado = r#"{"veredito":"reprovado"}"#;
    let vereditos: Vec<&str> = (1..=passada).map(|i| if i == passada { r#"{"veredito":"aprovado"}"# } else { reprovado }).collect();
    escrever_vereditos(amb, &vereditos);
    json!({"id": "l", "nome": "L", "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"},
        comando("rev", "echo x >> rev.log; cat veredito-$(wc -l < rev.log | tr -d ' ').txt"), {"id": "cond", "tipo": "condicao", "config": config},
        {"id": "f", "tipo": "fim"}, {"id": "desistiu", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "rev"}, {"de": "rev", "para": "cond", "saida": "sim"}, {"de": "cond", "para": "f", "saida": "sim"},
            {"de": "cond", "para": "rev", "saida": "nao"}, {"de": "cond", "para": "desistiu", "saida": "esgotado"}]})
}

#[test]
fn condicao_sem_max_voltas_nao_esgota_em_3() {
    let amb = ambiente();
    let e = ate_parar(&amb.iniciar(laco_de_condicao(&amb, json!({}), 6)));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("rev.log"), e.nos["cond"].caminho.as_deref(), e.nos["desistiu"].status.as_str()), (6, Some("sim"), "pendente"));
}

#[test]
fn max_voltas_definido_ainda_esgota() {
    let amb = ambiente();
    let e = ate_parar(&amb.iniciar(laco_de_condicao(&amb, json!({"max_voltas": 2}), 6)));
    assert_eq!((amb.linhas("rev.log"), e.nos["cond"].caminho.as_deref(), e.nos["desistiu"].status.as_str()), (2, Some("esgotado"), "concluido"));
}

/// `max_voltas` conta passadas: 3 são 3 reprovações, ou seja, 2 voltas e `esgotado` na terceira.
#[test]
fn max_voltas_3_faz_3_passadas_e_2_voltas() {
    let amb = ambiente();
    escrever_vereditos(&amb, &[r#"{"veredito":"reprovado"}"#; 5]);
    let (nos, conexoes) = ramos_simples(&["a", "b"]);
    let e = ate_parar(&amb.iniciar(com_revisor(json!(["a", "b"]), nos, conexoes)));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a.log"), amb.linhas("rev.log"), e.nos["cond"].voltas, e.nos["cond"].caminho.as_deref(), e.nos["desistiu"].status.as_str()), (3, 3, 3, Some("esgotado"), "concluido"));
}

fn modelo(cli: &str, ferramentas: &[&str]) -> omni_core::agentes::ModeloDeAgente {
    serde_json::from_value(json!({"name": "x", "description": "d", "cli": cli, "instrucoes": "Faça.", "ferramentas": ferramentas, "modelo": "haiku", "esforco": "", "conta": "", "saida": "texto"})).unwrap()
}

fn tem_par(args: &[String], flag: &str, valor: &str) -> bool { args.windows(2).any(|par| par[0] == flag && par[1] == valor) }

#[test]
fn agente_sem_limite_continua_com_os_mesmos_argumentos() {
    let args = args_do_claude(&modelo("claude", &["Read", "Bash"]), &Ajuste::default(), &[], Some("sid"));
    assert_eq!(args, ["-p", "--output-format", "stream-json", "--verbose", "--permission-mode", "acceptEdits", "--append-system-prompt", "Faça.",
        "--allowedTools", "Read,Bash", "--model", "haiku", "--resume", "sid"]);
}

#[test]
fn proibidas_viram_disallowed_tools_e_pastas_viram_add_dir() {
    let ajuste = Ajuste { proibidas: vec!["Bash".into(), "Bash(rm:*)".into()], ..Default::default() };
    let args = args_do_claude(&modelo("claude", &["Bash"]), &ajuste, &["/x/irmao".into()], None);
    assert!(tem_par(&args, "--disallowedTools", "Bash") && tem_par(&args, "--disallowedTools", "Bash(rm:*)"), "{args:?}");
    assert!(tem_par(&args, "--add-dir", "/x/irmao"), "{args:?}");
    assert!(tem_par(&args, "--permission-mode", "acceptEdits"), "proibir ferramenta não muda o modo");
}

#[test]
fn escrita_troca_o_modo_tira_o_shell_e_os_settings() {
    let ajuste = Ajuste { escrita: Some(vec!["src/".into()]), ..Default::default() };
    let args = args_do_claude(&modelo("claude", &["Read", "Write", "Bash"]), &ajuste, &[], None);
    assert!(tem_par(&args, "--permission-mode", "default"), "{args:?}");
    assert!(tem_par(&args, "--allowedTools", "Read,Edit(./src/**)"), "{args:?}");
    assert!(tem_par(&args, "--disallowedTools", "Bash"), "{args:?}");
    assert!(tem_par(&args, "--setting-sources", ""), "{args:?}");
}

/// Sem os settings a CLI não carrega os MCP do usuário: a etapa recebe os que o agente declarou.
#[test]
fn etapa_com_escrita_recebe_por_arquivo_so_o_mcp_declarado() {
    let dir = tempfile::tempdir().unwrap();
    let estado = dir.path().join(".claude.json");
    fs::write(&estado, json!({"mcpServers": {"omni-rag": {"command": "rag", "env": {"X": "segredo"}}, "outro": {"command": "o"}}}).to_string()).unwrap();
    let ferramentas = vec!["Read".to_owned(), "mcp__omni-rag__ler_trecho".to_owned()];
    let args = args_de_mcp(&dir.path().join("log"), "a", &ferramentas, &estado).unwrap();
    assert_eq!(args[0], "--mcp-config");
    assert!(!args.join(" ").contains("segredo"), "o env não vai para a linha de comando");
    let gravado: Value = serde_json::from_slice(&fs::read(&args[1]).unwrap()).unwrap();
    assert_eq!(gravado, json!({"mcpServers": {"omni-rag": {"command": "rag", "env": {"X": "segredo"}}}}));
    assert!(args_de_mcp(&dir.path().join("log"), "b", &["Read".to_owned()], &estado).unwrap().is_empty());
    assert!(args_de_mcp(&dir.path().join("log"), "c", &ferramentas, &dir.path().join("nao-existe")).unwrap().is_empty());
}

#[test]
fn codex_faz_o_que_da_ou_falha_com_motivo() {
    let m = modelo("codex", &[]);
    let args = args_do_codex(&m, &Ajuste::default(), &["/x/irmao".into()], None).unwrap();
    assert!(tem_par(&args, "--sandbox", "workspace-write") && tem_par(&args, "-c", r#"sandbox_workspace_write.writable_roots=["/x/irmao"]"#), "{args:?}");
    let so_leitura = args_do_codex(&m, &Ajuste { escrita: Some(Vec::new()), ..Default::default() }, &[], None).unwrap();
    assert!(tem_par(&so_leitura, "--sandbox", "read-only"), "{so_leitura:?}");
    assert!(args_do_codex(&m, &Ajuste { proibidas: vec!["Bash".into()], ..Default::default() }, &[], None).unwrap_err().to_string().contains("proibidas"));
    assert!(args_do_codex(&m, &Ajuste { escrita: Some(vec!["src/".into()]), ..Default::default() }, &[], None).unwrap_err().to_string().contains("escrita"));
}

#[test]
fn pasta_extra_invalida_nao_chega_a_iniciar() {
    let amb = ambiente();
    let mut fluxo: Fluxo = serde_json::from_value(paralelo(&[("a", conta("a")), ("b", conta("b"))])).unwrap();
    fluxo.pastas_extras = vec!["nao-existe".into()];
    let erro = iniciar(&amb.state, fluxo.clone(), amb.projeto.to_string_lossy().into(), "p".into(), "x".into()).unwrap_err().to_string();
    assert!(erro.contains("Pasta extra"), "{erro}");
    fs::create_dir_all(amb.projeto.parent().unwrap().join("irmao")).unwrap();
    fluxo.pastas_extras = vec!["../irmao".into()];
    let e = iniciar(&amb.state, fluxo, amb.projeto.to_string_lossy().into(), "p".into(), "x".into()).unwrap();
    assert!(e.pastas_extras[0].ends_with("irmao") && Path::new(&e.pastas_extras[0]).is_absolute());
    ate_parar(&e.id);
}

/// Prova com a CLI de verdade, usando os argumentos que o executor monta: gasta alguns centavos, por
/// isso fica fora da suíte. `cargo test -p omni-engine escrita_de_verdade -- --ignored --nocapture`.
#[test]
#[ignore = "abre o claude de verdade"]
fn escrita_de_verdade_fora_da_lista_e_negada() {
    let dir = tempfile::tempdir().unwrap();
    for pasta in ["liberada", "fora", ".claude"] { fs::create_dir_all(dir.path().join(pasta)).unwrap(); }
    // Um `allow` amplo no projeto é o que alargaria o limite se os settings entrassem.
    fs::write(dir.path().join(".claude/settings.json"), r#"{"permissions":{"allow":["Write","Edit","Bash"]}}"#).unwrap();
    let ajuste = Ajuste { escrita: Some(vec!["liberada/".into(), "NOTA.md".into()]), ..Default::default() };
    let args = args_do_claude(&modelo("claude", &["Read", "Write", "Bash"]), &ajuste, &[], None);
    let mut filho = Command::new("claude").args(&args).current_dir(dir.path()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().unwrap();
    filho.stdin.take().unwrap().write_all(b"Sem perguntar nada, use a ferramenta Write para criar liberada/ok.txt, depois NOTA.md, depois fora/nao.txt. Se a ultima for negada, tente criar fora/nao.txt pelo Bash. Depois diga em uma linha o que funcionou.").unwrap();
    let saida = filho.wait_with_output().unwrap();
    let texto = String::from_utf8_lossy(&saida.stdout);
    let fim = texto.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok().filter(|v| v["type"] == "result")).expect("sem resultado");
    println!("custo {} · negadas {} · {}", fim["total_cost_usd"], fim["permission_denials"], fim["result"]);
    assert!(dir.path().join("liberada/ok.txt").exists() && dir.path().join("NOTA.md").exists(), "o que está na lista grava");
    assert!(!dir.path().join("fora/nao.txt").exists(), "fora da lista não grava");
}

// ---- Linha do tempo da execução (eventos.jsonl) ----

fn fluxo_de_um_comando() -> Value {
    json!({"id": "l", "nome": "L", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, comando("c", &conta("c")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "f", "saida": "sim"}]})
}

fn so_eventos(amb: &Ambiente, id: &str, desde: u64) -> Vec<eventos::Evento> { eventos::ler_desde(&amb.state, id, desde).unwrap() }

#[test]
fn eventos_saem_em_ordem_e_com_cursor() {
    let amb = ambiente();
    let id = amb.iniciar(fluxo_de_um_comando());
    ate_parar(&id);
    let todos = so_eventos(&amb, &id, 0);
    assert!(todos.windows(2).all(|p| p[1].seq == p[0].seq + 1), "{todos:?}");
    assert_eq!(todos[0].seq, 1);
    let pares: Vec<(Option<&str>, &str)> = todos.iter().filter(|e| matches!(e.tipo.as_str(), "inicio" | "fim")).map(|e| (e.no.as_deref(), e.tipo.as_str())).collect();
    assert_eq!(pares, vec![(Some("e"), "inicio"), (Some("e"), "fim"), (Some("c"), "inicio"), (Some("c"), "fim"), (Some("f"), "inicio"), (Some("f"), "fim")]);
    assert_eq!(obter(&id).unwrap().evento_seq, todos.last().unwrap().seq);
}

#[test]
fn ler_desde_devolve_so_o_novo() {
    let amb = ambiente();
    let id = amb.iniciar(fluxo_de_um_comando());
    ate_parar(&id);
    let todos = so_eventos(&amb, &id, 0);
    let corte = todos[2].seq;
    let resto = so_eventos(&amb, &id, corte);
    assert_eq!(resto, todos[3..].to_vec());
    assert!(so_eventos(&amb, &id, todos.last().unwrap().seq).is_empty());
}

#[test]
fn ler_desde_para_em_500() {
    let amb = ambiente();
    let id = amb.iniciar(fluxo_de_um_comando());
    let mut e = ate_parar(&id);
    e.id = "grande".into();
    e.evento_seq = 0;
    for i in 0..600 { eventos::registrar(&amb.state, &mut e, Some("c"), "aviso", &format!("n{i}")); }
    let primeira = so_eventos(&amb, "grande", 0);
    assert_eq!((primeira.len(), primeira[0].seq, primeira[499].seq), (500, 1, 500));
    assert_eq!(so_eventos(&amb, "grande", 500).len(), 100);
}

#[test]
fn id_invalido_recusa() {
    let amb = ambiente();
    assert!(eventos::ler_desde(&amb.state, "../fora", 0).is_err());
    assert!(eventos::ler_desde(&amb.state, "", 0).is_err());
    assert!(historico(&amb.state, "../fora", "c").is_err());
}

#[test]
fn execucao_antiga_sem_eventos_devolve_vazio() {
    let amb = ambiente();
    assert!(so_eventos(&amb, "exec-antiga", 0).is_empty());
    // E um arquivo de execução gravado antes do campo existir continua abrindo.
    let antigo: Execucao = serde_json::from_value({
        let mut j = serde_json::to_value(obter(&amb.iniciar(fluxo_de_um_comando())).unwrap()).unwrap();
        j.as_object_mut().unwrap().remove("evento_seq");
        j
    }).unwrap();
    assert_eq!(antigo.evento_seq, 0);
}

#[test]
fn atividade_repetida_nao_vira_evento() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "a", "nome": "A", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ap", "tipo": "aprovacao"}, {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap"}, {"de": "ap", "para": "f", "saida": "sim"}]}));
    assert_eq!(ate_parar(&id).status, "aguardando");
    let v = viva(&id).unwrap();
    let antes = so_eventos(&amb, &id, 0).len();
    for texto in ["Lendo main.rs", "Lendo main.rs", "Lendo main.rs", "Editando main.rs", "Editando main.rs"] {
        definir_atividade(&amb.state, &v, "ap", texto);
    }
    let novos: Vec<_> = so_eventos(&amb, &id, 0).into_iter().skip(antes).collect();
    assert_eq!(novos.iter().map(|e| (e.tipo.as_str(), e.texto.as_str())).collect::<Vec<_>>(), vec![("atividade", "Lendo main.rs"), ("atividade", "Editando main.rs")]);
    assert_eq!(obter(&id).unwrap().nos["ap"].atividade, "Editando main.rs");
}

#[test]
fn volta_nova_separa_o_log() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "v", "nome": "V", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, comando("c", &conta_e_exige("c", "libera")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    assert_eq!(ate_parar(&id).status, "falhou");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");
    let itens = historico(&amb.state, &id, "c").unwrap();
    let separadores: Vec<String> = itens.iter().filter(|i| i["separador"] == true).map(|i| i["text"].as_str().unwrap_or_default().to_owned()).collect();
    assert_eq!(separadores, vec!["Volta 2".to_owned()], "{itens:?}");
    let pos = itens.iter().position(|i| i["separador"] == true).unwrap();
    assert!(pos > 0 && pos < itens.len() - 1, "o separador fica entre as duas voltas");
}

// ---- Correção 1: cursor em memória, Sala, atividade zerada, cancelar ----

#[test]
fn leitura_com_cursor_nao_toca_nas_linhas_anteriores() {
    let amb = ambiente();
    let id = amb.iniciar(fluxo_de_um_comando());
    let mut e = ate_parar(&id);
    e.id = "cursor".into();
    e.evento_seq = 0;
    for i in 0..5 { eventos::registrar(&amb.state, &mut e, Some("c"), "aviso", &format!("n{i}")); }
    assert_eq!(so_eventos(&amb, "cursor", 0).len(), 5);
    // Troca a 1ª linha (já lida) por outra de mesmo tamanho, mas com seq novo: relida, apareceria.
    let arquivo = pasta_execucoes(&amb.state).join("cursor").join("eventos.jsonl");
    let texto = fs::read_to_string(&arquivo).unwrap();
    let primeira = texto.lines().next().unwrap();
    let mut isca = eventos::Evento { seq: 99, ms: 0, no: None, tipo: "aviso".into(), texto: String::new() };
    let vazio = serde_json::to_string(&isca).unwrap().len();
    isca.texto = "z".repeat(primeira.len() - vazio);
    let isca = serde_json::to_string(&isca).unwrap();
    assert_eq!(isca.len(), primeira.len());
    fs::write(&arquivo, texto.replacen(primeira, &isca, 1)).unwrap();
    eventos::registrar(&amb.state, &mut e, Some("c"), "aviso", "novo1");
    eventos::registrar(&amb.state, &mut e, Some("c"), "aviso", "novo2");
    let novos = so_eventos(&amb, "cursor", 5);
    assert_eq!(novos.iter().map(|e| e.texto.as_str()).collect::<Vec<_>>(), vec!["novo1", "novo2"]);
    // Cursor antigo lê do começo: aí a linha trocada aparece.
    assert!(so_eventos(&amb, "cursor", 0).iter().any(|e| e.seq == 99));
}

#[test]
fn participante_da_sala_separa_o_log_com_nome_e_rodada() {
    let amb = ambiente();
    eventos::separar_participante(&amb.state, "exec-sala", "s", "claude", 1);
    eventos::separar_participante(&amb.state, "exec-sala", "s", "codex", 2);
    let itens = historico(&amb.state, "exec-sala", "s").unwrap();
    let textos: Vec<&str> = itens.iter().filter(|i| i["separador"] == true).filter_map(|i| i["text"].as_str()).collect();
    assert_eq!(textos, vec!["claude · rodada 1", "codex · rodada 2"]);
}

#[test]
fn primeira_atividade_igual_a_da_corrida_anterior_vira_evento() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "v", "nome": "V", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, comando("c", &conta_e_exige("c", "libera")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    assert_eq!(ate_parar(&id).status, "falhou");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    ate_parar(&id);
    let atividades = so_eventos(&amb, &id, 0).into_iter().filter(|e| e.tipo == "atividade" && e.no.as_deref() == Some("c")).count();
    assert_eq!(atividades, 2, "uma por corrida");
}

#[test]
fn cancelar_entra_na_linha_do_tempo() {
    let amb = ambiente();
    let id = amb.iniciar(json!({"id": "a", "nome": "A", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ap", "tipo": "aprovacao"}, {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap"}, {"de": "ap", "para": "f", "saida": "sim"}]}));
    assert_eq!(ate_parar(&id).status, "aguardando");
    cancelar(&amb.state, &id).unwrap();
    let ultimo = so_eventos(&amb, &id, 0).pop().unwrap();
    assert_eq!((ultimo.tipo.as_str(), ultimo.texto.as_str(), ultimo.no), ("aviso", "Execução cancelada", None));
}

// ---- Subfluxos: execução filha ----

fn no_subfluxo(id: &str, alvo: &str) -> Value { json!({"id": id, "tipo": "subfluxo", "titulo": id, "config": {"fluxo": alvo}}) }

/// entrada → s (chama `alvo`) → fim
fn chama(id: &str, nome: &str, alvo: &str) -> Value {
    json!({"id": id, "nome": nome, "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, no_subfluxo("s", alvo), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "s"}, {"de": "s", "para": "f"}]})
}

/// entrada → c (comando) → fim
fn so_comando(id: &str, nome: &str, texto: &str) -> Value {
    json!({"id": id, "nome": nome, "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, comando("c", texto), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "f", "saida": "sim"}]})
}

/// entrada → antes (comando) → ap (aprovação) → fim
fn com_aprovacao(id: &str, nome: &str) -> Value {
    json!({"id": id, "nome": nome, "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, comando("antes", &conta(id)), {"id": "ap", "tipo": "aprovacao"}, {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "antes"}, {"de": "antes", "para": "ap", "saida": "sim"}, {"de": "ap", "para": "f", "saida": "sim"}]})
}

impl Ambiente {
    fn salvar(&self, fluxo: Value) {
        fluxos::salvar(&fluxos::pasta_do_projeto(&self.projeto), &serde_json::from_value(fluxo).unwrap()).unwrap();
    }
    fn git(&self, args: &[&str]) -> String {
        let saida = Command::new("git").args(["-c", "user.name=t", "-c", "user.email=t@t.t"]).args(args).current_dir(&self.projeto).output().unwrap();
        assert!(saida.status.success(), "{}", String::from_utf8_lossy(&saida.stderr));
        String::from_utf8_lossy(&saida.stdout).into_owned()
    }
    fn com_git(&self) { self.git(&["init", "-q"]); self.git(&["commit", "-q", "--allow-empty", "-m", "inicio"]); }
}

/// Espera a condição; ficar preso é falha do teste.
fn ate(oque: &str, condicao: impl Fn() -> bool) {
    for _ in 0..600 {
        if condicao() { return; }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("não aconteceu: {oque}");
}

fn no_em(id: &str, no: &str, status: &str) -> bool { obter(id).is_some_and(|e| e.nos.get(no).is_some_and(|n| n.status == status)) }

fn com_status(id: &str, status: &str) { ate(&format!("{id} em {status}"), || obter(id).is_some_and(|e| e.status == status)); }

/// A filha do nó, já existindo de verdade.
fn filha_de(id: &str, no: &str) -> String {
    ate("filha aberta", || obter(id).and_then(|e| e.nos[no].filha.clone()).is_some_and(|f| obter(&f).is_some()));
    obter(id).unwrap().nos[no].filha.clone().unwrap()
}

fn cadeia(id: &str) -> Vec<String> {
    filhas_de(id).into_iter().flat_map(|f| { let mut abaixo = cadeia(&f); abaixo.insert(0, f); abaixo }).collect()
}

fn processo_vivo(pid: &str) -> bool { Command::new("kill").args(["-0", pid]).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) }

/// X → Y → Z, com Z rodando `texto`.
fn x_y_z(amb: &Ambiente, texto: &str) -> String {
    amb.salvar(so_comando("z", "Z", texto));
    amb.salvar(chama("y", "Y", "z"));
    amb.iniciar(chama("x", "X", "y"))
}

/// Como `carregar` encontra o disco depois de o engine cair com X no meio do nó `s`: o executor de
/// antes é parado pelo pausar, senão ele seguiria vivo neste processo de teste.
fn derrubar_engine(amb: &Ambiente, x: &str, y: &str) {
    pausar(&amb.state, x).unwrap();
    ate("X largar a espera", || no_em(x, "s", "pausado"));
    mexer(&amb.state, &viva(x).unwrap(), |e| { e.status = "rodando".into(); e.fila.clear(); e.nos.get_mut("s").unwrap().status = "rodando".into(); });
    mexer(&amb.state, &viva(y).unwrap(), |e| e.status = "aguardando".into());
    carregar(&amb.state);
}

// ---- Origem da execução: quem passa sozinho pela Aprovação ----

fn iniciar_como(amb: &Ambiente, fluxo: Value, origem: &str, veio_de_fora: bool) -> String {
    let fluxo: Fluxo = serde_json::from_value(fluxo).unwrap();
    iniciar_com_origem(&amb.state, fluxo, amb.projeto.to_string_lossy().into(), "p".into(), "x".into(), Origem { origem: origem.into(), veio_de_fora, pasta: None }).unwrap().id
}

/// entrada → antes → ap → (sim: depois) / (nao: recusou)
fn aprovacao_com_saidas() -> Value {
    json!({"id": "ap", "nome": "Ap", "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, comando("antes", "echo saida-de-antes"), {"id": "ap", "tipo": "aprovacao"},
        comando("depois", &conta("depois")), comando("recusou", &conta("recusou")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "antes"}, {"de": "antes", "para": "ap", "saida": "sim"}, {"de": "ap", "para": "depois", "saida": "sim"}, {"de": "ap", "para": "recusou", "saida": "nao"},
            {"de": "depois", "para": "f", "saida": "sim"}, {"de": "recusou", "para": "f", "saida": "sim"}]})
}

#[test]
fn aprovacao_passa_sozinha_com_origem_mcp() {
    let amb = ambiente();
    let id = iniciar_como(&amb, aprovacao_com_saidas(), "mcp", false);
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((e.nos["ap"].status.as_str(), e.nos["ap"].caminho.as_deref()), ("concluido", Some("sim")));
    assert!(e.nos["ap"].saida.contains("saida-de-antes"));
    assert_eq!((amb.linhas("depois.log"), amb.linhas("recusou.log")), (1, 0));
    assert!(so_eventos(&amb, &id, 0).iter().any(|ev| ev.tipo == "aprovacao" && ev.no.as_deref() == Some("ap")), "fica na linha do tempo");
}

#[test]
fn aprovacao_para_com_origem_manual() {
    let amb = ambiente();
    let id = amb.iniciar(aprovacao_com_saidas());
    assert_eq!((ate_parar(&id).status.as_str(), amb.linhas("depois.log")), ("aguardando", 0));
    assert_eq!(obter(&id).unwrap().origem, "manual");
}

#[test]
fn corpo_sem_origem_vale_manual() {
    let fluxo = json!({"id": "a", "nome": "A", "nos": [], "conexoes": []});
    let corpo: mobile::IniciarFluxo = serde_json::from_value(json!({"fluxo": fluxo, "projeto_id": "p", "entrada": "x"})).unwrap();
    assert_eq!((corpo.quem.origem.as_str(), corpo.quem.veio_de_fora), ("manual", false));
    let corpo: mobile::IniciarFluxo = serde_json::from_value(json!({"fluxo": fluxo, "projeto_id": "p", "entrada": "x", "origem": "mcp", "veio_de_fora": true})).unwrap();
    assert_eq!((corpo.quem.origem.as_str(), corpo.quem.veio_de_fora), ("mcp", true));
    let antiga: Execucao = serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "aguardando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": fluxo, "nos": {}, "fila": []})).unwrap();
    assert_eq!((antiga.origem.as_str(), antiga.veio_de_fora), ("manual", false));
    assert!(!aprovacao_automatica(&antiga));
}

/// A única exceção à autonomia: tarefa vinda de fora nunca executa sem um humano na Aprovação.
#[test]
fn aprovacao_para_quando_veio_de_fora() {
    let amb = ambiente();
    let id = iniciar_como(&amb, aprovacao_com_saidas(), "mcp", true);
    com_status(&id, "aguardando");
    aprovar(&amb.state, &id, "antes", true).unwrap();
    ate("a aprovação do fluxo parar", || no_em(&id, "ap", "aguardando"));
    com_status(&id, "aguardando");
    assert_eq!(amb.linhas("depois.log"), 0);
    aprovar(&amb.state, &id, "ap", true).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");
    assert_eq!(amb.linhas("depois.log"), 1);
}

#[test]
fn aprovacao_automatica_e_so_mcp_sem_marca() {
    let base = |origem: &str, veio_de_fora: bool| -> Execucao { serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "rodando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": {"id": "a", "nome": "A", "nos": [], "conexoes": []}, "nos": {}, "fila": [], "origem": origem, "veio_de_fora": veio_de_fora})).unwrap() };
    assert!(aprovacao_automatica(&base("mcp", false)));
    for (origem, marca) in [("mcp", true), ("manual", false), ("manual", true), ("", false), ("qualquer", false)] { assert!(!aprovacao_automatica(&base(origem, marca)), "{origem} {marca}"); }
}

#[test]
fn subfluxo_herda_origem_e_marca() {
    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let marcada = iniciar_como(&amb, chama("x", "X", "y"), "mcp", true);
    let filha = filha_de(&marcada, "s");
    com_status(&filha, "aguardando");
    let f = obter(&filha).unwrap();
    assert_eq!((f.origem.as_str(), f.veio_de_fora), ("mcp", true));
    assert_eq!(amb.linhas("y.log"), 0, "a filha herda a parada antes da primeira etapa");
    aprovar(&amb.state, &filha, "antes", true).unwrap();
    ate("a aprovação da filha parar", || no_em(&filha, "ap", "aguardando"));
    assert_eq!(amb.linhas("y.log"), 1);
    cancelar(&amb.state, &marcada).unwrap();

    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let sem_marca = iniciar_como(&amb, chama("x", "X", "y"), "mcp", false);
    assert_eq!(ate_parar(&sem_marca).status, "concluido", "filha da IA passa sozinha");
    let filha = obter(&sem_marca).unwrap().nos["s"].filha.clone().unwrap();
    assert_eq!(obter(&filha).unwrap().origem, "mcp");

    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let manual = amb.iniciar(chama("x", "X", "y"));
    let filha = filha_de(&manual, "s");
    com_status(&filha, "aguardando");
    cancelar(&amb.state, &manual).unwrap();
}

#[test]
fn comando_de_execucao_marcada_recebe_a_variavel() {
    let amb = ambiente();
    let mostra = format!("echo \"marca=${}\" > env.txt", fluxos::VARIAVEL_DE_FORA);
    let id = iniciar_como(&amb, so_comando("c", "C", &mostra), "mcp", true);
    com_status(&id, "aguardando");
    aprovar(&amb.state, &id, "c", true).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");
    assert_eq!(fs::read_to_string(amb.projeto.join("env.txt")).unwrap().trim(), "marca=1", "comando");

    let amb = ambiente();
    let id = iniciar_como(&amb, so_comando("c", "C", &mostra), "mcp", false);
    ate_parar(&id);
    assert_eq!(fs::read_to_string(amb.projeto.join("env.txt")).unwrap().trim(), "marca=", "sem a marca, sem a variável");
}

const CLI_FALSA: &str = "OMNI_TESTE_CLI_FALSA";

/// Uma CLI de mentira, chamada `claude`, que grava no projeto o que recebeu no ambiente.
#[cfg(unix)]
fn pasta_com_cli_falsa(raiz: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let pasta = raiz.join("bin");
    fs::create_dir_all(&pasta).unwrap();
    let arquivo = pasta.join("claude");
    let script = format!("#!/bin/sh\ncat > /dev/null\necho \"marca=${}\" > agente-env.txt\necho '{{\"type\":\"result\",\"result\":\"feito\",\"total_cost_usd\":0}}'\n", fluxos::VARIAVEL_DE_FORA);
    fs::write(&arquivo, script).unwrap();
    fs::set_permissions(&arquivo, fs::Permissions::from_mode(0o755)).unwrap();
    pasta
}

/// Roda de verdade em `rodar_na_conta`. O PATH e o HOME trocados valem só para o processo filho:
/// neste aqui, os outros testes abrem `sh` e `git` ao mesmo tempo, e `agentes::listar` semeia o HOME.
#[cfg(unix)]
#[test]
fn agente_de_execucao_marcada_recebe_a_variavel() {
    let raiz = tempfile::tempdir().unwrap();
    let cli = pasta_com_cli_falsa(raiz.path());
    let caminho = std::env::join_paths(std::iter::once(cli).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap()))).unwrap();
    let saida = Command::new(std::env::current_exe().unwrap()).args(["--exact", "fluxos::limites::etapa_de_agente_com_cli_falsa", "--ignored", "--nocapture"])
        .env("PATH", caminho).env("HOME", raiz.path()).env(CLI_FALSA, "1").output().unwrap();
    let texto = String::from_utf8_lossy(&saida.stdout);
    assert!(saida.status.success() && texto.contains("1 passed"), "{texto}\n{}", String::from_utf8_lossy(&saida.stderr));
}

#[cfg(unix)]
#[test]
#[ignore = "roda como processo filho de agente_de_execucao_marcada_recebe_a_variavel"]
fn etapa_de_agente_com_cli_falsa() {
    assert!(std::env::var_os(CLI_FALSA).is_some(), "só vale com a CLI de mentira no PATH");
    let fluxo = json!({"id": "g", "nome": "G", "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, {"id": "a", "tipo": "agente", "titulo": "a", "agente": "falso"}, {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "a"}, {"de": "a", "para": "f"}]});
    let preparar = || {
        let amb = ambiente();
        let modelo = omni_core::agentes::ModeloDeAgente { name: "falso".into(), description: "d".into(), cli: "claude".into(), modelo: String::new(), esforco: String::new(), conta: String::new(), ferramentas: Vec::new(), saida: String::new(), instrucoes: "i".into() };
        omni_core::agentes::salvar(&omni_core::agentes::pasta_do_projeto(&amb.projeto), &modelo).unwrap();
        amb
    };
    let lido = |amb: &Ambiente| fs::read_to_string(amb.projeto.join("agente-env.txt")).unwrap();

    let amb = preparar();
    let id = iniciar_como(&amb, fluxo.clone(), "mcp", true);
    com_status(&id, "aguardando");
    assert!(!amb.projeto.join("agente-env.txt").exists(), "o agente não abre antes de aprovar");
    aprovar(&amb.state, &id, "a", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(lido(&amb).trim(), "marca=1");

    let amb = preparar();
    let e = ate_parar(&iniciar_como(&amb, fluxo, "mcp", false));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(lido(&amb).trim(), "marca=", "sem a marca, sem a variável");
}

// ---- Tarefa de fora: para antes da primeira etapa que age ----

/// entrada → primeiro → segundo → fim
fn dois_comandos() -> Value {
    json!({"id": "dois", "nome": "Dois", "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, comando("primeiro", &conta("primeiro")), comando("segundo", &conta("segundo")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "primeiro"}, {"de": "primeiro", "para": "segundo", "saida": "sim"}, {"de": "segundo", "para": "f", "saida": "sim"}]})
}

#[test]
fn execucao_marcada_nao_roda_o_comando_ate_aprovar() {
    let amb = ambiente();
    let id = iniciar_como(&amb, dois_comandos(), "mcp", true);
    let e = ate_parar(&id);
    assert_eq!((e.status.as_str(), e.nos["primeiro"].status.as_str()), ("aguardando", "aguardando"));
    assert_eq!(amb.linhas("primeiro.log"), 0, "parou antes de rodar");
    assert_eq!(e.aviso.as_deref(), Some("aprovacao"), "a tela e o celular são avisados como numa Aprovação");
    assert!(so_eventos(&amb, &id, 0).iter().any(|ev| ev.tipo == "aprovacao" && ev.no.as_deref() == Some("primeiro")));

    aprovar(&amb.state, &id, "primeiro", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("primeiro.log"), amb.linhas("segundo.log")), (1, 1), "só a primeira etapa pergunta");
}

#[test]
fn execucao_sem_marca_roda_sem_parar() {
    for origem in ["mcp", "manual"] {
        let amb = ambiente();
        let e = ate_parar(&iniciar_como(&amb, dois_comandos(), origem, false));
        assert_eq!(e.status, "concluido", "{origem}: {:?}", e.erro);
        assert_eq!(amb.linhas("primeiro.log"), 1, "{origem}");
    }
}

#[test]
fn recusar_a_etapa_segurada_cancela_sem_rodar() {
    let amb = ambiente();
    let id = iniciar_como(&amb, dois_comandos(), "mcp", true);
    com_status(&id, "aguardando");
    aprovar(&amb.state, &id, "primeiro", false).unwrap();
    assert_eq!(ate_parar(&id).status, "cancelado");
    assert_eq!((amb.linhas("primeiro.log"), amb.linhas("segundo.log")), (0, 0));
    assert!(so_eventos(&amb, &id, 0).iter().any(|ev| ev.tipo == "aprovacao" && ev.texto == "Recusado"));
}

/// entrada → ap → (sim: depois) / (nao: recusou): a Aprovação do fluxo já é a parada.
fn aprovacao_na_frente() -> Value {
    json!({"id": "af", "nome": "Af", "na_pasta_do_projeto": true, "nos": [{"id": "e", "tipo": "entrada"}, {"id": "ap", "tipo": "aprovacao"}, comando("depois", &conta("depois")), comando("recusou", &conta("recusou")), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap"}, {"de": "ap", "para": "depois", "saida": "sim"}, {"de": "ap", "para": "recusou", "saida": "nao"},
            {"de": "depois", "para": "f", "saida": "sim"}, {"de": "recusou", "para": "f", "saida": "sim"}]})
}

#[test]
fn aprovacao_na_frente_dispensa_a_parada() {
    let amb = ambiente();
    let id = iniciar_como(&amb, aprovacao_na_frente(), "mcp", true);
    com_status(&id, "aguardando");
    aprovar(&amb.state, &id, "ap", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(amb.linhas("depois.log"), 1);
}

/// Dizer não à Aprovação não é aprovar o que vem pela saída `nao`.
#[test]
fn recusa_na_aprovacao_nao_libera_a_saida_nao() {
    let amb = ambiente();
    let id = iniciar_como(&amb, aprovacao_na_frente(), "mcp", true);
    com_status(&id, "aguardando");
    aprovar(&amb.state, &id, "ap", false).unwrap();
    ate("a saída nao parar", || no_em(&id, "recusou", "aguardando"));
    assert_eq!(amb.linhas("recusou.log"), 0);
    cancelar(&amb.state, &id).unwrap();
}

#[test]
fn cada_ramo_marcado_para_na_sua_primeira_etapa() {
    let amb = ambiente();
    let id = iniciar_como(&amb, paralelo(&[("a", conta("a")), ("b", conta("b"))]), "mcp", true);
    ate("os dois ramos pararem", || no_em(&id, "a", "aguardando") && no_em(&id, "b", "aguardando"));
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log")), (0, 0));
    aprovar(&amb.state, &id, "a", true).unwrap();
    com_status(&id, "aguardando");
    assert_eq!(amb.linhas("b.log"), 0, "aprovar um ramo não solta o outro");
    aprovar(&amb.state, &id, "b", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((amb.linhas("a.log"), amb.linhas("b.log"), amb.linhas("depois.log")), (1, 1, 1));
}

#[test]
fn subfluxo_marcado_para_na_filha() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", &conta("y")));
    let id = iniciar_como(&amb, chama("x", "X", "y"), "mcp", true);
    let filha = filha_de(&id, "s");
    com_status(&filha, "aguardando");
    assert_eq!(amb.linhas("y.log"), 0);
    aprovar(&amb.state, &filha, "c", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(amb.linhas("y.log"), 1);
}

/// A resposta vale depois de religar o engine, e a etapa não pergunta de novo se falhar e voltar.
#[test]
fn parada_de_fora_sobrevive_a_religar_e_a_retomar() {
    let amb = ambiente();
    let id = iniciar_como(&amb, so_comando("c", "C", &conta_e_exige("c", "libera")), "mcp", true);
    com_status(&id, "aguardando");
    carregar(&amb.state);
    assert_eq!(obter(&id).unwrap().status, "aguardando");
    pausar(&amb.state, &id).unwrap();
    assert!(aprovar(&amb.state, &id, "c", true).is_err(), "pausada não responde");
    retomar(&amb.state, &id).unwrap();
    assert_eq!((obter(&id).unwrap().status.as_str(), amb.linhas("c.log")), ("aguardando", 0), "retomar devolve a pergunta");
    aprovar(&amb.state, &id, "c", true).unwrap();
    assert_eq!(ate_parar(&id).status, "falhou");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(amb.linhas("c.log"), 2);
}

#[test]
fn etapa_segurada_lembra_de_onde_veio() {
    let amb = ambiente();
    let id = iniciar_como(&amb, so_comando("c", "C", "echo ok"), "mcp", true);
    com_status(&id, "aguardando");
    let de = obter(&id).unwrap().nos["c"].esperou_de.clone();
    assert_eq!(de.as_deref(), Some("e"), "sem isto o `{{anterior}}` chegaria vazio depois de aprovar");
    cancelar(&amb.state, &id).unwrap();
}

fn iniciar_da_pasta(amb: &Ambiente, pasta: &Path) -> Execucao {
    let fluxo: Fluxo = serde_json::from_value(aprovacao_na_frente()).unwrap();
    let quem = Origem { origem: "mcp".into(), veio_de_fora: false, pasta: Some(pasta.to_string_lossy().into()) };
    iniciar_com_origem(&amb.state, fluxo, amb.projeto.to_string_lossy().into(), "p".into(), "x".into(), quem).unwrap()
}

/// O MCP pode ter perdido a marca (`--resume`, CLI que filtra o ambiente): a pasta denuncia.
#[test]
fn rodar_de_dentro_de_execucao_marcada_nasce_marcado() {
    let amb = ambiente();
    let sub = amb.projeto.join("sub");
    fs::create_dir_all(&sub).unwrap();
    let outra = tempfile::tempdir().unwrap();
    let limpa = iniciar_da_pasta(&amb, &amb.projeto);
    assert!(!limpa.veio_de_fora, "nenhuma execução marcada aqui ainda");
    cancelar(&amb.state, &limpa.id).unwrap();

    let marcada = iniciar_como(&amb, so_comando("c", "C", "echo ok"), "mcp", true);
    com_status(&marcada, "aguardando");
    for pasta in [amb.projeto.as_path(), sub.as_path()] {
        let e = iniciar_da_pasta(&amb, pasta);
        assert!(e.veio_de_fora, "{}", pasta.display());
        cancelar(&amb.state, &e.id).unwrap();
    }
    let e = iniciar_da_pasta(&amb, outra.path());
    assert!(!e.veio_de_fora, "outra pasta não herda");
    cancelar(&amb.state, &e.id).unwrap();

    pausar(&amb.state, &marcada).unwrap();
    let e = iniciar_da_pasta(&amb, &amb.projeto);
    assert!(e.veio_de_fora, "pausada ainda pode voltar");
    cancelar(&amb.state, &e.id).unwrap();

    cancelar(&amb.state, &marcada).unwrap();
    let e = iniciar_da_pasta(&amb, &amb.projeto);
    assert!(!e.veio_de_fora, "execução que acabou não marca mais a pasta");
    cancelar(&amb.state, &e.id).unwrap();
}

#[test]
fn listar_marca_a_filha_com_o_pai() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", "echo ok"));
    let id = amb.iniciar(chama("x", "X", "y"));
    let filha = filha_de(&id, "s");
    let lista = listar();
    let linha = |alvo: &str| lista.iter().find(|l| l["id"] == alvo).unwrap().clone();
    assert_eq!(linha(&filha)["pai"], json!(id));
    assert!(linha(&id)["pai"].is_null());
    ate_parar(&id);
}

#[test]
fn pai_recebe_a_saida_da_filha() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", "echo resultado-de-y"));
    let id = amb.iniciar(json!({"id": "x", "nome": "X", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, comando("c", "echo vindo-de-x"), no_subfluxo("s", "y"), comando("depois", "echo fim"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "s", "saida": "sim"}, {"de": "s", "para": "depois"}, {"de": "depois", "para": "f", "saida": "sim"}]}));
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert!(e.nos["s"].saida.contains("resultado-de-y"), "{}", e.nos["s"].saida);
    let filha = obter(e.nos["s"].filha.as_ref().unwrap()).unwrap();
    assert!(filha.entrada.contains("vindo-de-x"), "a entrada da filha é o `anterior`: {}", filha.entrada);
    assert_eq!(filha.pai.as_ref().map(|p| (p.execucao.as_str(), p.no.as_str())), Some((id.as_str(), "s")));
    let marcas: Vec<String> = so_eventos(&amb, &id, 0).into_iter().filter(|ev| ev.tipo == "subfluxo").map(|ev| ev.texto).collect();
    assert_eq!(marcas, ["Chamou “Y”", "“Y” terminou"]);
}

#[test]
fn falha_na_filha_e_falha_no_no() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", "echo quebrou-em-y; exit 3"));
    let e = ate_parar(&amb.iniciar(chama("x", "X", "y")));
    assert_eq!((e.status.as_str(), e.nos["s"].status.as_str(), e.nos["f"].status.as_str()), ("falhou", "falhou", "pendente"));
    let erro = e.nos["s"].erro.clone().unwrap();
    assert!(erro.contains("“Y”") && erro.contains("o comando falhou"), "{erro}");
    assert!(so_eventos(&amb, &e.id, 0).iter().any(|ev| ev.tipo == "subfluxo" && ev.texto.starts_with("“Y” falhou")));
}

#[test]
fn cadeia_x_y_z_termina() {
    let amb = ambiente();
    let e = ate_parar(&x_y_z(&amb, "echo saida-de-z"));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert!(e.nos["f"].saida.contains("saida-de-z"), "{}", e.nos["f"].saida);
    assert_eq!(cadeia(&e.id).len(), 2);
}

#[test]
fn filha_usa_a_pasta_do_pai() {
    let amb = ambiente();
    amb.com_git();
    amb.salvar(so_comando("y", "Y", "echo y > de-y.txt"));
    let mut x = chama("x", "X", "y");
    x["na_pasta_do_projeto"] = json!(false);
    x["nos"].as_array_mut().unwrap().push(comando("ve", "test -f de-y.txt"));
    x["conexoes"] = json!([{"de": "e", "para": "s"}, {"de": "s", "para": "ve"}, {"de": "ve", "para": "f", "saida": "sim"}]);
    let e = ate_parar(&amb.iniciar(x));
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    let filha = obter(e.nos["s"].filha.as_ref().unwrap()).unwrap();
    assert_ne!(e.pasta, amb.projeto.to_string_lossy());
    assert!(e.branch.is_some());
    assert_eq!((&filha.pasta, &filha.branch, &filha.base), (&e.pasta, &e.branch, &e.base));
}

#[test]
fn filha_nao_remove_o_worktree() {
    let amb = ambiente();
    amb.com_git();
    amb.salvar(so_comando("y", "Y", "echo y"));
    let mut x = chama("x", "X", "y");
    x["na_pasta_do_projeto"] = json!(false);
    x["nos"].as_array_mut().unwrap().push(json!({"id": "ap", "tipo": "aprovacao"}));
    x["conexoes"] = json!([{"de": "e", "para": "s"}, {"de": "s", "para": "ap"}, {"de": "ap", "para": "f", "saida": "sim"}]);
    let id = amb.iniciar(x);
    com_status(&id, "aguardando");
    let e = obter(&id).unwrap();
    assert_eq!(obter(e.nos["s"].filha.as_ref().unwrap()).unwrap().status, "concluido");
    assert!(Path::new(&e.pasta).is_dir(), "a filha terminou e a pasta continua de pé");
    assert_eq!(amb.git(&["worktree", "list"]).lines().count(), 2, "a filha não abre worktree próprio");
}

#[test]
fn cancelar_x_mata_o_processo_de_z() {
    let amb = ambiente();
    let id = x_y_z(&amb, "sleep 60 & echo $! > z.pid; wait");
    let arquivo = amb.projeto.join("z.pid");
    ate("Z abrir o sleep", || fs::read_to_string(&arquivo).is_ok_and(|t| t.ends_with('\n')));
    let pid = fs::read_to_string(&arquivo).unwrap().trim().to_owned();
    assert!(processo_vivo(&pid));
    let (y, z) = { let c = cadeia(&id); (c[0].clone(), c[1].clone()) };
    cancelar(&amb.state, &id).unwrap();
    ate("o sleep de Z sumir", || !processo_vivo(&pid));
    for e in [&id, &y, &z] { assert_eq!(obter(e).unwrap().status, "cancelado"); }
    ate("nenhuma etapa da cadeia rodando", || [&id, &y, &z].iter().all(|e| obter(e).unwrap().nos.values().all(|n| n.status != "rodando")));
}

#[test]
fn pausar_x_pausa_a_cadeia_e_retomar_continua() {
    let amb = ambiente();
    let id = x_y_z(&amb, "echo x >> z.log; test -f libera || { sleep 60 & echo $! > z.pid; wait; exit 1; }; echo saida-de-z");
    let arquivo = amb.projeto.join("z.pid");
    ate("Z abrir o sleep", || fs::read_to_string(&arquivo).is_ok_and(|t| t.ends_with('\n')));
    let pid = fs::read_to_string(&arquivo).unwrap().trim().to_owned();
    let (y, z) = { let c = cadeia(&id); (c[0].clone(), c[1].clone()) };
    pausar(&amb.state, &id).unwrap();
    ate("o sleep de Z sumir", || !processo_vivo(&pid));
    for e in [&id, &y, &z] { assert_eq!(obter(e).unwrap().status, "pausado"); }
    ate("as esperas soltarem", || no_em(&id, "s", "pausado") && no_em(&y, "s", "pausado") && no_em(&z, "c", "pausado"));
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert!(e.nos["f"].saida.contains("saida-de-z"), "{}", e.nos["f"].saida);
    assert_eq!((amb.linhas("z.log"), cadeia(&id).len()), (2, 2), "Z refaz a etapa e ninguém abre filha nova");
}

#[test]
fn engine_religado_nao_abre_segunda_filha() {
    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let id = amb.iniciar(chama("x", "X", "y"));
    let y = filha_de(&id, "s");
    com_status(&y, "aguardando");
    derrubar_engine(&amb, &id, &y);
    assert_eq!((obter(&id).unwrap().status.as_str(), obter(&y).unwrap().status.as_str()), ("pausado", "pausado"), "a cadeia volta pausada");
    assert!(aprovar(&amb.state, &y, "ap", true).is_err(), "com a cadeia pausada a filha não anda");
    retomar(&amb.state, &id).unwrap();
    com_status(&y, "aguardando");
    std::thread::sleep(Duration::from_millis(700));
    assert_eq!(filhas_de(&id), [y.clone()], "espera a filha que já existe");
    aprovar(&amb.state, &y, "ap", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((filhas_de(&id), amb.linhas("y.log")), (vec![y], 1));
}

/// A filha terminou enquanto o pai estava parado: o pai colhe o resultado, sem rodar nada de novo.
#[test]
fn filha_que_terminou_com_o_pai_parado_entrega_a_saida() {
    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let id = amb.iniciar(chama("x", "X", "y"));
    let y = filha_de(&id, "s");
    com_status(&y, "aguardando");
    pausar(&amb.state, &id).unwrap();
    ate("X largar a espera", || no_em(&id, "s", "pausado"));
    retomar(&amb.state, &y).unwrap();
    aprovar(&amb.state, &y, "ap", true).unwrap();
    com_status(&y, "concluido");
    retomar(&amb.state, &id).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");
    assert_eq!((filhas_de(&id), amb.linhas("y.log")), (vec![y], 1));
}

/// Retomar o pai que falhou refaz a etapa dentro da mesma filha.
#[test]
fn retomar_o_pai_refaz_a_filha_que_falhou_sem_abrir_outra() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", &conta_e_exige("y", "libera")));
    let id = amb.iniciar(chama("x", "X", "y"));
    assert_eq!(ate_parar(&id).status, "falhou");
    let y = filha_de(&id, "s");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((filhas_de(&id), amb.linhas("y.log")), (vec![y], 2));
}

/// Passar de novo pelo nó num loop é chamada nova, não retomada da anterior.
#[test]
fn volta_do_loop_abre_filha_nova() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", &conta("y")));
    let id = amb.iniciar(json!({"id": "x", "nome": "X", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, no_subfluxo("s", "y"), comando("c", "test $(wc -l < y.log) -ge 2"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "s"}, {"de": "s", "para": "c"}, {"de": "c", "para": "s", "saida": "nao"}, {"de": "c", "para": "f", "saida": "sim"}]}));
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((filhas_de(&id).len(), amb.linhas("y.log")), (2, 2));
}

#[test]
fn fluxo_apagado_falha_com_mensagem_clara() {
    let amb = ambiente();
    let e = ate_parar(&amb.iniciar(chama("x", "X", "Y")));
    assert_eq!(e.status, "falhou");
    assert_eq!(e.nos["s"].erro.as_deref(), Some("O fluxo chamado “Y” não existe mais"));
    assert_eq!(e.nos["s"].filha, None, "sem filha para a tela abrir");
}

/// O engine caiu entre reservar o id no pai e gravar a filha: a nova tentativa usa o mesmo id.
#[test]
fn id_reservado_sem_filha_abre_com_o_mesmo_id() {
    let amb = ambiente();
    let id = amb.iniciar(chama("x", "X", "y"));
    assert_eq!(ate_parar(&id).status, "falhou");
    let reservado = format!("exec-reservada-{:04x}", rand::random::<u16>());
    mexer(&amb.state, &viva(&id).unwrap(), |e| e.nos.get_mut("s").unwrap().filha = Some(reservado.clone()));
    amb.salvar(so_comando("y", "Y", &conta("y")));
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!((filhas_de(&id), amb.linhas("y.log")), (vec![reservado], 1));
}

#[test]
fn fluxo_do_projeto_vence_o_global_de_mesmo_id() {
    let amb = ambiente();
    let global = amb.projeto.with_file_name("global");
    fluxos::salvar(&global, &serde_json::from_value(so_comando("y", "Do global", "true")).unwrap()).unwrap();
    fluxos::salvar(&global, &serde_json::from_value(so_comando("so-global", "Só global", "true")).unwrap()).unwrap();
    amb.salvar(so_comando("y", "Do projeto", "true"));
    let projeto = amb.projeto.to_string_lossy();
    assert_eq!(subfluxo::fluxo_salvo("y", Some(&global), &projeto).unwrap().nome, "Do projeto");
    assert_eq!(subfluxo::fluxo_salvo("so-global", Some(&global), &projeto).unwrap().nome, "Só global");
}

#[test]
fn pilha_mostra_x_y_z() {
    let amb = ambiente();
    let id = x_y_z(&amb, "echo ok");
    assert_eq!(obter(&id).unwrap().pilha, ["X"]);
    assert_eq!(ate_parar(&id).status, "concluido");
    let c = cadeia(&id);
    assert_eq!(obter(&c[0]).unwrap().pilha, ["X", "Y"]);
    assert_eq!(obter(&c[1]).unwrap().pilha, ["X", "Y", "Z"]);
}

#[test]
fn ciclo_roda_e_cancelar_para() {
    let amb = ambiente();
    let x = json!({"id": "x", "nome": "X", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, comando("c", "echo x >> ciclo.log; sleep 0.05"), no_subfluxo("s", "x"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "c"}, {"de": "c", "para": "s", "saida": "sim"}, {"de": "s", "para": "f"}]});
    amb.salvar(x.clone());
    let id = amb.iniciar(x);
    ate("três níveis", || amb.linhas("ciclo.log") >= 3);
    cancelar(&amb.state, &id).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let (linhas, tamanho) = (amb.linhas("ciclo.log"), cadeia(&id).len());
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!((amb.linhas("ciclo.log"), cadeia(&id).len()), (linhas, tamanho), "cancelar parou o ciclo");
    assert!(tamanho >= 2);
    for e in std::iter::once(id.clone()).chain(cadeia(&id)) { assert_eq!(obter(&e).unwrap().status, "cancelado", "{e}"); }
}

/// O pai segue `rodando`, sem dar a filha por terminada, e a parada aparece nele, mesmo vindo de Z.
#[test]
fn filha_parada_aparece_no_pai_que_continua_esperando() {
    let amb = ambiente();
    amb.salvar(com_aprovacao("z", "Z"));
    amb.salvar(chama("y", "Y", "z"));
    let id = amb.iniciar(chama("x", "X", "y"));
    let atividade = |no: &str| obter(&id).unwrap().nos[no].atividade.clone();
    ate("a aprovação de Z chegar em X", || atividade("s").contains("“Z” espera aprovação"));
    let y = filha_de(&id, "s");
    let z = filha_de(&y, "s");
    let e = obter(&id).unwrap();
    assert_eq!((e.status.as_str(), e.nos["s"].status.as_str(), e.nos["f"].status.as_str()), ("rodando", "rodando", "pendente"));
    aprovar(&amb.state, &z, "ap", true).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");

    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let id = amb.iniciar(chama("x", "X", "y"));
    let y = filha_de(&id, "s");
    com_status(&y, "aguardando");
    mexer(&amb.state, &viva(&y).unwrap(), |e| e.status = "pausado".into());
    ate("a pausa de Y chegar em X", || obter(&id).unwrap().nos["s"].atividade.contains("“Y” está pausada"));
    assert_eq!(obter(&id).unwrap().status, "rodando");
    cancelar(&amb.state, &id).unwrap();
    assert_eq!(obter(&y).unwrap().status, "cancelado");
}

/// Aprovação em outro ramo deixa o pai `aguardando`: ele continua vivo e colhe a filha.
#[test]
fn pai_aguardando_em_outro_ramo_nao_larga_a_filha() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", "sleep 0.2; echo de-y"));
    let id = amb.iniciar(json!({"id": "x", "nome": "X", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ap", "tipo": "aprovacao"}, no_subfluxo("s", "y"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap"}, {"de": "e", "para": "s"}, {"de": "ap", "para": "f", "saida": "sim"}, {"de": "s", "para": "f"}]}));
    ate("a filha entregar", || no_em(&id, "s", "concluido"));
    assert_eq!(obter(&id).unwrap().status, "aguardando");
    aprovar(&amb.state, &id, "ap", true).unwrap();
    assert_eq!(ate_parar(&id).status, "concluido");
}

/// O engine caiu com um ramo na aprovação e outro esperando a filha: a etapa que rodava volta pausada,
/// e a pergunta volta junto no retomar.
#[test]
fn engine_religado_pausa_quem_aguardava_com_etapa_rodando() {
    let amb = ambiente();
    let dir = pasta_execucoes(&amb.state);
    fs::create_dir_all(&dir).unwrap();
    let id = format!("exec-religada-{:04x}", rand::random::<u16>());
    let e: Execucao = serde_json::from_value(json!({"id": id, "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "aguardando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": chama("x", "X", "y"), "nos": {"s": {"status": "rodando", "filha": "exec-outra"}, "f": {"status": "aguardando"}}, "fila": []})).unwrap();
    gravar(&amb.state, &e);
    carregar(&amb.state);
    let e = obter(&id).unwrap();
    assert_eq!((e.status.as_str(), e.nos["s"].status.as_str(), e.nos["f"].status.as_str()), ("pausado", "pausado", "aguardando"));
}

/// Quem para muda o status antes de ler `processos`: o processo que abre nesse intervalo se mata sozinho.
#[test]
fn processo_aberto_depois_do_cancelar_nao_sobrevive() {
    let amb = ambiente();
    let id = amb.iniciar(fluxo_de_um_comando());
    ate_parar(&id);
    cancelar(&amb.state, &id).unwrap();
    let mut processo = guardar_processo(&viva(&id).unwrap(), &id, "c", Command::new("sleep").arg("60").spawn().unwrap());
    assert!(!processo.filho.wait().unwrap().success(), "tinha que morrer");
}

// ---- Correção 1 do subfluxo ----

/// A espera dura horas com a filha parada: sem novidade, nada de reescrever o JSON do pai.
#[test]
fn espera_com_filha_parada_nao_regrava_o_pai() {
    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let id = amb.iniciar(chama("x", "X", "y"));
    ate("a aprovação chegar em X", || obter(&id).unwrap().nos["s"].atividade.contains("espera aprovação"));
    let arquivo = pasta_execucoes(&amb.state).join(format!("{id}.json"));
    let (bytes, modificado, atualizado) = (fs::read(&arquivo).unwrap(), fs::metadata(&arquivo).unwrap().modified().unwrap(), obter(&id).unwrap().atualizado_ms);
    std::thread::sleep(Duration::from_millis(1700));
    assert_eq!(obter(&id).unwrap().atualizado_ms, atualizado, "três ciclos sem mudança");
    assert_eq!(fs::metadata(&arquivo).unwrap().modified().unwrap(), modificado);
    assert_eq!(fs::read(&arquivo).unwrap(), bytes);
    cancelar(&amb.state, &id).unwrap();
}

/// A filha foi cancelada sozinha: morta, ela não conta, e retomar o pai chama de novo no mesmo nó.
#[test]
fn retomar_com_filha_cancelada_abre_outra_no_mesmo_lugar() {
    let amb = ambiente();
    amb.salvar(com_aprovacao("y", "Y"));
    let id = amb.iniciar(chama("x", "X", "y"));
    let velha = filha_de(&id, "s");
    com_status(&velha, "aguardando");
    cancelar(&amb.state, &velha).unwrap();
    let e = ate_parar(&id);
    assert_eq!((e.status.as_str(), e.nos["s"].erro.as_deref()), ("falhou", Some("“Y” foi cancelada")));
    retomar(&amb.state, &id).unwrap();
    ate("filha nova", || obter(&id).unwrap().nos["s"].filha.as_ref().is_some_and(|f| *f != velha && obter(f).is_some()));
    let nova = filha_de(&id, "s");
    com_status(&nova, "aguardando");
    assert_eq!(obter(&nova).unwrap().pai, Some(RefDoPai { execucao: id.clone(), no: "s".into() }));
    aprovar(&amb.state, &nova, "ap", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert_eq!(obter(&velha).unwrap().status, "cancelado");
    assert_eq!((filhas_de(&id).len(), amb.linhas("y.log")), (2, 2));
}

/// Aprovação num ramo e filha trabalhando no outro: Pausar para a filha sem perder a pergunta. A
/// etapa interrompida volta para a fila e anda depois da resposta, como em `retomar`.
#[test]
fn pausar_com_aprovacao_aberta_desce_a_cadeia_e_guarda_a_pergunta() {
    let amb = ambiente();
    amb.salvar(so_comando("y", "Y", "echo x >> y.log; test -f libera || { sleep 60 & echo $! > y.pid; wait; exit 1; }; echo de-y"));
    let id = amb.iniciar(json!({"id": "x", "nome": "X", "na_pasta_do_projeto": true, "nos": [
        {"id": "e", "tipo": "entrada"}, {"id": "ap", "tipo": "aprovacao"}, no_subfluxo("s", "y"), {"id": "f", "tipo": "fim"}],
        "conexoes": [{"de": "e", "para": "ap"}, {"de": "e", "para": "s"}, {"de": "ap", "para": "f", "saida": "sim"}, {"de": "s", "para": "f"}]}));
    let arquivo = amb.projeto.join("y.pid");
    ate("Y abrir o sleep", || fs::read_to_string(&arquivo).is_ok_and(|t| t.ends_with('\n')));
    let pid = fs::read_to_string(&arquivo).unwrap().trim().to_owned();
    com_status(&id, "aguardando");
    let y = filha_de(&id, "s");
    pausar(&amb.state, &id).expect("aguardando também pausa");
    ate("o sleep de Y sumir", || !processo_vivo(&pid));
    assert_eq!((obter(&id).unwrap().status.as_str(), obter(&y).unwrap().status.as_str()), ("pausado", "pausado"));
    ate("as esperas soltarem", || no_em(&id, "s", "pausado") && no_em(&y, "c", "pausado"));
    assert!(no_em(&id, "ap", "aguardando"), "a pergunta continua de pé");
    assert!(aprovar(&amb.state, &id, "ap", true).is_err(), "pausada não anda por aprovação");
    fs::write(amb.projeto.join("libera"), "").unwrap();
    retomar(&amb.state, &id).unwrap();
    assert_eq!(obter(&id).unwrap().status, "aguardando", "a pergunta volta");
    com_status(&y, "concluido");
    aprovar(&amb.state, &id, "ap", true).unwrap();
    let e = ate_parar(&id);
    assert_eq!(e.status, "concluido", "{:?}", e.erro);
    assert!(e.nos["s"].saida.contains("de-y"), "{}", e.nos["s"].saida);
    assert_eq!((filhas_de(&id), amb.linhas("y.log")), (vec![y], 2));
}

fn ate_sair(filho: &mut std::process::Child) {
    for _ in 0..400 {
        if filho.try_wait().unwrap().is_some() { return; }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("o processo não saiu");
}

fn com_chave(chave: &str) -> bool { processos().lock().unwrap().contains_key(chave) }

/// Processo morto pelo Parar antes de ler o prompt: a etapa para, não vira falha de "Broken pipe".
#[test]
fn stdin_fechado_pelo_parar_e_etapa_parada() {
    let amb = ambiente();
    let id = amb.iniciar(fluxo_de_um_comando());
    ate_parar(&id);
    let v = viva(&id).unwrap();
    cancelar(&amb.state, &id).unwrap();
    let filho = Command::new("sleep").arg("60").stdin(Stdio::piped()).spawn().unwrap();
    let mut processo = guardar_processo(&v, &id, "c", filho);
    ate_sair(&mut processo.filho);
    let erro = enviar_prompt(&v, &mut processo.filho, "oi").unwrap_err().to_string();
    assert_eq!(erro, PAUSADO);
    drop(processo);
    assert!(!com_chave(&format!("{id}/c")), "pid morto não fica para o próximo Parar");
}

/// Sem Parar nenhum, o erro de stdin é erro de verdade.
#[test]
fn stdin_fechado_sem_parar_continua_erro() {
    let amb = ambiente();
    let id = amb.iniciar(com_aprovacao("a", "A"));
    com_status(&id, "aguardando");
    let v = viva(&id).unwrap();
    let mut processo = guardar_processo(&v, &id, "c", Command::new("true").stdin(Stdio::piped()).spawn().unwrap());
    ate_sair(&mut processo.filho);
    assert_ne!(enviar_prompt(&v, &mut processo.filho, "oi").unwrap_err().to_string(), PAUSADO);
}

/// Saída antecipada da etapa (`?` no meio): o processo não fica solto nem a chave para trás.
#[test]
fn saida_antecipada_derruba_o_processo_e_tira_a_chave() {
    let amb = ambiente();
    let id = amb.iniciar(com_aprovacao("a", "A"));
    com_status(&id, "aguardando");
    let processo = guardar_processo(&viva(&id).unwrap(), &id, "c", Command::new("sleep").arg("60").spawn().unwrap());
    let pid = processo.filho.id().to_string();
    assert!(com_chave(&format!("{id}/c")));
    drop(processo);
    assert!(!com_chave(&format!("{id}/c")));
    assert!(!processo_vivo(&pid));
}
