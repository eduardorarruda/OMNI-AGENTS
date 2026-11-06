//! Máscara de dados pessoais para o que sai pelo MCP. CPF, CNPJ, e-mail, telefone e valor têm formato;
//! nome não tem, então a regra é uma heurística: primeiro nome comum seguido de sobrenome. Ela deixa
//! passar nome fora da lista, nome sozinho e nome em minúsculas, e isso é aceito para não destruir
//! nome de tela como "Painel Comercial".
use crate::memoria::slug_de;
use regex::{Captures, Regex};
use std::{collections::HashSet, sync::OnceLock};

/// Já sem acento. Fora de propósito o que também é palavra comum: Rosa, Luz, Flor, Paz, Marco, Mercedes.
const PRIMEIROS_NOMES: &str = "
    adriana adriano afonso agnaldo agostinho aline alan alana alberto alessandra alessandro alex alexandra alexandre
    alice alisson allan almir altair alvaro amanda amelia ana anderson andre andrea andreia andressa angela
    angelica anita antonia antonio aparecida ariane arlete armando arnaldo arthur artur augusto aurea aurelio
    barbara beatriz benedito benjamin bento bernardo bianca breno brenda bruna bruno
    caio camila camile carina carla carlos carmem carmen carolina caroline cassia cassio catarina cecilia celia celso
    cesar charles cicero cintia clara clarice claudia claudio clayton cleber cleide cleiton conceicao cristiane cristiano
    cristina cynthia daiane daniel daniela danilo dante davi david debora denise dennis diana diego diogo dirce
    djalma domingos douglas dulce edilson edmilson edna edson eduarda eduardo elaine elen eliana eliane elias elisa
    elisabete elizabete eliseu elza emanuel emanuele emerson emilia enzo erica erick erika ester eunice eva evandro
    evelyn everton fabiana fabiano fabio fabricio fatima felipe fernanda fernando filipe flavia flavio francieli
    francisca francisco franciele gabriel gabriela gabriele geraldo gerson geovana giovana giovanna giovani giovanni
    gilberto gilmar gilson giselle gisele gislaine glaucia graziela gustavo guilherme helena helio heloisa henrique
    heitor hugo humberto iara icaro igor iracema irene iris isaac isabel isabela isabella isadora isaias ismael ivan
    ivana ivete ivone jacira jackson jadson jaime jair jaqueline janaina jane janete jeferson jefferson jennifer
    jessica joana joao joaquim joel joice jonas jonatas jonathan jorge jose josefa josiane josue joyce juarez julia
    juliana juliano julio junior jussara karen karina karine katia kelly kevin laercio laila lais larissa laura
    lauro lazaro leandro leila leonardo leticia levi lidia liliane lilian lindomar livia lorena lorenzo lourdes
    luana lucas lucia luciana luciano lucimar lucio luis luisa luiz luiza lurdes luzia lidiane magda maiara maicon
    maira manoel manuel manuela marcela marcelo marcia marcio marcos margarete margarida maria mariana marilene
    marina mario marisa marlene marli marlon marta martin martinho mateus matheus mauricio mauro mayara melissa
    michael michele michelle miguel milena milton miriam mirian moacir moises monica murilo nadia nair natalia
    natan nathalia nayara neide nelson neusa newton nicolas nicole nilson nilton nivaldo noemi norma odair olavo
    olivia orlando oscar osvaldo otavio pamela patricia paula paulo pedro poliana priscila priscilla rafael rafaela
    raimundo raissa ramon raquel raul rebeca regiane regina reginaldo reinaldo renan renata renato ricardo rita
    roberta roberto robson rodolfo rodrigo rogerio romario ronaldo rosana rosangela rosane rosilene rubens ruth
    sabrina salete samara samuel sandra sandro sara sarah sebastiana sebastiao selma sergio severino silvana silvia
    silvio simone sofia solange sonia sophia stefany stella sueli suely susana suzana tainara taina talita tania
    tatiana tatiane teresa tereza thais thaisa thales thayna thiago tiago tomas tulio ubiratan valdecir valdir
    valeria valmir valter vanda vanderlei vanessa vania vera vicente victor vilma vinicius virginia vitor vitoria
    viviane vivian wagner waldir walter wanda wanderley washington wellington wesley weslley william willian
    wilson yasmin yuri zelia zilda wanessa wanderson geovanna pamella jacqueline carol";
