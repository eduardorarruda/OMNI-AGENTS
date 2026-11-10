import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { forwardRef, useImperativeHandle } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { GrafoPanel } from "../features/grafo/GrafoPanel";
import type { Grafo } from "../features/grafo/grafoModel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const canvas = vi.hoisted(() => ({
  focar: vi.fn(),
  centralizar: vi.fn(),
  aproximar: vi.fn(),
  props: null as null | { grafo: Grafo; selecionada: string | null; destacadas?: ReadonlySet<string>; aoSelecionar: (slug: string | null) => void },
}));
vi.mock("../features/grafo/GrafoCanvas", () => ({
  GrafoCanvas: forwardRef((props: NonNullable<typeof canvas.props>, ref) => {
    canvas.props = props;
    useImperativeHandle(ref, () => ({ focar: canvas.focar, centralizar: canvas.centralizar, aproximar: canvas.aproximar }));
    return <canvas aria-hidden="true" />;
  }),
}));

const nota = (slug: string, titulo: string, projeto: string) => ({ slug, titulo, resumo: `Texto da nota ${titulo}`, tags: ["deploy"], projeto });
const grafo: Grafo = {
  notas: [nota("a", "Deploy", "api"), nota("b", "Esteira", "api"), nota("c", "Receita", "site")],
  ligacoes: [{ de: "a", para: "b", tipo: "wikilink", peso: 1 }],
};

function responder(valor: Grafo) {
  vi.mocked(invoke).mockImplementation(async (comando) => (comando === "memoria_grafo" ? valor : undefined));
}

describe("Painel do grafo", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    canvas.focar.mockReset(); canvas.centralizar.mockReset(); canvas.props = null;
  });

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
    fireEvent.click(screen.getByRole("combobox", { name: "Projeto" }));
    fireEvent.click(screen.getByRole("option", { name: /site/ }));
    await waitFor(() => expect(within(screen.getByRole("list", { name: "Notas da memória" })).getAllByRole("button")).toHaveLength(1));
    expect(canvas.props?.grafo.notas.map((n) => n.slug)).toEqual(["c"]);
    expect(screen.getByText("1 nota · 0 ligações")).toBeTruthy();
  });

  it("chip de tipo liga e desliga aquele tipo de ligação", async () => {
    responder({ ...grafo, ligacoes: [...grafo.ligacoes, { de: "b", para: "c", tipo: "tag", peso: 0.5 }] });
    render(<GrafoPanel />);
    const chip = await screen.findByRole("button", { name: /Citação \[\[nota\]\]/ });
    expect(chip.getAttribute("aria-pressed")).toBe("true");
    expect(screen.getByText("3 notas · 2 ligações")).toBeTruthy();
    fireEvent.click(chip);
    expect(chip.getAttribute("aria-pressed")).toBe("false");
    expect(canvas.props?.grafo.ligacoes.map((l) => l.tipo)).toEqual(["tag"]);
    expect(screen.getByText("3 notas · 1 ligação")).toBeTruthy();
    fireEvent.click(chip);
    expect(canvas.props?.grafo.ligacoes).toHaveLength(2);
  });

  it("busca destaca as notas achadas e Enter seleciona e centraliza a primeira", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    const busca = await screen.findByRole("searchbox", { name: "Buscar nota no grafo" });
    fireEvent.change(busca, { target: { value: "receíta" } });
    expect([...(canvas.props?.destacadas ?? [])]).toEqual(["c"]);
    expect(within(screen.getByRole("list", { name: "Notas da memória" })).getAllByRole("button")).toHaveLength(1);
    fireEvent.keyDown(busca, { key: "Enter" });
    expect(canvas.focar).toHaveBeenCalledWith("c");
    expect(within(screen.getByRole("region", { name: "Nota selecionada" })).getByText("Texto da nota Receita")).toBeTruthy();
  });

  it("busca sem resultado avisa e oferece limpar", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    fireEvent.change(await screen.findByRole("searchbox", { name: "Buscar nota no grafo" }), { target: { value: "xyz" } });
    expect(screen.getByText(/Nenhuma nota com “xyz”/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Limpar busca" }));
    expect(within(screen.getByRole("list", { name: "Notas da memória" })).getAllByRole("button")).toHaveLength(3);
  });

  it("clique no grafo seleciona; clique no vazio desfaz; fechar tira o detalhe", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    await screen.findByRole("list", { name: "Notas da memória" });
    act(() => canvas.props?.aoSelecionar("b"));
    expect(within(screen.getByRole("region", { name: "Nota selecionada" })).getByText("Texto da nota Esteira")).toBeTruthy();
    expect(canvas.props?.selecionada).toBe("b");
    act(() => canvas.props?.aoSelecionar(null));
    expect(screen.queryByRole("region", { name: "Nota selecionada" })).toBeNull();
    act(() => canvas.props?.aoSelecionar("a"));
    fireEvent.click(screen.getByRole("button", { name: "Fechar detalhe da nota" }));
    expect(screen.queryByRole("region", { name: "Nota selecionada" })).toBeNull();
  });

  it("clicar numa nota da lista centraliza o nó; botão centralizar enquadra tudo", async () => {
    responder(grafo);
    render(<GrafoPanel />);
    fireEvent.click(await screen.findByRole("button", { name: /Esteira/ }));
    expect(canvas.focar).toHaveBeenCalledWith("b");
    fireEvent.click(screen.getByRole("button", { name: "Centralizar o grafo" }));
    expect(canvas.centralizar).toHaveBeenCalledTimes(1);
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
