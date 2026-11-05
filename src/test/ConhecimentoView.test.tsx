import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { ConhecimentoView } from "../features/conhecimento/ConhecimentoView";
import { tamanhoLegivel } from "../features/conhecimento/conhecimentoService";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const fonte = { id: 1, nome: "studio.db", caminho: "/x/studio.db", tipo: "banco SQLite", tamanho: 2_000_000, trechos: 1200, estado: "pronto", erro: null, criado_em_ms: 1 };

function backend(extra: Record<string, unknown> = {}) {
  const respostas: Record<string, unknown> = { rag_listar: [fonte], rag_claude_ligado: false, rag_buscar: [], ...extra };
  vi.mocked(invoke).mockImplementation(async (comando) => respostas[comando as string]);
}

describe("Tela Conhecimento", () => {
  beforeEach(() => vi.mocked(invoke).mockReset());

  it("mostra os arquivos, o total de trechos e o motivo de um erro", async () => {
    backend({ rag_listar: [fonte, { ...fonte, id: 2, nome: "x.backup", estado: "erro", erro: "exporte em .sql", trechos: 0 }] });
    render(<ConhecimentoView onClose={() => {}} />);
    expect(await screen.findByText("studio.db")).toBeInTheDocument();
    expect(screen.getByText(/2 arquivos/)).toBeInTheDocument();
    expect(screen.getByText("exporte em .sql")).toBeInTheDocument();
  });

  it("remove um arquivo e liga o Claude pela caixa", async () => {
    backend();
    render(<ConhecimentoView onClose={() => {}} />);
    fireEvent.click(await screen.findByRole("button", { name: "Remover studio.db" }));
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("rag_remover", { id: 1 }));
    const caixa = await screen.findByRole("checkbox");
    await waitFor(() => expect(caixa).toBeEnabled());
    fireEvent.click(caixa);
    await waitFor(() => expect(invoke).toHaveBeenCalledWith("rag_claude_ligar", { ligado: true }));
  });

  it("busca enquanto digita", async () => {
    backend({ rag_buscar: [{ id: 9, fonte: "a.md", titulo: "a.md", resumo: "resumo achado" }] });
    render(<ConhecimentoView onClose={() => {}} />);
    fireEvent.change(await screen.findByRole("searchbox"), { target: { value: "deploy" } });
    expect(await screen.findByText("resumo achado")).toBeInTheDocument();
  });

  it("formata o tamanho", () => {
    expect(tamanhoLegivel(2048)).toBe("2 KB");
    expect(tamanhoLegivel(5 * 1024 ** 2)).toBe("5.0 MB");
  });
});
