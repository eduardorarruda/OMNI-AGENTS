//! Limites do executor com comandos de verdade no shell: retomada, teto de ramos, custo e volta seletiva.
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
    assert_eq!(abertos_no_pico(&id, &amb), 3, "sem o campo, o teto é 3");
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

fn com_custo(custo: f64, maximo: f64) -> Execucao {
    let mut e: Execucao = serde_json::from_value(json!({"id": "x", "projeto": "p", "projeto_id": "p", "entrada": "x", "pasta": ".", "status": "rodando", "criado_ms": 0, "atualizado_ms": 0,
        "fluxo": paralelo(&[("a", conta("a")), ("b", conta("b"))]), "nos": {"a": {"status": "pendente"}}, "fila": [{"no": "a", "de": "e"}], "custo_usd": custo})).unwrap();
    e.fluxo.custo_maximo = Some(maximo);
    e
}

#[test]
fn custo_que_chega_no_limite_pausa_antes_da_proxima_etapa_e_explica() {
    let mut abaixo = com_custo(0.99, 1.0);
    assert!(proximo_lote(&mut abaixo).is_some());
    let mut chegou = com_custo(1.0, 1.0);
    assert!(proximo_lote(&mut chegou).is_none());
    assert_eq!((chegou.status.as_str(), chegou.parou_por_custo, chegou.fila.len()), ("pausado", true, 1));
    let erro = chegou.erro.unwrap();
    assert!(erro.contains("US$ 1.00") && erro.contains("só é conhecido quando ela termina"), "{erro}");
    // Etapa ainda rodando pode custar mais: espera ela acabar, sem abrir outra.
    let mut outro_ramo = com_custo(2.0, 1.0);
    outro_ramo.nos.insert("b".into(), EstadoDoNo { status: "rodando".into(), ..Default::default() });
    assert!(proximo_lote(&mut outro_ramo).is_none());
    assert_eq!(outro_ramo.status, "rodando");
}

/// Retomar é aceitar o gasto: libera mais um `custo_maximo`, sem pausar de novo na mesma hora.
#[test]
fn retomar_depois_do_limite_vale_mais_um_limite() {
    let amb = ambiente();
    let mut fluxo = paralelo(&[("a", conta("a")), ("b", conta("b"))]);
    fluxo["custo_maximo"] = json!(0.5);
    let id = amb.iniciar(fluxo);
    assert_eq!(ate_parar(&id).status, "concluido", "comando não custa nada");
    let v = viva(&id).unwrap();
    mexer(&amb.state, &v, |e| { e.status = "rodando".into(); e.custo_usd = 0.6; e.fila.push(Passo { no: "f".into(), de: Some("depois".into()) }); });
    soltar(amb.state.clone(), id.clone());
    let pausada = ate_parar(&id);
    assert_eq!((pausada.status.as_str(), pausada.aviso.as_deref()), ("pausado", Some("custo")));
    retomar(&amb.state, &id).unwrap();
    let e = ate_parar(&id);
    assert_eq!((e.status.as_str(), e.parou_por_custo, e.erro), ("concluido", false, None));
    assert!((e.custo_aceito - 0.6).abs() < f64::EPSILON);
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
    let e = com_custo(0.0, 1.0);
    let mut e = Execucao { nos: HashMap::from([("cond".to_owned(), EstadoDoNo { saida: r#"{"veredito":"reprovado","refazer":["j"]}"#.into(), ..Default::default() })]), ..e };
    e.fluxo = serde_json::from_value(com_revisor(json!(["a", "b"]), ramos_simples(&["a", "b"]).0, ramos_simples(&["a", "b"]).1)).unwrap();
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
    fluxo.pastas_extras = vec!["..".into()];
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