const PARTICULAS: [&str; 5] = ["de", "da", "do", "dos", "das"];
/// Palavra que abre nome de lugar: em "São Paulo Capital", "Paulo" não começa um nome de pessoa.
const ANTES_DE_LUGAR: [&str; 5] = ["sao", "santo", "santa", "san", "dom"];
const LUGARES: [&str; 5] = ["joao-pessoa", "paulo-afonso", "bento-goncalves", "francisco-beltrao", "vitoria-da-conquista"];
/// Vem com maiúscula depois de um nome em título e tela, mas não é sobrenome.
const NAO_E_SOBRENOME: &str = "
    painel tela sistema comercial financeiro admin administrador dev teste api app web login usuario cliente aluno
    professor professora coordenador coordenadora gerente diretor diretora especialista suporte tarefa projeto
    relatorio cadastro turma curso pedagogico trafego sem com para por nao sim sao";

fn lista(texto: &'static str) -> HashSet<&'static str> {
    texto.split_whitespace().collect()
}

fn primeiros_nomes() -> &'static HashSet<&'static str> {
    static NOMES: OnceLock<HashSet<&str>> = OnceLock::new();
    NOMES.get_or_init(|| lista(PRIMEIROS_NOMES))
}

fn nao_sobrenomes() -> &'static HashSet<&'static str> {
    static PALAVRAS: OnceLock<HashSet<&str>> = OnceLock::new();
    PALAVRAS.get_or_init(|| lista(NAO_E_SOBRENOME))
}

fn digito_verificador(digitos: &[u32], pesos: impl Iterator<Item = u32>) -> u32 {
    let resto = digitos.iter().zip(pesos).map(|(digito, peso)| digito * peso).sum::<u32>() % 11;
    if resto < 2 { 0 } else { 11 - resto }
}

fn cpf_valido(digitos: &[u32]) -> bool {
    digitos[9] == digito_verificador(&digitos[..9], (2..=10).rev()) && digitos[10] == digito_verificador(&digitos[..10], (2..=11).rev())
}

fn cnpj_valido(digitos: &[u32]) -> bool {
    let pesos = |tamanho: usize| (0..tamanho).rev().map(|posicao| (posicao as u32 % 8) + 2);
    digitos[12] == digito_verificador(&digitos[..12], pesos(12)) && digitos[13] == digito_verificador(&digitos[..13], pesos(13))
}

fn celular(digitos: &[u32]) -> bool {
    digitos[0] != 0 && digitos[1] != 0 && digitos[2] == 9
}

/// Número sem pontuação pode ser id ou carimbo de tempo: só vira dado pessoal se a conta fechar.
fn tag_do_numero(numero: &str) -> Option<&'static str> {
    let digitos: Vec<u32> = numero.chars().filter_map(|letra| letra.to_digit(10)).collect();
    if digitos.iter().all(|digito| *digito == digitos[0]) { return None; }
    match digitos.len() {
        11 if cpf_valido(&digitos) => Some("[cpf]"),
        11 if celular(&digitos) => Some("[telefone]"),
        13 if digitos[..2] == [5, 5] && celular(&digitos[2..]) => Some("[telefone]"),
        14 if cnpj_valido(&digitos) => Some("[cnpj]"),
        _ => None,
    }
}

fn sem_numeros_soltos(texto: &str) -> String {
    static SOLTO: OnceLock<Regex> = OnceLock::new();
    let solto = SOLTO.get_or_init(|| Regex::new(r"\b(?:\d{9}-\d{2}|\d{11}|\d{13,14})\b").unwrap());
    solto.replace_all(texto, |achado: &Captures| tag_do_numero(&achado[0]).map_or_else(|| achado[0].to_owned(), str::to_owned)).into_owned()
}

