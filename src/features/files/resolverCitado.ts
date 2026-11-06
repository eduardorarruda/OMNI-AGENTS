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
    // Absoluto que não existe não vira busca: o usuário apontou um lugar exato.
    if (await deps.exists(direto)) return { tipo: "um", caminho: direto };
    return { tipo: "nenhum", nome, raiz };
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
  if (achados.length === 0) return { tipo: "nenhum", nome, raiz };
  // Citação com pastas que nenhum achado confirma nunca abre sozinha: o usuário escolhe.
  if (casados.length === 0) return { tipo: "varios", caminhos: achados };
  if (casados.length === 1) return { tipo: "um", caminho: casados[0] };
  return { tipo: "varios", caminhos: casados };
}

/** Texto do aviso quando nada foi achado: só o último nome da pasta, com `/`, `\` ou barra final. */
export function mensagemNaoAchei(nome: string, raiz: string): string {
  const pasta = raiz.split(PARTIR).filter(Boolean).pop() ?? raiz;
  return `Não achei ${nome} em ${pasta}`;
}
