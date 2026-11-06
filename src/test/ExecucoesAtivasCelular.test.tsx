import { expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

const request = vi.fn();
vi.mock("../mobile/api", () => ({ request: (...a: unknown[]) => request(...a) }));
const { ExecucoesAtivas } = await import("../mobile/ExecucoesAtivas");

it("celular lista os fluxos rodando e ignora os concluídos", async () => {
  request.mockImplementation(async (path: string) => path === "/fluxos/execucoes" ? [{ id: "e1", status: "rodando" }, { id: "e2", status: "concluido" }] : {
    id: "e1", status: "rodando",
    fluxo: { nome: "Release", nos: [{ id: "n", tipo: "agente", titulo: "Testar", config: {} }], conexoes: [] },
    nos: { n: { status: "rodando", inicio_ms: 1, saida: "" } },
  });
  render(<ExecucoesAtivas />);
  await act(async () => { await Promise.resolve(); await Promise.resolve(); await Promise.resolve(); });
  expect(await screen.findByText("Release")).toBeInTheDocument();
  expect(screen.getByText("Testar")).toBeInTheDocument();
  expect(request).not.toHaveBeenCalledWith("/fluxos/execucoes/e2");
});

it("celular: detalhe que falha não derruba as outras execuções", async () => {
  request.mockImplementation(async (path: string) => {
    if (path === "/fluxos/execucoes") return [{ id: "e1", status: "rodando" }, { id: "sumiu", status: "rodando" }];
    if (path.endsWith("/sumiu")) throw new Error("Execução não encontrada");
    return { id: "e1", status: "rodando", fluxo: { nome: "Release2", nos: [{ id: "n", tipo: "agente", titulo: "Testar", config: {} }], conexoes: [] }, nos: { n: { status: "rodando", inicio_ms: 1, saida: "" } } };
  });
  render(<ExecucoesAtivas />);
  expect(await screen.findByText("Release2")).toBeInTheDocument();
  expect(screen.queryByText(/Sem atualizar/)).not.toBeInTheDocument();
});
