import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { AprovacaoDeAparelho, descreverAparelho, type ListaDePedidos } from "../features/mobile/AprovacaoDeAparelho";

const invoke = vi.mocked((await import("@tauri-apps/api/core")).invoke);

const ANDROID = "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 Chrome/131.0 Mobile Safari/537.36";
const vazia: ListaDePedidos = { pedidos: [], pareamento_desligado: false, pausado_ate_ms: null, alerta: null };
const comPedido: ListaDePedidos = { ...vazia, pedidos: [{ id: "abc123", aparelho: ANDROID, ip: "203.0.113.7",
  em_ms: Date.UTC(2026, 10, 9, 15, 30), expira_em_ms: Date.UTC(2026, 10, 9, 15, 32) }] };

function engine(rotas: Record<string, ListaDePedidos>) {
  invoke.mockImplementation(async (comando: string, args?: Record<string, unknown>) => {
    if (comando !== "engine_api") return null;
    const corpo = rotas[`${args?.method} ${args?.path}`];
    return corpo ? { type: "api", status: 200, body: JSON.stringify(corpo) } : { type: "api", status: 404, body: "{}" };
  });
}

describe("aprovação de aparelho novo no PC", () => {
  beforeEach(() => invoke.mockReset());

  it("descreve o aparelho pelo navegador", () => {
    expect(descreverAparelho(ANDROID)).toBe("Android · Chrome");
    expect(descreverAparelho("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Version/18.0 Mobile/15E148 Safari/604.1")).toBe("iPhone · Safari");
    expect(descreverAparelho("")).toBe("Aparelho desconhecido");
  });

  it("sem pedido não mostra nada", async () => {
    engine({ "GET /tunel/pedidos": vazia });
    const { container } = render(<AprovacaoDeAparelho />);
    await act(async () => { await Promise.resolve(); });
    expect(container).toBeEmptyDOMElement();
  });

  it("mostra aparelho, IP e hora, e Permitir manda aprovar só aquele pedido", async () => {
    engine({ "GET /tunel/pedidos": comPedido, "POST /tunel/pedidos/abc123/permitir": vazia });
    render(<AprovacaoDeAparelho />);
    const caixa = await screen.findByRole("alertdialog", { name: "Aparelho quer entrar" });
    expect(caixa).toHaveTextContent("Android · Chrome");
    expect(caixa).toHaveTextContent("203.0.113.7");
    const permitir = screen.getByRole("button", { name: "Permitir" });
    await act(async () => { await userEvent.click(permitir); });
    expect(invoke).toHaveBeenCalledWith("engine_api", expect.objectContaining({ method: "POST", path: "/tunel/pedidos/abc123/permitir" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("Recusar manda recusar", async () => {
    engine({ "GET /tunel/pedidos": comPedido, "POST /tunel/pedidos/abc123/recusar": vazia });
    render(<AprovacaoDeAparelho />);
    const recusar = await screen.findByRole("button", { name: "Recusar" });
    await act(async () => { await userEvent.click(recusar); });
    expect(invoke).toHaveBeenCalledWith("engine_api", expect.objectContaining({ path: "/tunel/pedidos/abc123/recusar" }));
  });

  it("pareamento desligado por excesso de erros avisa e deixa reativar", async () => {
    engine({
      "GET /tunel/pedidos": { ...vazia, pareamento_desligado: true, alerta: { em_ms: 1, texto: "Códigos do Authy errados demais hoje pelo link seguro: o pareamento de aparelhos novos foi desligado." } },
      "POST /tunel/pareamento/reativar": vazia,
    });
    render(<AprovacaoDeAparelho />);
    expect(await screen.findByRole("alert")).toHaveTextContent("foi desligado");
    await act(async () => { await userEvent.click(screen.getByRole("button", { name: "Reativar pareamento" })); });
    expect(invoke).toHaveBeenCalledWith("engine_api", expect.objectContaining({ path: "/tunel/pareamento/reativar" }));
  });
});