fn sem_emails(texto: &str) -> String {
    static EMAIL: OnceLock<Regex> = OnceLock::new();
    let email = EMAIL.get_or_init(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b").unwrap());
    // `git@github.com` é endereço de repositório, não de alguém.
    email.replace_all(texto, |achado: &Captures| if achado[0].starts_with("git@") { achado[0].to_owned() } else { "[email]".to_owned() }).into_owned()
}

fn regras_de_formato() -> &'static [(Regex, &'static str)] {
    static REGRAS: OnceLock<Vec<(Regex, &str)>> = OnceLock::new();
    REGRAS.get_or_init(|| vec![
        (Regex::new(r"\b\d{2}\.\d{3}\.\d{3}/\d{4}-\d{2}\b").unwrap(), "[cnpj]"),
        (Regex::new(r"\b\d{3}\.\d{3}\.\d{3}-\d{2}\b").unwrap(), "[cpf]"),
        (Regex::new(r"(?x)
            \+55[\s.-]?\(?\d{2}\)?[\s.-]?9?\d{4}[\s.-]?\d{4}\b
            | \(\d{2}\)[\s.-]?9?\d{4}[\s.-]?\d{4}\b
            | \b[1-9]\d[\s.-]?(?:9\d{4}|[3-5]\d{3})-\d{4}\b
            | \b9\d{4}-\d{4}\b").unwrap(), "[telefone]"),
        (Regex::new(r"(?xi)
            (?:\bR\$|\bBRL\b)\s*-?\s*\d[\d.,]*(?:\s*(?:mil|mi|bi|milh(?:ão|ao|ões|oes)|bilh(?:ão|ao|ões|oes))\b)?
            | \b\d[\d.]*(?:,\d{1,2})?\s*reais\b
            | \b\d{1,3}(?:\.\d{3})+,\d{2}\b").unwrap(), "[valor]"),
    ])
}

fn resto_minusculo_ou_tudo_maiusculo(palavra: &str) -> bool {
    let mut letras = palavra.chars();
    let inicial = letras.next().is_some_and(char::is_uppercase);
    let resto = letras.as_str();
    inicial && (resto.chars().all(|letra| !letra.is_uppercase()) || resto.chars().all(|letra| !letra.is_lowercase()))
}

fn parece_primeiro_nome(palavra: &str) -> bool {
    resto_minusculo_ou_tudo_maiusculo(palavra) && primeiros_nomes().contains(slug_de(palavra).as_str())
}

fn caixa_alta(palavra: &str) -> bool {
    palavra.chars().all(|letra| !letra.is_lowercase())
}

/// A caixa acompanha a do primeiro nome: em "Maria FROM Silva", FROM é SQL, não sobrenome.
fn parece_sobrenome(palavra: &str, primeiro: &str) -> bool {
    resto_minusculo_ou_tudo_maiusculo(palavra) && palavra.chars().count() >= 2 && caixa_alta(palavra) == caixa_alta(primeiro)
        && !nao_sobrenomes().contains(slug_de(palavra).as_str())
}

/// Uma quebra de linha ou pontuação separa dois assuntos: "Maria\nSilva" e "Maria, Silva" não são um nome.
fn so_espaco_entre(texto: &str, palavras: &[regex::Match], indice: usize) -> bool {
    texto[palavras[indice].end()..palavras[indice + 1].start()].chars().all(|letra| letra == ' ' || letra == '\t')
}

/// Índice da última palavra do nome que começa em `inicio`, se começar um ali.
fn fim_do_nome(texto: &str, palavras: &[regex::Match], inicio: usize) -> Option<usize> {
    if !parece_primeiro_nome(palavras[inicio].as_str()) { return None; }
    if inicio > 0 && ANTES_DE_LUGAR.contains(&slug_de(palavras[inicio - 1].as_str()).as_str()) { return None; }
    let (mut atual, mut fim) = (inicio, None);
    while atual + 1 < palavras.len() && so_espaco_entre(texto, palavras, atual) {
        let particula = PARTICULAS.contains(&palavras[atual + 1].as_str().to_lowercase().as_str()) && atual + 2 < palavras.len() && so_espaco_entre(texto, palavras, atual + 1);
        let salto = if particula { 2 } else { 1 };
        if !parece_sobrenome(palavras[atual + salto].as_str(), palavras[inicio].as_str()) { break; }
        atual += salto;
        fim = Some(atual);
    }
    let fim = fim?;
    (!LUGARES.contains(&slug_de(&texto[palavras[inicio].start()..palavras[fim].end()]).as_str())).then_some(fim)
}

fn sem_nomes(texto: &str) -> String {
    static PALAVRA: OnceLock<Regex> = OnceLock::new();
    let palavras: Vec<regex::Match> = PALAVRA.get_or_init(|| Regex::new(r"\p{L}[\p{L}'’]*").unwrap()).find_iter(texto).collect();
    let (mut saida, mut copiado, mut indice) = (String::with_capacity(texto.len()), 0, 0);
    while indice < palavras.len() {
        let Some(fim) = fim_do_nome(texto, &palavras, indice) else { indice += 1; continue };
        saida.push_str(&texto[copiado..palavras[indice].start()]);
        saida.push_str("[nome]");
        copiado = palavras[fim].end();
        indice = fim + 1;
    }
    saida.push_str(&texto[copiado..]);
    saida
}

pub fn mascarar(texto: &str) -> String {
    let formatado = regras_de_formato().iter().fold(sem_emails(texto), |atual, (padrao, tag)| padrao.replace_all(&atual, *tag).into_owned());
    sem_nomes(&sem_numeros_soltos(&formatado))
}
