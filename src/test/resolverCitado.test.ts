import { describe, expect, test, vi } from "vitest";
import { resolverCitado } from "../features/files/resolverCitado";

function deps(o: { achados?: string[]; existem?: string[] } = {}) {
  return {
    exists: vi.fn(async (p: string) => (o.existem ?? []).includes(p)),
    homeDir: vi.fn(async () => "/home/u"),
    localizar: vi.fn(async () => o.achados ?? []),
  };
}

describe("resolverCitado", () => {
  test("nome solto acha o arquivo dentro de subpasta", async () => {
    const r = await resolverCitado("analise-academico.md", ["/p"], deps({ achados: ["/p/docs/analise-360/analise-academico.md"] }));
    expect(r).toEqual({ tipo: "um", caminho: "/p/docs/analise-360/analise-academico.md" });
  });
  test("caminho parcial acha o arquivo pelo fim do caminho", async () => {
    const r = await resolverCitado("plans/x.md", ["/p"], deps({ achados: ["/p/docs/superpowers/plans/x.md", "/p/outro/x.md"] }));
    expect(r).toEqual({ tipo: "um", caminho: "/p/docs/superpowers/plans/x.md" });
  });
  test("mesmo nome em duas pastas devolve a lista", async () => {
    const r = await resolverCitado("x.md", ["/p"], deps({ achados: ["/p/a/x.md", "/p/b/x.md"] }));
    expect(r).toEqual({ tipo: "varios", caminhos: ["/p/a/x.md", "/p/b/x.md"] });
  });
  test("caminho relativo que existe não dispara busca", async () => {
    const d = deps({ existem: ["/p/src/a.ts"] });
    expect(await resolverCitado("src/a.ts", ["/p"], d)).toEqual({ tipo: "um", caminho: "/p/src/a.ts" });
    expect(d.localizar).not.toHaveBeenCalled();
  });
  test("relativo à pasta da conversa vence a raiz do projeto", async () => {
    const d = deps({ existem: ["/p/sub/a.ts", "/p/a.ts"] });
    expect(await resolverCitado("a.ts", ["/p/sub", "/p"], d)).toEqual({ tipo: "um", caminho: "/p/sub/a.ts" });
  });
  test("absoluto e ~ valem como estão", async () => {
    const d = deps({ existem: ["/etc/a.conf", "/home/u/x.md"] });
    expect(await resolverCitado("/etc/a.conf", ["/p"], d)).toEqual({ tipo: "um", caminho: "/etc/a.conf" });
    expect(await resolverCitado("~/x.md", ["/p"], d)).toEqual({ tipo: "um", caminho: "/home/u/x.md" });
    expect(d.localizar).not.toHaveBeenCalled();
  });
  test("relativo com pasta inexistente busca só pelo nome final", async () => {
    const d = deps({ achados: ["/p/outra/x.md"] });
    expect(await resolverCitado("naoexiste/x.md", ["/p"], d)).toEqual({ tipo: "um", caminho: "/p/outra/x.md" });
    expect(d.localizar).toHaveBeenCalledWith("/p", "x.md");
  });
  test("nada encontrado informa nome e raiz", async () => {
    expect(await resolverCitado("a/b.md", ["/p/sub", "/p"], deps())).toEqual({ tipo: "nenhum", nome: "b.md", raiz: "/p" });
  });
});
