import { matizDoProjeto } from "./grafoModel";

/** Funções puras do desenho do grafo: tamanho, cor, enquadramento e escolha de rótulos. */

export type Tema = "claro" | "escuro";

/** Câmera: tela = mundo × k + (x, y). */
export interface Camera {
  x: number;
  y: number;
  k: number;
}

export const ZOOM_MIN = 0.08;
export const ZOOM_MAX = 6;
const RAIO_BASE = 4;
const RAIO_MAX = 20;

/** Raio pela raiz do grau: quem tem muita ligação cresce, mas sem engolir a tela. */
export function raioDoNo(grau: number): number {
  return Math.min(RAIO_MAX, RAIO_BASE + Math.sqrt(Math.max(0, grau)) * 2.4);
}

/** Oito matizes bem afastadas entre si e do laranja de destaque do app (≈25°). */
const MATIZES = [212, 150, 42, 355, 272, 186, 95, 322];

/** Mesma nota-projeto vira sempre a mesma cor. A chave é o nome da pasta que vem do backend,
 *  não o nome exibido do projeto: renomear um projeto na lateral não troca a cor. */
export function corDoProjeto(projeto: string, tema: Tema, paleta?: ReadonlyMap<string, number>): string {
  const matiz = MATIZES[paleta?.get(projeto) ?? matizDoProjeto(projeto) % MATIZES.length];
  return tema === "claro" ? `hsl(${matiz} 72% 35%)` : `hsl(${matiz} 75% 66%)`;
}

/** Índice de cor de cada projeto: o hash diz a preferida; se outra já ocupou, pega a próxima livre.
 *  Ordem por nome deixa o resultado igual não importa a ordem das notas, e o filtro não mexe nas cores
 *  porque o painel calcula isto sobre o grafo inteiro. */
export function paletaDosProjetos(projetos: Iterable<string>): Map<string, number> {
  const paleta = new Map<string, number>();
  const usadas = new Set<number>();
  for (const projeto of [...new Set(projetos)].filter(Boolean).sort()) {
    let indice = matizDoProjeto(projeto) % MATIZES.length;
    for (let i = 0; i < MATIZES.length && usadas.has(indice); i++) indice = (indice + 1) % MATIZES.length;
    usadas.add(indice);
    if (usadas.size >= MATIZES.length) usadas.clear();
    paleta.set(projeto, indice);
  }
  return paleta;
}

/** rgb de uma cor `hsl(h s% l%)` — usado para pôr transparência sem reler o CSS. */
export function rgbDaCor(cor: string): [number, number, number] {
  const partes = /hsl\((\d+(?:\.\d+)?) (\d+)% (\d+)%\)/.exec(cor);
  if (!partes) return [128, 128, 128];
  const h = Number(partes[1]), s = Number(partes[2]) / 100, l = Number(partes[3]) / 100;
  const a = s * Math.min(l, 1 - l);
  const canal = (n: number) => {
    const k = (n + h / 30) % 12;
    return Math.round(255 * (l - a * Math.max(-1, Math.min(k - 3, 9 - k, 1))));
  };
  return [canal(0), canal(8), canal(4)];
}

/** Câmera que cabe todos os pontos na tela com folga; sem pontos, origem no meio. */
export function enquadrar(pontos: { x: number; y: number; r: number }[], largura: number, altura: number, folga = 40): Camera {
  if (pontos.length === 0) return { x: largura / 2, y: altura / 2, k: 1 };
  let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
  for (const p of pontos) {
    x0 = Math.min(x0, p.x - p.r); y0 = Math.min(y0, p.y - p.r);
    x1 = Math.max(x1, p.x + p.r); y1 = Math.max(y1, p.y + p.r);
  }
  const uteisX = Math.max(1, largura - folga * 2), uteisY = Math.max(1, altura - folga * 2);
  const k = Math.max(ZOOM_MIN, Math.min(1.6, uteisX / Math.max(1, x1 - x0), uteisY / Math.max(1, y1 - y0)));
  return { k, x: largura / 2 - ((x0 + x1) / 2) * k, y: altura / 2 - ((y0 + y1) / 2) * k };
}

export interface CandidatoARotulo {
  slug: string;
  /** Canto superior esquerdo da caixa do rótulo, em px de tela. */
  x: number;
  y: number;
  largura: number;
  altura: number;
  prioridade: number;
}

/** Guloso: do mais importante ao menos, aceita o rótulo que não encosta em nenhum já aceito. */
export function escolherRotulos(candidatos: CandidatoARotulo[], limite = 400): Set<string> {
  const ordem = [...candidatos].sort((a, b) => b.prioridade - a.prioridade);
  const aceitos: CandidatoARotulo[] = [];
  const folga = 2;
  for (const c of ordem) {
    if (aceitos.length >= limite) break;
    const colide = aceitos.some((o) => c.x < o.x + o.largura + folga && o.x < c.x + c.largura + folga
      && c.y < o.y + o.altura + folga && o.y < c.y + c.altura + folga);
    if (!colide) aceitos.push(c);
  }
  return new Set(aceitos.map((c) => c.slug));
}
