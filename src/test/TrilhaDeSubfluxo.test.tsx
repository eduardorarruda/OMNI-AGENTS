import { afterEach, describe, expect, it, vi } from "vitest";
import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ReactNode } from "react";

type Saida = { id: string };
const obterExecucao = vi.fn();
const listarExecucoes = vi.fn();
const listarFluxos = vi.fn();
const salvarFluxo = vi.fn();
const avisosDoFluxo = vi.fn();
const acaoNaExecucao = vi.fn(async () => ({ ok: true }));

vi.mock("../features/fluxos/fluxosService", async (original) => ({
  ...(await original<typeof import("../features/fluxos/fluxosService")>()),
  lerEventos: async () => [], validarFluxo: async () => undefined,
  listarFluxos: (...a: unknown[]) => listarFluxos(...a), listarExecucoes: (...a: unknown[]) => listarExecucoes(...a),
  obterExecucao: (...a: unknown[]) => obterExecucao(...a), salvarFluxo: (...a: unknown[]) => salvarFluxo(...a),
  avisosDoFluxo: (...a: unknown[]) => avisosDoFluxo(...a), acaoNaExecucao: (...a: unknown[]) => acaoNaExecucao(...a),
}));
vi.mock("../features/agentes/agentesService", async (original) => ({
  ...(await original<typeof import("../features/agentes/agentesService")>()),
  listarAgentes: async () => [], listarModelos: async () => ({}),
}));
vi.mock("../features/terminal/terminalService", async (original) => ({
  ...(await original<typeof import("../features/terminal/terminalService")>()), listProfiles: async () => [],
}));
vi.mock("@tauri-apps/plugin-opener", () => ({ revealItemInDir: vi.fn() }));
// O canvas de verdade precisa de medidas que o jsdom não tem: cada nó vira um botão com o mesmo clique.
vi.mock("@xyflow/react", async (original) => ({
  ...(await original<typeof import("@xyflow/react")>()),
  ReactFlow: ({ nodes, onNodeClick }: { nodes: Saida[]; onNodeClick?: (e: unknown, n: Saida) => void; children?: ReactNode }) => (
    <div>{nodes.map((n) => <button key={n.id} type="button" onClick={(e) => onNodeClick?.(e, n)}>{`nó ${n.id}`}</button>)}</div>
  ),
}));

const { FluxosView } = await import("../features/fluxos/FluxosView");
const { SeletorDeSubfluxo, TrilhaDeSubfluxo } = await import("../features/fluxos/TrilhaDeSubfluxo");
const { pedirExecucao, consumirPedidoDeExecucao } = await import("../features/fluxos/pedidoDeExecucao");
type Execucao = import("../features/fluxos/fluxosService").Execucao;
type FluxoSalvo = import("../features/fluxos/fluxosService").FluxoSalvo;

vi.stubGlobal("ResizeObserver", class { observe() {} unobserve() {} disconnect() {} });
afterEach(() => { vi.resetAllMocks(); acaoNaExecucao.mockResolvedValue({ ok: true }); consumirPedidoDeExecucao(); });

const props = { projectId: null, projectPath: null, onAbrirSessao: () => undefined, onClose: () => undefined };
const fluxo = (id: string, nome: string, nos: unknown[] = []) => ({ id, nome, descricao: "", nos, conexoes: [] }) as unknown as Execucao["fluxo"];
const salvo = (id: string, nome: string, escopo: "global" | "projeto" = "global"): FluxoSalvo => ({ ...fluxo(id, nome), escopo, caminho: "" });
const execucao = (id: string, parte: Partial<Execucao>): Execucao =>
  ({ id, fluxo: fluxo(id, id), status: "rodando", entrada: "", pasta: "/p", nos: {}, custo_usd: 0, criado_ms: 1, atualizado_ms: 1, ...parte }) as Execucao;

