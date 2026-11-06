import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { KanbanBoard } from "../features/kanban/KanbanBoard";
import { emptyKanban } from "../features/kanban/kanbanReducer";
import { definirAvisoDoKanban, limparAvisoDoKanban } from "../features/kanban/kanbanAviso";
import { KANBAN_STORAGE_KEY, carregarKanbanComOrigem } from "../services/kanbanService";
import { engineApi } from "../features/terminal/terminalService";

vi.mock("../features/terminal/terminalService", () => ({ engineApi: vi.fn(), listAgentClis: vi.fn(async () => []) }));

const api = vi.mocked(engineApi);

function quadro() {
  return render(<KanbanBoard projectId="p" kanban={emptyKanban()} dispatch={vi.fn()} />);
}

beforeEach(() => {
  localStorage.clear();
  limparAvisoDoKanban();
  api.mockReset();
});

describe("aviso do Kanban na tela", () => {
  it("cartão de fora da migração aparece e o usuário dispensa", async () => {
    localStorage.setItem(KANBAN_STORAGE_KEY, JSON.stringify({ version: 1, tasks: [{ id: "x" }] }));
    api.mockImplementation(async (metodo, caminho) => ({
      type: "api" as const,
      status: 200,
      body: JSON.stringify(caminho === "/kanban/migrar" ? { migrou: false, deFora: 0 } : metodo === "GET" && caminho === "/kanban" ? { versao: 1, tarefas: [] } : {}),
    }));
    await carregarKanbanComOrigem();
    quadro();
    expect(await screen.findByRole("alert")).toHaveTextContent("1 cartão do Kanban antigo");
    fireEvent.click(screen.getByRole("button", { name: "Entendi" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("aviso passageiro não tem botão de dispensar", () => {
    definirAvisoDoKanban("O engine do OMNI está fora do ar");
    quadro();
    expect(screen.getByRole("alert")).toHaveTextContent("fora do ar");
    expect(screen.queryByRole("button", { name: "Entendi" })).toBeNull();
  });
});
