import type { TerminalSession } from "../terminal/terminalService";
import type { Project } from "../../types/workspace";
import { collectResourceIds } from "./workspaceReducer";

const PARADA = new Set<TerminalSession["state"]>(["stopped", "orphan", "crashed"]);

export function perguntaDosAgentes(quantos: number): string {
  return `Há ${quantos} agente${quantos === 1 ? "" : "s"} rodando em segundo plano neste projeto. Encerrar junto?`;
}

/**
 * Fechar o projeto (ou apagar o workspace) encerra as sessões das abas dele, como sempre. Desde que
 * fechar a aba deixou de encerrar o agente, pode haver agente vivo do projeto sem aba nenhuma: a
 * lateral só mostra o projeto ativo e o id do projeto não volta ao reabrir a pasta, então ele
 * ficaria rodando sem dono. Com agente assim, pergunta uma vez: aceitar encerra tudo, recusar não
 * fecha nada.
 *
 * Devolve os ids a encerrar, ou `null` quando a pessoa cancelou.
 */
export function sessoesAoFechar(
  fechando: readonly Project[],
  outros: readonly Project[],
  sessoes: readonly TerminalSession[],
  confirmar: (pergunta: string) => boolean,
): string[] | null {
  const comAba = fechando.flatMap((projeto) => collectResourceIds(projeto.layout));
  const ids = new Set(fechando.map((projeto) => projeto.id));
  // Sessão aberta numa aba de outro projeto continua tendo dono.
  const abertasEmOutro = new Set([...comAba, ...outros.flatMap((projeto) => collectResourceIds(projeto.layout))]);
  const soltas = sessoes.filter((sessao) => ids.has(sessao.project_id) && !abertasEmOutro.has(sessao.id));
  const vivas = soltas.filter((sessao) => sessao.provider && !PARADA.has(sessao.state));
  if (vivas.length === 0) return comAba;
  if (!confirmar(perguntaDosAgentes(vivas.length))) return null;
  return [...comAba, ...soltas.map((sessao) => sessao.id)];
}
