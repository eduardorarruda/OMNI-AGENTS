import { useEffect, useState } from "react";

/** O evento do Chrome/Android que permite abrir o "Instalar app" por um botão nosso. */
export interface PedidoDeInstalacao extends Event {
  prompt: () => Promise<void>;
  userChoice: Promise<{ outcome: "accepted" | "dismissed" }>;
}

export type ComoInstalar = "instalado" | "botao" | "iphone" | "sem-https" | "menu";

let guardado: PedidoDeInstalacao | null = null;
const ouvintes = new Set<() => void>();
const avisar = () => ouvintes.forEach((ouvinte) => ouvinte());

/** Registrado na carga do módulo: o Chrome dispara o evento uma vez só, logo que a página abre —
 *  antes de alguém abrir os Ajustes. */
export function escutarInstalacao(alvo: Window = window): void {
  alvo.addEventListener("beforeinstallprompt", (evento) => {
    evento.preventDefault(); // guarda para o nosso botão em vez da faixa automática
    guardado = evento as PedidoDeInstalacao;
    avisar();
  });
  alvo.addEventListener("appinstalled", () => { guardado = null; avisar(); });
}

if (typeof window !== "undefined") {
  escutarInstalacao();
  // Com o service worker registrado o Chrome considera a página instalável; ele não guarda nada.
  if (window.isSecureContext && "serviceWorker" in navigator) {
    void navigator.serviceWorker.register("/sw.js").catch(() => undefined);
  }
}

function abertoComoApp(): boolean {
  const standalone = typeof window.matchMedia === "function" && window.matchMedia("(display-mode: standalone)").matches;
  return standalone || (navigator as Navigator & { standalone?: boolean }).standalone === true;
}

function ehIphone(): boolean {
  // iPad no iPadOS 13+ se apresenta como Mac; o toque é o que denuncia.
  return /iPhone|iPad|iPod/.test(navigator.userAgent) || (navigator.userAgent.includes("Macintosh") && navigator.maxTouchPoints > 1);
}

export function comoInstalar(temPedido: boolean): ComoInstalar {
  if (abertoComoApp()) return "instalado";
  if (!window.isSecureContext) return "sem-https";
  if (temPedido) return "botao";
  if (ehIphone()) return "iphone";
  return "menu";
}

/** Abre o pedido do navegador. Devolve se a pessoa aceitou. Cada pedido só pode ser usado uma vez. */
export async function pedirInstalacao(): Promise<boolean> {
  const pedido = guardado;
  if (!pedido) return false;
  guardado = null;
  await pedido.prompt();
  const { outcome } = await pedido.userChoice;
  avisar();
  return outcome === "accepted";
}

export function useComoInstalar(): ComoInstalar {
  const [modo, setModo] = useState(() => comoInstalar(guardado !== null));
  useEffect(() => {
    const atualizar = () => setModo(comoInstalar(guardado !== null));
    ouvintes.add(atualizar);
    atualizar();
    return () => { ouvintes.delete(atualizar); };
  }, []);
  return modo;
}
