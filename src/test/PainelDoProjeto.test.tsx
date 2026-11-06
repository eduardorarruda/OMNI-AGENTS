import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it, vi } from "vitest";
import { FaixaDeExecucao } from "../features/panorama/FaixaDeExecucao";
import { PainelDoProjeto } from "../features/panorama/PainelDoProjeto";
import type { Execucao } from "../features/fluxos/fluxosService";
import type { ItemDeFundo } from "../features/segundoPlano/useSegundoPlano";

vi.mock("../features/fluxos/useExecucoesAtivas", () => ({ useExecucoesAtivas: vi.fn() }));
vi.mock("../features/segundoPlano/useSegundoPlano", () => ({ useSegundoPlano: vi.fn() }));
// jsdom não mede layout: o ReactFlow de verdade não desenha nós. O dublê expõe o que o componente pediu.
vi.mock("@xyflow/react", () => ({
  ReactFlow: (props: { nodes: { id: string; className?: string; data: { label: string } }[]; nodesDraggable?: boolean; nodesConnectable?: boolean; elementsSelectable?: boolean }) => (
    <div data-testid="grafo" data-draggable={String(props.nodesDraggable)} data-connectable={String(props.nodesConnectable)} data-selectable={String(props.elementsSelectable)}>
      {props.nodes.map((n) => <span key={n.id} data-testid={`no-${n.id}`} className={n.className}>{n.data.label}</span>)}
    </div>
  ),
  ReactFlowProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
  Background: () => null,
}));

const { useExecucoesAtivas } = vi.mocked(await import("../features/fluxos/useExecucoesAtivas"));
const { useSegundoPlano } = vi.mocked(await import("../features/segundoPlano/useSegundoPlano"));

function execucao(id: string, projetoId: string, nome = "Revisão", ativo = "b"): Execucao {
  const no = (nid: string, titulo: string, x: number) => ({ id: nid, tipo: "agente", titulo, config: {}, x, y: 0 });
  return {
    id, projeto_id: projetoId, projeto: "P", entrada: "", pasta: "", branch: null, base: null, status: "rodando", erro: null,
    criado_ms: 1, atualizado_ms: 1, pr_url: null, custo_usd: 0,
    fluxo: { id: "f", nome, descricao: "", nos: [no("a", "Planejar", 0), no("b", "Implementar", 200), no("c", "Revisar", 400)],
      conexoes: [{ de: "a", para: "b" }, { de: "b", para: "c" }] },
    nos: {
      a: { status: "concluido", voltas: 0, inicio_ms: 1, fim_ms: 2, saida: "", session_id: null, custo_usd: 0, erro: null, cli: null, conta: null, caminho: null },
      [ativo]: { status: "rodando", voltas: 0, inicio_ms: 3, fim_ms: 0, saida: "", session_id: null, custo_usd: 0, erro: null, cli: null, conta: null, caminho: null, atividade: "Edit x.rs" },
    },
  } as unknown as Execucao;
}
const tarefa = (projetoId: string, titulo = "Subagente X"): ItemDeFundo =>
  ({ tipo: "subagente", titulo, atividade: "Read y", desde_ms: 1, projeto_id: projetoId, sessao: "c", execucao: null });

function ativas(lista: Execucao[]) {
  useExecucoesAtivas.mockReturnValue({ ativas: lista, aguardando: lista.filter((e) => e.status === "aguardando"), erro: null });
}

beforeEach(() => {
  vi.clearAllMocks();
  ativas([]);
  useSegundoPlano.mockReturnValue({ itens: [], erro: null });
});

it("faixa só aparece com algo rodando", () => {
  const { container, rerender } = render(<FaixaDeExecucao projetoId="p" onAbrir={vi.fn()} />);
  expect(container).toBeEmptyDOMElement();
  ativas([execucao("e1", "p")]);
  rerender(<FaixaDeExecucao projetoId="p" onAbrir={vi.fn()} />);
  expect(screen.getByRole("button", { name: /1 fluxo rodando/ })).toBeInTheDocument();
});

it("faixa conta só o projeto atual", () => {
  ativas([execucao("e1", "p"), execucao("e2", "outro"), execucao("e3", "outro")]);
  useSegundoPlano.mockReturnValue({ itens: [tarefa("p"), tarefa("outro", "Alheio")], erro: null });
  render(<FaixaDeExecucao projetoId="p" onAbrir={vi.fn()} />);
  const faixa = screen.getByRole("button", { name: /rodando neste projeto/ });
  expect(faixa).toHaveTextContent("1 fluxo");
  expect(faixa).toHaveTextContent("1 em segundo plano");
});

it("só tarefas em segundo plano também acendem a faixa, sem falar de fluxo", () => {
  useSegundoPlano.mockReturnValue({ itens: [tarefa("p"), tarefa("p", "Outro")], erro: null });
  render(<FaixaDeExecucao projetoId="p" onAbrir={vi.fn()} />);
  const faixa = screen.getByRole("button", { name: /rodando neste projeto/ });
  expect(faixa).toHaveTextContent("2 em segundo plano");
  expect(faixa).not.toHaveTextContent("fluxo");
});

it("clique abre a aba", async () => {
  ativas([execucao("e1", "p")]);
  const onAbrir = vi.fn();
  render(<FaixaDeExecucao projetoId="p" onAbrir={onAbrir} />);
  await userEvent.click(screen.getByRole("button", { name: /rodando neste projeto/ }));
  expect(onAbrir).toHaveBeenCalledTimes(1);
});

it("mini-grafo acende o nó ativo", () => {
  ativas([execucao("e1", "p")]);
  render(<PainelDoProjeto projetoId="p" />);
  expect(screen.getByTestId("no-b")).toHaveClass("is-ativo");
  expect(screen.getByTestId("no-a")).not.toHaveClass("is-ativo");
  expect(screen.getByTestId("no-a")).toHaveClass("is-feito");
  expect(screen.getByTestId("no-c")).not.toHaveClass("is-ativo");
  expect(screen.getByText("Edit x.rs")).toBeInTheDocument();
});

it("mini-grafo não deixa editar", () => {
  ativas([execucao("e1", "p")]);
  render(<PainelDoProjeto projetoId="p" />);
  const grafo = screen.getByTestId("grafo");
  expect(grafo).toHaveAttribute("data-draggable", "false");
  expect(grafo).toHaveAttribute("data-connectable", "false");
  expect(grafo).toHaveAttribute("data-selectable", "false");
});

it("painel mostra só os fluxos do projeto e as tarefas em segundo plano", () => {
  ativas([execucao("e1", "p", "Do projeto"), execucao("e2", "outro", "De outro")]);
  useSegundoPlano.mockReturnValue({ itens: [tarefa("p")], erro: null });
  render(<PainelDoProjeto projetoId="p" />);
  expect(screen.getByText("Do projeto")).toBeInTheDocument();
  expect(screen.queryByText("De outro")).not.toBeInTheDocument();
  const tarefas = screen.getByRole("region", { name: "Tarefas em segundo plano" });
  expect(within(tarefas).getByText("Subagente X")).toBeInTheDocument();
});

it("execução terminada sai sem fechar a aba", () => {
  ativas([execucao("e1", "p")]);
  const { rerender } = render(<PainelDoProjeto projetoId="p" />);
  expect(screen.getByTestId("grafo")).toBeInTheDocument();
  ativas([]);
  rerender(<PainelDoProjeto projetoId="p" />);
  expect(screen.queryByTestId("grafo")).not.toBeInTheDocument();
  expect(screen.getByText(/Nada rodando neste projeto agora/)).toBeInTheDocument();
});
