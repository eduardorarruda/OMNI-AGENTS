import { describe, expect, it } from "vitest";
import { filtrarGrafo, vizinhas, type Grafo } from "../features/grafo/grafoModel";

const nota = (slug: string, projeto = "") => ({ slug, titulo: slug, resumo: `resumo de ${slug}`, tags: [], projeto });
const grafo: Grafo = {
  notas: [nota("a", "api"), nota("b", "api"), nota("c", "site")],
  ligacoes: [
    { de: "a", para: "b", tipo: "wikilink", peso: 1 },
    { de: "b", para: "c", tipo: "tag", peso: 0.4 },
    { de: "c", para: "a", tipo: "texto", peso: 0.2 },
  ],
};
const TODOS = ["wikilink", "conversa", "tag", "texto"] as const;

describe("modelo do grafo", () => {
  it("filtro por tipo remove ligações e mantém notas", () => {
    const filtrado = filtrarGrafo(grafo, { projeto: null, tipos: ["wikilink"] });
    expect(filtrado.ligacoes.map((l) => l.tipo)).toEqual(["wikilink"]);
    expect(filtrado.notas).toHaveLength(3);
  });

  it("filtro por projeto tira ligações órfãs", () => {
    const filtrado = filtrarGrafo(grafo, { projeto: "api", tipos: [...TODOS] });
    expect(filtrado.notas.map((n) => n.slug)).toEqual(["a", "b"]);
    expect(filtrado.ligacoes).toEqual([grafo.ligacoes[0]]);
  });

  it("vizinhas inclui os dois sentidos", () => {
    expect([...vizinhas(grafo, "a")].sort()).toEqual(["b", "c"]);
    expect([...vizinhas(grafo, "b")].sort()).toEqual(["a", "c"]);
    expect(vizinhas(grafo, "inexistente").size).toBe(0);
  });
});
