import { writeTerminal } from "../terminal/terminalService";

/** Pergunta se alguma tela de conversa cuida do texto: quem cuida chama `preventDefault`. */
export const EVENTO_TEXTO_DA_SESSAO = "omni:texto-da-sessao";
/** O terminal da sessão recebeu a colagem: é nele que a pessoa aperta Enter. */
export const EVENTO_FOCAR_TERMINAL = "omni:focar-terminal";

export interface TextoDaSessao {
  sessionId: string;
  texto: string;
}

/**
 * Bracketed paste, sem `\r`: a CLI recebe como colagem e fica esperando o Enter da pessoa. Controle
 * vira espaço porque o texto pode vir de terceiros: um `ESC[201~` no meio encerraria a colagem, e o
 * que viesse depois valeria como tecla.
 */
export function colagemEntreAspas(texto: string): string {
  // eslint-disable-next-line no-control-regex
  return `\x1b[200~${texto.replace(/[\u0000-\u001f\u007f-\u009f]/g, " ")}\x1b[201~`;
}

/**
 * Deixa o texto no campo da sessão, sem enviar. No modo Conversa o campo é o da tela (o terminal fica
 * escondido por baixo, e a pessoa não veria nada). Sem conversa na tela, vai para a CLI.
 */
export async function colarNaSessao(sessionId: string, texto: string): Promise<void> {
  const pedido = new CustomEvent<TextoDaSessao>(EVENTO_TEXTO_DA_SESSAO, { cancelable: true, detail: { sessionId, texto } });
  if (!window.dispatchEvent(pedido)) return;
  await writeTerminal(sessionId, colagemEntreAspas(texto));
  window.dispatchEvent(new CustomEvent<string>(EVENTO_FOCAR_TERMINAL, { detail: sessionId }));
}
