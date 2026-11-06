export const POLL_ATIVO_MS = 16;
export const POLL_OCIOSO_MS = 100;
export const POLL_FALHANDO_MS = 1_000;
/** Atrás do chat ninguém vê o terminal: cada leitura a mais é só IPC disputando a thread da tela. */
export const POLL_OCULTO_MS = 500;
export const POLL_ATIVO_LEVE_MS = 50;
export const POLL_OCIOSO_LEVE_MS = 300;
export const POLL_OCULTO_LEVE_MS = 1_000;

interface Situacao {
  falhando: boolean;
  oculto: boolean;
  ativo: boolean;
  leve: boolean;
}

/** Intervalo até a próxima leitura do terminal. */
export function intervaloDaLeitura({ falhando, oculto, ativo, leve }: Situacao) {
  if (falhando) return POLL_FALHANDO_MS;
  if (oculto) return leve ? POLL_OCULTO_LEVE_MS : POLL_OCULTO_MS;
  if (ativo) return leve ? POLL_ATIVO_LEVE_MS : POLL_ATIVO_MS;
  return leve ? POLL_OCIOSO_LEVE_MS : POLL_OCIOSO_MS;
}
