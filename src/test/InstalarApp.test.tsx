import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { InstalarApp } from "../mobile/InstalarApp";
import { MANIFEST } from "../mobile/manifest";

function seguro(valor: boolean) { Object.defineProperty(window, "isSecureContext", { value: valor, configurable: true }); }
function agente(ua: string) { Object.defineProperty(navigator, "userAgent", { value: ua, configurable: true }); }

describe("instalar como app (PWA)", () => {
  afterEach(() => { seguro(false); agente("jsdom"); });

  it("no Android, o botão abre o pedido do navegador", async () => {
    seguro(true);
    render(<InstalarApp />);
    const prompt = vi.fn(async () => undefined);
    const evento = Object.assign(new Event("beforeinstallprompt", { cancelable: true }),
      { prompt, userChoice: Promise.resolve({ outcome: "accepted" as const }) });
    act(() => { window.dispatchEvent(evento); });
    expect(evento.defaultPrevented).toBe(true);
    const botao = await screen.findByRole("button", { name: "Instalar no celular" });
    await act(async () => { await userEvent.click(botao); });
    expect(prompt).toHaveBeenCalledOnce();
    // Pedido usado não volta: o navegador recusaria o segundo `prompt()`.
    expect(screen.queryByRole("button", { name: "Instalar no celular" })).not.toBeInTheDocument();
  });

  it("no iPhone, ensina o Adicionar à Tela de Início", () => {
    seguro(true);
    agente("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Safari/604.1");
    render(<InstalarApp />);
    expect(screen.getByText("Adicionar à Tela de Início")).toBeInTheDocument();
  });

  it("sem https, explica que precisa do link seguro", () => {
    render(<InstalarApp />);
    expect(screen.getByText(/link seguro/)).toBeInTheDocument();
    expect(screen.queryByRole("button")).not.toBeInTheDocument();
  });

  it("manifest tem o que o Chrome exige para instalar", () => {
    expect(MANIFEST).toMatchObject({ name: "OMNI AGENTS", short_name: "OMNI", start_url: "/", scope: "/", display: "standalone" });
    expect(MANIFEST.theme_color).toMatch(/^#/);
    expect(MANIFEST.icons.map((i) => i.sizes)).toEqual(["192x192", "512x512"]);
  });
});
