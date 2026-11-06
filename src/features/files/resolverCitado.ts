import { joinPath } from "./filesService";

export interface DepsResolverCitado {
  exists(caminho: string): Promise<boolean>;
  homeDir(): Promise<string>;
  localizar(raiz: string, nome: string): Promise<string[]>;
}

export type CitadoResolvido =
  | { tipo: "um"; caminho: string }
  | { tipo: "varios"; caminhos: string[] }
  | { tipo: "nenhum"; nome: string; raiz: string };

const ABSOLUTO = /^([A-Za-z]:)?[\\/]/;
const PARTIR = /[\\/]+/;

/**
 * Acha o arquivo que o agente citou: absoluto/`~` como está; relativo à pasta da conversa e à raiz
 * do projeto (`bases`, nessa ordem); por fim busca pelo nome na última base (a raiz). Se a citação
 * traz pastas (`plans/x.md`), ficam os achados que terminam nesse trecho; sem nenhum, vale o nome.
 */
export async function resolverCitado(citado: string, bases: string[], deps: DepsResolverCitado): Promise<CitadoResolvido> {
  const limpo = citado.replace(/^\.[\\/]/, "");
  const partes = limpo.split(PARTIR).filter(Boolean);
  const nome = partes[partes.length - 1] ?? limpo;
  const raiz = bases[bases.length - 1] ?? "";

  const direto = /^~[\\/]/.test(limpo) ? joinPath(await deps.homeDir(), limpo.slice(2)) : ABSOLUTO.test(limpo) ? limpo : null;
  if (direto !== null) {
    if (await deps.exists(direto)) return { tipo: "um", caminho: direto };
  } else {
    for (const base of bases) {
      const candidato = joinPath(base, limpo);
      if (await deps.exists(candidato)) return { tipo: "um", caminho: candidato };
    }
  }
  if (!raiz) return { tipo: "nenhum", nome, raiz };

  const achados = [...new Set(await deps.localizar(raiz, nome))];
  const terminaEm = partes.join("/");
  const casados = partes.length > 1
    ? achados.filter((a) => a.split(PARTIR).filter(Boolean).join("/").endsWith(`/${terminaEm}`))
    : achados;
  const finais = casados.length > 0 ? casados : achados;
  if (finais.length === 1) return { tipo: "um", caminho: finais[0] };
  if (finais.length > 1) return { tipo: "varios", caminhos: finais };
  return { tipo: "nenhum", nome, raiz };
}
