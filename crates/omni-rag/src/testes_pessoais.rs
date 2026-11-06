//! Os dados de teste são inventados; o CPF e o CNPJ só fecham a conta dos dígitos.
use crate::sem_dados_pessoais as mascara;

#[test]
fn documentos_formatados_e_soltos_viram_tag() {
    assert_eq!(mascara("CPF 529.982.247-25 do aluno"), "CPF [cpf] do aluno");
    assert_eq!(mascara("cpf: 52998224725."), "cpf: [cpf].");
    assert_eq!(mascara("doc 529982247-25"), "doc [cpf]");
    assert_eq!(mascara("CNPJ 11.222.333/0001-81"), "CNPJ [cnpj]");
    assert_eq!(mascara("cnpj 11222333000181"), "cnpj [cnpj]");
}

#[test]
fn email_vira_tag_e_endereco_de_repositorio_fica() {
    assert_eq!(mascara("fale com fulano.tal+x@empresa.com.br hoje"), "fale com [email] hoje");
    assert_eq!(mascara("git clone git@github.com:org/repo.git"), "git clone git@github.com:org/repo.git");
    assert_eq!(mascara("instale react@18.3.1 e @types/node"), "instale react@18.3.1 e @types/node");
}

#[test]
fn telefones_brasileiros_viram_tag() {
    for telefone in ["(11) 98765-4321", "(11)3456-7890", "+55 11 98765-4321", "+5511987654321", "11 98765-4321", "98765-4321", "11987654321", "5511987654321"] {
        assert_eq!(mascara(&format!("ligar {telefone} agora")), "ligar [telefone] agora", "{telefone}");
    }
}

#[test]
fn valores_em_reais_viram_tag() {
    for valor in ["R$ 1.500,00", "R$1500", "r$ 12,50", "R$ 2 milhões", "R$ 1,5 mil", "1.500,00", "350 reais", "BRL 90,00"] {
        assert_eq!(mascara(&format!("custo {valor} no mês")), "custo [valor] no mês", "{valor}");
    }
}

#[test]
fn nome_comum_com_sobrenome_vira_tag() {
    assert_eq!(mascara("Falar com Maria Silva amanhã"), "Falar com [nome] amanhã");
    assert_eq!(mascara("Cliente João Pedro de Oliveira Santos pagou"), "Cliente [nome] pagou");
    assert_eq!(mascara("JOSÉ DA SILVA"), "[nome]");
    assert_eq!(mascara("**Ana Paula**, Lucas Souza e Bruna Lima"), "**[nome]**, [nome] e [nome]");
}

#[test]
fn nome_de_tela_e_lugar_nao_sao_pessoas() {
    for texto in ["Painel Comercial", "Central Financeiro", "Maria Comercial", "São Paulo Capital", "João Pessoa", "Vitória da Conquista", "Visual Studio Code", "Eduardo Painel Comercial"] {
        assert_eq!(mascara(texto), texto);
    }
}

#[test]
fn texto_tecnico_passa_intacto() {
    for texto in [
        "2026-11-05T10:20:30.123Z", "1793915262827", "1793915262", "550e8400-e29b-41d4-a716-446655440000", "192.168.0.10:8080",
        "versão 1.234.567", "porta 3000-3010", "turma 2024-2025", "commit a1b2c3d4e5f6", "SELECT Maria FROM Silva_tabela", "id=12345678901",
        "const nomeCompleto = pessoa.nome", "use JavaScript e TypeScript no Next.js", "Claude Code e Docker Compose", "11111111111", "99999999999999",
    ] {
        assert_eq!(mascara(texto), texto);
    }
}

#[test]
fn quebra_de_linha_e_pontuacao_separam_nomes() {
    assert_eq!(mascara("Maria\nSilva"), "Maria\nSilva");
    assert_eq!(mascara("Maria, Silva"), "Maria, Silva");
    assert_eq!(mascara("Maria de férias"), "Maria de férias");
}

#[test]
fn limite_conhecido_da_heuristica_de_nomes() {
    assert_eq!(mascara("a maria silva"), "a maria silva");
    assert_eq!(mascara("Maria pediu"), "Maria pediu");
    assert_eq!(mascara("Zuleide Tavares"), "Zuleide Tavares");
    assert_eq!(mascara("Maria SILVA"), "Maria SILVA");
}

#[test]
fn mascarar_de_novo_nao_muda_nada() {
    let uma_vez = mascara("Maria Silva, 529.982.247-25, R$ 10,00, a@b.com");
    assert_eq!(mascara(&uma_vez), uma_vez);
}
