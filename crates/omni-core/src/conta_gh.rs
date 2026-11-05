//! Conta do GitHub por workspace: só o **nome** da conta viaja e é gravado.

/// Variável da sessão com o nome da conta. É o engine que troca o nome pelo token, na hora de abrir.
pub const VARIAVEL_DA_CONTA: &str = "OMNI_GH_CONTA";
/// Maior login que o GitHub aceita.
const TAMANHO_MAXIMO: usize = 39;

/// Letras, números e hífen. Começar por hífen é recusado: o nome vira argumento do `gh`.
pub fn nome_valido(conta: &str) -> bool {
    conta.len() <= TAMANHO_MAXIMO
        && conta.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && conta.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aceita_login_do_github() {
        for conta in ["conta-a", "conta-b", "Conta2", "a"] {
            assert!(nome_valido(conta), "{conta}");
        }
    }

    #[test]
    fn recusa_o_que_poderia_virar_opcao_ou_comando() {
        for conta in ["", "-conta", "--user", "conta a", "conta;ls", "$(id)", "conta/a", "conta\n", "contá"] {
            assert!(!nome_valido(conta), "{conta:?}");
        }
        assert!(!nome_valido(&"a".repeat(TAMANHO_MAXIMO + 1)));
        assert!(nome_valido(&"a".repeat(TAMANHO_MAXIMO)));
    }
}
