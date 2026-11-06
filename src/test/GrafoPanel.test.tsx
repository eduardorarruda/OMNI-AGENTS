import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { GrafoPanel } from "../features/grafo/GrafoPanel";
import type { Grafo } from "../features/grafo/grafoModel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("../features/grafo/GrafoCanvas", () => ({ GrafoCanvas: () => <canvas aria-hidden="true" /> }));

const nota = (slug: string, titulo: string, projeto: string) => ({ slug, titulo, resumo: `Texto da nota ${titulo}`, tags: ["deploy"], projeto });
const grafo: Grafo = {
  notas: [nota("a", "Deploy", "api"), nota("b", "Esteira", "api"), nota("c", "Receita", "site")],
  ligacoes: [{ de: "a", para: "b", tipo: "wikilink", peso: 1 }],
};

function responder(valor: Grafo) {
  vi.mocked(invoke).mockImplementation(async (comando) => (comando === "memoria_grafo" ? valor : undefined));
}

describe("Painel do grafo", () => {
  beforeEach(() => { vi.mocked(invoke).mockReset(); });

  it("enquanto calcula avisa que na primeira vez pode demorar", async () => {
    vi.mocked(invoke).mockImplementation(() => new Promise(() => {}));
    render(<GrafoPanel />);
    expect(await screen.findByText("Calculando as ligações. Na primeira vez pode demorar um pouco.")).toBeTruthy();
    expect(screen.queryByText(/ainda não há notas/i)).toBeNull();
  });

  it("sem notas explica como a memória nasce", async () => {
    responder({ notas: [], ligacoes: [] });
    render(<GrafoPanel />);
    expect(await screen.findByText(/ainda não há notas/i)).toBeTruthy();
    expect(screen.getByText(/ao fim de cada turno/i)).toBeTruthy();
  });

  it("clicar na nota mostra o texto e destaca as vizinhas", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /Deploy/ }));
    const detalhe = screen.getByRole("region", { name: "Nota selecionada" });
    expect(within(detalhe).getByText("Texto da nota Deploy")).toBeTruthy();
    expect(within(detalhe).getByText("Esteira")).toBeTruthy();
    expect(screen.getByRole("button", { name: /Esteira/ }).getAttribute("data-vizinha")).toBe("true");
    expect(screen.getByRole("button", { name: /Receita/ }).getAttribute("data-vizinha")).toBe("false");
  });

  it("lista de notas acessível por teclado", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    const lista = await screen.findByRole("list", { name: "Notas da memória" });
    const botoes = within(lista).getAllByRole("button");
    expect(botoes).toHaveLength(3);
    botoes[1].focus();
    expect(document.activeElement).toBe(botoes[1]);
    fireEvent.click(botoes[1]);
    expect(botoes[1].getAttribute("aria-pressed")).toBe("true");
  });

  it("filtra por projeto e por tipo de ligação", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    await screen.findByRole("list", { name: "Notas da memória" });
    fireEvent.change(screen.getByLabelText("Projeto"), { target: { value: "site" } });
    await waitFor(() => expect(within(screen.getByRole("list", { name: "Notas da memória" })).getAllByRole("button")).toHaveLength(1));
  });

  it("abrir grande cria a aba", async () => {
    responder(grafo);
    const aoAbrir = vi.fn();
    render(<GrafoPanel onAbrirGrande={aoAbrir} />);
    fireEvent.click(await screen.findByRole("button", { name: "Abrir em aba grande" }));
    expect(aoAbrir).toHaveBeenCalledTimes(1);
  });

  it("falha ao ler a memória aparece como erro, não como vazio", async () => {
    vi.mocked(invoke).mockImplementation(() => Promise.reject("banco travado"));
    render(<GrafoPanel />);
    expect(await screen.findByRole("alert")).toBeTruthy();
    expect(screen.queryByText(/ainda não há notas/i)).toBeNull();
  });
});
