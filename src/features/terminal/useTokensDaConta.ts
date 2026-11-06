import { useEffect, useState } from "react";
import { engineApi } from "./terminalService";
import type { TokensDaConta } from "./DetalheDaConta";

/**
 * Tokens da conta, lidos do engine só enquanto o detalhe está aberto (nenhum temporizador).
 * `undefined` = lendo; `null` = a leitura falhou (a tela mostra "não disponível").
 */
export function useTokensDaConta(profileId: string | null, ativo: boolean): TokensDaConta | null | undefined {
  const [tokens, setTokens] = useState<TokensDaConta | null | undefined>(undefined);
  useEffect(() => {
    setTokens(undefined);
    if (!ativo || !profileId) return;
    let cancelado = false;
    engineApi("GET", `/contas/${encodeURIComponent(profileId)}/tokens`, null)
      .then((resposta) => {
        if (cancelado) return;
        setTokens(resposta.status >= 200 && resposta.status < 300 ? (JSON.parse(resposta.body) as TokensDaConta) : null);
      })
      .catch(() => { if (!cancelado) setTokens(null); });
    return () => { cancelado = true; };
  }, [profileId, ativo]);
  return tokens;
}
