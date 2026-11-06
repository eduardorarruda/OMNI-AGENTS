import { afterEach, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const request = vi.fn();
vi.mock("../mobile/api", () => ({ request: (...args: unknown[]) => request(...args) }));
const { AprovacoesDeFluxo } = await import("../mobile/AprovacoesDeFluxo");

afterEach(() => request.mockReset());

const execucao = {
  id: "ex1", status: "aguardando", entrada: "subir a 0.7.8",
  fluxo: {
    nome: "Release", conexoes: [{ de: "validar", para: "ok" }],
    nos: [{ id: "validar", tipo: "comando", titulo: "Validar", config: {} }, { id: "ok", tipo: "aprovacao", titulo: "Publicar?", config: { pergunta: "Publicar a versão?" } }],
  },
  nos: { validar: { status: "concluido", saida: "testes ok" }, ok: { status: "aguardando", saida: "" } },
};

it("fluxo parado na aprovação aparece com a saída anterior e Aprovar manda sim", async () => {
  request.mockImplementation(async (path: string) => {
    if (path === "/fluxos/execucoes") return [{ id: "ex1", status: "aguardando" }, { id: "ex2", status: "rodando" }];
    if (path === "/fluxos/execucoes/ex1") return execucao;
    return { ok: true };
  });
  render(<AprovacoesDeFluxo />);
  expect(await screen.findByText("Publicar a versão?")).toBeInTheDocument();
  expect(screen.getByText("testes ok")).toBeInTheDocument();
  expect(request).not.toHaveBeenCalledWith("/fluxos/execucoes/ex2");

  await userEvent.click(screen.getByRole("button", { name: "Aprovar" }));
  expect(request).toHaveBeenCalledWith("/fluxos/execucoes/ex1/nos/ok/aprovar", expect.objectContaining({ method: "POST", body: JSON.stringify({ sim: true }) }));
  expect(screen.queryByText("Publicar a versão?")).not.toBeInTheDocument();
});

it("celular mostra a etapa de tarefa de fora parada, sem nó Aprovação", async () => {
  const segurada = {
    ...execucao, fluxo: { nome: "Vinda de fora", conexoes: [], nos: [{ id: "c", tipo: "comando", titulo: "Rodar script", config: { comando: "./deploy.sh" } }] },
    nos: { c: { status: "aguardando", saida: "" } },
  };
  request.mockImplementation(async (path: string) => {
    if (path === "/fluxos/execucoes") return [{ id: "ex1", status: "aguardando" }];
    if (path === "/fluxos/execucoes/ex1") return segurada;
    return { ok: true };
  });
  render(<AprovacoesDeFluxo />);
  expect(await screen.findByText(/Tarefa de fora: liberar “Rodar script”/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Recusar" }));
  expect(request).toHaveBeenCalledWith("/fluxos/execucoes/ex1/nos/c/aprovar", expect.objectContaining({ body: JSON.stringify({ sim: false }) }));
});

it("engine sem as rotas de fluxo não mostra nada", async () => {
  request.mockRejectedValue(new Error("Falha HTTP 404"));
  const { container } = render(<AprovacoesDeFluxo />);
  await Promise.resolve();
  expect(container).toBeEmptyDOMElement();
});

it("celular lista todas as aprovações da execução", async () => {
  const duas = {
    ...execucao,
    fluxo: { ...execucao.fluxo, nos: [...execucao.fluxo.nos, { id: "ok2", tipo: "aprovacao", titulo: "Avisar?", config: { pergunta: "Avisar o time?" } }] },
    nos: { ...execucao.nos, ok2: { status: "aguardando", saida: "" } },
  };
  request.mockImplementation(async (path: string) => path === "/fluxos/execucoes" ? [{ id: "ex1", status: "aguardando" }] : duas);
  render(<AprovacoesDeFluxo />);
  expect(await screen.findByText("Publicar a versão?")).toBeInTheDocument();
  expect(screen.getByText("Avisar o time?")).toBeInTheDocument();
  expect(screen.getAllByRole("button", { name: "Aprovar" })).toHaveLength(2);
});
