export type TipoDeLigacao = "wikilink" | "conversa" | "tag" | "texto";

export interface NotaDoGrafo {
  slug: string;
  titulo: string;
  resumo: string;
  tags: string[];
  projeto: string;
}

export interface Ligacao {
  de: string;
  para: string;
  tipo: TipoDeLigacao;
  peso: number;
}

export interface Grafo {
  notas: NotaDoGrafo[];
  ligacoes: Ligacao[];
}

export interface FiltroDoGrafo {
  projeto: string | null;
  tipos: TipoDeLigacao[];
}

export const TIPOS_DE_LIGACAO: { tipo: TipoDeLigacao; rotulo: string }[] = [
  { tipo: "wikilink", rotulo: "Citação [[nota]]" },
  { tipo: "conversa", rotulo: "Mesma conversa" },
  { tipo: "tag", rotulo: "Tag em comum" },
  { tipo: "texto", rotulo: "Texto parecido" },
];

export function filtrarGrafo(grafo: Grafo, filtro: FiltroDoGrafo): Grafo {
  const notas = filtro.projeto === null ? grafo.notas : grafo.notas.filter((nota) => nota.projeto === filtro.projeto);
  const slugs = new Set(notas.map((nota) => nota.slug));
  const ligacoes = grafo.ligacoes.filter((l) => filtro.tipos.includes(l.tipo) && slugs.has(l.de) && slugs.has(l.para));
  return { notas, ligacoes };
}

export function vizinhas(grafo: Grafo, slug: string): Set<string> {
  const achadas = new Set<string>();
  for (const l of grafo.ligacoes) {
    if (l.de === slug) achadas.add(l.para);
    else if (l.para === slug) achadas.add(l.de);
  }
  return achadas;
}

/** Mesmo projeto vira a mesma cor (e não uma ligação): matiz estável a partir do nome. */
export function matizDoProjeto(projeto: string): number {
  let soma = 0;
  for (const letra of projeto) soma = (soma * 31 + letra.charCodeAt(0)) % 360;
  return soma;
}

/** Quantas ligações cada nota tem (as duas pontas contam). Nota sem ligação aparece com 0. */
export function grausDasNotas(grafo: Grafo): Map<string, number> {
  const graus = new Map(grafo.notas.map((nota) => [nota.slug, 0]));
  for (const l of grafo.ligacoes) {
    graus.set(l.de, (graus.get(l.de) ?? 0) + 1);
    graus.set(l.para, (graus.get(l.para) ?? 0) + 1);
  }
  return graus;
}

const semAcento = (texto: string) => texto.normalize("NFD").replace(/\p{Diacritic}/gu, "").toLowerCase();

/** Slugs das notas cujo título contém o termo (sem acento, sem caixa); quem começa com ele vem antes. */
export function buscarNotas(grafo: Grafo, termo: string): string[] {
  const alvo = semAcento(termo.trim());
  if (!alvo) return [];
  const comeca: string[] = [], contem: string[] = [];
  for (const nota of grafo.notas) {
    const titulo = semAcento(nota.titulo);
    if (titulo.startsWith(alvo)) comeca.push(nota.slug);
    else if (titulo.includes(alvo)) contem.push(nota.slug);
  }
  return [...comeca, ...contem];
}

export function contarPorTipo(grafo: Grafo): Record<TipoDeLigacao, number> {
  const contagem: Record<TipoDeLigacao, number> = { wikilink: 0, conversa: 0, tag: 0, texto: 0 };
  for (const l of grafo.ligacoes) contagem[l.tipo] += 1;
  return contagem;
}
