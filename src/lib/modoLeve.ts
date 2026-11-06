/**
 * Modo leve: para máquina fraca. Corta efeitos e, principalmente, diminui as consultas ao engine.
 * O valor mora fora do React porque os laços de polling leem o ritmo sem re-renderizar; a tela
 * acompanha por `useModoLeve`.
 */
import { useSyncExternalStore } from "react";

let ligado = false;
const ouvintes = new Set<() => void>();

/** Chamado por useSettings durante o render: avisar ali mesmo seria setState em outro componente. */
export function definirModoLeve(valor: boolean) {
  if (valor === ligado) return;
  ligado = valor;
  queueMicrotask(() => ouvintes.forEach((avisar) => avisar()));
}

export function modoLeveLigado() {
  return ligado;
}

export function ritmo(normalMs: number, leveMs: number) {
  return ligado ? leveMs : normalMs;
}

function assinar(avisar: () => void) {
  ouvintes.add(avisar);
  return () => { ouvintes.delete(avisar); };
}

export function useModoLeve() {
  return useSyncExternalStore(assinar, modoLeveLigado);
}
