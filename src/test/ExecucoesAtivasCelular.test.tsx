import { expect, it, vi } from "vitest";
import { act, render, screen } from "@testing-library/react";

const requestSeMudou = vi.fn();
const request = vi.fn();
vi.mock("../mobile/api", () => ({ request: (...a: unknown[]) => request(...a), requestSeMudou: (...a: unknown[]) => requestSeMudou(...a) }));
const { ExecucoesAtivas } = await import("../mobile/ExecucoesAtivas");

it("celular lista os fluxos rodando e ignora os concluídos", async () => {
  requestSeMudou.mockResolvedValue([{ id: "e1", status: "rodando" }, { id: "e2", status: "concluido" }]);
  request.mockResolvedValue({
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
