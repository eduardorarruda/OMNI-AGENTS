import { describe, expect, it } from "vitest";
import { corDoProjeto, enquadrar, escolherRotulos, raioDoNo, rgbDaCor } from "../features/grafo/grafoDesenho";

/** Contraste WCAG entre duas cores rgb. */
function contraste(a: [number, number, number], b: [number, number, number]): number {
  const lum = ([r, g, bl]: [number, number, number]) => {
    const canal = (c: number) => { const s = c / 255; return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4; };
    return 0.2126 * canal(r) + 0.7152 * canal(g) + 0.0722 * canal(bl);
  };
  const [x, y] = [lum(a), lum(b)].sort((p, q) => q - p);
  return (x + 0.05) / (y + 0.05);
}

const FUNDO_ESCURO: [number, number, number] = [14, 14, 20];
const FUNDO_CLARO: [number, number, number] = [249, 249, 251];
const PROJETOS = ["api", "site", "Core-4-ERP", "omni", "central_ebpos", "x", "nmon-next", "kanban", "a", "zz"];

describe("desenho do grafo", () => {
  it("nó cresce com as ligações, com teto", () => {
    expect(raioDoNo(0)).toBeGreaterThanOrEqual(4);
    expect(raioDoNo(5)).toBeGreaterThan(raioDoNo(1));
    expect(raioDoNo(10_000)).toBeLessThanOrEqual(22);
  });

  it("cor do projeto é estável e lê bem nos dois temas", () => {
    expect(corDoProjeto("api", "escuro")).toBe(corDoProjeto("api", "escuro"));
    for (const p of PROJETOS) {
      expect(contraste(rgbDaCor(corDoProjeto(p, "escuro")), FUNDO_ESCURO)).toBeGreaterThanOrEqual(3);
      expect(contraste(rgbDaCor(corDoProjeto(p, "claro")), FUNDO_CLARO)).toBeGreaterThanOrEqual(3);
    }
    const distintas = new Set(PROJETOS.map((p) => corDoProjeto(p, "escuro")));
    expect(distintas.size).toBeGreaterThanOrEqual(5);
  });

  it("enquadrar põe todos os nós dentro da tela", () => {
    const camera = enquadrar([{ x: -100, y: -50, r: 5 }, { x: 300, y: 150, r: 5 }], 800, 400);
    for (const p of [{ x: -100, y: -50 }, { x: 300, y: 150 }]) {
      const sx = p.x * camera.k + camera.x, sy = p.y * camera.k + camera.y;
      expect(sx).toBeGreaterThanOrEqual(0); expect(sx).toBeLessThanOrEqual(800);
      expect(sy).toBeGreaterThanOrEqual(0); expect(sy).toBeLessThanOrEqual(400);
    }
    expect(enquadrar([], 800, 400)).toEqual({ x: 400, y: 200, k: 1 });
  });

  it("rótulos não se sobrepõem e o de maior prioridade vence", () => {
    const escolhidos = escolherRotulos([
      { slug: "pequeno", x: 0, y: 0, largura: 80, altura: 14, prioridade: 1 },
      { slug: "grande", x: 10, y: 4, largura: 80, altura: 14, prioridade: 9 },
      { slug: "longe", x: 300, y: 300, largura: 80, altura: 14, prioridade: 0 },
    ]);
    expect([...escolhidos].sort()).toEqual(["grande", "longe"]);
  });
});

describe("paleta dos projetos", () => {
  it("projetos diferentes não repetem cor enquanto houver cor livre, e a ordem de entrada não importa", async () => {
    const { paletaDosProjetos, corDoProjeto: cor } = await import("../features/grafo/grafoDesenho");
    const nomes = ["OMNI-AGENTS", "Core-4-ERP", "central_ebpos", "nmon-next", "EBPOS_360"];
    const paleta = paletaDosProjetos(nomes);
    const cores = nomes.map((p) => cor(p, "escuro", paleta));
    expect(new Set(cores).size).toBe(nomes.length);
    const deTras = paletaDosProjetos([...nomes].reverse());
    expect(nomes.map((p) => cor(p, "escuro", deTras))).toEqual(cores);
  });
});
