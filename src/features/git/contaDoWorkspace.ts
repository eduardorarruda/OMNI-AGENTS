import { createContext, useContext } from "react";

/** Conta do GitHub do workspace ativo. `conta: null` = a conta ativa do `gh`. Sem `escolher`, o
 *  rodapé só mostra a conta. */
export interface ContaDoWorkspace {
  conta: string | null;
  escolher?: (conta: string | null) => void;
}

export const ContaDoWorkspaceContext = createContext<ContaDoWorkspace>({ conta: null });

export function useContaDoWorkspace() {
  return useContext(ContaDoWorkspaceContext);
}