describe("trilha da cadeia", () => {
  const z = execucao("ex-z", { pai: { execucao: "ex-y", no: "s" }, pilha: ["X", "Y", "Z"] });
  const mapa: Record<string, Execucao> = {
    "ex-y": execucao("ex-y", { pai: { execucao: "ex-x", no: "s" }, pilha: ["X", "Y"] }), "ex-x": execucao("ex-x", {}),
  };

  it("trilha mostra X › Y › Z", async () => {
    obterExecucao.mockImplementation(async (id: string) => mapa[id]);
    render(<TrilhaDeSubfluxo execucao={z} onAbrir={() => undefined} />);
    expect(screen.getByRole("navigation", { name: "Cadeia de subfluxos" })).toHaveTextContent("X › Y › Z");
    await waitFor(() => expect(screen.getByRole("button", { name: "X" })).toBeInTheDocument());
  });

  it("clicar na trilha volta ao pai", async () => {
    obterExecucao.mockImplementation(async (id: string) => mapa[id]);
    const onAbrir = vi.fn();
    render(<TrilhaDeSubfluxo execucao={z} onAbrir={onAbrir} />);
    await userEvent.click(await screen.findByRole("button", { name: "Y" }));
    expect(onAbrir).toHaveBeenCalledWith("ex-y");
    await userEvent.click(await screen.findByRole("button", { name: "X" }));
    expect(onAbrir).toHaveBeenCalledWith("ex-x");
    expect(screen.queryByRole("button", { name: "Z" })).not.toBeInTheDocument();
  });

  it("avô que sumiu fica como texto e o pai segue clicável", async () => {
    obterExecucao.mockRejectedValue(new Error("Execução não encontrada"));
    render(<TrilhaDeSubfluxo execucao={z} onAbrir={() => undefined} />);
    expect(await screen.findByRole("button", { name: "Y" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "X" })).not.toBeInTheDocument();
  });

  it("execução sem pai não mostra trilha", () => {
    const { container } = render(<TrilhaDeSubfluxo execucao={execucao("ex-x", {})} onAbrir={() => undefined} />);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("seletor do fluxo chamado", () => {
  it("seletor lista os fluxos sem o atual marcado como ciclo", async () => {
    render(<SeletorDeSubfluxo id="s" fluxos={[salvo("a", "Atual"), salvo("b", "Outro")]} atualId="a" valor="" onMudar={() => undefined} />);
    await userEvent.click(screen.getByRole("combobox"));
    expect(screen.getByRole("option", { name: /Atual/ })).toHaveTextContent("ciclo");
    expect(screen.getByRole("option", { name: /Outro/ })).not.toHaveTextContent("ciclo");
  });

  it("escolher um fluxo guarda o id", async () => {
    const onMudar = vi.fn();
    render(<SeletorDeSubfluxo id="s" fluxos={[salvo("b", "Outro")]} atualId="a" valor="" onMudar={onMudar} />);
    await userEvent.click(screen.getByRole("combobox"));
    await userEvent.click(screen.getByRole("option", { name: /Outro/ }));
    expect(onMudar).toHaveBeenCalledWith("b");
  });

  it("mesmo id global e de projeto aparece uma vez, como o engine resolve", async () => {
    render(<SeletorDeSubfluxo id="s" fluxos={[salvo("b", "Global", "global"), salvo("b", "Do projeto", "projeto")]} valor="" onMudar={() => undefined} />);
    await userEvent.click(screen.getByRole("combobox"));
    expect(screen.getByRole("option", { name: /Do projeto/ })).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Global/ })).not.toBeInTheDocument();
  });
});

describe("na tela de Fluxos", () => {
  const abrir = async (x: Execucao) => {
    listarFluxos.mockResolvedValue([]);
    listarExecucoes.mockResolvedValue([]);
    obterExecucao.mockResolvedValue(x);
    render(<FluxosView {...props} />);
    await act(async () => { await Promise.resolve(); });
    act(() => pedirExecucao(x.id));
    await screen.findByText(x.fluxo.nome);
  };
  const noSub = { id: "s", tipo: "subfluxo", titulo: "Chamar Y", config: { fluxo: "y" }, x: 0, y: 0 };

  it("clicar no nó abre a filha", async () => {
    const pai = execucao("ex-pai", { fluxo: fluxo("x", "X", [noSub]), nos: { s: { status: "rodando", filha: "ex-filha" } as Execucao["nos"][string] } });
    await abrir(pai);
    await userEvent.click(screen.getByRole("button", { name: "nó s" }));
    await waitFor(() => expect(obterExecucao).toHaveBeenCalledWith("ex-filha"));
  });

  it("nó sem filha só seleciona", async () => {
    const pai = execucao("ex-pai", { fluxo: fluxo("x", "X", [noSub]), nos: { s: { status: "pendente" } as Execucao["nos"][string] } });
    await abrir(pai);
    await userEvent.click(screen.getByRole("button", { name: "nó s" }));
    expect(obterExecucao).not.toHaveBeenCalledWith(undefined);
  });

  it("Pausar aparece com a execução esperando aprovação", async () => {
    await abrir(execucao("ex-a", { fluxo: fluxo("x", "X"), status: "aguardando" }));
    await userEvent.click(screen.getByRole("button", { name: "Pausar" }));
    expect(acaoNaExecucao).toHaveBeenCalledWith("ex-a", "pausar");
  });

  it("filha aparece marcada como subfluxo na lista de execuções", async () => {
    listarFluxos.mockResolvedValue([]);
    listarExecucoes.mockResolvedValue([
      { id: "f", fluxo: "Filha", status: "rodando", criado_ms: 1, entrada: "", pai: "p" }, { id: "p", fluxo: "Raiz", status: "rodando", criado_ms: 1, entrada: "" },
    ]);
    render(<FluxosView {...props} />);
    await userEvent.click(screen.getByRole("button", { name: /Fluxos/ }));
    const filha = (await screen.findByText("Filha")).closest("button");
    const raiz = screen.getByText("Raiz").closest("button");
    expect(filha).toHaveTextContent("Subfluxo");
    expect(raiz).not.toHaveTextContent("Subfluxo");
  });

  it("salvar com ciclo mostra aviso e salva", async () => {
    listarFluxos.mockResolvedValue([salvo("x", "Fluxo X")]);
    listarExecucoes.mockResolvedValue([]);
    salvarFluxo.mockResolvedValue("caminho");
    avisosDoFluxo.mockResolvedValue(["Ciclo de subfluxos: x › y › x."]);
    render(<FluxosView {...props} />);
    await userEvent.click(await screen.findByText("Fluxo X"));
    await userEvent.click(screen.getByRole("button", { name: "Salvar" }));
    await waitFor(() => expect(salvarFluxo).toHaveBeenCalled());
    expect(await screen.findByRole("status")).toHaveTextContent("Ciclo de subfluxos: x › y › x.");
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("falha ao conferir ciclos não desfaz o salvar", async () => {
    listarFluxos.mockResolvedValue([salvo("x", "Fluxo X")]);
    listarExecucoes.mockResolvedValue([]);
    salvarFluxo.mockResolvedValue("caminho");
    avisosDoFluxo.mockRejectedValue(new Error("x"));
    render(<FluxosView {...props} />);
    await userEvent.click(await screen.findByText("Fluxo X"));
    await userEvent.click(screen.getByRole("button", { name: "Salvar" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Fluxo salvo");
  });
});
