import { useSyncExternalStore } from "react";

/** Aviso do Kanban (engine fora do ar, gravação desfeita). Fica num cantinho de módulo para o quadro
 * mostrar sem passar propriedade por App e WorkspaceView. */
let aviso: string | null = null;
const ouvintes = new Set<() => void>();

export function avisoDoKanban(): string | null {
  return aviso;
}

export function definirAvisoDoKanban(mensagem: string | null): void {
  if (aviso === mensagem) return;
  aviso = mensagem;
  ouvintes.forEach((ouvinte) => ouvinte());
}

export function limparAvisoDoKanban(): void {
  definirAvisoDoKanban(null);
}

export function useAvisoDoKanban(): string | null {
  return useSyncExternalStore(
    (ouvinte) => {
      ouvintes.add(ouvinte);
      return () => ouvintes.delete(ouvinte);
    },
    avisoDoKanban,
    avisoDoKanban,
  );
}
